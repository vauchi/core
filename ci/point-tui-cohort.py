#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Align the job-private TUI assembly with a candidate Core's released contract."""

import argparse
import json
import re
import runpy
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path


def _aligner():
    beside = Path(__file__).with_name("align-contract-pins.py")
    source = beside if beside.is_file() else Path(__file__).resolve().parents[1] / "scripts/align-contract-pins.py"
    return runpy.run_path(str(source))


def checkout_release(url, tag, destination):
    subprocess.run(["git", "init", "--quiet", str(destination)], check=True)
    subprocess.run(["git", "-C", str(destination), "fetch", "--quiet", "--depth=1", url, f"refs/tags/{tag}"], check=True)
    subprocess.run(["git", "-C", str(destination), "checkout", "--quiet", "--detach", "FETCH_HEAD"], check=True)


def _point_dependency(text, name, dependency, split_comment):
    result = []
    in_dependencies = False
    matches = 0
    for line in text.splitlines(keepends=True):
        code, comment = split_comment(line)
        if code.strip().startswith("["):
            in_dependencies = code.strip() == "[dependencies]"
        if in_dependencies and re.match(r"^\s*" + re.escape(name) + r"\s*=", code):
            if not re.fullmatch(r"\s*" + re.escape(name) + r"\s*=\s*\{.*\}\s*", code):
                raise ValueError(f"{name} dependency must use a one-line inline table")
            fields = ", ".join(f"{key} = {json.dumps(value)}" for key, value in dependency.items())
            ending = " " + comment if comment else "\n" if line.endswith("\n") else ""
            line = name + " = { " + fields + " }" + ending
            matches += 1
        result.append(line)
    if matches != 1:
        raise ValueError(f"expected one {name} dependency in the consumer")
    rewritten = "".join(result)
    tomllib.loads(rewritten)
    return rewritten


def point_tui_cohort(core_dir, manifest, checkout=checkout_release):
    core_manifest = Path(core_dir) / "Cargo.toml"
    manifest = Path(manifest)
    helpers = _aligner()
    helpers["_release_tag"](core_manifest)
    helpers["_manifests"](manifest)
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    components = []
    for name, repository in [("tui-display", "tui-display"), ("vauchi-linux-hw", "linux-hw")]:
        dependency = data.get("dependencies", {}).get(name)
        if dependency is None:
            continue
        url = f"https://gitlab.com/vauchi/{repository}"
        if not isinstance(dependency, dict) or dependency.get("git") not in {url, url + ".git"} or not re.fullmatch(r"v\d+\.\d+\.\d+", dependency.get("tag", "")):
            raise ValueError(f"TUI test assembly requires canonical {name} at a release tag")
        if dependency.get("version") != "=" + dependency["tag"][1:]:
            raise ValueError(f"{name} tag and exact version disagree")
        allowed = {"package", "git", "tag", "version", "optional", "default-features", "features"}
        if dependency.keys() - allowed:
            raise ValueError(f"unsupported {name} dependency attributes")
        local = {key: value for key, value in dependency.items() if key not in {"git", "tag"}}
        local["path"] = "fixture-validation"
        _point_dependency(manifest.read_text(encoding="utf-8"), name, local, helpers["_comment"])
        components.append((name, dependency, local))
    if not components:
        helpers["align_contract_pins"](manifest, core_manifest)
        return
    owned = Path(tempfile.mkdtemp(prefix=".ci-tui-cohort-", dir=manifest.parent))
    try:
        for name, dependency, local in components:
            component = owned / name
            checkout(dependency["git"], dependency["tag"], component)
            component_manifest = component / "Cargo.toml"
            package = tomllib.loads(component_manifest.read_text(encoding="utf-8")).get("package", {})
            if package.get("name") != dependency.get("package", name) or package.get("version") != dependency["tag"][1:]:
                raise ValueError(f"tagged {name} package or version disagrees with its pin")
            helpers["align_contract_pins"](component_manifest, core_manifest)
            local["path"] = str(component.resolve())
        helpers["align_contract_pins"](manifest, core_manifest)
        text = manifest.read_text(encoding="utf-8")
        for name, _, local in components:
            text = _point_dependency(text, name, local, helpers["_comment"])
        manifest.write_text(text, encoding="utf-8")
    except Exception:
        shutil.rmtree(owned)
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("core_dir", type=Path)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    try:
        point_tui_cohort(args.core_dir, args.manifest)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"TUI test cohort: {error}", file=sys.stderr)
        sys.exit(2)
