# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import runpy
import tempfile
import unittest
from pathlib import Path

ALIGN = runpy.run_path(str(Path(__file__).resolve().parents[2] / "scripts/align-contract-pins.py"))["align_contract_pins"]
OLD = 'vauchi-contract = { git = "https://gitlab.com/vauchi/contract.git", tag = "v0.1.0", version = "=0.1.0" }\n'
NEW = OLD.replace("0.1.0", "0.97.2")


class ContractPinsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "consumer"
        self.root.mkdir()
        self.core = Path(self.temp.name) / "core.toml"
        self.core.write_text('[workspace.dependencies]\n' + NEW)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    # @internal
    def test_root_and_workspace_pins_move_together_and_second_run_is_empty(self):
        manifest = self.write("Cargo.toml", '[workspace]\nmembers=["display"]\n[dependencies]\n' + OLD)
        display = self.write("display/Cargo.toml", '[dependencies]\n' + OLD + 'serde = "1"\n')
        self.assertEqual(ALIGN(manifest, self.core), ["Cargo.toml", "display/Cargo.toml"])
        self.assertEqual(manifest.read_text(), '[workspace]\nmembers=["display"]\n[dependencies]\n' + NEW)
        self.assertEqual(display.read_text(), '[dependencies]\n' + NEW + 'serde = "1"\n')
        self.assertEqual(ALIGN(manifest, self.core), [])

    # @internal
    def test_globs_and_exclusions_leave_vendor_manifests_untouched(self):
        manifest = self.write("Cargo.toml", '[workspace]\nmembers=["crates/*"]\nexclude=["crates/excluded"]\n')
        included = self.write("crates/display/Cargo.toml", '[dependencies]\n' + OLD)
        excluded = self.write("crates/excluded/Cargo.toml", '[dependencies]\n' + OLD)
        vendor = self.write("vendor/Cargo.toml", '[dependencies]\n' + OLD)
        self.assertEqual(ALIGN(manifest, self.core), ["crates/display/Cargo.toml"])
        self.assertEqual(included.read_text(), '[dependencies]\n' + NEW)
        self.assertEqual(excluded.read_text(), '[dependencies]\n' + OLD)
        self.assertEqual(vendor.read_text(), '[dependencies]\n' + OLD)

    # @internal
    def test_escaping_members_fail_before_any_manifest_is_written(self):
        for member in ["../outside", "linked"]:
            with self.subTest(member=member):
                manifest = self.write("Cargo.toml", f'[workspace]\nmembers=["{member}"]\n[dependencies]\n' + OLD)
                outside = Path(self.temp.name) / "outside"
                outside.mkdir(exist_ok=True)
                target = outside / "Cargo.toml"
                target.write_text('[dependencies]\n' + OLD)
                link = self.root / "linked"
                if not link.exists():
                    link.symlink_to(outside, target_is_directory=True)
                before = manifest.read_text()
                with self.assertRaisesRegex(ValueError, "outside|escape"):
                    ALIGN(manifest, self.core)
                self.assertEqual(manifest.read_text(), before)
                self.assertEqual(target.read_text(), '[dependencies]\n' + OLD)

    # @internal
    def test_malformed_member_does_not_partially_update_the_root(self):
        manifest = self.write("Cargo.toml", '[workspace]\nmembers=["display"]\n[dependencies]\n' + OLD)
        self.write("display/Cargo.toml", 'not valid TOML')
        before = manifest.read_text()
        with self.assertRaises(ValueError):
            ALIGN(manifest, self.core)
        self.assertEqual(manifest.read_text(), before)

    # @internal
    def test_comments_and_unrelated_dependencies_are_preserved(self):
        text = '[dependencies]\n# ' + OLD + 'serde = "1"\n' + OLD.rstrip() + ' # old tag = "v0.1.0"\n'
        manifest = self.write("Cargo.toml", text)
        self.assertEqual(ALIGN(manifest, self.core), ["Cargo.toml"])
        self.assertEqual(manifest.read_text(), '[dependencies]\n# ' + OLD + 'serde = "1"\n' + NEW.rstrip() + ' # old tag = "v0.1.0"\n')

    # @internal
    def test_contract_urls_in_package_metadata_are_preserved(self):
        metadata = '[package.metadata.example]\n' + OLD
        manifest = self.write("Cargo.toml", '[dependencies]\n' + OLD + metadata)
        self.assertEqual(ALIGN(manifest, self.core), ["Cargo.toml"])
        self.assertEqual(manifest.read_text(), '[dependencies]\n' + NEW + metadata)

    # @internal
    def test_consumer_without_a_contract_dependency_is_unchanged(self):
        manifest = self.write("Cargo.toml", '[dependencies]\nserde = "1"\n')
        self.assertEqual(ALIGN(manifest, self.core), [])
        self.assertEqual(manifest.read_text(), '[dependencies]\nserde = "1"\n')

    # @internal
    def test_invalid_release_contract_tag_is_rejected(self):
        manifest = self.write("Cargo.toml", '[dependencies]\n' + OLD)
        self.core.write_text('[workspace.dependencies]\n' + NEW.replace('tag = "v0.97.2"', 'branch = "main"'))
        with self.assertRaisesRegex(ValueError, "tag"):
            ALIGN(manifest, self.core)
        self.assertEqual(manifest.read_text(), '[dependencies]\n' + OLD)


if __name__ == "__main__":
    unittest.main()
