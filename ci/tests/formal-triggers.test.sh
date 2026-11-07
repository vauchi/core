#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The formal checks run on a merge request only when it changes what they
# check (vauchi/private#1032, decision D2), so a path missing from a job's
# trigger list lets a broken proof merge unnoticed until the nightly. Two
# such gaps existed: the Shamir and delta Kani harnesses and the gate's
# device-link Quint model sat outside test:formal's paths (#950). This
# checks the contract in .gitlab-ci.yml:
#
# - test:formal-code runs the Kani harnesses and wipe tests, and its paths
#   cover every harness file, the module it proves, the wipe tests,
#   Cargo.lock and the script.
# - test:formal-models runs the Tamarin and Quint gate, and its paths cover
#   every model file and the script; models read no Rust, so Rust changes
#   do not start it.
# - both also run on the nightly schedule and on release tags.
# - every Kani harness is named in the script, run or listed as run by
#   hand, and every harness the script runs exists.
# - a pipeline that runs test:formal-models keeps running when another job
#   fails: the workflow turns off rust-base.yml's on_job_failure: all for a
#   merge request changing the models' paths, the nightly and release tags,
#   in rules that come before the ones they shadow (vauchi/private#1115).
#
# Usage: ci/tests/formal-triggers.test.sh [ci-config] [formal-script]
#        (from the core repo root)

set -eu
# Trigger globs are data here; word-splitting them must not expand them
# against the working tree.
set -f

ci_config="${1:-.gitlab-ci.yml}"
formal_script="${2:-ci/formal-verification.sh}"
failures=0

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

job_block() {
    awk -v job="$1" '
        $0 == job ":" { in_job = 1; next }
        in_job && /^[^[:space:]#]/ { exit }
        in_job { print }
    ' "$ci_config"
}

# The `changes` paths of a job: YAML list items that name a file or glob.
trigger_paths() {
    printf '%s\n' "$1" | sed -n 's/^ *- *"\{0,1\}\([A-Za-z][^"]*[/.][^"]*\)"\{0,1\} *$/\1/p' |
        grep -v '^if:' | grep -v '^job:' || true
}

# GitLab globs as shell patterns: `*` in a case pattern also matches `/`.
covered() { # file paths
    c_file=$1
    for c_glob in $2; do
        c_pat=$(printf '%s' "$c_glob" | sed 's#\*\*/#*#g; s#\*\*#*#g')
        # shellcheck disable=SC2254 # the pattern is meant to glob
        case "$c_file" in $c_pat) return 0 ;; esac
    done
    return 1
}

check_job() { # job set
    block=$(job_block "$1")
    if [ -z "$block" ]; then
        fail "$1 is not in $ci_config"
        return 1
    fi
    printf '%s\n' "$block" | grep -q "FORMAL_SET=$2 sh $formal_script" ||
        fail "$1 must run FORMAL_SET=$2 sh $formal_script"
    printf '%s\n' "$block" | grep -q 'CI_PIPELINE_SOURCE == "schedule"' ||
        fail "$1 must run on the nightly schedule"
    printf '%s\n' "$block" | grep -q 'CI_COMMIT_TAG' ||
        fail "$1 must run on release tags"
    return 0
}

require_covered() { # job paths file...
    r_job=$1
    r_paths=$2
    shift 2
    for r_file in "$@"; do
        [ -e "$r_file" ] || continue
        if covered "$r_file" "$r_paths"; then
            echo "PASS: $r_job triggers on $r_file"
        else
            fail "$r_job does not trigger on $r_file"
        fi
    done
}

if job_block "test:formal" | grep -q .; then
    fail "test:formal still exists; it runs as test:formal-code and test:formal-models"
fi

harness_files=$(grep -rl 'kani::proof' vauchi-core/src | sort)
[ -n "$harness_files" ] || fail "no Kani harness found under vauchi-core/src"

# A harness file proves its sibling module: foo_proofs.rs proves foo.rs,
# dir/proofs.rs proves dir/mod.rs or dir.rs.
proved_modules=""
for h in $harness_files; do
    case "$h" in
        */proofs.rs) d=${h%/proofs.rs}; proved_modules="$proved_modules $d/mod.rs $d.rs" ;;
        *_proofs.rs) proved_modules="$proved_modules ${h%_proofs.rs}.rs" ;;
    esac
done

if check_job "test:formal-code" code; then
    code_paths=$(trigger_paths "$(job_block test:formal-code)")
    # shellcheck disable=SC2086 # word lists of repository paths
    require_covered test:formal-code "$code_paths" $harness_files $proved_modules \
        vauchi-core/tests/wipe_on_drop_tests.rs vauchi-core/tests/heap_wipe_on_drop_tests.rs \
        Cargo.lock "$formal_script"
fi

model_files=$(find vauchi-core/verification vauchi-app/verification \
    -name '*.spthy' -o -name '*.qnt' 2>/dev/null | sort)
[ -n "$model_files" ] || fail "no Tamarin or Quint model found under */verification"

if check_job "test:formal-models" models; then
    model_paths=$(trigger_paths "$(job_block test:formal-models)")
    # shellcheck disable=SC2086 # word list of repository paths
    require_covered test:formal-models "$model_paths" $model_files "$formal_script"
    for p in $model_paths; do
        case "$p" in
            *.rs | */src/*) fail "test:formal-models triggers on Rust ($p); the models read none" ;;
        esac
    done
fi

# The first workflow rule whose `if:` line contains $1.
first_rule() {
    awk -v pat="$1" '
        $0 == "workflow:" { in_wf = 1; next }
        in_wf && /^[^[:space:]#]/ { exit }
        in_wf && /^    - / { if (done) exit; in_rule = index($0, pat) > 0; if (in_rule) done = 1 }
        in_wf && in_rule { print }
    ' "$ci_config"
}

# The line of the first workflow rule whose `if:` line is exactly $1.
# Through the environment: awk -v would read the regex's \d as an escape.
rule_line() {
    RULE="$1" awk '
        $0 == "workflow:" { in_wf = 1; next }
        in_wf && /^[^[:space:]#]/ { exit }
        in_wf && $0 == "    - if: " ENVIRON["RULE"] { print NR; exit }
    ' "$ci_config"
}

mr_rule=$(first_rule 'CI_PIPELINE_SOURCE == "merge_request_event"')
if printf '%s\n' "$mr_rule" | grep -q 'on_job_failure: none'; then
    echo "PASS: the first merge request rule turns off fail-fast"
else
    fail "the first merge request workflow rule must set on_job_failure: none, or a failing job cancels test:formal-models"
fi
if job_block test:formal-models | grep -q .; then
    # From the rules only: trigger_paths also matches the script line.
    wanted=$(trigger_paths "$(job_block test:formal-models | sed -n '/^  rules:/,$p')" | sort -u)
    got=$(trigger_paths "$mr_rule" | sort -u)
    [ "$wanted" = "$got" ] ||
        fail "the workflow's fail-fast rule must name exactly test:formal-models' paths; job: $(echo $wanted) workflow: $(echo $got)"
fi

nightly_rule=$(first_rule '$CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH')
if printf '%s\n' "$nightly_rule" | grep -q 'CI_PIPELINE_SOURCE == "schedule"' &&
    printf '%s\n' "$nightly_rule" | grep -q 'on_job_failure: none' &&
    printf '%s\n' "$nightly_rule" | grep -q 'on_new_commit: none'; then
    echo "PASS: the nightly on main turns off fail-fast and keeps main's merge guard"
else
    fail "the first default-branch workflow rule must be the schedule, with on_job_failure: none and on_new_commit: none"
fi

tag_rule=$(first_rule 'CI_COMMIT_TAG')
tag_line=$(rule_line '$CI_COMMIT_TAG =~ /^v\d+\.\d+\.\d+$/')
push_line=$(rule_line '$CI_PIPELINE_SOURCE == "push"')
if printf '%s\n' "$tag_rule" | grep -q 'on_job_failure: none' &&
    [ -n "$tag_line" ] && [ -n "$push_line" ] && [ "$tag_line" -lt "$push_line" ]; then
    echo "PASS: a release tag turns off fail-fast before the push rule"
else
    fail "a release-tag workflow rule with on_job_failure: none must come before the push rule"
fi

# shellcheck disable=SC2086 # word list of repository paths
harnesses=$(grep -h -A4 'kani::proof' $harness_files | sed -n 's/^ *fn \([a-z0-9_]*\).*/\1/p' | sort -u)
for name in $harnesses; do
    if grep -qw "$name" "$formal_script"; then
        echo "PASS: $formal_script names harness $name"
    else
        fail "harness $name is neither run by $formal_script nor listed there as run by hand"
    fi
done

scripted=$(awk '/^for harness in/ { in_list = 1; next } in_list { print } /; do$/ { in_list = 0 }' \
    "$formal_script" | tr -d ' \\;' | sed 's/do$//' | grep -E '^[a-z0-9_]+$' || true)
for name in $scripted; do
    printf '%s\n' "$harnesses" | grep -qx "$name" ||
        fail "$formal_script runs harness $name, which no kani::proof defines"
done

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "OK: every formal check triggers on what it checks"
