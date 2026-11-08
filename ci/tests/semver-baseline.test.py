#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "semver-baseline.sh"


class SemverBaselineContract(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name)
        self.git("init", "-q")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                 "commit", "--allow-empty", "-qm", "baseline")
        self.baseline = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("update-ref", "refs/remotes/origin/main", self.baseline)
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith("CI_MERGE_REQUEST_")}

    def git(self, *arguments):
        return subprocess.run(["git", "-C", str(self.repo), *arguments],
                              text=True, capture_output=True, check=True)

    def select(self, **variables):
        return subprocess.run(["sh", str(SCRIPT)], cwd=self.repo,
                              env=dict(self.env, **variables), text=True,
                              capture_output=True, check=False)

    def test_merged_result_and_train_keep_pipeline_target_when_main_advances(self):
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                 "commit", "--allow-empty", "-qm", "later API")
        later = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("update-ref", "refs/remotes/origin/main", later)
        self.assertNotEqual(later, self.baseline)
        for kind in ["merged_result", "merge_train"]:
            with self.subTest(kind=kind):
                result = self.select(CI_MERGE_REQUEST_IID="1",
                                     CI_MERGE_REQUEST_EVENT_TYPE=kind,
                                     CI_MERGE_REQUEST_TARGET_BRANCH_SHA=self.baseline,
                                     CI_MERGE_REQUEST_DIFF_BASE_SHA=later)
                self.assertEqual((result.returncode, result.stdout.strip()), (0, self.baseline))

    def test_detached_pipeline_uses_its_diff_base(self):
        result = self.select(CI_MERGE_REQUEST_IID="1", CI_MERGE_REQUEST_EVENT_TYPE="detached",
                             CI_MERGE_REQUEST_DIFF_BASE_SHA=self.baseline)
        self.assertEqual((result.returncode, result.stdout.strip()), (0, self.baseline))

    def test_missing_or_invalid_mr_metadata_never_falls_back_to_main(self):
        cases = [dict(CI_MERGE_REQUEST_EVENT_TYPE="merged_result"),
                 dict(CI_MERGE_REQUEST_EVENT_TYPE="detached"),
                 dict(CI_MERGE_REQUEST_EVENT_TYPE="unknown"),
                 dict(CI_MERGE_REQUEST_EVENT_TYPE="merged_result",
                      CI_MERGE_REQUEST_TARGET_BRANCH_SHA="origin/main"),
                 dict(CI_MERGE_REQUEST_EVENT_TYPE="merged_result",
                      CI_MERGE_REQUEST_TARGET_BRANCH_SHA="0" * 40),
                 dict(CI_MERGE_REQUEST_EVENT_TYPE="detached",
                      CI_MERGE_REQUEST_DIFF_BASE_SHA="g" * 40)]
        for variables in cases:
            with self.subTest(variables=variables):
                result = self.select(CI_MERGE_REQUEST_IID="1", **variables)
                self.assertEqual((result.returncode, result.stdout), (2, ""))
                self.assertIn("FAIL:", result.stderr)

    def test_non_mr_retains_fetched_main_baseline(self):
        result = self.select()
        self.assertEqual((result.returncode, result.stdout.strip()), (0, self.baseline))

    def test_unavailable_non_mr_baseline_is_not_a_clean_result(self):
        self.git("update-ref", "-d", "refs/remotes/origin/main")
        result = self.select()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
