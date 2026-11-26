# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import runpy
import tempfile
import tomllib
import unittest
from pathlib import Path

POINT = runpy.run_path(str(Path(__file__).resolve().parents[1] / "point-tui-cohort.py"))["point_tui_cohort"]
URL = "https://gitlab.com/vauchi/contract.git"
TUI = "https://gitlab.com/vauchi/tui-display.git"
OLD = f'vauchi-contract = {{ git = "{URL}", tag = "v0.101.0", version = "=0.101.0" }}\n'
NEW = OLD.replace("0.101.0", "0.102.0")
DISPLAY = f'tui-display = {{ package = "vauchi-tui-display", git = "{TUI}", tag = "v0.6.0", version = "=0.6.0", optional = true, features = ["fixture"] }}\n'


class TuiCohortTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.core = self.root / "core"
        self.core.mkdir()
        (self.core / "Cargo.toml").write_text("[workspace.dependencies]\n" + NEW)
        self.consumer = self.root / "consumer"
        self.consumer.mkdir()
        self.manifest = self.consumer / "Cargo.toml"
        self.manifest.write_text('[dependencies]\nserde = "1"\n' + OLD + DISPLAY)
        self.original = self.manifest.read_text()
        self.renderer = '[package]\nname="vauchi-tui-display"\nversion="0.6.0"\n[dependencies]\n' + OLD
        self.checkouts = []

    def checkout(self, url, tag, destination):
        self.checkouts.append((url, tag))
        destination.mkdir(parents=True)
        (destination / "Cargo.toml").write_text(self.renderer)

    def contract(self, path):
        return tomllib.loads(path.read_text())["dependencies"]["vauchi-contract"]

    # @internal
    def test_consumer_and_tagged_renderer_share_the_candidate_contract(self):
        core_before = (self.core / "Cargo.toml").read_bytes()
        POINT(self.core, self.manifest, self.checkout)
        dependencies = tomllib.loads(self.manifest.read_text())["dependencies"]
        self.assertEqual(self.checkouts, [(TUI, "v0.6.0")])
        self.assertEqual(self.contract(self.manifest), {"git": URL, "tag": "v0.102.0", "version": "=0.102.0"})
        display = dependencies["tui-display"]
        self.assertNotIn("git", display)
        self.assertNotIn("tag", display)
        self.assertEqual(display["version"], "=0.6.0")
        self.assertEqual(display["package"], "vauchi-tui-display")
        self.assertTrue(display["optional"])
        self.assertEqual(display["features"], ["fixture"])
        renderer = Path(display["path"]) / "Cargo.toml"
        self.assertTrue(renderer.resolve().is_relative_to(self.consumer.resolve()))
        self.assertEqual(self.contract(renderer), self.contract(self.manifest))
        self.assertEqual(dependencies["serde"], "1")
        self.assertEqual((self.core / "Cargo.toml").read_bytes(), core_before)

    # @internal
    def test_a_consumer_without_a_renderer_still_aligns_its_contract(self):
        self.manifest.write_text("[dependencies]\n" + OLD)
        POINT(self.core, self.manifest, self.checkout)
        self.assertEqual(self.contract(self.manifest)["tag"], "v0.102.0")
        self.assertEqual(self.checkouts, [])

    # @internal
    def test_the_native_hardware_shell_uses_the_same_contract_too(self):
        url = "https://gitlab.com/vauchi/linux-hw.git"
        self.manifest.write_text(self.original + f'vauchi-linux-hw = {{ git = "{url}", tag = "v0.103.0", version = "=0.103.0", features = ["audio", "camera"] }}\n')

        def checkout(url, tag, destination):
            if tag == "v0.6.0":
                return self.checkout(url, tag, destination)
            self.checkouts.append((url, tag))
            destination.mkdir(parents=True)
            (destination / "Cargo.toml").write_text('[package]\nname="vauchi-linux-hw"\nversion="0.103.0"\n[dependencies]\n' + OLD)

        POINT(self.core, self.manifest, checkout)
        dependencies = tomllib.loads(self.manifest.read_text())["dependencies"]
        hardware = dependencies["vauchi-linux-hw"]
        self.assertNotIn("git", hardware)
        self.assertEqual(hardware["version"], "=0.103.0")
        self.assertEqual(hardware["features"], ["audio", "camera"])
        self.assertEqual(self.contract(Path(hardware["path"]) / "Cargo.toml"), self.contract(self.manifest))
        self.assertEqual(self.checkouts, [(TUI, "v0.6.0"), (url, "v0.103.0")])

    # @internal
    def test_a_consumer_without_contract_types_stays_unchanged(self):
        self.manifest.write_text('[dependencies]\nserde="1"\n')
        before = self.manifest.read_bytes()
        POINT(self.core, self.manifest, self.checkout)
        self.assertEqual(self.manifest.read_bytes(), before)
        self.assertEqual(self.checkouts, [])

    # @internal
    def test_mutable_or_foreign_renderer_sources_fail_before_mutation(self):
        for altered in [DISPLAY.replace('tag = "v0.6.0"', 'branch = "main"'), DISPLAY.replace(TUI, "https://example.invalid/tui.git")]:
            with self.subTest(dependency=altered):
                self.manifest.write_text("[dependencies]\n" + OLD + altered)
                before = self.manifest.read_bytes()
                with self.assertRaises(ValueError):
                    POINT(self.core, self.manifest, self.checkout)
                self.assertEqual(self.manifest.read_bytes(), before)
                self.assertEqual(self.checkouts, [])

    # @internal
    def test_a_renderer_with_the_wrong_package_or_version_cannot_replace_the_pin(self):
        for renderer in [self.renderer.replace('name="vauchi-tui-display"', 'name="another-package"'), self.renderer.replace('version="0.6.0"', 'version="0.7.0"')]:
            with self.subTest(renderer=renderer):
                self.renderer = renderer
                self.manifest.write_text(self.original)
                with self.assertRaises(ValueError):
                    POINT(self.core, self.manifest, self.checkout)
                self.assertEqual(self.manifest.read_text(), self.original)

    # @internal
    def test_failed_checkout_leaves_the_consumer_untouched(self):
        def unavailable(*_):
            raise OSError("fixture checkout unavailable")

        with self.assertRaises(OSError):
            POINT(self.core, self.manifest, unavailable)
        self.assertEqual(self.manifest.read_text(), self.original)
        self.assertEqual(list(self.consumer.iterdir()), [self.manifest])

    # @internal
    def test_a_later_component_checkout_failure_preserves_the_consumer(self):
        self.manifest.write_text(self.original + 'vauchi-linux-hw = { git = "https://gitlab.com/vauchi/linux-hw.git", tag = "v0.103.0", version = "=0.103.0" }\n')
        before = self.manifest.read_bytes()

        def partially_available(url, tag, destination):
            if tag == "v0.6.0":
                return self.checkout(url, tag, destination)
            raise OSError("hardware checkout unavailable")

        with self.assertRaises(OSError):
            POINT(self.core, self.manifest, partially_available)
        self.assertEqual(self.manifest.read_bytes(), before)
        self.assertEqual(list(self.consumer.iterdir()), [self.manifest])

    # @internal
    def test_branch_only_contracts_cannot_become_a_test_cohort(self):
        (self.core / "Cargo.toml").write_text("[workspace.dependencies]\n" + NEW.replace('tag = "v0.102.0"', 'branch = "main"'))
        with self.assertRaises(ValueError):
            POINT(self.core, self.manifest, self.checkout)
        self.assertEqual(self.manifest.read_text(), self.original)
        self.assertEqual(self.checkouts, [])


if __name__ == "__main__":
    unittest.main()
