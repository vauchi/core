#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later

set -eu

[ "${RUSTC_WRAPPER:-}" = sccache ] || {
    echo 'FAIL: Rust compilation must use sccache' >&2
    exit 1
}
[ "${CARGO_INCREMENTAL:-}" = 0 ] || {
    echo 'FAIL: incremental compilation prevents Rust sccache reuse' >&2
    exit 1
}

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/src"
cat >"$scratch/Cargo.toml" <<'EOF'
[package]
name = "vauchi-cache-probe"
version = "0.0.0"
edition = "2021"
EOF
printf 'pub fn value() -> u32 { 42 }\n' >"$scratch/src/lib.rs"
printf 'fn main() { println!("{}", vauchi_cache_probe::value()); }\n' >"$scratch/src/main.rs"

cache_hits() {
    sccache --show-stats >"$scratch/stats"
    awk '/^Cache hits[[:space:]]+[0-9]+$/ { print $NF; found = 1; exit }
        END { if (!found) exit 1 }' "$scratch/stats"
}

# Separate target directories force recompilation instead of Cargo freshness.
for build in first second; do
    hits_before=$(cache_hits)
    cargo build --quiet --offline --manifest-path "$scratch/Cargo.toml" \
        --target-dir "$scratch/$build" --jobs "${CARGO_BUILD_JOBS:-4}"
    [ "$("$scratch/$build/debug/vauchi-cache-probe")" = 42 ] || {
        echo 'FAIL: cached build returned the wrong result' >&2
        exit 1
    }
done
hits_after=$(cache_hits)
[ "$hits_after" -gt "$hits_before" ] || {
    echo 'FAIL: rebuilding into a fresh target directory produced no cache hit' >&2
    cat "$scratch/stats" >&2
    exit 1
}
echo "PASS: second build reused sccache ($hits_before -> $hits_after hits)"
