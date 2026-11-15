#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# scripts/check-empty-then-steps.py holds Core's empty cucumber `then`
# bodies at the count in .empty-then-steps-baseline (vauchi/private#862).
# This runs it on fixture trees: an empty `then` counts (also stacked
# attributes and comment-only bodies), a non-empty `then` and empty
# given/when bodies do not, and a baseline that drifted either way fails.

set -eu

script="$(cd "$(dirname "$0")/../.." && pwd)/scripts/check-empty-then-steps.py"
failures=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

# Three empty `then` bodies: a plain one, two stacked on one comment-only
# body. The checked `then` and the empty given/when must not count.
make_tree() {
    root="$tmp/$1"
    mkdir -p "$root/vauchi-core/tests/steps"
    cat > "$root/vauchi-core/tests/steps/fixture.rs" <<'EOF'
#[then("an empty assertion")]
fn empty_then(_world: &mut VauchiWorld) {}

#[then("a checked assertion")]
fn checked_then(world: &mut VauchiWorld) {
    assert!(world.ready());
}

#[given("an empty setup")]
fn empty_given(_world: &mut VauchiWorld) {}

#[when("an empty action")]
fn empty_when(_world: &mut VauchiWorld) {}

#[then("stacked one")]
#[then("stacked two")]
fn stacked(_world: &mut VauchiWorld) {
    // nothing but a comment
}
EOF
    printf '# a comment line is not the count\n%s\n' "$2" > "$root/.empty-then-steps-baseline"
    printf '%s' "$root"
}

expect() {
    name="$1"; want="$2"; root="$3"
    if python3 "$script" "$root" > "$tmp/out" 2>&1; then got=0; else got=$?; fi
    if [ "$got" -eq "$want" ]; then
        echo "PASS: $name (exit $got)"
    else
        fail "$name: expected exit $want, got $got: $(cat "$tmp/out")"
    fi
}

expect "the count equal to the baseline passes" 0 "$(make_tree equal 3)"
expect "an empty then above the baseline fails" 1 "$(make_tree above 2)"
expect "a removed empty then with the baseline not lowered fails" 1 "$(make_tree below 4)"

root=$(make_tree lower 4)
python3 "$script" "$root" > "$tmp/out" 2>&1 || true
if grep -q 'lower .empty-then-steps-baseline to 3' "$tmp/out"; then
    echo "PASS: a lower count names the new baseline"
else
    fail "a lower count must name the new baseline: $(cat "$tmp/out")"
fi

root=$(make_tree missing 3)
rm "$root/.empty-then-steps-baseline"
expect "a missing baseline is a usage error" 2 "$root"

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed" >&2
    exit 1
fi
echo "All empty-then-steps ratchet checks passed"
