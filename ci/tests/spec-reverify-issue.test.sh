#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
# Behaviour of scripts/open-spec-reverify-issue.sh (notify:spec,
# vauchi/private#874): a fake curl answers the two GitLab calls and logs
# them. A release opens one issue naming the tag; a second run reuses the
# open one; a refused POST is reported with its status and fails.
set -eu
HERE=$(cd "$(dirname "$0")/../.." && pwd)
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fail() {
    echo "FAIL: $*" >&2
    exit 1
}

mkdir -p "$T/bin"
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
    "GET "*/issues\?*) if [ "${FAKE_OPEN:-}" = 1 ]; then body='[{"web_url":"https://gitlab.example/spec/-/issues/4"}]'; else body='[]'; fi; status=200 ;;
    "POST "*/issues) if [ "${FAKE_POST_STATUS:-}" = 403 ]; then body='{"message":"403 Forbidden"}'; status=403; else body='{"web_url":"https://gitlab.example/spec/-/issues/5"}'; status=201; fi ;;
    *) body='{}'; status=404 ;;
esac
printf '%s' "$body" > "$out"
printf '%s' "$status"
EOF
chmod +x "$T/bin/curl"

run() { # $1=log name, rest=env assignments
    log="$T/$1"
    shift
    : > "$log"
    status=0
    env PATH="$T/bin:$PATH" FAKE_LOG="$log" PROJECT_ACCESS_TOKEN=token \
        CI_API_V4_URL=https://gitlab.example/api/v4 "$@" \
        bash "$HERE/scripts/open-spec-reverify-issue.sh" v0.70.0 > "$T/out" 2>&1 || status=$?
}

# A release with no open issue: one GET, one POST naming the tag, exit 0.
run first
[ "$status" -eq 0 ] || fail "a fresh release must exit 0 (got $status): $(cat "$T/out")"
grep -q '^GET https://gitlab.example/api/v4/projects/vauchi%2Fspec/issues?state=opened&search=' "$T/first" || fail "must look for an open issue first"
grep -q '^POST https://gitlab.example/api/v4/projects/vauchi%2Fspec/issues ' "$T/first" || fail "must open the issue on the spec project"
grep -q 'Core v0.70.0' "$T/first" || fail "the issue title must name the tag"
grep -q 'check:claim-annotations' "$T/first" || fail "the issue must name the drift checks"
grep -q 'issues/5' "$T/out" || fail "must print the new issue's url"
echo "PASS: a release opens one issue naming the tag"

# The issue is already open: no POST, exit 0, the existing url printed.
run second FAKE_OPEN=1
[ "$status" -eq 0 ] || fail "an open issue must exit 0 (got $status)"
grep -q '^POST' "$T/second" && fail "an open issue must not be duplicated"
grep -q 'issues/4' "$T/out" || fail "must print the open issue's url"
echo "PASS: an open issue for the tag is reused"

# GitLab refuses the POST: non-zero, the status in the output.
run third FAKE_POST_STATUS=403
[ "$status" -ne 0 ] || fail "a refused POST must fail the job"
grep -q 'HTTP 403' "$T/out" || fail "a refused POST must be reported with its status: $(cat "$T/out")"
echo "PASS: a refused POST fails with its status"

# A bare version is accepted and prefixed.
: > "$T/fourth"
env PATH="$T/bin:$PATH" FAKE_LOG="$T/fourth" PROJECT_ACCESS_TOKEN=token CI_API_V4_URL=https://gitlab.example/api/v4 \
    bash "$HERE/scripts/open-spec-reverify-issue.sh" 0.70.0 > /dev/null 2>&1 || fail "a bare version must be accepted"
grep -q 'Core v0.70.0' "$T/fourth" || fail "a bare version must be prefixed with v"
echo "PASS: a bare version is prefixed"
