#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs the CI self-tests that need nothing but alpine's sh in one job.
# Each was its own job; on allyson a docker job spends ~27 s in setup for
# ~10 s of script, ~320 times a week per job (vauchi/private#1179). Every
# test runs even after one fails, and the failures are listed at the end,
# so one job still names each broken check. A listed test that is missing
# fails, so a rename cannot drop a check silently.
#
# Usage: sh ci/run-ci-self-tests.sh [test-name...]   (default: all)
# CI_SELF_TESTS_DIR overrides ci/tests, for the runner's own test.

set -u
cd "$(dirname "$0")/.." || exit 1

ALL="run-ci-self-tests dev-tag-no-retest formal-triggers core-triggers auto-tag-order windows-trigger-mr
swift-bindings-timeout packaging-matrix-isolation security-provenance
fuzz-security mutation-nightly-budget mutation-nightly-exit-codes
coverage-features e2e-smoke"

dir=${CI_SELF_TESTS_DIR:-ci/tests}
tests=${*:-$ALL}
failed=""
for name in $tests; do
    file="$dir/$name.test.sh"
    if [ ! -f "$file" ]; then
        echo "MISSING: $file"
        failed="$failed $name"
        continue
    fi
    echo "=== $name"
    if sh "$file"; then
        echo "=== $name: pass"
    else
        echo "=== $name: FAIL (exit $?)"
        failed="$failed $name"
    fi
done

if [ -n "$failed" ]; then
    echo "Failed:$failed"
    exit 1
fi
echo "All CI self-tests passed."
