#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

set -eu

if [ -z "${CI_MERGE_REQUEST_IID:-}" ]; then
    git rev-parse --verify 'origin/main^{commit}'
    exit
fi

case "${CI_MERGE_REQUEST_EVENT_TYPE:-}" in
    merged_result | merge_train)
        baseline=${CI_MERGE_REQUEST_TARGET_BRANCH_SHA:-}
        ;;
    detached)
        baseline=${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}
        ;;
    *)
        echo 'FAIL: missing or unsupported MR pipeline type for semver baseline' >&2
        exit 2
        ;;
esac

case "$baseline" in
    '' | *[!0-9a-fA-F]* | 0000000000000000000000000000000000000000)
        echo 'FAIL: semver baseline requires an immutable MR commit SHA' >&2
        exit 2
        ;;
esac
if [ "${#baseline}" -ne 40 ]; then
    echo 'FAIL: semver baseline requires a full MR commit SHA' >&2
    exit 2
fi
printf '%s\n' "$baseline"
