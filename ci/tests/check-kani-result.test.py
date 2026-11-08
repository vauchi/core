#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check-kani-result.py"
HARNESS = "crypto::padding_proofs::unpad_never_panics_and_only_accepts_a_prefixed_plaintext"
WITNESSES = ["maximum modeled input length is reachable", "accepted nonempty plaintext is reachable", "rejected input is reachable"]


def healthy_report():
    return {
        "metadata": {"version": "1.0", "kani_version": "0.68.0", "target": "x86_64-unknown-linux-gnu"},
        "harness_metadata": [{"pretty_name": HARNESS, "source": {"file": "vauchi-core/src/crypto/padding_proofs.rs"}}],
        "verification_results": {
            "summary": {"total_harnesses": 1, "executed": 1, "status": "completed", "successful": 1, "failed": 0},
            "results": [{"harness_id": HARNESS, "status": "Success", "checks": [
                {"category": "cover", "status": "Satisfied", "description": name} for name in WITNESSES
            ] + [{"category": "assertion", "status": "Success", "description": "padding format"},
                 {"category": "assertion", "status": "Unreachable", "description": "impossible panic"}]}],
        },
    }


class KaniResultContract(unittest.TestCase):
    def run_checker(self, document, backend_exit=0):
        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / "proof.json"
            if document is not None:
                report.write_text(document if isinstance(document, str) else json.dumps(document))
            command = [sys.executable, str(SCRIPT), str(report), "--harness", HARNESS,
                       "--backend-exit", str(backend_exit), "--verifier-version", "0.68.0"]
            for name in WITNESSES:
                command.extend(["--require-cover", name])
            outcome = subprocess.run(command, capture_output=True, text=True, check=False)
            return outcome.returncode, json.loads(outcome.stdout)

    def assert_verdict(self, document, expected, backend_exit=0):
        code, report = self.run_checker(document, backend_exit)
        self.assertEqual((code, report["verdict"], report["harness"]), (*expected, HARNESS))
        return report

    def test_missing_checker_trigger_cannot_disable_its_formal_job(self):
        root = SCRIPT.parent.parent
        configuration = (root / ".gitlab-ci.yml").read_text()
        for path in ["ci/check-kani-result.py", "ci/tests/check-kani-result.test.py"]:
            with self.subTest(path=path), tempfile.TemporaryDirectory() as temporary:
                modified = Path(temporary) / "ci.yml"
                modified.write_text("\n".join(line for line in configuration.splitlines() if line.strip() != "- " + path) + "\n")
                result = subprocess.run(["sh", "ci/tests/formal-triggers.test.sh", str(modified)],
                                        cwd=root, capture_output=True, text=True, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("test:formal-code does not trigger on " + path, result.stdout + result.stderr)

    def test_completed_proof_requires_all_witnesses_but_allows_unreachable_safety_checks(self):
        report = self.assert_verdict(healthy_report(), (0, "verified_conditional"))
        self.assertEqual(report["witnesses"], WITNESSES)
        self.assertEqual(report["verifier_version"], "0.68.0")
        self.assertEqual(report["target"], "x86_64-unknown-linux-gnu")

    def test_contradictory_assumptions_cannot_report_verified(self):
        document = healthy_report()
        document["verification_results"]["results"][0]["checks"][1]["status"] = "Unreachable"
        self.assert_verdict(document, (2, "invalid_harness"))

    def test_missing_or_duplicate_required_witness_is_invalid(self):
        for duplicate in [False, True]:
            document = healthy_report()
            checks = document["verification_results"]["results"][0]["checks"]
            if duplicate:
                checks.append(copy.deepcopy(checks[0]))
            else:
                checks.pop(0)
            with self.subTest(duplicate=duplicate):
                self.assert_verdict(document, (2, "invalid_harness"))

    def test_zero_executed_harnesses_and_wrong_selection_are_invalid(self):
        document = healthy_report()
        document["verification_results"]["summary"].update(total_harnesses=0, executed=0, successful=0)
        document["verification_results"]["results"] = []
        self.assert_verdict(document, (2, "invalid_harness"))
        document = healthy_report()
        document["verification_results"]["results"][0]["harness_id"] = "another_harness"
        self.assert_verdict(document, (2, "invalid_harness"))

    def test_actual_property_failure_is_distinct_from_backend_failure(self):
        document = healthy_report()
        document["verification_results"]["summary"].update(successful=0, failed=1)
        result = document["verification_results"]["results"][0]
        result["status"] = "Failure"
        result["checks"][-2]["status"] = "Failure"
        self.assert_verdict(document, (1, "counterexample"), backend_exit=1)

    def test_backend_errors_never_accept_an_otherwise_green_report(self):
        for status in [2, 124, 137]:
            with self.subTest(status=status):
                report = self.assert_verdict(healthy_report(), (2, "inconclusive"), backend_exit=status)
                self.assertEqual(report["backend_exit"], status)

    def test_missing_target_cannot_support_a_target_specific_proof(self):
        document = healthy_report()
        del document["metadata"]["target"]
        self.assert_verdict(document, (2, "inconclusive"))

    def test_unsupported_construct_is_a_model_limit_not_a_code_counterexample(self):
        document = healthy_report()
        document["verification_results"]["summary"].update(successful=0, failed=1)
        result = document["verification_results"]["results"][0]
        result["status"] = "Failure"
        result["checks"].append({"category": "unsupported_construct", "status": "Failure", "description": "not modeled"})
        self.assert_verdict(document, (2, "inconclusive"), backend_exit=1)

    def test_unwinding_failure_is_inconclusive(self):
        document = healthy_report()
        document["verification_results"]["summary"].update(successful=0, failed=1)
        result = document["verification_results"]["results"][0]
        result["status"] = "Failure"
        result["checks"].append({"category": "unwind", "status": "Failure", "description": "bound too small"})
        self.assert_verdict(document, (2, "inconclusive"), backend_exit=1)

    def test_missing_malformed_or_partial_results_are_inconclusive(self):
        for document in [None, "{", {}, {"verification_results": {}}]:
            with self.subTest(document=document):
                self.assert_verdict(document, (2, "inconclusive"))

    def test_changed_schema_or_verifier_version_is_not_silently_accepted(self):
        for key, value in [("version", "2.0"), ("kani_version", "0.69.0")]:
            document = healthy_report()
            document["metadata"][key] = value
            with self.subTest(key=key):
                self.assert_verdict(document, (2, "inconclusive"))

    def test_unknown_check_status_or_incomplete_summary_is_inconclusive(self):
        for summary in [False, True]:
            document = healthy_report()
            if summary:
                document["verification_results"]["summary"]["status"] = "incomplete"
            else:
                document["verification_results"]["results"][0]["checks"][-2]["status"] = "Undetermined"
            with self.subTest(summary=summary):
                self.assert_verdict(document, (2, "inconclusive"))


if __name__ == "__main__":
    unittest.main()
