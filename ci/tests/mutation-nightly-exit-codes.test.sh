#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
# mutation:nightly failed on 2026-10-28, 29 and 30 with "Score: 0.00%
# (0/0 mutants)": its unmutated baseline was red (cargo-mutants exit 4),
# and allow_failure: true kept the pipeline green, so nobody saw that
# mutation testing had stopped (vauchi/private#974). Missed mutants
# (exit 2) and timeouts (exit 3) stay advisory; a baseline that does not
# run, a usage error or an internal error must fail the pipeline.
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
fail() { printf 'mutation-nightly-exit-codes: %s\n' "$1" >&2; exit 1; }
printf '%s\n' "$job" | grep -q '^mutation:nightly:' || fail "job mutation:nightly not found in $PIPELINE"
if printf '%s\n' "$job" | grep -Eq '^[[:space:]]+allow_failure:[[:space:]]*true[[:space:]]*$'; then
    fail "mutation:nightly carries allow_failure: true; a red baseline (exit 4) would pass unseen"
fi
printf '%s\n' "$job" | grep -Eq '^[[:space:]]+exit_codes:[[:space:]]*\[2, 3\]' \
    || fail "mutation:nightly must declare allow_failure: exit_codes: [2, 3] (missed mutants and timeouts advisory, everything else red)"
echo "mutation-nightly-exit-codes: OK"
