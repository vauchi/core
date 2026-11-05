<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Core product journeys

These files describe product behavior produced by this Core revision.
They are not the standalone shell conformance suite. The contract owns
examples of its primitives, encoding, defaults, bounds and lifecycle.
Core tests own expectations such as whether onboarding offers recovery.

The `check:journey-fixtures` job checks the journeys against the pinned
contract types and checks deterministic presentation regeneration. The
catalog's existing structural freshness check remains in place: seeded
contact keys make full catalog regeneration nondeterministic.

`publish:journey-fixtures` publishes plain JSON files and `manifest.json`
to the Core generic package registry as `core-journey-fixtures/<full SHA>`.
The manifest names the exact Core revision/version and resolved contract
tag/version/revision, and records each file's byte length and SHA-256.
Identical reruns are accepted; different bytes at the same revision fail.

A shell or app can download `ci/journey_fixtures.py` from an explicitly
selected Core revision, then run it with Python 3.11 or later:

```sh
python3 journey_fixtures.py fetch evidence \
  --core-revision <full-Core-commit-SHA> --contract-version <exact-version>
```

The helper fetches only data and checks provenance and integrity before
installing it. It requires no Rust toolchain, Core library or Core build.
There is no fallback to `main`, a latest catalog, or another contract.

For a Rust app assembly, derive the selection directly from its lockfile:

```sh
python3 journey_fixtures.py fetch-assembly evidence --assembly-lock Cargo.lock
```

This rejects evidence from a different Core commit or contract revision,
even if the human-readable contract version is identical.

App CI selects the SHA from its pinned assembly and records it with the
render/replay results. Display shells verify that supplied commands render
and supplied action ids are reported faithfully. Hardware shells verify
supplied operations and observations. They do not independently assert
that recovery belongs in onboarding. Standalone shell builds use contract
conformance fixtures and need not adopt this catalog to release.
