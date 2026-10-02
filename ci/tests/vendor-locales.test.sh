#!/bin/sh

# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later

# Behaviour of scripts/vendor-locales.sh (#432). An MR checks that the
# vendored catalogue is exactly the locales revision it names, so a locales
# merge does not turn every open core MR red; main keeps the freshness check.
# A local git repo stands in for the locales checkout.

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

if [ "$failures" -ne 0 ]; then
    echo "vendor-locales.test: $failures failure(s)" >&2
    exit 1
fi
echo "vendor-locales.test: all passed"
