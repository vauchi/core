#!/bin/sh

# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# Behaviour of scripts/vendor-locales.sh (#432). An MR checks that the
# vendored catalogue is exactly the locales revision it names, so a locales
# merge does not turn every open core MR red; a commit that cuts a release keeps
# the freshness check. A local git repo stands in for the locales checkout.

set -eu

HERE=$(cd "$(dirname "$0")/../.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

git_quiet() { git -c init.defaultBranch=main -c user.name=t -c user.email=t@example.invalid "$@" >/dev/null 2>&1; }

mkdir -p "$T/core/scripts" "$T/core/vauchi-app"
cp "$HERE/scripts/vendor-locales.sh" "$T/core/scripts/"
VENDOR="$T/core/vauchi-app/locales"
LOCALES="$T/locales"

git_quiet init -q "$LOCALES"
printf '{"a":"one"}\n' > "$LOCALES/en.json"
git_quiet -C "$LOCALES" add en.json
git_quiet -C "$LOCALES" commit -q -m first
VAUCHI_LOCALES_DIR="$LOCALES" sh "$T/core/scripts/vendor-locales.sh" >/dev/null

printf '{"a":"one","b":"two"}\n' > "$LOCALES/en.json"
git_quiet -C "$LOCALES" commit -q -am second

run() {
    set +e
    VAUCHI_LOCALES_DIR="$LOCALES" sh "$T/core/scripts/vendor-locales.sh" "$1" >"$T/out" 2>&1
    rc=$?
    set -e
}

run --check-revision
[ "$rc" -eq 0 ] || fail "a copy that matches its REVISION passes on an MR although locales moved on; rc=$rc: $(cat "$T/out")"
grep -q 'behind locales' "$T/out" || fail "a copy behind locales main says so"

run --check
[ "$rc" -ne 0 ] || fail "--check still fails a copy behind locales main"

cp "$VENDOR/en.json" "$T/en.json.saved"
printf '{"a":"edited by hand"}\n' > "$VENDOR/en.json"
run --check-revision
[ "$rc" -ne 0 ] || fail "a hand-edited copy fails --check-revision"
cp "$T/en.json.saved" "$VENDOR/en.json"

cp "$VENDOR/REVISION" "$T/REVISION.saved"
printf 'unknown\n' > "$VENDOR/REVISION"
run --check-revision
[ "$rc" -ne 0 ] || fail "REVISION 'unknown' fails --check-revision"

printf '0123456789abcdef0123456789abcdef01234567\n' > "$VENDOR/REVISION"
run --check-revision
[ "$rc" -ne 0 ] || fail "a REVISION the checkout lacks fails --check-revision"

rm "$VENDOR/REVISION"
run --check-revision
[ "$rc" -ne 0 ] || fail "a missing REVISION fails --check-revision"
cp "$T/REVISION.saved" "$VENDOR/REVISION"

VAUCHI_LOCALES_DIR="$LOCALES" sh "$T/core/scripts/vendor-locales.sh" >/dev/null
run --check-revision
[ "$rc" -eq 0 ] || fail "a fresh copy passes --check-revision; rc=$rc: $(cat "$T/out")"
if grep -q 'behind locales' "$T/out"; then
    fail "a fresh copy is not reported as behind"
fi

# Which check a pipeline gets (private#1112). A commit that cuts a release
# needs fresh copy; any other commit on main does not, and failing it would
# only hold up its dev tag until the re-vendor lands.
cp "$HERE/ci/check-vendored-locales.sh" "$T/core/"
printf '[workspace.package]\nversion = "1.2.3"\n' > "$T/core/Cargo.toml"
git_quiet init -q "$T/core"
git_quiet init -q --bare "$T/origin.git"
git_quiet -C "$T/core" remote add origin "$T/origin.git"
git_quiet -C "$T/core" add Cargo.toml
git_quiet -C "$T/core" commit -q -m core
git_quiet -C "$T/core" push -q origin HEAD:refs/heads/main

gate() { # pipeline-source, then trigger repo
    set +e
    (cd "$T/core" && VAUCHI_LOCALES_DIR="$LOCALES" CI_PIPELINE_SOURCE="$1" CONTENT_TRIGGER_REPO="${2:-}" \
        sh check-vendored-locales.sh) >"$T/out" 2>&1
    rc=$?
    set -e
}

# The copy above is fresh; a new locales commit leaves it behind.
gate push
[ "$rc" -eq 0 ] || fail "a fresh copy passes on a commit that cuts a release; rc=$rc: $(cat "$T/out")"

printf '{"a":"one","b":"two","c":"three"}\n' > "$LOCALES/en.json"
git_quiet -C "$LOCALES" commit -q -am third

gate push
[ "$rc" -ne 0 ] || fail "a commit that cuts a release fails on stale copy"
grep -q 'cuts release v1.2.3' "$T/out" || fail "the refusal names the release it holds: $(cat "$T/out")"

gate merge_request_event
[ "$rc" -eq 0 ] || fail "an MR tolerates locales moving on; rc=$rc: $(cat "$T/out")"

gate pipeline locales
[ "$rc" -eq 0 ] || fail "the locales-triggered pipeline tolerates its own merge; rc=$rc: $(cat "$T/out")"

gate pipeline other
[ "$rc" -ne 0 ] || fail "a pipeline triggered by another repo gets no tolerance"

git_quiet -C "$T/core" tag v1.2.3
git_quiet -C "$T/core" push -q origin v1.2.3
gate push
[ "$rc" -eq 0 ] || fail "a commit with no release to cut tolerates locales moving on; rc=$rc: $(cat "$T/out")"
grep -q 'behind locales' "$T/out" || fail "that commit still reports the copy as behind: $(cat "$T/out")"

cp "$VENDOR/en.json" "$T/en.json.saved"
printf '{"a":"edited by hand"}\n' > "$VENDOR/en.json"
gate push
[ "$rc" -ne 0 ] || fail "a hand-edited copy fails even with no release to cut"
cp "$T/en.json.saved" "$VENDOR/en.json"

git_quiet -C "$T/core" remote set-url origin "$T/no-such-remote.git"
gate push
[ "$rc" -ne 0 ] || fail "a tag lookup that cannot answer is treated as a release, so stale copy fails"
git_quiet -C "$T/core" remote set-url origin "$T/origin.git"

if [ "$failures" -ne 0 ]; then
    echo "vendor-locales.test: $failures failure(s)" >&2
    exit 1
fi
echo "vendor-locales.test: all passed"
