#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# After a push to Core's main, open the follow-up MRs the merge owes
# (vauchi/private#1010): in vauchi/spec, drop the `name core!NNNN` lines of
# ci/claim-names.pending whose MR is now merged (check:claim-names fails a
# pending name that is on main); in vauchi/features, flip the scenarios whose
# scenario annotation (the marker, a feature stem and a scenario name) the
# push added from planned to implemented. What changes is decided by follow_ups.py beside this file;
# this script clones, commits, pushes and opens the MRs. Merging stays with
# the maintainer; each MR pipeline proves the change. A failure in one
# repository never stops the other; the job exits non-zero at the end.
#
# Usage: open-follow-up-mrs.sh <before-sha> <after-sha>
#        (CI_COMMIT_BEFORE_SHA and CI_COMMIT_SHA of the main pipeline)
# Env:   PROJECT_ACCESS_TOKEN  group token: git push and MR API
#        CI_API_V4_URL         GitLab API base
#        CONSUMER_GIT_BASE     where spec and features live (tests use local bare repos)
#        CORE_PROJECT          URL-encoded Core project path (default vauchi%2Fcore)
#        CORE_DIR              the Core checkout to diff (default .)

set -euo pipefail

BEFORE="${1:?usage: open-follow-up-mrs.sh <before-sha> <after-sha>}"
AFTER="${2:?after-sha}"
API="${CI_API_V4_URL:-https://gitlab.com/api/v4}"
GIT_BASE="${CONSUMER_GIT_BASE:-https://vauchi-gitlab-runner:${PROJECT_ACCESS_TOKEN}@gitlab.com/vauchi}"
CORE_PROJECT="${CORE_PROJECT:-vauchi%2Fcore}"
CORE_DIR="${CORE_DIR:-.}"
HERE=$(cd "$(dirname "$0")" && pwd)
LOGIC="$HERE/follow_ups.py"
SHORT="${AFTER:0:8}"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Status first, body second: an error body must never be parsed as data.
api() {
    local method=$1 url=$2 data=${3:-} status
    : > "$WORK/response.json"
    if [ -n "$data" ]; then
        status=$(curl -sS -o "$WORK/response.json" -w '%{http_code}' --request "$method" \
            --header "PRIVATE-TOKEN: ${PROJECT_ACCESS_TOKEN}" \
            --header "Content-Type: application/json" --data "$data" "$url") || status=000
    else
        status=$(curl -sS -o "$WORK/response.json" -w '%{http_code}' --request "$method" \
            --header "PRIVATE-TOKEN: ${PROJECT_ACCESS_TOKEN}" "$url") || status=000
    fi
    printf '%s' "$status"
}

json_field() {
    python3 -c 'import json, sys
data = json.load(open(sys.argv[1]))
if isinstance(data, list):
    data = data[0] if data else {}
print(data.get(sys.argv[2]) or "")' "$WORK/response.json" "$1"
}

# The first push to a branch has no before-sha; GitLab sends all zeros.
case "$BEFORE" in
    0000000000000000000000000000000000000000|"") RANGE="$AFTER~1..$AFTER" ;;
    *) RANGE="$BEFORE..$AFTER" ;;
esac

IIDS=""
for sha in $(git -C "$CORE_DIR" rev-list "$RANGE"); do
    status=$(api GET "$API/projects/$CORE_PROJECT/repository/commits/$sha/merge_requests")
    if [ "$status" != 200 ]; then
        echo "ERROR: merge requests of $sha → HTTP $status" >&2
        exit 1
    fi
    IIDS="$IIDS $(python3 "$LOGIC" merged-iids "$WORK/response.json")"
done
# shellcheck disable=SC2086
IIDS=$(printf '%s\n' $IIDS | sort -un | tr '\n' ' ' | sed 's/ $//')
git -C "$CORE_DIR" diff "$RANGE" -- '*.rs' | python3 "$LOGIC" scenarios-added > "$WORK/scenarios"
echo "Core $RANGE: merged MRs: ${IIDS:-none}; annotations added: $(wc -l < "$WORK/scenarios" | tr -d ' ')"

status=$(api GET "$API/user")
if [ "$status" != 200 ]; then
    echo "ERROR: cannot resolve the bot identity ($API/user → HTTP $status)" >&2
    exit 1
fi
BOT_NAME=$(json_field name)
BOT_EMAIL=$(json_field commit_email)
[ -n "$BOT_EMAIL" ] || BOT_EMAIL=$(json_field email)

change_spec() {
    # shellcheck disable=SC2086
    python3 "$LOGIC" drop-pending "$1/ci/claim-names.pending" --iid $IIDS
}

change_features() {
    python3 "$LOGIC" flip "$1" --scenarios-file "$WORK/scenarios"
}

# open_mr <repo> <branch> <title> <changer>: the changer edits the clone and
# prints the summary that becomes the commit body and the MR description.
open_mr() {
    local repo=$1 branch=$2 title=$3 changer=$4 dir="$WORK/$1" project="vauchi%2F$1" status url body
    git clone -q --depth 1 "$GIT_BASE/$repo.git" "$dir"
    if git -C "$dir" ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
        status=$(api GET "$API/projects/$project/merge_requests?state=opened&source_branch=$branch")
        url=""
        [ "$status" = 200 ] && url=$(json_field web_url)
        if [ -n "$url" ]; then
            echo "$repo: already under review: $url"
            return 0
        fi
        echo "$repo: ERROR: branch $branch exists without an open merge request (HTTP $status)" >&2
        return 1
    fi
    "$changer" "$dir" > "$WORK/summary.$repo"
    sed 's/^/  /' "$WORK/summary.$repo"
    if git -C "$dir" diff --quiet; then
        echo "$repo: nothing to do"
        return 0
    fi
    git -C "$dir" checkout -q -b "$branch"
    git -C "$dir" -c user.name="$BOT_NAME" -c user.email="$BOT_EMAIL" commit -q -a \
        -m "$title" -m "$(cat "$WORK/summary.$repo")" \
        -m "Opened by Core's main pipeline (open:follow-ups) for Core $SHORT (vauchi/private#1010)."
    # GitLab's internal pre-receive call can fail for one push and pass the
    # next; one blip left cli unbumped for Core 0.84.1 (#704).
    local attempt=1
    until git -C "$dir" push -q -u origin "$branch"; do
        if [ "$attempt" -ge 3 ]; then
            echo "$repo: ERROR: push of $branch declined $attempt times" >&2
            return 1
        fi
        echo "$repo: push declined, retrying ($attempt of 3)" >&2
        attempt=$((attempt + 1))
        sleep "${PUSH_RETRY_DELAY:-10}"
    done
    body=$(python3 -c 'import json, sys
title, branch, summary, short = sys.argv[1:5]
print(json.dumps({
    "source_branch": branch,
    "target_branch": "main",
    "title": title,
    "description": (
        summary + "\n\nOpened by Core'"'"'s main pipeline (`open:follow-ups`, "
        f"vauchi/private#1010) for Core {short}. Merge once this pipeline is green."
    ),
    "remove_source_branch": True,
}))' "$title" "$branch" "$(cat "$WORK/summary.$repo")" "$SHORT")
    status=$(api POST "$API/projects/$project/merge_requests" "$body")
    if [ "$status" != 201 ]; then
        echo "$repo: ERROR: pushed $branch but opening the MR failed (HTTP $status)" >&2
        return 1
    fi
    echo "$repo: opened $(json_field web_url)"
}

FAILED=""
if [ -n "$IIDS" ]; then
    open_mr spec "chore/core-$SHORT-landed" \
        "chore: the claim names of Core MR ${IIDS// /, } are on main, so their pending lines go" \
        change_spec || FAILED="$FAILED spec"
else
    echo "spec: no merged Core MR in the range, nothing to do"
fi
if [ -s "$WORK/scenarios" ]; then
    open_mr features "chore/core-$SHORT-implemented" \
        "feat: the scenarios whose tests landed with Core $SHORT are implemented" \
        change_features || FAILED="$FAILED features"
else
    echo "features: no scenario annotation added in the range, nothing to do"
fi

if [ -n "$FAILED" ]; then
    echo "Not opened:$FAILED" >&2
    exit 1
fi
