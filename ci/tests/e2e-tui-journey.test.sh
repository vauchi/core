#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# test:e2e-tui-journey runs e2e's TUI journeys on the merge request's own
# Core (vauchi/private#1297): a change to the app engine must start it, it
# must hand the journeys this Core, and a failure must be attributed as the
# smoke gate's is.
#
# Usage: ci/tests/e2e-tui-journey.test.sh [ci-config]
# Run from the core checkout's root.

set -eu
set -f

ci_config=${1:-.gitlab-ci.yml}
job="test:e2e-tui-journey"
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
        'CI_MERGE_REQUEST_TARGET_BRANCH_NAME' \
        'sh ci/e2e-tui-journey.sh "$E2E_DIR" "$CI_PROJECT_DIR" "$TARGET_CORE_DIR"' \
        'merge_request_event' \
        'exit_codes: [3]'; do
        if printf '%s\n' "$block" | grep -qF -- "$step"; then
            echo "PASS: $job has $step"
        else
            fail "$job must have: $step"
        fi
    done
    for path in 'vauchi-app/src/**/*' 'vauchi-core/src/sync/**/*' \
        'vauchi-core/src/network/**/*' 'Cargo.lock'; do
        if printf '%s\n' "$block" | grep -qF -- "- $path"; then
            echo "PASS: $job triggers on $path"
        else
            fail "$job must trigger on $path"
        fi
    done
fi

# Fakes stand in for e2e's scripts. The fake journey script fails on a Core
# directory holding BROKEN, and also unless it was handed that Core and a
# filter, so a script that ran the journeys on tui's own pin would not pass.
fake=$(mktemp -d)
trap 'rm -rf "$fake"' EXIT
mkdir -p "$fake/e2e/ci" "$fake/mr" "$fake/target"
printf '#!/bin/sh\nexit 0\n' > "$fake/e2e/ci/point-core-at.sh"
cat > "$fake/e2e/ci/tui-journey.sh" <<'FAKE'
#!/bin/sh
[ -n "${E2E_CORE_DIR:-}" ] || { echo "no E2E_CORE_DIR"; exit 9; }
[ "${2:-}" = -E ] && [ -n "${3:-}" ] || { echo "no filter"; exit 9; }
echo "journeys on $E2E_CORE_DIR"
[ ! -f "$E2E_CORE_DIR/BROKEN" ] || exit 1
[ -n "${E2E_BIN_DIR:-}" ] || { echo "no E2E_BIN_DIR"; exit 9; }
if [ -f "$E2E_BIN_DIR/relay.sha" ]; then
    echo "relay reused"
else
    echo "relay built"
    echo "${FAKE_RELAY_SHA:-aaaa}" > "$E2E_BIN_DIR/relay.sha"
    echo profile > "$E2E_BIN_DIR/relay.build-profile"
    : > "$E2E_BIN_DIR/vauchi-relay"
fi
FAKE
attribution() { # mr-broken target-broken expected
    rm -f "$fake/mr/BROKEN" "$fake/target/BROKEN"
    [ "$1" = yes ] && : > "$fake/mr/BROKEN"
    [ "$2" = yes ] && : > "$fake/target/BROKEN"
    rc=0
    sh ci/e2e-tui-journey.sh "$fake/e2e" "$fake/mr" "$fake/target" > "$fake/out" 2>&1 || rc=$?
    if [ "$rc" = "$3" ] && grep -qF "journeys on $fake/mr" "$fake/out"; then
        echo "PASS: MR broken=$1, target broken=$2 exits $3"
    else
        fail "MR broken=$1, target broken=$2 must exit $3 on the MR's Core, got $rc: $(tail -3 "$fake/out")"
    fi
}
attribution no no 0
attribution yes no 1
attribution yes yes 3

# The relay binaries are kept between jobs: the first run builds and saves
# them, the next one reuses them, and a job never works in the kept
# directory itself. Without the variable nothing is kept.
cached_run() { # expected-line
    rc=0
    E2E_PINNED_BIN_CACHE="$fake/kept" sh ci/e2e-tui-journey.sh "$fake/e2e" "$fake/mr" "$fake/target" > "$fake/out" 2>&1 || rc=$?
    if [ "$rc" = 0 ] && grep -qF "$1" "$fake/out"; then
        echo "PASS: with a kept directory: $1"
    else
        fail "expected '$1', got rc=$rc: $(tail -3 "$fake/out")"
    fi
}
rm -f "$fake/mr/BROKEN" "$fake/target/BROKEN"
rm -rf "$fake/kept"
cached_run "relay built"
[ -f "$fake/kept/relay.sha" ] && [ -f "$fake/kept/vauchi-relay" ] ||
    fail "the first run must save the binaries and their markers"
cached_run "relay reused"
sh ci/e2e-tui-journey.sh "$fake/e2e" "$fake/mr" "$fake/target" > "$fake/out" 2>&1 || true
if grep -qF "relay built" "$fake/out"; then
    echo "PASS: without E2E_PINNED_BIN_CACHE nothing is reused"
else
    fail "a run without E2E_PINNED_BIN_CACHE must not read the kept directory"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "e2e-tui-journey: all checks passed"
