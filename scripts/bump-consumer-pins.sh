#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# Open a pin-bump MR in every consumer that ci/consumer-pins.json assigns
# to bump:consumer-pins (ADR-026 amendment 2026-09-30, #434). The native
# pin is rewritten by vauchi/scripts' core-pins.py; a Cargo consumer also
# gets `cargo update --workspace`, so the MR carries a lockfile its
# pipeline can build with --locked. Merging stays with the maintainer.
# Each MR opens as a Draft and goes through the ready queue (queue_mr).
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
ALIGN_CONTRACT_PINS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/align-contract-pins.py"

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

# Ready queue (vauchi/private#1179, maintainer 2026-11-09): one live ready
# MR per repository, so merging what is ready merges a tested tree. A bump
# MR opens as a Draft; behind a ready MR whose pipeline is not red it stays
# Draft and both MRs say so. Its turn comes in session-init's "READY QUEUE"
# list: `just gitlab mr-rebase`, then `just gitlab mr-ready`.
queue_mr() {
    local repo=$1 project=$2 iid=$3 url=$4 status other ahead="" pipeline body
    status=$(api GET "$API/projects/$project/merge_requests?state=opened&per_page=100")
    if [ "$status" != 200 ]; then
        echo "$repo: ERROR: listing open MRs failed (HTTP $status); $url stays Draft" >&2
        return 1
    fi
    for other in $(python3 -c 'import json, sys
mrs = json.load(open(sys.argv[1]))
print(" ".join(str(i) for i in sorted(m["iid"] for m in mrs if not m["draft"] and m["iid"] != int(sys.argv[2]))))' \
            "$WORK/response.json" "$iid"); do
        status=$(api GET "$API/projects/$project/merge_requests/$other")
        if [ "$status" != 200 ]; then
            echo "$repo: ERROR: reading !$other failed (HTTP $status); $url stays Draft" >&2
            return 1
        fi
        pipeline=$(python3 -c 'import json, sys
print((json.load(open(sys.argv[1])).get("head_pipeline") or {}).get("status") or "")' "$WORK/response.json")
        # A ready MR with a red pipeline cannot be merged as it is: it holds nothing.
        case "$pipeline" in
            failed|canceled) ;;
            *) ahead=$other; break ;;
        esac
    done
    if [ -z "$ahead" ]; then
        body=$(python3 -c 'import json, sys; print(json.dumps({"title": f"chore: pin Core to v{sys.argv[1]}"}))' "$VERSION")
        status=$(api PUT "$API/projects/$project/merge_requests/$iid" "$body")
        if [ "$status" != 200 ]; then
            echo "$repo: ERROR: marking $url ready failed (HTTP $status); it stays Draft" >&2
            return 1
        fi
        echo "$repo: opened $url"
        return 0
    fi
    body=$(python3 -c 'import json, sys
iid, ahead, repo = sys.argv[1:4]
print(json.dumps({"body": f"ready-queue: waiting behind !{ahead}, which is ready, so this MR stays Draft. "
                  f"After !{ahead} merges: `just gitlab mr-rebase {iid} {repo}`, then `just gitlab mr-ready {iid} {repo}`."}))' \
        "$iid" "$ahead" "$repo")
    status=$(api POST "$API/projects/$project/merge_requests/$iid/notes" "$body")
    if [ "$status" != 201 ]; then
        echo "$repo: ERROR: $url waits behind !$ahead but its comment failed (HTTP $status)" >&2
        return 1
    fi
    body=$(python3 -c 'import json, sys
print(json.dumps({"body": f"ready-queue: !{sys.argv[1]} waits for this MR to merge, then rebases on it."}))' "$iid")
    status=$(api POST "$API/projects/$project/merge_requests/$ahead/notes" "$body")
    if [ "$status" != 201 ]; then
        echo "$repo: ERROR: commenting on !$ahead failed (HTTP $status)" >&2
        return 1
    fi
    echo "$repo: opened $url (Draft, waiting behind !$ahead)"
}

# unvetted_crates <repo> <dir>: "crate version" for each crate the bumped
# lockfile brings that cargo-vet does not cover. New dependencies get their
# exemptions by hand, after review (maintainer, 2026-11-09), so the bump
# names them on its MR instead of adding them (private#1183).
unvetted_crates() {
    local repo=$1 dir=$2 out
    [ -f "$dir/supply-chain/config.toml" ] || return 0
    if ! (cd "$dir" && cargo vet --version) >/dev/null 2>&1; then
        echo "$repo: cargo-vet is not installed here; security:vet will tell" >&2
        return 0
    fi
    if out=$(cd "$dir" && cargo vet --locked 2>&1); then
        return 0
    fi
    out=$(printf '%s\n' "$out" | sed -n 's/^ *\([A-Za-z0-9_-]*\):\([^ ]*\) missing .*/\1 \2/p')
    if [ -z "$out" ]; then
        echo "$repo: cargo vet failed without naming a crate; security:vet will tell" >&2
        return 0
    fi
    printf '%s\n' "$out"
}

# vet_note <repo> <project> <iid> <crates>: says on the MR why security:vet
# will be red. A failed comment is reported, never a failed bump.
vet_note() {
    local repo=$1 project=$2 iid=$3 crates=$4 body status
    body=$(python3 -c 'import json, sys
crates = [line.split() for line in sys.argv[1].splitlines() if line.strip()]
listed = "\n".join(f"- `{name} {version}`" for name, version in crates)
print(json.dumps({"body": (
    f"cargo-vet: this bump brings {len(crates)} crate(s) with no audit or exemption, "
    "so `security:vet` fails until they are reviewed:\n\n" + listed + "\n\n"
    "After review, add each as `[[exemptions.<crate>]]` in `supply-chain/config.toml` "
    "on this branch; never `[[audits]]` (VSC01).")}))' "$crates")
    status=$(api POST "$API/projects/$project/merge_requests/$iid/notes" "$body")
    [ "$status" = 201 ] || echo "$repo: WARNING: the cargo-vet note on !$iid failed (HTTP $status)" >&2
}

# bump_one <repo> <kind> <file> [<kind> <file>...]: a consumer may pin Core in
# several files (e2e: Cargo.toml and ci/pins.env, private#1034); they move in
# one clone, one commit, one branch and one MR.
bump_one() {
    local repo=$1 dir="$WORK/$1" project="vauchi%2F$1" current="" pinned kind file moved=0 url status contract_changes
    shift
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

    while [ "$#" -ge 2 ]; do
        kind=$1
        file=$2
        shift 2
        pinned=$(python3 "$PINS" read --kind "$kind" "$dir/$file")
        [ -n "$current" ] || current=$pinned
        contract_changes=""
        if [ "$kind" = cargo ]; then
            contract_changes=$(python3 "$ALIGN_CONTRACT_PINS" "$dir/$file")
        fi
        if [ "$pinned" = "v$VERSION" ] && [ -z "$contract_changes" ]; then
            continue
        fi
        python3 "$PINS" bump --kind "$kind" --version "$VERSION" "$dir/$file" >/dev/null
        # --workspace re-resolves only what the rewritten manifest changed. Not
        # `-p <crate>`: e2e locks relay's vauchi-protocol beside its own, and a
        # bare name is ambiguous there (#558).
        if [ "$kind" = cargo ]; then
            (cd "$(dirname "$dir/$file")" && cargo update --workspace)
        fi
        moved=$((moved + 1))
    done
    if [ "$moved" -eq 0 ]; then
        echo "$repo: already pins v$VERSION"
        return 0
    fi
    local unvetted
    unvetted=$(unvetted_crates "$repo" "$dir")

    git -C "$dir" checkout -q -b "$BRANCH"
    git -C "$dir" -c user.name="$BOT_NAME" -c user.email="$BOT_EMAIL" commit -q -a \
        -m "chore: pin Core to v$VERSION" \
        -m "Opened by Core's release pipeline (bump:consumer-pins). $repo pinned $current."
    # GitLab's internal pre-receive call can fail for one push and pass the
    # next; one blip left cli unbumped for Core 0.84.1 (#704).
    local attempt=1
    until git -C "$dir" push -q -u origin "$BRANCH"; do
        if [ "$attempt" -ge 3 ]; then
            echo "$repo: ERROR: push of $BRANCH declined $attempt times" >&2
            return 1
        fi
        echo "$repo: push declined, retrying ($attempt of 3)" >&2
        attempt=$((attempt + 1))
        sleep "${PUSH_RETRY_DELAY:-10}"
    done

    local body
    body=$(python3 -c 'import json, sys
version, repo, current, branch = sys.argv[1:5]
print(json.dumps({
    "source_branch": branch,
    "target_branch": "main",
    "title": f"Draft: chore: pin Core to v{version}",
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
    local iid
    iid=$(json_field iid)
    url=$(json_field web_url)
    [ -z "$unvetted" ] || vet_note "$repo" "$project" "$iid" "$unvetted"
    queue_mr "$repo" "$project" "$iid" "$url"
}

if [ "${MODE:-}" = one ]; then
    bump_one "${@:4}"
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
# One line per consumer repo, in table order: repo kind file [kind file...].
python3 "$PINS" consumers --table "$TABLE" --bumped-by bump:consumer-pins |
    awk '!($1 in files) { order[++n] = $1 }
         { files[$1] = files[$1] " " $2 " " $3 }
         END { for (i = 1; i <= n; i++) print order[i] files[order[i]] }' > "$WORK/consumers"
while read -r repo pin_files; do
    echo "── $repo ($pin_files)"
    # shellcheck disable=SC2086 # kind and file pairs; no pin path holds a space
    if ! bash "$0" --one "v$VERSION" "$TABLE" "$PINS" "$repo" $pin_files < /dev/null; then
        FAILED="$FAILED $repo"
    fi
done < "$WORK/consumers"

if [ -n "$FAILED" ]; then
    echo "Not bumped:$FAILED" >&2
    exit 1
fi
