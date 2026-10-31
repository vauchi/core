#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# scripts/check-ignored-results.sh holds Core's let_underscore_must_use
# allows at the count in .ignored-results-baseline (vauchi/private#747).
# This runs it on fixture trees: an allow added above the baseline fails, an
# allow removed without lowering the baseline fails, and a mention in a
# comment is not an allow.

set -eu

script="$(cd "$(dirname "$0")/../.." && pwd)/scripts/check-ignored-results.sh"
failures=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

# A tree with two allows: a plain one, one inside cfg_attr beside another
# lint, plus a comment and a doc comment that only name the lint.
make_tree() {
    root="$tmp/$1"
    mkdir -p "$root/vauchi-core/src" "$root/vauchi-app/src" "$root/target/debug"
    cat > "$root/vauchi-core/src/lib.rs" <<'EOF'
// `#[allow(clippy::let_underscore_must_use)]` needs a justification.
/// Mentions clippy::let_underscore_must_use in a doc comment.
#[allow(clippy::let_underscore_must_use)]
fn a() {}
EOF
    cat > "$root/vauchi-app/src/lib.rs" <<'EOF'
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::let_underscore_must_use))]
EOF
    # Build output is not Core's source.
    printf '#[allow(clippy::let_underscore_must_use)]\n' > "$root/target/debug/gen.rs"
    printf '%s\n' "$2" > "$root/.ignored-results-baseline"
    printf '%s' "$root"
}

expect() {
    name="$1"; want="$2"; root="$3"
    if sh "$script" "$root" > "$tmp/out" 2>&1; then got=0; else got=$?; fi
    if [ "$got" -eq "$want" ]; then
        echo "PASS: $name (exit $got)"
    else
        fail "$name: expected exit $want, got $got: $(cat "$tmp/out")"
    fi
}

expect "the count equal to the baseline passes" 0 "$(make_tree equal 2)"
expect "an allow above the baseline fails" 1 "$(make_tree above 1)"
expect "a removed allow with the baseline not lowered fails" 1 "$(make_tree below 3)"

root=$(make_tree lower 3)
sh "$script" "$root" > "$tmp/out" 2>&1 || true
if grep -q 'lower .ignored-results-baseline to 2' "$tmp/out"; then
    echo "PASS: a lower count names the new baseline"
else
    fail "a lower count must name the new baseline: $(cat "$tmp/out")"
fi

root=$(make_tree missing 2)
rm "$root/.ignored-results-baseline"
expect "a missing baseline is a usage error" 2 "$root"

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed" >&2
    exit 1
fi
echo "All ignored-results ratchet checks passed"
