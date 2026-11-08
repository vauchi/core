#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""The module guard distinguishes violations from incomplete searches."""

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check-module-boundaries.sh"
BASH = shutil.which("bash")


class ModuleBoundaryGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "vauchi-core/src/storage/one.rs"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("use crate::crypto::SymmetricKey;\n")
        self.bin = self.root / "bin"
        self.bin.mkdir()

    def run_guard(self, path=None, cwd=None):
        env = dict(os.environ)
        if path is not None:
            env["PATH"] = path
        return subprocess.run([BASH, str(SCRIPT)], cwd=cwd or self.root,
                              env=env, capture_output=True, text=True)

    def fail_search(self, partial=False):
        stub = self.bin / "grep"
        stub.write_text("#!/bin/sh\n" + (
            "echo 'vauchi-core/src/storage/one.rs:1:use crate::api::Vauchi;'\n"
            if partial else "") + "echo 'injected search failure' >&2\nexit 2\n")
        stub.chmod(0o755)
        return str(self.bin) + os.pathsep + os.environ["PATH"]

    def test_clean_source_with_absent_optional_modules_succeeds(self):
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Module boundaries OK", result.stdout)

    def test_forbidden_import_fails_with_source_location(self):
        self.source.write_text("use crate::api::Vauchi;\n")
        result = self.run_guard()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("VIOLATION", result.stdout)
        self.assertIn("storage/one.rs:1:", result.stdout)

    def test_search_failure_propagates_and_cannot_report_clean(self):
        self.source.write_text("use crate::api::Vauchi;\n")
        result = self.run_guard(path=self.fail_search())
        self.assertEqual(result.returncode, 2)
        self.assertIn("injected search failure", result.stderr)
        self.assertNotIn("Module boundaries OK", result.stdout)

    def test_partial_search_output_does_not_hide_backend_failure(self):
        result = self.run_guard(path=self.fail_search(partial=True))
        self.assertEqual(result.returncode, 2)
        self.assertIn("injected search failure", result.stderr)
        self.assertNotIn("Module boundaries OK", result.stdout)

    def test_missing_source_tree_is_not_a_completed_check(self):
        empty = self.root / "empty"
        empty.mkdir()
        result = self.run_guard(cwd=empty)
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("Module boundaries OK", result.stdout)

    def test_source_tree_without_any_rule_targets_is_not_a_completed_check(self):
        self.source.unlink()
        result = self.run_guard()
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("Module boundaries OK", result.stdout)

    def test_missing_search_tool_is_an_error(self):
        result = self.run_guard(path=str(self.bin))
        self.assertEqual(result.returncode, 127)
        self.assertNotIn("Module boundaries OK", result.stdout)


if __name__ == "__main__":
    unittest.main()
