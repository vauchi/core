#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The tests behind check:journey-fixtures: the checked-in journey fixtures
# match a fresh run, and regenerating them changes nothing (the job's
# `git diff` after this script holds the second half).
#
# In a pipeline that has build:tests, the test binaries are in its
# vauchi-app archive on this host, and compiling them here again took the
# job four of its six minutes (vauchi/private#1305). Without the archive
# (a tag pipeline, or a merge request that touches no build:tests path)
# they are compiled here, with the archive's features.
#
# A selection that matches no test fails: from 2026-11-11 the two ceremony
# steps asked vauchi-core for tests that had moved to vauchi-app, ran none
# and passed.
#
# Usage: ci/journey-fixture-tests.sh   (from the core repo root)
#   JOURNEY_FIXTURE_ARCHIVE   the archive to run, instead of this
#                             pipeline's (for a local run)

set -eu

archive=${JOURNEY_FIXTURE_ARCHIVE:-$HOME/.cache/vauchi-ci/archives/${CI_PROJECT_NAME:-core}/${CI_PIPELINE_ID:-none}/app.tar.zst}
built_on=$(dirname "$archive")/built-on

if [ -f "$archive" ] && [ -f "$built_on" ] && [ "$(cat "$built_on")" = "$(hostname)" ]; then
    echo "journey fixtures: running build:tests' archive $archive"
    mkdir -p target/archive-run
    run() {
        cargo nextest run --profile ci --no-tests=fail \
            --workspace-remap "$PWD" --extract-to target/archive-run --extract-overwrite \
            --archive-file "$archive" "$@"
    }
else
    echo "journey fixtures: no archive from build:tests on this host for this pipeline; compiling the tests here"
    run() {
        cargo nextest run --profile ci --no-tests=fail \
            -p vauchi-app --features test-kdf,network-http,testing "$@"
    }
fi

run -E 'binary(presentation_contract_fixture_tests) | binary(screen_catalog_fixture_tests)'
run -E 'binary(it) & test(recovery_ceremony_acceptance_tests::fixture::)'
run --run-ignored only -E 'binary(presentation_contract_fixture_tests) & test(regenerate_shared_fixture)'
run --run-ignored only -E 'binary(it) & test(regenerate_ceremony_fixture)'
