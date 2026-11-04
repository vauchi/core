#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Align a consumer workspace's existing contract pins with released Core."""

import argparse
import re
import sys
import tomllib
from pathlib import Path

CONTRACT_URLS = {"https://gitlab.com/vauchi/contract", "https://gitlab.com/vauchi/contract.git"}
GIT = re.compile(r"\bgit\s*=\s*(['\"])https://gitlab\.com/vauchi/contract(?:\.git)?\1")
TAG = re.compile(r"\btag\s*=\s*(['\"])[^'\"]+\1")
BRANCH = re.compile(r"\bbranch\s*=\s*(['\"])[^'\"]+\1")
VERSION = re.compile(r"\bversion\s*=\s*(['\"])[^'\"]+\1")


def _table(data, name):
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be a table")
    return value


def _release_tag(core_manifest):
    data = tomllib.loads(core_manifest.read_text(encoding="utf-8"))
    dependency = _table(_table(_table(data, "workspace"), "dependencies"), "vauchi-contract")
    tag = dependency.get("tag", "")
    if dependency.get("git") not in CONTRACT_URLS or not isinstance(tag, str) or not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        raise ValueError("released Core must pin the canonical contract to a release tag")
    return tag


def _strings(data, name):
    values = data.get(name, [])
    if not isinstance(values, list) or not all(isinstance(value, str) for value in values):
        raise ValueError(f"workspace.{name} must be an array of paths")
    return values


def _matches(root, pattern):
    if Path(pattern).is_absolute() or ".." in Path(pattern).parts:
        raise ValueError(f"workspace path can escape outside the clone: {pattern}")
    paths = sorted(root.glob(pattern))
    for path in paths:
        if not path.resolve().is_relative_to(root):
            raise ValueError(f"workspace path points outside the clone: {pattern}")
    return paths


def _manifests(manifest):
    root = manifest.parent.resolve()
    if not manifest.resolve().is_relative_to(root):
        raise ValueError("root manifest points outside the clone")
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    workspace = _table(data, "workspace")
    excluded = {path.resolve() for pattern in _strings(workspace, "exclude") for path in _matches(root, pattern)}
    paths = {manifest.resolve()}
    for pattern in _strings(workspace, "members"):
        matches = _matches(root, pattern)
        if not matches:
            raise ValueError(f"workspace member has no manifest: {pattern}")
        for member in matches:
            if member.resolve() not in excluded:
                candidate = member / "Cargo.toml"
                if not candidate.resolve().is_relative_to(root):
                    raise ValueError(f"member manifest points outside the clone: {pattern}")
                paths.add(candidate.resolve())
    return root, sorted(paths)


def _comment(line):
    quote = None
    escaped = False
    for index, char in enumerate(line):
        if escaped:
            escaped = False
        elif quote == '"' and char == "\\":
            escaped = True
        elif quote:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char == "#":
            return line[:index], line[index:]
    return line, ""


def _rewrite(text, tag):
    tomllib.loads(text)
    result = []
    dependency_table = False
    for line in text.splitlines(keepends=True):
        code, comment = _comment(line)
        if code.lstrip().startswith("["):
            table = tomllib.loads(code)
            parts = []
            while isinstance(table, dict) and len(table) == 1:
                name, table = next(iter(table.items()))
                parts.append(name)
            kinds = {"dependencies", "dev-dependencies", "build-dependencies"}
            dependency_table = bool(parts) and (
                parts[0] in kinds
                or parts[:2] == ["workspace", "dependencies"]
                or len(parts) >= 3 and parts[0] == "target" and parts[2] in kinds
            )
        if dependency_table and GIT.search(code):
            if "{" not in code or "}" not in code:
                raise ValueError("contract dependencies must use one-line inline tables")
            pin = TAG if TAG.search(code) else BRANCH
            if not pin.search(code):
                raise ValueError("contract dependency has no tag or branch to align")
            code = pin.sub(f'tag = "{tag}"', code, count=1)
            code = VERSION.sub(f'version = "={tag[1:]}"', code, count=1)
        result.append(code + comment)
    return "".join(result)


def align_contract_pins(manifest, core_manifest):
    tag = _release_tag(core_manifest)
    root, manifests = _manifests(manifest)
    updates = []
    for path in manifests:
        before = path.read_text(encoding="utf-8")
        after = _rewrite(before, tag)
        if after != before:
            updates.append((path, after))
    # Validate every member before touching the clone (DC-03).
    for path, text in updates:
        path.write_text(text, encoding="utf-8")
    return [str(path.relative_to(root)) for path, _ in updates]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--core-manifest", type=Path, default=Path(__file__).resolve().parents[1] / "Cargo.toml")
    args = parser.parse_args()
    try:
        for changed in align_contract_pins(args.manifest, args.core_manifest):
            print(changed)
    except (ValueError, OSError) as error:
        print(f"contract-pins: {error}", file=sys.stderr)
        sys.exit(2)
