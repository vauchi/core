#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The spec cites Core's test, lemma and function names as the proof behind
# its claims, and check:spec-names runs the spec's own check against this
# checkout so an MR learns that it removes a cited name (vauchi/private#874).
# This checks the job's contract in .gitlab-ci.yml: the spec's self-test
# runs first, the check sees every crate of this checkout, a change in any
# of them triggers the job, and a changed vectors fixture only warns.

set -eu

ci_config="${1:-.gitlab-ci.yml}"
job="check:spec-names"
failures=0

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

job_block() {
    awk -v job="$1" '
        $0 == job ":" { in_job = 1 }
        in_job && $0 != job ":" && /^[^[:space:]#]/ { exit }
        in_job { print }
    ' "$ci_config"
}

block=$(job_block "$job")
[ -n "$block" ] || {
    echo "FAIL: $job is not in $ci_config" >&2
    exit 1
}

self_test=$(printf '%s\n' "$block" | grep -n 'check-claim-names.sh --self-test' | head -1 | cut -d: -f1)
# shellcheck disable=SC2016 # matches the literal YAML text
chapters=$(printf '%s\n' "$block" | grep -n 'check-claim-names.sh "\$f"' | head -1 | cut -d: -f1)
if [ -n "$self_test" ] && [ -n "$chapters" ] && [ "$self_test" -lt "$chapters" ]; then
    echo "PASS: $job runs the spec's self-test before the chapters"
else
    fail "$job must run check-claim-names.sh --self-test before checking the chapters"
fi

# Every workspace member of Cargo.toml: a cited name may live in any crate.
for crate in vauchi-app vauchi-cabi vauchi-core vauchi-hardware vauchi-platform vauchi-protocol; do
    # shellcheck disable=SC2016 # matches the literal YAML text
    if printf '%s\n' "$block" | grep 'check-claim-names.sh "\$f"' | grep -q " $crate "; then
        echo "PASS: $job searches $crate of this checkout"
    else
        fail "$job must pass $crate of this checkout as a search root"
    fi
    if printf '%s\n' "$block" | grep -q "^        - \"$crate/\*\*/\*\"\$"; then
        echo "PASS: a change in $crate triggers $job"
    else
        fail "$job rules must list \"$crate/**/*\" under changes"
    fi
done

if printf '%s\n' "$block" | grep 'cmp .*vauchi-protocol-vectors-v1.json' | grep -q 'exit 3' &&
    printf '%s\n' "$block" | grep -q 'exit_codes: \[3\]'; then
    echo "PASS: a changed vectors fixture warns with exit 3 and does not fail $job"
else
    fail "$job must compare the vectors fixture with the spec's copy, exit 3 on a difference, and allow exit code 3"
fi

[ "$failures" -eq 0 ]
