#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The cold-rebuild-tree lock serves one build at a time. A train car's
# pipeline joins the queue after the merge request pipelines already
# waiting, and main then compiled the commit its car had just built:
# cars waited up to 123 minutes for the lock (vauchi/private#1399). So
# the lock has a lane per pipeline kind, and main's build:tests reuses
# the car's archives for the same commit on the same host.
#
# Usage: ci/tests/build-lock.test.sh   (from anywhere)

set -eu
cd "$(dirname "$0")/../.."

failures=0
pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; failures=$((failures + 1)); }

# ── wiring ───────────────────────────────────────────────────────────
lock_line=$(awk '
    /^\.timeout-cold-rebuild:/ { in_job = 1; next }
    /^[^[:space:]#]/ { in_job = 0 }
    in_job && /^[[:space:]]+resource_group:/ { print }
' .gitlab-ci.yml)
case "$lock_line" in
    *'cold-rebuild-tree-${CI_MERGE_REQUEST_EVENT_TYPE}'*) pass "the lock has a lane per pipeline kind" ;;
    *) fail "the cold-rebuild-tree lock is one lane for every pipeline kind: '$lock_line'" ;;
esac

build_tests=$(awk '
    /^build:tests:/ { in_job = 1; next }
    /^[^[:space:]#]/ { in_job = 0 }
    in_job { print }
' .gitlab-ci.yml)
case "$build_tests" in
    *'"$CI_DEFAULT_BRANCH" ] && sh ci/train-archives.sh reuse'*) pass "main's build:tests tries its train car's archives" ;;
    *) fail "main's build:tests does not try its train car's archives" ;;
esac
case "$build_tests" in
    *'= merge_train ]; then sh ci/train-archives.sh record'*) pass "a train car's build:tests records its archives" ;;
    *) fail "a train car's build:tests does not record its archives" ;;
esac

# ── ci/train-archives.sh ─────────────────────────────────────────────
script=$PWD/ci/train-archives.sh
if [ ! -f "$script" ]; then
    fail "ci/train-archives.sh is missing"
    echo "$failures check(s) failed"
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

host=$(hostname)
run() {
    env HOME="$work/home" CI_PROJECT_NAME=core CI_COMMIT_SHA="$1" CI_PIPELINE_ID="$2" \
        sh "$script" "$3" > "$work/out" 2>&1
}
archives=$work/home/.cache/vauchi-ci/archives/core
car() {
    rm -rf "${archives:?}/$1"
    mkdir -p "$archives/$1"
    echo "core archive of $1" > "$archives/$1/core.tar.zst"
    printf '%s\n' "${2:-$host}" > "$archives/$1/built-on"
}

if run aaa 200 reuse; then fail "reuse without a recorded car succeeded"; else pass "no recorded car: main builds"; fi
[ ! -e "$archives/200" ] && pass "no recorded car leaves no directory" || fail "no recorded car left $archives/200"

car 100
run aaa 100 record
if run aaa 200 reuse && cmp -s "$archives/100/core.tar.zst" "$archives/200/core.tar.zst"; then
    pass "main reuses the car's archives for the same commit"
else
    fail "main did not reuse the car's archives: $(cat "$work/out")"
fi
if [ "$(ls -i "$archives/100/core.tar.zst" | awk '{print $1}')" = "$(ls -i "$archives/200/core.tar.zst" | awk '{print $1}')" ]; then
    pass "the reused archives are hard links, not copies"
else
    fail "the reused archives were copied"
fi
[ "$(cat "$archives/200/built-on")" = "$host" ] && pass "the reused directory names the host the test jobs check" || fail "built-on was not carried"
grep -q "pipeline 100" "$work/out" && pass "the job log names the car it reused" || fail "the log does not name the car: $(cat "$work/out")"

if run bbb 300 reuse; then fail "another commit reused the car's archives"; else pass "another commit builds"; fi

car 101 other-host
run ccc 101 record
if run ccc 301 reuse; then fail "archives from another host were reused"; else pass "archives built on another host are not reused"; fi
[ ! -e "$archives/301" ] && pass "a refused reuse leaves no directory" || fail "a refused reuse left $archives/301"

car 102
mkdir "$archives/102/nested"
run ddd 102 record
if run ddd 302 reuse; then fail "a directory that is not flat was reused"; else pass "a directory with a subdirectory is not reused"; fi
[ ! -e "$archives/302" ] && [ ! -e "$archives/302.tmp" ] && pass "a failed link leaves nothing behind" || fail "a failed link left a partial directory"

run eee 103 record
rm -rf "${archives:?}/103"
if run eee 303 reuse; then fail "a pruned car's archives were reused"; else pass "a car whose archives were pruned is not reused"; fi

mkdir -p "$work/home/.cache/vauchi-ci/archives-by-sha/core"
echo "../../etc" > "$work/home/.cache/vauchi-ci/archives-by-sha/core/fff"
if run fff 304 reuse; then fail "an index that is not a pipeline id was followed"; else pass "an index that is not a pipeline id is refused"; fi

if run aaa 200 bogus; then fail "an unknown mode succeeded"; else pass "an unknown mode fails"; fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "PASS: build lock lanes and train archive reuse"
