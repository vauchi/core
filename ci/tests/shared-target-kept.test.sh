#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every shell job of a runner slot builds in the slot's one target/, and a
# merge request's build:tests is quick only while that directory is warm
# (4 to 6 minutes warm, 13 and more cold). A job that runs `cargo clean`
# there makes the next build:tests in the slot compile everything again:
# verify:reproducible did that after every merge to main, on the dev tag
# (vauchi/private#1305). A job that cleans a whole target directory must
# name one of its own; `cargo clean -p <crate>` removes one crate and is
# left alone.
#
# Two checkouts empty it as well (vauchi/private#1395): GIT_STRATEGY clone
# deletes the slot's directory and clones it again, and the runner's own
# fetch runs `git clean -ffdx`, which removes target/ and .cargo/ unless
# GIT_CLEAN_FLAGS keeps them. lint:typos, test:packaging-matrix and
# auto-tag:version emptied a slot before 8 of 8 cold build:tests runs.
#
# Usage: ci/tests/shared-target-kept.test.sh [ci-config...]
#        (from anywhere; defaults to Core's pipeline files)

set -eu
cd "$(dirname "$0")/../.."

[ "$#" -gt 0 ] || set -- .gitlab-ci.yml ci/*.yml

offenders=$(awk '
    function report() {
        if (job != "" && cleans && !own_target) print FILENAME ": " job
    }
    FNR == 1 { report(); job = ""; cleans = 0; own_target = 0 }
    /^[^[:space:]#][^[:space:]]*:[[:space:]]*$/ {
        report()
        job = $0; sub(/:[[:space:]]*$/, "", job)
        cleans = 0; own_target = 0
        next
    }
    /^[[:space:]]*#/ { next }
    /CARGO_TARGET_DIR:/ { own_target = 1 }
    /cargo clean/ && !/cargo clean +-p / { cleans = 1 }
    END { report() }
' "$@")

failed=0
if [ -n "$offenders" ]; then
    echo "FAIL: these jobs run cargo clean in the slot's shared target/:"
    echo "$offenders" | sed 's/^/  /'
    echo "  Give the job its own CARGO_TARGET_DIR, or clean one crate with -p."
    failed=1
fi

clones=$(awk '
    FNR == 1 { job = "" }
    /^[^[:space:]#][^[:space:]]*:[[:space:]]*(#.*)?$/ {
        job = $0; sub(/:[[:space:]]*(#.*)?$/, "", job)
        next
    }
    /^[[:space:]]*#/ { next }
    /GIT_STRATEGY:[[:space:]]*"?clone/ { print FILENAME ": " job }
' "$@")
if [ -n "$clones" ]; then
    echo "FAIL: these jobs clone afresh, which deletes the slot and its target/:"
    echo "$clones" | sed 's/^/  /'
    echo "  Use GIT_STRATEGY: none and check out into a directory of the job's own."
    failed=1
fi

clean_flags=$(awk '
    /^variables:[[:space:]]*$/ { top = 1; next }
    /^[^[:space:]#]/ { top = 0 }
    top && /^[[:space:]]+GIT_CLEAN_FLAGS:/ { print }
' "$@")
for kept in target/ .cargo/ .cache/; do
    case "$clean_flags" in
        *"-e $kept"*) ;;
        *)
            echo "FAIL: the top-level GIT_CLEAN_FLAGS does not keep $kept, so the runner's own checkout removes it"
            failed=1
            ;;
    esac
done

[ "$failed" -eq 0 ] || exit 1
echo "PASS: no job cleans, clones over or git-cleans the target directory the other jobs build in"
