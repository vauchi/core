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
    for raw in blobs.values():
        journey_document(raw)
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
        journey_document(payload)
        blobs[name] = payload
    return metadata, blobs


def fetch(destination, core_revision, contract_version, request=download):
    metadata, blobs = checked(core_revision, contract_version, request)
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
    parser.add_argument("operation", choices=("build", "fetch", "publish"))
    parser.add_argument("output", type=Path)
    parser.add_argument("--core-revision", required=True)
    parser.add_argument("--contract-version")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        if args.operation == "build":
            build(args.root, args.output, args.core_revision)
        elif args.operation == "fetch":
            require(args.contract_version is not None, "fetch requires an exact contract version")
            fetch(args.output, args.core_revision, args.contract_version)
        else:
            publish(args.output, args.core_revision)
    except (FixtureError, OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, "journey fixtures: " + (str(error) if isinstance(error, FixtureError)
                                             else "artifact operation failed") + "\n")


if __name__ == "__main__":
    main()
