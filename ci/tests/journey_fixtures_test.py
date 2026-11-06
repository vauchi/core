# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import importlib.util
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("journeys", Path(__file__).parents[1] / "journey_fixtures.py")
journeys = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(journeys)
SHA = "a" * 40
CONTRACT_SHA = "b" * 40
DOCUMENTS = {
    "presentation_contract_v1.json": {
        "schema_version": 1, "initial_commands": ["QrRequestScan"],
        "steps": [{"event": "ImagePickCancelled", "commands": ["QrRequestScan"]}],
        "effect_commands": [], "file_picker_contract": {}, "expected_state": {},
    },
    "screen_catalog_v1.json": {
        "schema_version": 1,
        "screens": [{"code_id": "example", "title": "Example", "locale": "en",
                     "commands": ["QrRequestScan"]}],
    },
    "screen_catalog_inventory_v1.json": {"schema_version": 1, "screen_ids": ["example"]},
    "recovery_ceremony_v1.json": {
        "schema_version": 1,
        "scenarios": [{"scenario": "Example", "steps": [
            {"actor": "phone", "event": None, "commands": ["QrRequestScan"]},
            {"elided_packets": True},
        ]}],
    },
}


def invalid_corpora():
    catalog = "screen_catalog_v1.json"
    inventory = "screen_catalog_inventory_v1.json"
    presentation = "presentation_contract_v1.json"
    for key, value in (
        ("empty-inventory", []), ("unexpected-id", ["other"]),
        ("additional-id", ["example", "extra"]),
        ("duplicate-id", ["example", "example"]), ("invalid-id", [42]),
        ("wrong-id-container", {}),
    ):
        data = copy.deepcopy(DOCUMENTS)
        data[inventory]["screen_ids"] = value
        yield key, data
    data = copy.deepcopy(DOCUMENTS)
    data[catalog]["screens"].append(data[catalog]["screens"][0] | {"code_id": "second"})
    yield "missing-id", data
    ceremony = "recovery_ceremony_v1.json"
    for key, value in (
        ("empty-ceremony", []), ("wrong-ceremony-container", {}),
        ("ceremony-without-steps", [{"scenario": "Example", "steps": []}]),
        ("ceremony-unknown-actor",
         [{"scenario": "Example", "steps": [{"actor": "relay", "commands": []}]}]),
        ("ceremony-step-without-commands",
         [{"scenario": "Example", "steps": [{"actor": "phone"}]}]),
    ):
        data = copy.deepcopy(DOCUMENTS)
        data[ceremony]["scenarios"] = value
        yield key, data
    for key, value in (
        ("empty-catalog", []), ("wrong-screen-container", {}),
        ("duplicate-screen", DOCUMENTS[catalog]["screens"] * 2),
        ("invalid-screen", [None]),
        ("empty-commands", [DOCUMENTS[catalog]["screens"][0] | {"commands": []}]),
        ("wrong-command-container", [DOCUMENTS[catalog]["screens"][0] | {"commands": {}}]),
    ):
        data = copy.deepcopy(DOCUMENTS)
        data[catalog]["screens"] = value
        yield key, data
    for key, value in (("wrong-steps", {}), ("invalid-step", [None]),
                       ("missing-event", [{"commands": []}]),
                       ("wrong-step-commands", [{"event": "ImagePickCancelled", "commands": {}}])):
        data = copy.deepcopy(DOCUMENTS)
        data[presentation]["steps"] = value
        yield key, data
    data = copy.deepcopy(DOCUMENTS)
    del data[presentation]["initial_commands"]
    yield "missing-initial-commands", data
    for key, value in (("effect_commands", {}), ("file_picker_contract", []),
                       ("expected_state", [])):
        data = copy.deepcopy(DOCUMENTS)
        data[presentation][key] = value
        yield "wrong-" + key, data


class JourneyFixturesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "vauchi-app/fixtures/journeys"
        self.source.mkdir(parents=True)
        self.write_documents(DOCUMENTS)
        (self.root / "Cargo.toml").write_text(
            '[workspace.package]\nversion="0.99.1"\n[workspace.dependencies]\n'
            'vauchi-contract={git="https://gitlab.com/vauchi/contract.git",tag="v0.97.2"}\n')
        (self.root / "Cargo.lock").write_text(
            '[[package]]\nname="vauchi-contract"\nversion="0.97.2"\n'
            'source="git+https://gitlab.com/vauchi/contract.git?tag=v0.97.2#' + CONTRACT_SHA + '"\n')
        self.output = self.root / "artifact"

    def write_documents(self, documents):
        for name, value in documents.items():
            (self.source / name).write_text(json.dumps(value))

    def rehash_artifact(self, documents):
        metadata = json.loads((self.output / "manifest.json").read_bytes())
        for name, value in documents.items():
            raw = json.dumps(value).encode()
            (self.output / name).write_bytes(raw)
            metadata["files"][name] = {"sha256": hashlib.sha256(raw).hexdigest(), "bytes": len(raw)}
        (self.output / "manifest.json").write_text(json.dumps(metadata))

    def download(self, revision, filename):
        self.assertEqual(revision, SHA)
        return (self.output / filename).read_bytes()

    def test_artifact_identifies_immutable_core_and_resolved_contract(self):
        manifest = journeys.build(self.root, self.output, SHA)
        self.assertEqual(manifest["core_revision"], SHA)
        self.assertEqual(manifest["core_version"], "0.99.1")
        self.assertEqual(manifest["contract"], {
            "version": "0.97.2", "tag": "v0.97.2", "revision": CONTRACT_SHA})
        destination = self.root / "consumer"
        journeys.fetch(destination, SHA, "0.97.2", self.download)
        for name in journeys.FILES:
            self.assertEqual((destination / name).read_bytes(), (self.source / name).read_bytes())

    def test_floating_revision_is_rejected_before_download(self):
        for revision in ("main", "latest", "v0.99.1", "a" * 39, "A" * 40):
            with self.subTest(revision=revision), self.assertRaises(journeys.FixtureError):
                journeys.fetch(self.root / "consumer", revision, "0.97.2", self.download)

    def test_wrong_core_contract_or_hash_never_installs_artifact(self):
        for mutation in ("core", "contract", "hash", "filename", "missing", "oversized"):
            with self.subTest(mutation=mutation):
                journeys.build(self.root, self.output, SHA)
                manifest = json.loads((self.output / "manifest.json").read_bytes())
                if mutation == "core":
                    manifest["core_revision"] = CONTRACT_SHA
                elif mutation == "contract":
                    manifest["contract"]["version"] = "0.97.3"
                elif mutation == "filename":
                    manifest["files"]["../escape.json"] = manifest["files"].pop(journeys.FILES[0])
                elif mutation == "missing":
                    del manifest["files"][journeys.FILES[0]]
                elif mutation == "oversized":
                    manifest["files"][journeys.FILES[0]]["bytes"] = journeys.MAX_BYTES + 1
                else:
                    (self.output / journeys.FILES[0]).write_bytes(b"tampered")
                (self.output / "manifest.json").write_text(json.dumps(manifest))
                destination = self.root / ("consumer-" + mutation)
                with self.assertRaises(journeys.FixtureError):
                    journeys.fetch(destination, SHA, "0.97.2", self.download)
                self.assertFalse(destination.exists())

    def test_local_path_override_is_not_release_provenance(self):
        (self.root / "Cargo.lock").write_text(
            '[[package]]\nname="vauchi-contract"\nversion="0.97.2"\n')
        with self.assertRaises(journeys.FixtureError):
            journeys.build(self.root, self.output, SHA)

    def test_invalid_schema_and_oversized_data_are_refused(self):
        for raw in (b"null", b"{", b'{"schema_version":true}', b'{"schema_version":2}',
                    b"x" * (journeys.MAX_BYTES + 1)):
            with self.subTest(size=len(raw)), self.assertRaises(journeys.FixtureError):
                journeys.journey_document(raw)

    def test_publish_is_immutable_and_idempotent_with_manifest_last(self):
        journeys.build(self.root, self.output, SHA)
        registry, writes = {}, []
        def get(revision, filename):
            self.assertEqual(revision, SHA)
            return registry.get(filename)
        def put(revision, filename, raw):
            self.assertEqual(revision, SHA)
            writes.append(filename)
            registry[filename] = raw
        journeys.publish(self.output, SHA, get, put)
        self.assertEqual(writes, [*journeys.FILES, "manifest.json"])
        journeys.publish(self.output, SHA, get, put)
        self.assertEqual(len(writes), len(journeys.FILES) + 1)

        registry[journeys.FILES[0]] = b"other bytes at the same revision"
        with self.assertRaises(journeys.FixtureError):
            journeys.publish(self.output, SHA, get, put)
        self.assertEqual(len(writes), len(journeys.FILES) + 1)

    def test_assembly_evidence_uses_locked_core_and_contract_revisions(self):
        journeys.build(self.root, self.output, SHA)
        lock = self.root / "assembly.lock"
        lock.write_text((self.root / "Cargo.lock").read_text()
            + '\n[[package]]\nname="vauchi-core"\nversion="0.99.1"\n'
            + 'source="git+https://gitlab.com/vauchi/core.git?tag=v0.99.1#' + SHA + '"\n')
        destination = self.root / "assembly-evidence"
        journeys.fetch_assembly(destination, lock, self.download)
        self.assertEqual((destination / "manifest.json").read_bytes(),
                         (self.output / "manifest.json").read_bytes())
        lock.write_text(lock.read_text().replace(CONTRACT_SHA, "c" * 40))
        with self.assertRaises(journeys.FixtureError):
            journeys.fetch_assembly(self.root / "wrong-assembly", lock, self.download)
        self.assertFalse((self.root / "wrong-assembly").exists())

    def test_build_rejects_malformed_or_incomplete_corpora_before_installing(self):
        for name, documents in invalid_corpora():
            with self.subTest(mutation=name):
                self.write_documents(documents)
                destination = self.root / ("build-" + name)
                with self.assertRaises(journeys.FixtureError):
                    journeys.build(self.root, destination, SHA)
                self.assertFalse(destination.exists())

    def test_rehashed_invalid_artifacts_are_rejected_before_fetch_install_or_publish(self):
        for name, documents in invalid_corpora():
            with self.subTest(mutation=name):
                journeys.build(self.root, self.output, SHA)
                self.rehash_artifact(documents)
                destination = self.root / ("fetch-" + name)
                with self.assertRaises(journeys.FixtureError):
                    journeys.fetch(destination, SHA, "0.97.2", self.download)
                self.assertFalse(destination.exists())
                writes = []
                with self.assertRaises(journeys.FixtureError):
                    journeys.publish(self.output, SHA, lambda rev, filename: None,
                                     lambda *args: writes.append(args))
                self.assertEqual([], writes)

    def test_missing_inventory_entry_is_rejected_for_an_exact_assembly_lock(self):
        journeys.build(self.root, self.output, SHA)
        data = copy.deepcopy(DOCUMENTS)
        data["screen_catalog_inventory_v1.json"]["screen_ids"] = ["other"]
        self.rehash_artifact(data)
        lock = self.root / "assembly.lock"
        lock.write_text((self.root / "Cargo.lock").read_text()
                       + '\n[[package]]\nname="vauchi-core"\nversion="0.99.1"\n'
                       + 'source="git+https://gitlab.com/vauchi/core.git?tag=v0.99.1#' + SHA + '"\n')
        destination = self.root / "assembly-evidence"
        with self.assertRaises(journeys.FixtureError):
            journeys.fetch_assembly(destination, lock, self.download)
        self.assertFalse(destination.exists())

    def test_invalid_fetch_preserves_a_previously_installed_destination(self):
        journeys.build(self.root, self.output, SHA)
        destination = self.root / "existing"
        journeys.fetch(destination, SHA, "0.97.2", self.download)
        previous = {path.name: path.read_bytes() for path in destination.iterdir()}
        invalid = copy.deepcopy(DOCUMENTS)
        invalid["screen_catalog_v1.json"]["screens"] = []
        self.rehash_artifact(invalid)
        with self.assertRaises(journeys.FixtureError):
            journeys.fetch(destination, SHA, "0.97.2", self.download)
        self.assertEqual(previous, {path.name: path.read_bytes() for path in destination.iterdir()})

    def test_committed_product_fixtures_are_accepted_without_rewriting_their_bytes(self):
        source = Path(__file__).resolve().parents[2] / "vauchi-app/fixtures/journeys"
        for name in journeys.FILES:
            (self.source / name).write_bytes((source / name).read_bytes())
        journeys.build(self.root, self.output, SHA)
        destination = self.root / "real-product-evidence"
        journeys.fetch(destination, SHA, "0.97.2", self.download)
        for name in journeys.FILES:
            self.assertEqual((source / name).read_bytes(), (destination / name).read_bytes())


if __name__ == "__main__":
    unittest.main()
