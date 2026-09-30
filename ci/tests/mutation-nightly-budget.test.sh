#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

# mutation:nightly shares allyson with every MR's cargo jobs. A 1/7 shard
# (2080 mutants at ~61/h) never finished in 12 h and ran through the
# working day, tripling the time core's serialized jobs held their lock
# (vauchi/private#435). The run must end within 4 h, and the shard picked
# per night must walk every shard in turn.

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/ci/mutation-and-fuzz.yml"

job=$(
    awk '
        /^mutation:nightly:/ { capture = 1; print; next }
        capture && /^[^ #]/ { capture = 0 }
        capture { print }
    ' "$PIPELINE"
)

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

printf '%s\n' "$job" | grep -Eq '^  timeout: 4h$' \
    || fail "mutation:nightly must stop within 4h"

shards=$(printf '%s\n' "$job" | sed -n 's/^    MUTATION_SHARDS: "\([0-9]*\)"$/\1/p')
[ -n "$shards" ] || fail "mutation:nightly must set MUTATION_SHARDS"
[ "$shards" -ge 80 ] \
    || fail "MUTATION_SHARDS=$shards leaves more mutants per night than 4h tests"

printf '%s\n' "$job" | grep -Fq -- '--shard ${MUTATION_SHARD}/${MUTATION_SHARDS}' \
    || fail "the cargo-mutants shard flag must divide by MUTATION_SHARDS"

pick=$(printf '%s\n' "$job" | sed -n 's/^    - \(export MUTATION_SHARD=.*\)$/\1/p')
[ -n "$pick" ] || fail "mutation:nightly must export MUTATION_SHARD"

seen=$(
    export MUTATION_SHARDS="$shards"
    day=0
    while [ "$day" -lt "$shards" ]; do
        FAKE_EPOCH=$(( (20000 + day) * 86400 + 5400 ))
        date() { echo "$FAKE_EPOCH"; }
        eval "$pick"
        echo "$MUTATION_SHARD"
        day=$((day + 1))
    done | sort -n | uniq
)

[ "$(printf '%s\n' "$seen" | wc -l | tr -d ' ')" -eq "$shards" ] \
    || fail "$shards consecutive nights must test $shards distinct shards"
[ "$(printf '%s\n' "$seen" | head -n 1)" -eq 0 ] \
    || fail "shards are 0-indexed; the lowest must be 0"
[ "$(printf '%s\n' "$seen" | tail -n 1)" -eq $((shards - 1)) ] \
    || fail "the highest shard must be MUTATION_SHARDS - 1"

echo "PASS: mutation:nightly ends within 4h and cycles through all $shards shards"
