#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The pure logic behind scripts/open-follow-up-mrs.sh (vauchi/private#1010).

When Core merges to main, two repositories owe a follow-up: the spec drops
the `name core!NNNN` lines of ci/claim-names.pending whose MR landed (its
ratchet fails them otherwise), and features flips the scenarios whose
annotated test landed from @planned to @implemented. The shell script
clones and pushes; this file decides what changes.

    scenarios-added          stdin: a unified diff; prints "stem :: name" per added annotation
    merged-iids FILE...      GitLab merge_requests JSON; prints the iids in state merged
    drop-pending FILE --iid N...     rewrites FILE; prints each removed name
    flip DIR --scenarios-file FILE   rewrites DIR/<stem>.feature; prints flipped and missing
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ANNOTATION_RE = re.compile(r"^\+(?!\+\+).*//\s*@scenario:\s*([a-z0-9_]+)\s*::\s*(.+?)\s*$")
SCENARIO_LINE_RE = re.compile(r"^\s*Scenario(?: Outline)?:\s*(.+?)\s*$")
TAG_PLANNED_RE = re.compile(r"(?<![\w-])@planned(?![\w-])")


def scenarios_added(diff: str) -> list[tuple[str, str]]:
    seen: list[tuple[str, str]] = []
    for line in diff.splitlines():
        m = ANNOTATION_RE.match(line)
        if m and (m.group(1), m.group(2)) not in seen:
            seen.append((m.group(1), m.group(2)))
    return seen


def merged_iids(payloads: list[str]) -> list[int]:
    iids: set[int] = set()
    for text in payloads:
        data = json.loads(text or "[]")
        for mr in data if isinstance(data, list) else []:
            if mr.get("state") == "merged" and isinstance(mr.get("iid"), int):
                iids.add(mr["iid"])
    return sorted(iids)


def pending_without(text: str, iids: set[int]) -> tuple[str, list[str]]:
    kept, removed = [], []
    for line in text.splitlines(keepends=True):
        parts = line.split()
        if (len(parts) == 2 and parts[1].startswith("core!") and parts[1][5:].isdigit()
                and int(parts[1][5:]) in iids):
            removed.append(parts[0])
        else:
            kept.append(line)
    return "".join(kept), removed


def flip_scenarios(text: str, names: set[str]) -> tuple[str, list[str]]:
    """@planned becomes @implemented on the tag line directly above a named scenario."""
    lines = text.splitlines(keepends=True)
    flipped: list[str] = []
    for i, line in enumerate(lines):
        m = SCENARIO_LINE_RE.match(line)
        if not m or m.group(1) not in names or i == 0:
            continue
        tags = lines[i - 1]
        if tags.lstrip().startswith("@") and TAG_PLANNED_RE.search(tags):
            lines[i - 1] = TAG_PLANNED_RE.sub("@implemented", tags, count=1)
            flipped.append(m.group(1))
    return "".join(lines), flipped


def _cmd_flip(directory: Path, pairs: list[tuple[str, str]]) -> int:
    by_stem: dict[str, set[str]] = {}
    for stem, name in pairs:
        by_stem.setdefault(stem, set()).add(name)
    for stem, names in sorted(by_stem.items()):
        path = directory / f"{stem}.feature"
        flipped: list[str] = []
        if path.is_file():
            text = path.read_text(encoding="utf-8")
            new, flipped = flip_scenarios(text, names)
            if new != text:
                path.write_text(new, encoding="utf-8")
        for name in sorted(names):
            state = "flipped" if name in flipped else "not planned in features"
            print(f"{state}: {stem} :: {name}")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("scenarios-added")
    p = sub.add_parser("merged-iids")
    p.add_argument("files", nargs="+", type=Path)
    p = sub.add_parser("drop-pending")
    p.add_argument("file", type=Path)
    p.add_argument("--iid", nargs="+", type=int, required=True)
    p = sub.add_parser("flip")
    p.add_argument("dir", type=Path)
    p.add_argument("--scenarios-file", type=Path, required=True)
    a = ap.parse_args(argv)
    if a.cmd == "scenarios-added":
        for stem, name in scenarios_added(sys.stdin.read()):
            print(f"{stem} :: {name}")
        return 0
    if a.cmd == "merged-iids":
        print(" ".join(str(i) for i in merged_iids([f.read_text(encoding="utf-8") for f in a.files])))
        return 0
    if a.cmd == "drop-pending":
        text = a.file.read_text(encoding="utf-8")
        new, removed = pending_without(text, set(a.iid))
        if new != text:
            a.file.write_text(new, encoding="utf-8")
        for name in removed:
            print(name)
        return 0
    pairs = []
    for line in a.scenarios_file.read_text(encoding="utf-8").splitlines():
        if " :: " in line:
            stem, name = line.split(" :: ", 1)
            pairs.append((stem.strip(), name.strip()))
    return _cmd_flip(a.dir, pairs)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
