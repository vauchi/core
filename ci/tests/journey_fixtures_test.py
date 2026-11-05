# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("journeys", Path(__file__).parents[1] / "journey_fixtures.py")
journeys = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(journeys)
SHA = "a" * 40
CONTRACT_SHA = "b" * 40


class JourneyFixturesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "vauchi-app/fixtures/journeys"
        self.source.mkdir(parents=True)
        for name in journeys.FILES:
            (self.source / name).write_text('{"schema_version":1}\n')
        (self.root / "Cargo.toml").write_text(
            '[workspace.package]\nversion="0.99.1"\n[workspace.dependencies]\n'
            'vauchi-contract={git="https://gitlab.com/vauchi/contract.git",tag="v0.97.2"}\n')
        (self.root / "Cargo.lock").write_text(
            '[[package]]\nname="vauchi-contract"\nversion="0.97.2"\n'
            'source="git+https://gitlab.com/vauchi/contract.git?tag=v0.97.2#' + CONTRACT_SHA + '"\n')
        self.output = self.root / "artifact"

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


if __name__ == "__main__":
    unittest.main()
