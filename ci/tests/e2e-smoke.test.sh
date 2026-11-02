#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# A Core merge request that changes protocol code runs e2e's smoke suite
# against its own commit (vauchi/private#1032, decision D8). Before,
# verify:downstream-rust triggered e2e, which built its own pinned Core
# tag and proved nothing about the merge (problems/
# 2026-09-07-downstream-verify-tests-the-pin); e2e!292 then failed to
# compile against Core 0.95.0 only on the bump. This checks the contract
# in .gitlab-ci.yml and ci/publish-and-trigger.yml:
#
# - test:e2e-smoke clones e2e, points e2e's own Core at this checkout
#   (ci/point-core-at.sh), builds e2e's pinned relay and CLI against it
#   (E2E_CORE_DIR with ci/build-pinned-binaries.sh) and runs smoke_ with
#   those binaries;
# - it runs on merge requests that touch the protocol paths VGT12 names,
#   vauchi-protocol and the lockfile;
# - neither downstream trigger matrix lists vauchi/e2e any more.
#
# Usage: ci/tests/e2e-smoke.test.sh [ci-config] [trigger-config]

set -eu
set -f

ci_config=${1:-.gitlab-ci.yml}
trigger_config=${2:-ci/publish-and-trigger.yml}
job="test:e2e-smoke"
failures=0
fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

block=$(awk -v job="$job" '
    $0 == job ":" { in_job = 1; next }
    in_job && /^[^[:space:]#]/ { exit }
    in_job { print }
' "$ci_config")

if [ -z "$block" ]; then
    fail "$job is not in $ci_config"
else
    for step in \
        'gitlab.com/vauchi/e2e.git' \
        'ci/point-core-at.sh" "$CI_PROJECT_DIR" "$E2E_DIR/Cargo.toml"' \
        'E2E_CORE_DIR="$CI_PROJECT_DIR" sh "$E2E_DIR/ci/build-pinned-binaries.sh"' \
        'E2E_BIN_DIR="$E2E_DIR/.bin" cargo nextest run' \
        '-- smoke_'; do
        if printf '%s\n' "$block" | grep -qF -- "$step"; then
            echo "PASS: $job runs $step"
        else
            fail "$job must run: $step"
        fi
    done
    for path in \
        'vauchi-core/src/crypto/**/*' 'vauchi-core/src/exchange/**/*' \
        'vauchi-core/src/network/**/*' 'vauchi-core/src/backup/**/*' \
        'vauchi-core/src/recovery/**/*' 'vauchi-core/src/sync/**/*' \
        'vauchi-protocol/**/*' 'Cargo.lock'; do
        if printf '%s\n' "$block" | grep -qF -- "- $path"; then
            echo "PASS: $job triggers on $path"
        else
            fail "$job must trigger on $path"
        fi
    done
    printf '%s\n' "$block" | grep -q 'merge_request_event' ||
        fail "$job must run on merge requests"
fi

if grep -E 'DOWNSTREAM:' "$trigger_config" | grep -q 'vauchi/e2e'; then
    fail "$trigger_config still triggers vauchi/e2e downstream; it tests e2e's own pin, not this commit"
else
    echo "PASS: no downstream trigger runs e2e against its own pin"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "OK: Core merge requests run e2e smoke against their own commit"
