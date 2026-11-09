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
#   (ci/point-core-at.sh), builds e2e's pinned relays against it
#   (E2E_CORE_DIR with ci/build-pinned-binaries.sh) and runs smoke_ on
#   e2e's Core device (E2E_DEVICE_BACKEND=core), so no CLI is built
#   (vauchi/private#1118);
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
        'gitlab.com/vauchi/core.git' \
        'CI_MERGE_REQUEST_TARGET_BRANCH_NAME' \
        'sh ci/e2e-smoke.sh "$E2E_DIR" "$CI_PROJECT_DIR" "$TARGET_CORE_DIR"' \
        'exit_codes: [3]'; do
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

# ci/e2e-smoke.sh attributes a failure (decision of 2026-11-02): smoke on
# the MR's Core; on failure the same against the target branch's Core.
# 0 = the MR passes, 1 = only the MR fails, 3 = the target fails too, so
# the pinned set is behind Core and the job only warns. Fakes stand in for
# e2e's scripts and cargo; a core directory holding BROKEN fails.
fake=$(mktemp -d)
mkdir -p "$fake/e2e/ci" "$fake/bin" "$fake/mr" "$fake/target"
printf '#!/bin/sh\nexit 0\n' > "$fake/e2e/ci/point-core-at.sh"
printf '#!/bin/sh\n[ "${E2E_DEVICE_BACKEND:-}" = core ] || exit 9\n[ ! -f "$E2E_CORE_DIR/BROKEN" ]\n' > "$fake/e2e/ci/build-pinned-binaries.sh"
printf '#!/bin/sh\n[ "${E2E_DEVICE_BACKEND:-}" = core ] || exit 9\nexit 0\n' > "$fake/bin/cargo"
chmod +x "$fake/bin/cargo"
attribution() { # mr-broken target-broken expected
    rm -f "$fake/mr/BROKEN" "$fake/target/BROKEN"
    [ "$1" = yes ] && : > "$fake/mr/BROKEN"
    [ "$2" = yes ] && : > "$fake/target/BROKEN"
    rc=0
    PATH="$fake/bin:$PATH" sh ci/e2e-smoke.sh "$fake/e2e" "$fake/mr" "$fake/target" > "$fake/out" 2>&1 || rc=$?
    if [ "$rc" = "$3" ]; then
        echo "PASS: MR broken=$1, target broken=$2 exits $3"
    else
        fail "MR broken=$1, target broken=$2 must exit $3, got $rc: $(tail -3 "$fake/out")"
    fi
}
attribution no no 0
attribution yes no 1
attribution yes yes 3

# E2E_SMOKE_RELAY_SHA (vauchi/private#1183): a dependency both Core and the
# relay link, such as libsqlite3-sys, can move in only one of them per
# build, so a Core MR that moves it cannot resolve against e2e's pinned
# relay. The override names the relay commit that moved with it, for the
# MR's attempt only: the target's attempt keeps e2e's pins, so a real MR
# failure still exits 1 rather than hiding behind a target failure.
pinned=1111111111111111111111111111111111111111
override=2222222222222222222222222222222222222222
printf 'vauchi-relay = { git = "https://gitlab.com/vauchi/relay.git", rev = "%s" }\n' "$pinned" > "$fake/e2e/Cargo.toml"
printf 'CORE_TAG=v0.1.0\nRELAY_SHA=%s\n' "$pinned" > "$fake/e2e/ci/pins.env"
cat > "$fake/e2e/ci/build-pinned-binaries.sh" <<'FAKE'
#!/bin/sh
here=$(dirname "$0")
echo "built relay $(sed -n 's/^RELAY_SHA=//p' "$here/pins.env") dep $(sed -n 's/.*rev = "\([0-9a-f]*\)".*/\1/p' "$here/../Cargo.toml") for $E2E_CORE_DIR"
[ ! -f "$E2E_CORE_DIR/BROKEN" ]
FAKE
smoke_with_override() { # override mr-broken
    rm -f "$fake/mr/BROKEN" "$fake/target/BROKEN"
    [ "$2" = yes ] && : > "$fake/mr/BROKEN"
    rc=0
    E2E_SMOKE_RELAY_SHA=$1 PATH="$fake/bin:$PATH" E2E_DEVICE_BACKEND=core \
        sh ci/e2e-smoke.sh "$fake/e2e" "$fake/mr" "$fake/target" > "$fake/out" 2>&1 || rc=$?
}
smoke_with_override "$override" no
if [ "$rc" = 0 ] && grep -qF "built relay $override dep $override for $fake/mr" "$fake/out" &&
    grep -qF "relay override: $override (e2e pins $pinned)" "$fake/out"; then
    echo "PASS: the override builds and depends on the named relay for the MR's Core, and says so"
else
    fail "the override must set the relay dependency and binary for the MR's Core and print it, got $rc: $(cat "$fake/out")"
fi
smoke_with_override "$override" yes
if [ "$rc" = 1 ] && grep -qF "built relay $pinned dep $pinned for $fake/target" "$fake/out"; then
    echo "PASS: the target's attempt keeps e2e's pinned relay, so an MR failure exits 1"
else
    fail "the target's attempt must keep e2e's pins and the MR failure exit 1, got $rc: $(cat "$fake/out")"
fi
if grep -qF "RELAY_SHA=$pinned" "$fake/e2e/ci/pins.env" && grep -qF "rev = \"$pinned\"" "$fake/e2e/Cargo.toml"; then
    echo "PASS: the override leaves the e2e checkout itself unchanged"
else
    fail "the override must rewrite only the attempt's copy of e2e"
fi
smoke_with_override "not-a-sha" no
if [ "$rc" = 2 ] && ! grep -q 'built relay' "$fake/out"; then
    echo "PASS: an override that is not a full commit sha stops before any build, exit 2"
else
    fail "an override that is not a 40-hex sha must exit 2 before building, got $rc: $(cat "$fake/out")"
fi
rm -rf "$fake"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "OK: Core merge requests run e2e smoke against their own commit"
