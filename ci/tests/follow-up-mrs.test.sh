#!/bin/sh

# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later

# Behaviour of scripts/open-follow-up-mrs.sh (open:follow-ups, private#1010):
# local bare repos stand in for spec and features, a local Core history
# stands in for the push, and a fake curl records what the script asks of
# GitLab. CC-27: every assertion below was observed red before the script
# existed (the test ran against a missing file and failed on the branch
# checks), then green.

set -eu

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

mkdir -p "$T/remotes" "$T/bin"

remote spec ci/claim-names.pending '# header line
a_second_key_offer_with_the_same_echo_is_refused core!2124
a_qr_session_stores_the_exchange_timestamp core!2125
qr_auto_advance_names_the_contact_after_the_peer_qr core!2125
some_future_name core!2130
'
remote features contact_exchange.feature 'Feature: Contact exchange

  @exchange @planned
  Scenario: A QR contact carries its real exchange timestamp
    Given a QR exchange
    Then the contact is stamped

  @exchange @audio @planned
  Scenario: In a mutual QR exchange one side emits the audio challenge first
    Given two scanners
    Then one emits first

  @exchange @implemented
  Scenario: Already implemented elsewhere
    Then nothing changes
' other.feature 'Feature: Other

  @planned
  Scenario: A QR contact carries its real exchange timestamp
    Then the same name in another file is left alone
'

# A Core history of three commits: base, a merge of !2125 that annotates two
# tests (one scenario exists in features, one does not), and a docs-only
# commit whose annotation-looking line is not Rust and must be ignored.
git_quiet init -q "$T/src/core"
mkdir -p "$T/src/core/tests" "$T/src/core/docs"
printf 'fn base() {}\n' > "$T/src/core/tests/a.rs"
git_quiet -C "$T/src/core" add -A
git_quiet -C "$T/src/core" commit -q -m base
BASE=$(git -C "$T/src/core" rev-parse HEAD)
printf 'fn base() {}\n// @scenario: contact_exchange :: A QR contact carries its real exchange timestamp\nfn a_qr_session_stores_the_exchange_timestamp() {}\n// @scenario: contact_exchange :: Not in features yet\nfn other() {}\n' > "$T/src/core/tests/a.rs"
git_quiet -C "$T/src/core" add -A
git_quiet -C "$T/src/core" commit -q -m "fix: stamp (private#948)"
MERGED=$(git -C "$T/src/core" rev-parse HEAD)
printf '// @scenario: contact_exchange :: In a mutual QR exchange one side emits the audio challenge first\n' > "$T/src/core/docs/note.md"
git_quiet -C "$T/src/core" add -A
git_quiet -C "$T/src/core" commit -q -m "docs: note"
AFTER=$(git -C "$T/src/core" rev-parse HEAD)
SHORT=$(printf '%s' "$AFTER" | cut -c1-8)

# Fake curl: the merged-MR lookup per commit, the bot identity, the
# open-MR lookup per branch, and the MR creation. FAKE_OPEN_MRS=1 makes
# every branch look like it already has an open MR.
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
    "GET "*/user) body='{"name":"Vauchi Bot","email":"bot@example.invalid"}'; status=200 ;;
    "GET "*/repository/commits/$FAKE_MERGED_SHA/merge_requests)
        body='[{"iid":2125,"state":"merged"},{"iid":2200,"state":"opened"}]'; status=200 ;;
    "GET "*/repository/commits/*/merge_requests) body='[]'; status=200 ;;
    "GET "*/merge_requests?state=opened*)
        if [ -n "${FAKE_OPEN_MRS:-}" ]; then body='[{"web_url":"https://gitlab.example/x/-/merge_requests/9"}]'; else body='[]'; fi
        status=200 ;;
    "POST "*/merge_requests) body='{"web_url":"https://gitlab.example/mr/new"}'; status=201 ;;
    *) body='{}'; status=404 ;;
esac
printf '%s' "$body" > "$out"
printf '%s' "$status"
EOF
chmod +x "$T/bin/curl"

run() {
    status=0
    PATH="$T/bin:$PATH" FAKE_LOG="$T/log" FAKE_MERGED_SHA="$MERGED" PROJECT_ACCESS_TOKEN=token \
        CI_API_V4_URL=https://gitlab.example/api/v4 CONSUMER_GIT_BASE="$T/remotes" \
        CORE_DIR="$T/src/core" PUSH_RETRY_DELAY=0 \
        bash "$HERE/scripts/open-follow-up-mrs.sh" "$@" > "$T/out" 2>&1 || status=$?
    cat "$T/out"
}

# 1. A push that merged !2125 and annotated two tests.
: > "$T/log"
run "$BASE" "$AFTER"
[ "$status" -eq 0 ] || fail "a push with follow-ups must succeed (rc=$status)"

git_quiet clone -q --branch "chore/core-$SHORT-landed" "$T/remotes/spec.git" "$T/check-spec" \
    || fail "spec must get branch chore/core-$SHORT-landed"
grep -q 'core!2125' "$T/check-spec/ci/claim-names.pending" && fail "the merged MR's pending lines must go"
grep -q 'a_second_key_offer_with_the_same_echo_is_refused core!2124' "$T/check-spec/ci/claim-names.pending" \
    || fail "a pending line of an MR that did not merge stays"
grep -q 'some_future_name core!2130' "$T/check-spec/ci/claim-names.pending" || fail "an unrelated pending line stays"
grep -q '^# header line' "$T/check-spec/ci/claim-names.pending" || fail "the header stays"
[ "$(git -C "$T/check-spec" log -1 --format=%ae)" = bot@example.invalid ] || fail "the spec commit must be authored by the bot"
git -C "$T/check-spec" log -1 --format=%B | grep -q 'a_qr_session_stores_the_exchange_timestamp' \
    || fail "the spec commit body must name the dropped claim names"

git_quiet clone -q --branch "chore/core-$SHORT-implemented" "$T/remotes/features.git" "$T/check-features" \
    || fail "features must get branch chore/core-$SHORT-implemented"
grep -B1 'Scenario: A QR contact carries its real exchange timestamp' "$T/check-features/contact_exchange.feature" \
    | grep -q '^  @exchange @implemented$' || fail "the annotated scenario must flip to implemented, keeping its other tags"
grep -B1 'Scenario: In a mutual QR exchange' "$T/check-features/contact_exchange.feature" \
    | grep -q '@planned' || fail "a scenario annotated only in a non-Rust file stays planned"
grep -B1 'Scenario: A QR contact carries' "$T/check-features/other.feature" | grep -q '@planned' \
    || fail "the same scenario name in another feature file is left alone"
grep -q 'not planned in features: contact_exchange :: Not in features yet' "$T/out" \
    || fail "an annotation without a planned scenario must be reported, not invented"
[ "$(grep -c '^POST .*/merge_requests' "$T/log")" -eq 2 ] || fail "exactly two MRs must be opened"
grep -q 'POST .*vauchi%2Fspec/merge_requests .*"source_branch": "chore/core-'"$SHORT"'-landed"' "$T/log" \
    || fail "the spec MR must come from its branch"
grep -q '"remove_source_branch": true' "$T/log" || fail "the follow-up branch is deleted on merge"

# 2. The same push again: both branches exist with open MRs, nothing is pushed twice.
: > "$T/log"
FAKE_OPEN_MRS=1 run "$BASE" "$AFTER"
[ "$status" -eq 0 ] || fail "a repeated run must succeed (rc=$status)"
grep -q 'spec: already under review' "$T/out" || fail "a spec branch under review is reported, not re-pushed"
grep -q 'features: already under review' "$T/out" || fail "a features branch under review is reported, not re-pushed"
grep -q '^POST ' "$T/log" && fail "a repeated run must open no MR"

# 3. A push with no merged MR and no annotation: nothing to do, exit 0.
: > "$T/log"
run "$MERGED" "$AFTER"
[ "$status" -eq 0 ] || fail "a push without follow-ups must succeed (rc=$status)"
grep -q 'spec: no merged Core MR in the range, nothing to do' "$T/out" || fail "no merged MR means no spec work"
grep -q 'features: no scenario annotation added in the range, nothing to do' "$T/out" \
    || fail "an annotation in a non-Rust file is no features work"
grep -q '^POST ' "$T/log" && fail "nothing to do must open no MR"

# 4. GitLab's first-push before-sha (all zeros) means the last commit only.
: > "$T/log"
run 0000000000000000000000000000000000000000 "$MERGED"
[ "$status" -eq 0 ] || fail "an all-zero before-sha must be handled (rc=$status)"
grep -q "Core $MERGED~1..$MERGED" "$T/out" || fail "an all-zero before-sha diffs the last commit only"

echo "follow-up-mrs.test.sh: OK"
