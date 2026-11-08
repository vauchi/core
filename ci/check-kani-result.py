#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Validate one conditional Kani proof and its explicitly required witnesses."""

import argparse
import json
from pathlib import Path


def assess(document, harness, witnesses, backend_exit, version):
    metadata = document["metadata"]
    base = {"harness": harness, "backend_exit": backend_exit,
            "verifier_version": metadata.get("kani_version"), "target": metadata.get("target")}

    def result(verdict, reason):
        return dict(base, verdict=verdict, reason=reason)

    if metadata["version"] != "1.0" or metadata["kani_version"] != version:
        return result("inconclusive", "unsupported_verifier_contract")
    if not isinstance(metadata.get("target"), str) or not metadata["target"]:
        return result("inconclusive", "missing_target")
    verification = document["verification_results"]
    summary = verification["summary"]
    if summary["status"] != "completed":
        return result("inconclusive", "incomplete_verification")
    results = verification["results"]
    harnesses = document["harness_metadata"]
    if (summary["total_harnesses"] != 1 or summary["executed"] != 1
            or len(results) != 1 or len(harnesses) != 1
            or results[0]["harness_id"] != harness or harnesses[0]["pretty_name"] != harness):
        return result("invalid_harness", "wrong_or_empty_selection")
    proof = results[0]
    checks = proof["checks"]
    if not checks or not isinstance(checks, list):
        return result("inconclusive", "missing_property_results")
    if any(check["status"] not in ("Success", "Unreachable", "Failure", "Satisfied", "Unsatisfiable")
           for check in checks):
        return result("inconclusive", "unresolved_property")
    failed = [check for check in checks if check["status"] == "Failure"]
    if any(check["category"] == "unwind" for check in failed):
        return result("inconclusive", "unwinding_incomplete")
    if any(check["category"] == "unsupported_construct" for check in failed):
        return result("inconclusive", "unsupported_construct")
    if failed:
        if backend_exit == 1 and proof["status"] == "Failure" and summary["failed"] == 1:
            return result("counterexample", "property_violated")
        return result("inconclusive", "inconsistent_failure_result")
    if (backend_exit != 0 or proof["status"] != "Success"
            or summary["successful"] != 1 or summary["failed"] != 0):
        return result("inconclusive", "inconsistent_success_result")
    covers = [check for check in checks if check["category"] == "cover"]
    for witness in witnesses:
        matching = [check for check in covers if check["description"] == witness]
        if len(matching) != 1 or matching[0]["status"] != "Satisfied":
            return result("invalid_harness", "missing_or_unreachable_witness")
    if any(check["status"] != "Satisfied" for check in covers):
        return result("invalid_harness", "unreachable_cover")
    if any(check["status"] not in ("Success", "Unreachable")
           for check in checks if check["category"] != "cover"):
        return result("inconclusive", "unexpected_property_status")
    return dict(base, verdict="verified_conditional", witnesses=witnesses,
                source=harnesses[0]["source"]["file"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--harness", required=True)
    parser.add_argument("--backend-exit", type=int, required=True)
    parser.add_argument("--verifier-version", required=True)
    parser.add_argument("--require-cover", action="append", required=True)
    args = parser.parse_args()
    base = {"harness": args.harness, "backend_exit": args.backend_exit}
    if args.backend_exit not in (0, 1):
        report = dict(base, verdict="inconclusive", reason="backend_failed")
    else:
        try:
            document = json.loads(args.report.read_text())
            report = assess(document, args.harness, args.require_cover,
                            args.backend_exit, args.verifier_version)
        except (OSError, ValueError, KeyError, TypeError, IndexError):
            report = dict(base, verdict="inconclusive", reason="missing_or_invalid_report")
    print(json.dumps(report, sort_keys=True))
    return {"verified_conditional": 0, "counterexample": 1}.get(report["verdict"], 2)


if __name__ == "__main__":
    raise SystemExit(main())
