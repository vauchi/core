#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# Core-only jobs must not rerun for vauchi-app-only changes now that the
# dependency points from vauchi-app to vauchi-core, never back (private#1230).

set -eu
set -f

ci_config="${1:-.gitlab-ci.yml}"
failures=0

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

job_block() {
    awk -v job="$1" '
        $0 == job ":" { in_job = 1; next }
        in_job && /^[^[:space:]#]/ { exit }
        in_job { print }
    ' "$ci_config"
}

rules_block() {
    job_block "$1" | sed -n '/^  rules:/,$p'
}

trigger_paths() {
    printf '%s\n' "$1" | sed -n 's/^ *- *"\{0,1\}\([^"]*[/.][^"]*\)"\{0,1\} *$/\1/p' |
        grep -v '^if:' | grep -v '^job:' | grep -v '^!reference' || true
}

covered() {
    c_file=$1
    for c_glob in $2; do
        c_pat=$(printf '%s' "$c_glob" | sed 's#\*\*/#*#g; s#\*\*#*#g')
        # shellcheck disable=SC2254 # the pattern is meant to glob
        case "$c_file" in $c_pat) return 0 ;; esac
    done
    return 1
}

require_covered() {
    r_job=$1
    r_paths=$2
    shift 2
    for r_file in "$@"; do
        if covered "$r_file" "$r_paths"; then
            echo "PASS: $r_job triggers on $r_file"
        else
            fail "$r_job does not trigger on $r_file"
        fi
    done
}

require_skipped() {
    r_job=$1
    r_paths=$2
    shift 2
    for r_file in "$@"; do
        if covered "$r_file" "$r_paths"; then
            fail "$r_job reruns for unrelated $r_file"
        else
            echo "PASS: $r_job skips $r_file"
        fi
    done
}

require_shared_rules() {
    if job_block "$1" | grep -Fq '!reference [.validation-core-build, rules]'; then
        echo "PASS: $1 includes the shared Core build inputs"
    else
        fail "$1 does not include .validation-core-build"
    fi
}

shared_paths=$(trigger_paths "$(rules_block .validation-core-build)")
require_covered .validation-core-build "$shared_paths" \
    Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml .gitlab-ci.yml
require_skipped .validation-core-build "$shared_paths" \
    vauchi-app/src/lib.rs vauchi-platform/lib.rs vauchi-cabi/src/lib.rs

core_paths=$(trigger_paths "$(rules_block test:core)")
require_shared_rules test:core
require_covered test:core "$core_paths" \
    vauchi-core/src/lib.rs vauchi-core/build.rs vauchi-core/tests/it/main.rs \
    vauchi-core-testkit/src/lib.rs vauchi-protocol/src/lib.rs .config/nextest.toml
require_skipped test:core "$core_paths" \
    vauchi-app/src/lib.rs vauchi-app/tests/it/main.rs \
    vauchi-platform/lib.rs vauchi-cabi/src/lib.rs

protocol_paths=$(trigger_paths "$(rules_block test:protocol)")
require_shared_rules test:protocol
require_covered test:protocol "$protocol_paths" \
    vauchi-protocol/src/lib.rs .config/nextest.toml
require_skipped test:protocol "$protocol_paths" \
    vauchi-core/src/lib.rs vauchi-core-testkit/src/lib.rs \
    vauchi-app/src/lib.rs vauchi-platform/lib.rs vauchi-cabi/src/lib.rs

bit32_paths=$(trigger_paths "$(rules_block test:core-32bit)")
require_shared_rules test:core-32bit
require_covered test:core-32bit "$bit32_paths" \
    vauchi-core/src/lib.rs vauchi-core/tests/it/main.rs \
    vauchi-core-testkit/src/lib.rs vauchi-protocol/src/lib.rs ci/tests/rust-cache.test.sh
require_skipped test:core-32bit "$bit32_paths" \
    vauchi-app/src/lib.rs vauchi-platform/lib.rs vauchi-cabi/src/lib.rs

semver_paths=$(trigger_paths "$(rules_block lint:semver)")
require_shared_rules lint:semver
require_covered lint:semver "$semver_paths" \
    vauchi-core/src/lib.rs vauchi-protocol/src/lib.rs \
    ci/semver-baseline.sh ci/tests/semver-baseline.test.py
require_skipped lint:semver "$semver_paths" \
    vauchi-app/src/lib.rs vauchi-core-testkit/src/lib.rs \
    vauchi-platform/lib.rs vauchi-cabi/src/lib.rs

formal_paths=$(trigger_paths "$(rules_block test:formal-code)")
require_skipped test:formal-code "$formal_paths" \
    vauchi-app/src/lib.rs vauchi-app/tests/it/main.rs

[ "$failures" -eq 0 ]
