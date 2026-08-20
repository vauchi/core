#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/.gitlab-ci.yml"
RULESET="$ROOT/.gitlab/sast-ruleset.toml"
BUILD="$ROOT/ci/build.yml"
EXPECTED_REF=50ef738a54b44b0695795a12131ee47d6c4b7a67
failed=0

fail() {
    echo "FAIL: $1" >&2
    failed=$((failed + 1))
}

template_ref=$(awk '
    /project: .vauchi\/scripts./ { in_scripts = 1; next }
    in_scripts && /ref:/ { print $2; exit }
' "$PIPELINE")

runtime_ref=$(awk '/VAUCHI_SCRIPTS_REF:/ { gsub(/"/, "", $2); print $2; exit }' "$PIPELINE")

[ "$template_ref" = "$EXPECTED_REF" ] || fail "shared template ref is $template_ref"
[ "$runtime_ref" = "$EXPECTED_REF" ] || fail "runtime scripts ref is $runtime_ref"

grep -q "vauchi-security.yml/raw?ref=$EXPECTED_REF" "$RULESET" \
    || fail "custom SAST rules are not pinned to $EXPECTED_REF"

grep -q 'raw?ref=${VAUCHI_SCRIPTS_REF}' "$BUILD" \
    || fail "binding ABI fetch bypasses the pinned runtime ref"

gate=$(awk '
    /^security:sast-severity:/ { in_job = 1 }
    in_job && NR > 1 && /^[^ ]/ && !/^security:sast-severity:/ { exit }
    in_job { print }
' "$PIPELINE")

printf '%s\n' "$gate" | grep -q 'extends: .security-sast-severity-gate' \
    || fail "Critical severity gate is not enabled"
printf '%s\n' "$gate" | grep -q '!reference \[semgrep-sast, rules\]' \
    || fail "severity gate rules can drift from semgrep-sast"

if grep -Eq 'vauchi-security[.]yml/raw[?]ref=main|scripts[^ ]*/raw[?]ref=main' \
    "$PIPELINE" "$RULESET" "$BUILD"; then
    fail "a mutable scripts/main fetch remains"
fi

if grep -Fq '../scripts' "$PIPELINE"; then
    fail "security jobs mutate a shared sibling scripts checkout"
fi

isolated_dirs=$(grep -Fc 'VAUCHI_CI_SCRIPTS_DIR=$(mktemp -d)' "$PIPELINE" || true)
remotes=$(grep -Fc 'git -C "${VAUCHI_CI_SCRIPTS_DIR}" remote add origin https://gitlab.com/vauchi/scripts.git' "$PIPELINE" || true)
fetches=$(grep -Fc 'git -C "${VAUCHI_CI_SCRIPTS_DIR}" fetch --depth=1 origin "${VAUCHI_SCRIPTS_REF}"' "$PIPELINE" || true)
checkouts=$(grep -Fc 'git -C "${VAUCHI_CI_SCRIPTS_DIR}" checkout --detach FETCH_HEAD' "$PIPELINE" || true)
[ "$remotes" -eq "$isolated_dirs" ] || fail "a scripts fetch targets a shared directory"
[ "$fetches" -eq "$isolated_dirs" ] || fail "a scripts checkout does not fetch the exact commit"
[ "$checkouts" -eq "$isolated_dirs" ] || fail "a scripts checkout can follow a mutable ref"

[ "$failed" -eq 0 ] || exit 1
echo "PASS: core security policy uses immutable scripts provenance"
