#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later

# Open a pin-bump MR in every consumer that ci/consumer-pins.json assigns
# to bump:consumer-pins (ADR-026 amendment 2026-09-30, #434). The native
# pin is rewritten by vauchi/scripts' core-pins.py; a Cargo consumer also
# gets `cargo update --workspace`, so the MR carries a lockfile its
# pipeline can build with --locked. Merging stays with the maintainer.
#
# One consumer failing never stops the others; the job reports every
# failure and exits non-zero at the end.
#
# Usage: bump-consumer-pins.sh <vX.Y.Z> <consumer-pins.json> <core-pins.py>
# Env:   PROJECT_ACCESS_TOKEN  group token: git push and MR API
#        CI_API_V4_URL         GitLab API base
#        CONSUMER_GIT_BASE     where consumer repos live (tests use local bare repos)

set -euo pipefail

# Each consumer is bumped in its own bash process (--one): bash ignores
# `set -e` inside a function called from an `||` list, so an in-process
# bump that failed half-way would carry on and open an MR for it.
if [ "${1:-}" = "--one" ]; then
    MODE=one
    shift
fi

VERSION="${1:?usage: bump-consumer-pins.sh <vX.Y.Z> <table> <core-pins.py>}"
VERSION="${VERSION#v}"
TABLE="${2:?table}"
PINS="${3:?core-pins.py}"
API="${CI_API_V4_URL:-https://gitlab.com/api/v4}"
GIT_BASE="${CONSUMER_GIT_BASE:-https://vauchi-gitlab-runner:${PROJECT_ACCESS_TOKEN}@gitlab.com/vauchi}"
BRANCH="chore/bump-core-${VERSION}"

if [ "${MODE:-}" != one ]; then
    WORK=$(mktemp -d)
    export WORK
    trap 'rm -rf "$WORK"' EXIT
fi

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

bump_one() {
    local repo=$1 kind=$2 file=$3 dir="$WORK/$1" project="vauchi%2F$1" current url status
    git clone -q --depth 1 "$GIT_BASE/$repo.git" "$dir"

    if git -C "$dir" ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
        status=$(api GET "$API/projects/$project/merge_requests?state=opened&source_branch=$BRANCH")
        url=""
        [ "$status" = 200 ] && url=$(json_field web_url)
        if [ -n "$url" ]; then
            echo "$repo: already under review: $url"
            return 0
        fi
        echo "$repo: ERROR: branch $BRANCH exists without an open merge request (HTTP $status)" >&2
        return 1
    fi

    current=$(python3 "$PINS" read --kind "$kind" "$dir/$file")
    if [ "$current" = "v$VERSION" ]; then
        echo "$repo: already pins v$VERSION"
        return 0
    fi

    python3 "$PINS" bump --kind "$kind" --version "$VERSION" "$dir/$file" >/dev/null
    # --workspace re-resolves only what the rewritten manifest changed. Not
    # `-p <crate>`: e2e locks relay's vauchi-protocol beside its own, and a
    # bare name is ambiguous there (#558).
    if [ "$kind" = cargo ]; then
        (cd "$(dirname "$dir/$file")" && cargo update --workspace)
    fi

    git -C "$dir" checkout -q -b "$BRANCH"
    git -C "$dir" -c user.name="$BOT_NAME" -c user.email="$BOT_EMAIL" commit -q -a \
        -m "chore: pin Core to v$VERSION" \
        -m "Opened by Core's release pipeline (bump:consumer-pins). $repo pinned $current."
    git -C "$dir" push -q -u origin "$BRANCH"

    local body
    body=$(python3 -c 'import json, sys
version, repo, current, branch = sys.argv[1:5]
print(json.dumps({
    "source_branch": branch,
    "target_branch": "main",
    "title": f"chore: pin Core to v{version}",
    "description": (
        f"Core v{version} was released; {repo} pinned {current}.\n\n"
        "Opened by Core'"'"'s release pipeline (`bump:consumer-pins`, ADR-026 "
        "amendment 2026-09-30). Merge once this pipeline is green; a red "
        f"pipeline means {repo} needs changes for this Core release."
    ),
    "remove_source_branch": True,
}))' "$VERSION" "$repo" "$current" "$BRANCH")
    status=$(api POST "$API/projects/$project/merge_requests" "$body")
    if [ "$status" != 201 ]; then
        echo "$repo: ERROR: pushed $BRANCH but opening the MR failed (HTTP $status)" >&2
        return 1
    fi
    echo "$repo: opened $(json_field web_url)"
}

if [ "${MODE:-}" = one ]; then
    bump_one "$4" "$5" "$6"
    exit 0
fi

status=$(api GET "$API/user")
if [ "$status" != 200 ]; then
    echo "ERROR: cannot resolve the bot identity ($API/user → HTTP $status)" >&2
    exit 1
fi
BOT_NAME=$(json_field name)
BOT_EMAIL=$(json_field commit_email)
[ -n "$BOT_EMAIL" ] || BOT_EMAIL=$(json_field email)
export BOT_NAME BOT_EMAIL

FAILED=""
python3 "$PINS" consumers --table "$TABLE" --bumped-by bump:consumer-pins > "$WORK/consumers"
while read -r repo kind file; do
    echo "── $repo ($kind $file)"
    if ! bash "$0" --one "v$VERSION" "$TABLE" "$PINS" "$repo" "$kind" "$file" < /dev/null; then
        FAILED="$FAILED $repo"
    fi
done < "$WORK/consumers"

if [ -n "$FAILED" ]; then
    echo "Not bumped:$FAILED" >&2
    exit 1
fi
