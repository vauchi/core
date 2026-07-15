#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/.gitlab-ci.yml"
AUTO_TAG="$ROOT/ci/auto-tag-and-dev-rc.yml"
failed=0

last_stage=$(awk '
    /^stages:/ { in_stages = 1; next }
    in_stages && /^  - / { last = $2; next }
    in_stages && /^[^ ]/ { exit }
    END { print last }
' "$PIPELINE")

if [ "$last_stage" != "tag" ]; then
    echo "FAIL: final pipeline stage is '$last_stage', expected 'tag'" >&2
    failed=$((failed + 1))
fi

job=$(awk '
    /^auto-tag:version:/ { in_job = 1 }
    in_job && NR > 1 && /^[^ ]/ && !/^auto-tag:version:/ { exit }
    in_job { print }
' "$AUTO_TAG")
job_stage=$(printf '%s\n' "$job" | awk '/^  stage:/ { print $2; exit }')

if [ "$job_stage" != "tag" ]; then
    echo "FAIL: auto-tag job stage is '$job_stage', expected 'tag'" >&2
    failed=$((failed + 1))
fi

if printf '%s\n' "$job" | grep -q '^  needs:'; then
    echo "FAIL: auto-tag job bypasses stage ordering with needs" >&2
    failed=$((failed + 1))
fi

if [ "$failed" -ne 0 ]; then
    exit 1
fi

echo "PASS: stable tagging follows every prior pipeline stage"
