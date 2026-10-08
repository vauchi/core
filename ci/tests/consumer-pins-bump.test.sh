#!/bin/sh

# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later

# Behaviour of scripts/bump-consumer-pins.sh (bump:consumer-pins, #434):
# local bare repos stand in for the consumers, and fake curl / cargo
# record what the script asks of GitLab and Cargo.
#
# Needs CORE_PINS (path to vauchi/scripts' scripts/core-pins.py).

set -eu

CORE_PINS=${CORE_PINS:?set CORE_PINS to vauchi/scripts/scripts/core-pins.py}
HERE=$(cd "$(dirname "$0")/../.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

git_quiet() { git -c init.defaultBranch=main -c user.name=t -c user.email=t@example.invalid "$@" >/dev/null 2>&1; }

remote() {
    name=$1
    shift
    git_quiet init -q "$T/src/$name"
    while [ "$#" -gt 1 ]; do
        mkdir -p "$(dirname "$T/src/$name/$1")"
        printf '%s' "$2" > "$T/src/$name/$1"
        shift 2
    done
    git_quiet -C "$T/src/$name" add -A
    git_quiet -C "$T/src/$name" commit -q -m init
    git_quiet clone -q --bare "$T/src/$name" "$T/remotes/$name.git"
}

CARGO_TOML='[dependencies]
vauchi-core = { git = "https://gitlab.com/vauchi/core.git", tag = "v0.69.0", version = "=0.69.0" }
vauchi-app = { git = "https://gitlab.com/vauchi/core.git", tag = "v0.69.0", version = "=0.69.0" }
'
mkdir -p "$T/remotes" "$T/bin"
remote cli Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"
remote linux-qt core-ref "v0.70.0
"
remote tui Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"
git_quiet -C "$T/src/tui" push -q "$T/remotes/tui.git" HEAD:refs/heads/chore/bump-core-0.70.0
remote e2e README.md "no manifest
"
remote relay Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"
git_quiet -C "$T/src/relay" push -q "$T/remotes/relay.git" HEAD:refs/heads/chore/bump-core-0.70.0
# web-demo: GitLab refuses the MR. linux-gtk: cargo update fails.
remote web-demo Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"
remote linux-gtk Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"

cat > "$T/table.json" <<'EOF'
{ "consumers": [
  { "repo": "cli", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "linux-qt", "kind": "ref-file", "file": "core-ref", "bumped_by": "bump:consumer-pins" },
  { "repo": "tui", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "e2e", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "relay", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "web-demo", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "linux-gtk", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" },
  { "repo": "android", "kind": "gradle", "file": "app/build.gradle.kts", "bumped_by": "trigger:android" }
] }
EOF

# Fake curl: answers the three GitLab calls the script makes and logs them.
# tui has an open MR for its branch; relay's branch has none.
cat > "$T/bin/curl" <<'EOF'
#!/bin/sh
out=""; method=GET; url=""; data=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift ;;
        --request) method=$2; shift ;;
        --data) data=$2; shift ;;
        -w|--header) shift ;;
        -*) ;;
        *) url=$1 ;;
    esac
    shift
done
echo "$method $url $data" >> "$FAKE_LOG"
case "$method $url" in
    "GET "*/user) [ -n "${FAKE_USER_STATUS:-}" ] && { printf '%s' '{}' > "$out"; printf '%s' "$FAKE_USER_STATUS"; exit 0; }
        body='{"name":"Vauchi Bot","email":"bot@example.invalid"}'; status=200 ;;
    "GET "*vauchi%2Ftui/merge_requests*) body='[{"web_url":"https://gitlab.example/tui/-/merge_requests/7"}]'; status=200 ;;
    "GET "*/merge_requests*) body='[]'; status=200 ;;
    "POST "*vauchi%2Fweb-demo/merge_requests) body='{"message":"500 Internal Server Error"}'; status=500 ;;
    "POST "*/merge_requests) body='{"web_url":"https://gitlab.example/mr/new"}'; status=201 ;;
    *) body='{}'; status=404 ;;
esac
printf '%s' "$body" > "$out"
printf '%s' "$status"
EOF
cat > "$T/bin/cargo" <<'EOF'
#!/bin/sh
echo "cargo $* (in $(basename "$PWD"))" >> "$FAKE_LOG"
case "$PWD" in */linux-gtk) echo "error: failed to select a version" >&2; exit 101 ;; esac
printf '# lock updated\n' > Cargo.lock
EOF
chmod +x "$T/bin/curl" "$T/bin/cargo"

status=0
PATH="$T/bin:$PATH" FAKE_LOG="$T/log" PROJECT_ACCESS_TOKEN=token \
    CI_API_V4_URL=https://gitlab.example/api/v4 CONSUMER_GIT_BASE="$T/remotes" \
    bash "$HERE/scripts/bump-consumer-pins.sh" v0.70.0 "$T/table.json" "$CORE_PINS" \
    > "$T/out" 2>&1 || status=$?
cat "$T/out"

[ "$status" -ne 0 ] || fail "a consumer that cannot be bumped (e2e) must make the job red"
grep -qx "Not bumped: e2e relay web-demo linux-gtk" "$T/out" \
    || fail "the failure summary must name exactly e2e, relay, web-demo and linux-gtk"
grep -q "web-demo: ERROR: pushed chore/bump-core-0.70.0 but opening the MR failed (HTTP 500)" "$T/out" \
    || fail "a refused MR must be reported with its HTTP status"
if git -C "$T/remotes/linux-gtk.git" rev-parse -q --verify "refs/heads/chore/bump-core-0.70.0" >/dev/null; then
    fail "a failed cargo update must push nothing"
fi
grep -q 'vauchi%2Flinux-gtk/merge_requests' "$T/log" && fail "a failed cargo update must open no MR"
grep -q 'vauchi%2Fe2e/merge_requests' "$T/log" && fail "e2e has no manifest and must get no MR"
if git -C "$T/remotes/e2e.git" rev-parse -q --verify "refs/heads/chore/bump-core-0.70.0" >/dev/null; then
    fail "e2e must get no branch"
fi

branch=chore/bump-core-0.70.0
git_quiet clone -q --branch "$branch" "$T/remotes/cli.git" "$T/check-cli" || fail "cli must get branch $branch"
grep -q 'tag = "v0.70.0", version = "=0.70.0"' "$T/check-cli/Cargo.toml" || fail "cli's Cargo.toml must pin v0.70.0"
grep -q '# lock updated' "$T/check-cli/Cargo.lock" || fail "cli's Cargo.lock must be committed with the bump"
[ "$(git -C "$T/check-cli" log -1 --format=%ae)" = bot@example.invalid ] || fail "the commit must be authored by the bot identity"
grep -q "cargo update --workspace (in cli)" "$T/log" || fail "cargo update must re-resolve the bumped manifest"
# e2e locks relay's vauchi-protocol beside its own (#558): a -p by name is
# ambiguous there, and the bump is only the manifest change anyway.
grep -q "cargo update.* -p " "$T/log" && fail "cargo update must not name crates"
[ "$(grep -c '^POST .*vauchi%2Fcli/merge_requests' "$T/log")" -eq 1 ] || fail "cli must get exactly one MR"
grep '^POST .*vauchi%2Fcli/merge_requests' "$T/log" | grep -q '"source_branch": *"chore/bump-core-0.70.0"' \
    || fail "cli's MR must come from the bump branch"
grep '^POST .*vauchi%2Fcli/merge_requests' "$T/log" | grep -q '"target_branch": *"main"' \
    || fail "cli's MR must target main"

if git -C "$T/remotes/linux-qt.git" rev-parse -q --verify "refs/heads/$branch" >/dev/null; then
    fail "linux-qt already pins v0.70.0 and must get no branch"
fi
grep -q 'vauchi%2Flinux-qt/merge_requests' "$T/log" && fail "linux-qt must get no MR"

grep -q '^POST .*vauchi%2Ftui/merge_requests' "$T/log" && fail "tui already has an open MR and must not get another"
grep -q "merge_requests/7" "$T/out" || fail "the job must point at tui's open MR"

grep -q '^POST .*vauchi%2Frelay/merge_requests' "$T/log" && fail "relay's orphan branch must not be overwritten"
grep -q "relay" "$T/out" || fail "relay's orphan branch must be reported"

grep -q 'android' "$T/log" && fail "android is bumped by trigger:android, not this job"

for repo in cli linux-qt tui relay; do
    [ "$(git -C "$T/remotes/$repo.git" rev-parse main)" = "$(git -C "$T/src/$repo" rev-parse HEAD)" ] \
        || fail "$repo's main must be untouched"
done

# Without a bot identity nothing may be pushed: commits would carry no
# verified author and the pre-receive hook would reject them anyway.
remote gtk2 Cargo.toml "$CARGO_TOML" Cargo.lock "# lock
"
printf '%s\n' '{ "consumers": [ { "repo": "gtk2", "kind": "cargo", "file": "Cargo.toml", "bumped_by": "bump:consumer-pins" } ] }' > "$T/table2.json"
status=0
PATH="$T/bin:$PATH" FAKE_LOG="$T/log2" FAKE_USER_STATUS=401 PROJECT_ACCESS_TOKEN=token \
    CI_API_V4_URL=https://gitlab.example/api/v4 CONSUMER_GIT_BASE="$T/remotes" \
    bash "$HERE/scripts/bump-consumer-pins.sh" v0.70.0 "$T/table2.json" "$CORE_PINS" \
    > "$T/out2" 2>&1 || status=$?
[ "$status" -ne 0 ] || fail "an unresolvable bot identity must make the job red"
grep -q "cannot resolve the bot identity" "$T/out2" || fail "the identity failure must be named"
if git -C "$T/remotes/gtk2.git" rev-parse -q --verify "refs/heads/chore/bump-core-0.70.0" >/dev/null; then
    fail "without a bot identity nothing may be pushed"
fi

echo "consumer-pins-bump: OK"
