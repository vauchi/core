#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
failed=0

fail() {
    echo "FAIL: $1" >&2
    failed=$((failed + 1))
}

check_policy() {
    file=$1
    label=$2

    dev_line=$(grep -nF "if [[ \"\$VERSION\" == dev-* ]]; then" "$file" \
        | head -1 | cut -d: -f1 || true)
    key_line=$(grep -nF "elif [[ -n \"\${COSIGN_KEY:-}\" ]]; then" "$file" \
        | head -1 | cut -d: -f1 || true)
    ci_line=$(grep -nF "elif [[ -n \"\${CI:-}\" ]]; then" "$file" \
        | head -1 | cut -d: -f1 || true)

    [ -n "$dev_line" ] || fail "$label has no version-first development skip"
    [ -n "$key_line" ] || fail "$label checks signing material before artifact class"
    [ -n "$ci_line" ] || fail "$label has no fail-closed CI release branch"

    if [ -n "$dev_line" ] && [ -n "$key_line" ] && [ "$dev_line" -ge "$key_line" ]; then
        fail "$label can expose COSIGN_KEY to a development package"
    fi
    if [ -n "$key_line" ] && [ -n "$ci_line" ] && [ "$key_line" -ge "$ci_line" ]; then
        fail "$label checks missing CI signing material before a supplied key"
    fi

    grep -Fq 'Development package — skipping checksum signing' "$file" \
        || fail "$label does not explain the development-only skip"
    grep -Fq 'COSIGN_KEY is required in CI for release signing' "$file" \
        || fail "$label no longer fails closed for unsigned CI releases"
}

check_policy "$ROOT/scripts/package-xcframework.sh" "Apple packaging"
check_policy "$ROOT/scripts/package-android.sh" "Android packaging"

[ "$failed" -eq 0 ] || exit 1
echo "PASS: development packages cannot consume release signing material"
