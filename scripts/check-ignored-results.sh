#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Holds Core's `let_underscore_must_use` allows at the count recorded in
# .ignored-results-baseline (vauchi/private#747). Each allow lets a
# `Result` be dropped unseen, so a new one fails here. The baseline only
# goes down: an MR that removes allows lowers it in the same change, or
# this fails too, so the number never drifts above the real count.
#
# Usage: check-ignored-results.sh [core_root]
# Exit 0 at the baseline, 1 above or below it, 2 on a usage error.

set -eu

root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
baseline_file="$root/.ignored-results-baseline"

if [ ! -f "$baseline_file" ]; then
    echo "check-ignored-results: $baseline_file not found" >&2
    exit 2
fi
baseline=$(grep -vE '^[[:space:]]*(#|$)' "$baseline_file" | tr -d '[:space:]' || true)
case "$baseline" in
    '' | *[!0-9]*)
        echo "check-ignored-results: $baseline_file must hold one number, not '$baseline'" >&2
        exit 2
        ;;
esac

# Only attribute lines count; a comment that names the lint is not an
# allow. The crates are the vauchi-* directories, never target/.
count=$(find "$root"/vauchi-* -name '*.rs' -type f -exec \
    grep -hE '^[[:space:]]*#!?\[.*let_underscore_must_use' {} + 2>/dev/null \
    | wc -l | tr -d '[:space:]')

if [ "$count" -gt "$baseline" ]; then
    echo "check-ignored-results: $count let_underscore_must_use allows, baseline $baseline." >&2
    echo "Handle or return the Result instead of adding an allow (vauchi/private#747)." >&2
    exit 1
fi
if [ "$count" -lt "$baseline" ]; then
    echo "check-ignored-results: $count let_underscore_must_use allows, baseline $baseline." >&2
    echo "Allows were removed: lower .ignored-results-baseline to $count in this MR." >&2
    exit 1
fi
echo "check-ignored-results: OK, $count let_underscore_must_use allows (baseline $baseline)"
