# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import importlib.util
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[2] / "scripts/retire-swift-hardware.py"
MANIFEST = '''let version = "0.109.0"
let useLocalXCFramework = true
let vauchiPlatformProduct = "keep platform"
let coreUIModelsProduct = "keep models"
/// Shared Apple-platform hardware services.
/// Depends on the generated events.
let vauchiHardwareProduct: PackageDescription.Product = .library(
    name: "VauchiHardware",
    targets: ["VauchiHardware"]
)
let vauchiHardwareTarget: PackageDescription.Target = .target(
    name: "VauchiHardware",
    dependencies: ["VauchiPlatform"],
    path: "Sources/VauchiHardware"
)
let products = [vauchiPlatformProduct, vauchiHardwareProduct, coreUIModelsProduct]
let targets = [vauchiPlatformTarget, vauchiHardwareTarget, coreUIModelsTarget, binaryTarget]
'''


class RetireSwiftHardwareTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.package = self.root / "package"
        self.package.mkdir()
        self.manifest = self.package / "Package.swift"
        self.manifest.write_text(MANIFEST)
        self.hardware = self.package / "Sources/VauchiHardware"
        self.hardware.mkdir(parents=True)
        (self.hardware / "LocationService.swift").write_text("legacy location")
        self.bindings = self.package / "Sources/VauchiPlatform/bindings.swift"
        self.bindings.parent.mkdir()
        self.bindings.write_text("keep bindings")

    def retire(self):
        return subprocess.run([sys.executable, str(SCRIPT), str(self.package)],
                              capture_output=True, text=True, check=False)

    def test_retires_hardware_and_preserves_bindings_and_package_settings(self):
        result = self.retire()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = self.manifest.read_text()
        self.assertNotIn("VauchiHardware", manifest)
        self.assertNotIn("vauchiHardware", manifest)
        self.assertNotIn("Shared Apple-platform hardware services", manifest)
        self.assertIn('let version = "0.109.0"', manifest)
        self.assertIn("let useLocalXCFramework = true", manifest)
        self.assertIn("[vauchiPlatformProduct, coreUIModelsProduct]", manifest)
        self.assertIn("[vauchiPlatformTarget, coreUIModelsTarget, binaryTarget]", manifest)
        self.assertFalse(self.hardware.exists())
        self.assertEqual(self.bindings.read_text(), "keep bindings")

    def test_later_releases_are_idempotent(self):
        self.assertEqual(self.retire().returncode, 0)
        manifest = self.manifest.read_text()
        result = self.retire()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.manifest.read_text(), manifest)
        self.assertEqual(self.bindings.read_text(), "keep bindings")

    def test_unknown_manifest_layout_fails_without_partially_retiring_the_module(self):
        manifest = MANIFEST.replace("let vauchiHardwareProduct:", "let unexpectedHardwareProduct:")
        self.manifest.write_text(manifest)
        result = self.retire()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported VauchiHardware manifest layout", result.stderr)
        self.assertEqual(self.manifest.read_text(), manifest)
        self.assertEqual((self.hardware / "LocationService.swift").read_text(), "legacy location")

    def test_later_removal_failure_restores_manifest_and_partially_deleted_source(self):
        spec = importlib.util.spec_from_file_location("retirement", SCRIPT)
        retirement = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(retirement)
        original_rmtree = shutil.rmtree

        def failing_removal(path, *args, **kwargs):
            if Path(path) == self.hardware:
                (self.hardware / "LocationService.swift").unlink()
                raise OSError("injected partial removal failure")
            return original_rmtree(path, *args, **kwargs)

        with patch.object(retirement.shutil, "rmtree", side_effect=failing_removal):
            with self.assertRaisesRegex(OSError, "injected partial removal failure"):
                retirement.retire(self.package)

        self.assertEqual(self.manifest.read_text(), MANIFEST)
        self.assertEqual((self.hardware / "LocationService.swift").read_text(), "legacy location")
        self.assertEqual(self.bindings.read_text(), "keep bindings")

    def test_source_symlink_is_rejected_without_touching_its_target_or_manifest(self):
        (self.hardware / "LocationService.swift").unlink()
        self.hardware.rmdir()
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "keep.swift").write_text("keep outside")
        self.hardware.symlink_to(outside, target_is_directory=True)
        result = self.retire()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symlink", result.stderr)
        self.assertEqual(self.manifest.read_text(), MANIFEST)
        self.assertEqual((outside / "keep.swift").read_text(), "keep outside")
        self.assertTrue(self.hardware.is_symlink())


if __name__ == "__main__":
    unittest.main()
