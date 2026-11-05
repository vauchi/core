# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Publishable Core journey data; consumers select an immutable Core revision."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tomllib
from urllib.request import HTTPRedirectHandler, Request, build_opener
from urllib.error import HTTPError

FILES = ("presentation_contract_v1.json", "screen_catalog_v1.json", "screen_catalog_inventory_v1.json")
MAX_BYTES = 8 * 1024 * 1024
SHA = r"[a-f0-9]{40}"
VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.]+)?"


class FixtureError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise FixtureError(message)


def revision(value):
    require(isinstance(value, str) and re.fullmatch(SHA, value), "Core revision must be a full immutable SHA")
    return value


def read(path):
    with path.open("rb") as stream:
        raw = stream.read(MAX_BYTES + 1)
    require(len(raw) <= MAX_BYTES, "fixture exceeds byte budget")
    return raw


def document(raw):
    require(isinstance(raw, bytes) and len(raw) <= MAX_BYTES, "fixture exceeds byte budget")
    try:
        value = json.loads(raw)
    except (ValueError, RecursionError) as error:
        raise FixtureError("invalid fixture JSON") from error
    require(type(value) is dict, "fixture document must be an object")
    return value


def digest(raw):
    return {"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}


def journey_document(raw):
    value = document(raw)
    require(type(value.get("schema_version")) is int and value["schema_version"] == 1,
            "unsupported journey schema")
    return value


def message(value):
    return (isinstance(value, str) and bool(value)) or (
        type(value) is dict and len(value) == 1 and all(value)
    )


def commands(value, nonempty=False):
    return type(value) is list and (bool(value) or not nonempty) and all(map(message, value))


def validate_journeys(blobs):
    """Check category envelopes and screen coverage; Core CI owns typed decoding."""
    documents = {name: journey_document(blobs[name]) for name in FILES}
    presentation = documents["presentation_contract_v1.json"]
    require(commands(presentation.get("initial_commands"))
            and commands(presentation.get("effect_commands"))
            and type(presentation.get("file_picker_contract")) is dict
            and type(presentation.get("expected_state")) is dict,
            "invalid presentation journey envelope")
    steps = presentation.get("steps")
    require(type(steps) is list, "journey steps must be an array")
    for step in steps:
        require(type(step) is dict and message(step.get("event"))
                and commands(step.get("commands")), "invalid journey step")

    screens = documents["screen_catalog_v1.json"].get("screens")
    require(type(screens) is list and bool(screens), "catalog must contain screens")
    actual = set()
    for screen in screens:
        require(type(screen) is dict and isinstance(screen.get("code_id"), str)
                and bool(screen["code_id"].strip())
                and isinstance(screen.get("title"), str)
                and isinstance(screen.get("locale"), str)
                and commands(screen.get("commands"), nonempty=True), "invalid catalog screen")
        require(screen["code_id"] not in actual, "duplicate catalog screen id")
        actual.add(screen["code_id"])

    expected = documents["screen_catalog_inventory_v1.json"].get("screen_ids")
    require(type(expected) is list and bool(expected)
            and all(isinstance(value, str) and bool(value.strip()) for value in expected),
            "screen inventory must contain ids")
    require(len(set(expected)) == len(expected), "duplicate screen inventory id")
    require(actual == set(expected), "catalog and screen inventory disagree")


def install(destination, blobs):
    destination.mkdir(parents=True, exist_ok=True)
    for name, raw in blobs.items():
        (destination / name).write_bytes(raw)


def build(root, destination, core_revision):
    revision(core_revision)
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    dependency = manifest["workspace"]["dependencies"]["vauchi-contract"]
    packages = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    contracts = [p for p in packages if p["name"] == "vauchi-contract"]
    require(len(contracts) == 1, "exactly one resolved contract is required")
    contract = contracts[0]
    match = re.fullmatch(r"git\+https://gitlab.com/vauchi/contract.git\?tag=(v" + VERSION + r")#(" + SHA + ")", contract.get("source", ""))
    require(match is not None, "contract must resolve to a released Git revision, not a path override")
    require(dependency.get("git") == "https://gitlab.com/vauchi/contract.git"
            and dependency.get("tag") == match[1] and contract["version"] == match[1][1:],
            "manifest and resolved contract disagree")
    blobs = {name: read(root / "vauchi-app/fixtures/journeys" / name) for name in FILES}
    validate_journeys(blobs)
    metadata = {
        "artifact_schema_version": 1, "core_revision": core_revision,
        "core_version": manifest["workspace"]["package"]["version"],
        "contract": {"version": contract["version"], "tag": match[1], "revision": match[2]},
        "files": {name: digest(raw) for name, raw in blobs.items()},
    }
    blobs["manifest.json"] = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
    install(destination, blobs)
    return metadata


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise FixtureError("artifact redirect refused")


def transfer(core_revision, filename, payload=None):
    revision(core_revision)
    require(filename in (*FILES, "manifest.json"), "unexpected artifact filename")
    api = os.environ.get("CI_API_V4_URL", "https://gitlab.com/api/v4").rstrip("/")
    require(api.startswith("https://"), "artifact API requires HTTPS")
    url = f"{api}/projects/vauchi%2Fcore/packages/generic/core-journey-fixtures/{core_revision}/{filename}"
    headers = {"JOB-TOKEN": os.environ["CI_JOB_TOKEN"]} if os.environ.get("CI_JOB_TOKEN") else {}
    request = Request(url, headers=headers, data=payload, method="GET" if payload is None else "PUT")
    with build_opener(NoRedirect).open(request, timeout=30) as response:
        raw = response.read(MAX_BYTES + 1)
    require(len(raw) <= MAX_BYTES, "artifact exceeds byte budget")
    return raw


def download(core_revision, filename):
    return transfer(core_revision, filename)


def existing(core_revision, filename):
    try:
        return download(core_revision, filename)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise


def checked(core_revision, contract_version, request):
    revision(core_revision)
    require(re.fullmatch(VERSION, contract_version), "required contract version must be exact")
    raw = request(core_revision, "manifest.json")
    metadata = document(raw)
    require(type(metadata.get("artifact_schema_version")) is int
            and metadata["artifact_schema_version"] == 1, "unsupported artifact schema")
    require(metadata.get("core_revision") == core_revision, "Core revision mismatch")
    contract = metadata.get("contract", {})
    require(type(contract) is dict and contract.get("version") == contract_version
            and contract.get("tag") == "v" + contract_version
            and isinstance(contract.get("revision"), str)
            and re.fullmatch(SHA, contract["revision"]), "required contract mismatch")
    entries = metadata.get("files")
    require(type(entries) is dict and set(entries) == set(FILES), "artifact file inventory mismatch")
    blobs = {"manifest.json": raw}
    for name in FILES:
        expected = entries[name]
        require(type(expected) is dict and type(expected.get("bytes")) is int
                and 0 < expected["bytes"] <= MAX_BYTES, "invalid artifact byte budget")
        payload = request(core_revision, name)
        require(isinstance(payload, bytes) and len(payload) <= MAX_BYTES
                and digest(payload) == expected, "artifact integrity mismatch")
        blobs[name] = payload
    validate_journeys(blobs)
    return metadata, blobs


def fetch(destination, core_revision, contract_version, request=download):
    metadata, blobs = checked(core_revision, contract_version, request)
    install(destination, blobs)
    return metadata


def fetch_assembly(destination, lockfile, request=download):
    packages = tomllib.loads(lockfile.read_text())["package"]
    cores = [p for p in packages if p["name"] == "vauchi-core"]
    contracts = [p for p in packages if p["name"] == "vauchi-contract"]
    require(len(cores) == len(contracts) == 1, "assembly must lock one Core and one contract")
    core = re.fullmatch(r"git\+https://gitlab.com/vauchi/core.git\?[^#]+#(" + SHA + ")",
                        cores[0].get("source", ""))
    contract = re.fullmatch(r"git\+https://gitlab.com/vauchi/contract.git\?[^#]+#(" + SHA + ")",
                            contracts[0].get("source", ""))
    require(core is not None and contract is not None, "assembly requires immutable Git pins")
    metadata, blobs = checked(core[1], contracts[0]["version"], request)
    require(metadata["contract"]["revision"] == contract[1]
            and metadata.get("core_version") == cores[0]["version"], "assembly revision mismatch")
    install(destination, blobs)
    return metadata


def publish(directory, core_revision, get=existing, put=transfer):
    blobs = {name: read(directory / name) for name in (*FILES, "manifest.json")}
    metadata = document(blobs["manifest.json"])
    checked(core_revision, metadata["contract"]["version"], lambda rev, name: blobs[name])
    for name in (*FILES, "manifest.json"):
        previous = get(core_revision, name)
        if previous is None:
            put(core_revision, name, blobs[name])
        else:
            require(previous == blobs[name], "immutable artifact revision already has different bytes")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("build", "fetch", "fetch-assembly", "publish"))
    parser.add_argument("output", type=Path)
    parser.add_argument("--core-revision")
    parser.add_argument("--assembly-lock", type=Path)
    parser.add_argument("--contract-version")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        require(args.operation == "fetch-assembly" or args.core_revision is not None,
                "an explicit Core revision is required")
        if args.operation == "build":
            build(args.root, args.output, args.core_revision)
        elif args.operation == "fetch":
            require(args.contract_version is not None, "fetch requires an exact contract version")
            fetch(args.output, args.core_revision, args.contract_version)
        elif args.operation == "fetch-assembly":
            require(args.assembly_lock is not None, "fetch-assembly requires a pinned lockfile")
            fetch_assembly(args.output, args.assembly_lock)
        else:
            publish(args.output, args.core_revision)
    except (FixtureError, OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, "journey fixtures: " + (str(error) if isinstance(error, FixtureError)
                                             else "artifact operation failed") + "\n")


if __name__ == "__main__":
    main()
