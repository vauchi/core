#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Holds Core's empty cucumber `then` bodies at a ratcheted baseline.

An empty `then` step lets a Gherkin scenario pass without checking its
assertion (vauchi/private#862). Each empty body above the baseline fails
here; an MR that implements or removes some lowers the baseline in the
same change, or this fails too. Empty given/when bodies are prose no-ops
and do not count — only the assertion position is ratcheted.

Usage: check-empty-then-steps.py [core_root]
Exit 0 at the baseline, 1 above or below it, 2 on a usage error.
"""

import re
import sys
from pathlib import Path

THEN_ATTR = re.compile(r"^\s*#\[then\(")
ANY_ATTR = re.compile(r"^\s*#\[")
# A step fn whose body holds only whitespace and // comments. Anchored at
# the fn line: attributes and blank lines between the attribute and the fn
# are skipped by the caller, so a match here is the step's own body.
EMPTY_FN = re.compile(
    r"(?:pub\s+)?fn\s+\w+\s*\([^)]*\)\s*(?:->[^{\n]+?)?"
    r"\{(?:\s|//[^\n]*\n)*\}"
)


def empty_then_count(root: Path) -> int:
    count = 0
    for step_file in sorted(root.glob("vauchi-*/tests/steps/*.rs")):
        lines = step_file.read_text(encoding="utf-8").splitlines(keepends=True)
        for i, line in enumerate(lines):
            if not THEN_ATTR.match(line):
                continue
            j = i + 1
            while j < len(lines) and (
                ANY_ATTR.match(lines[j]) or not lines[j].strip()
            ):
                j += 1
            if EMPTY_FN.match("".join(lines[j : j + 12])):
                count += 1
    return count


def main() -> int:
    if len(sys.argv) > 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = (
        Path(sys.argv[1])
        if len(sys.argv) == 2
        else Path(__file__).resolve().parent.parent
    )
    baseline_file = root / ".empty-then-steps-baseline"
    if not baseline_file.is_file():
        print(
            f"check-empty-then-steps: {baseline_file} is not a file", file=sys.stderr
        )
        return 2
    try:
        content = "\n".join(
            line
            for line in baseline_file.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.strip().startswith("#")
        )
        baseline = int(content.strip())
    except ValueError:
        print(
            f"check-empty-then-steps: {baseline_file} must hold one number",
            file=sys.stderr,
        )
        return 2

    count = empty_then_count(root)
    if count == baseline:
        print(f"check-empty-then-steps: {count} empty then bodies, at baseline")
        return 0
    if count > baseline:
        print(
            f"check-empty-then-steps: {count} empty then bodies, above the "
            f"baseline {baseline}. An empty then asserts nothing: implement "
            "the assertion or delete the step.",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-empty-then-steps: {count} empty then bodies, below the "
        f"baseline {baseline} — lower .empty-then-steps-baseline to {count} "
        "in the same change."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
