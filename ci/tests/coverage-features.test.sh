#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
# coverage:workspace ran 3 598 of core's 9 798 tests on 2026-10-30: it
# passed only --features test-kdf, so the `it` target (required-features =
# ["testing"]) and vauchi-app's gated tests were never built, and the
# coverage figure the project reads described a third of its suite
# (vauchi/private#975). The job must enable the same features as
# `just test core` (scripts/scripts/resolve-repo.sh, repo_test_features).
set -eu
ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/.gitlab-ci.yml"
job=$(
    awk '
        /^coverage:workspace:/ { capture = 1; print; next }
        capture && /^[^ #]/ { capture = 0 }
        capture { print }
    ' "$PIPELINE"
)
fail() { printf 'coverage-features: %s\n' "$1" >&2; exit 1; }
printf '%s\n' "$job" | grep -q '^coverage:workspace:' || fail "job coverage:workspace not found in $PIPELINE"
flags=$(printf '%s\n' "$job" | sed -n 's/^[[:space:]]*COVERAGE_EXTRA_FLAGS:[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p')
[ -n "$flags" ] || fail "coverage:workspace declares no COVERAGE_EXTRA_FLAGS"
for feature in vauchi-core/testing vauchi-core/test-kdf vauchi-app/testing; do
    case " $flags " in
        *"$feature"*) ;;
        *) fail "COVERAGE_EXTRA_FLAGS lacks $feature (got: $flags); the it target and gated tests stay out of the figure" ;;
    esac
done
echo "coverage-features: OK ($flags)"
