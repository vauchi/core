#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Runs Core's formal checks (#757): the Kani parser proofs and the Tamarin
# model of the in-person QR exchange. The Tamarin model must still fail
# without identity binding, or a weakened lemma would pass unnoticed.
#
# Usage: ci/formal-verification.sh   (from the core repo root)
# Tools are cached under $FORMAL_HOME (default: ./.formal).
set -eu

KANI_VERSION=0.68.0
TAMARIN_VERSION=1.12.0
MAUDE_VERSION=3.5.1
FORMAL_HOME=${FORMAL_HOME:-$PWD/.formal}
mkdir -p "$FORMAL_HOME/bin"
PATH="$FORMAL_HOME/bin:$PATH"
export PATH

fetch() { # url dest
    status=$(curl -sL -o "$2" -w '%{http_code}' "$1") || status=000
    [ "$status" = 200 ] || {
        echo "FAIL: $1 → HTTP $status" >&2
        exit 1
    }
}

# ── Kani ──────────────────────────────────────────────────────────────
# Kani compiles with its own nightly rustc; .cargo/config.toml's sccache
# wrapper is not in this image and would not cache that toolchain anyway.
RUSTC_WRAPPER=
export RUSTC_WRAPPER
# Under $FORMAL_HOME, so a CI cache of that one directory keeps both the
# binaries and the toolchain `kani setup` downloads.
KANI_HOME=$FORMAL_HOME/kani
export KANI_HOME
if ! cargo kani --version 2>/dev/null | grep -q "$KANI_VERSION"; then
    cargo install --locked --root "$FORMAL_HOME" kani-verifier --version "$KANI_VERSION"
    cargo kani setup
fi
for harness in \
    unpad_never_panics_and_only_accepts_a_prefixed_plaintext \
    parse_exchange_payload_never_panics_and_reads_each_field_at_its_offset \
    ble_advertisement_parse_never_panics_and_reads_each_field_at_its_offset \
    recovery_claim_parse_never_panics_and_reads_each_field_at_its_offset; do
    cargo kani -p vauchi-core --harness "$harness"
done

# ── Tamarin ───────────────────────────────────────────────────────────
if [ ! -x "$FORMAL_HOME/bin/tamarin-prover" ]; then
    scratch=$(mktemp -d)
    fetch "https://github.com/tamarin-prover/tamarin-prover/releases/download/$TAMARIN_VERSION/tamarin-prover-$TAMARIN_VERSION-linux64-ubuntu.tar.gz" "$scratch/tamarin.tar.gz"
    tar -xzf "$scratch/tamarin.tar.gz" -C "$FORMAL_HOME/bin" tamarin-prover
    fetch "https://github.com/maude-lang/Maude/releases/download/Maude$MAUDE_VERSION/Maude-$MAUDE_VERSION-linux-x86_64.zip" "$scratch/maude.zip"
    mkdir -p "$FORMAL_HOME/maude"
    python3 -m zipfile -e "$scratch/maude.zip" "$FORMAL_HOME/maude"
    chmod +x "$FORMAL_HOME/maude/maude"
    printf '#!/bin/sh\nMAUDE_LIB="%s" exec "%s" "$@"\n' \
        "$FORMAL_HOME/maude" "$FORMAL_HOME/maude/maude" > "$FORMAL_HOME/bin/maude"
    chmod +x "$FORMAL_HOME/bin/maude"
    rm -rf "$scratch"
fi

# Tamarin reads files in the locale's encoding; CI images default to C,
# where the model's UTF-8 comments fail to decode.
LC_ALL=C.UTF-8
export LC_ALL
model=vauchi-core/verification/qr_exchange.spthy
out=$(tamarin-prover --prove "$model" 2>&1) || {
    printf '%s\n' "$out"
    exit 1
}
printf '%s\n' "$out" | grep -E '^  [a-z_]+ \((exists|all)-trace' || true
for lemma in executable session_key_secrecy no_misbinding; do
    printf '%s\n' "$out" | grep -qE "^  $lemma \((exists|all)-traces?\): verified" || {
        echo "FAIL: Tamarin lemma $lemma is not verified" >&2
        exit 1
    }
done

out=$(tamarin-prover --prove -DNO_IDENTITY_BINDING "$model" 2>&1) || {
    printf '%s\n' "$out"
    exit 1
}
printf '%s\n' "$out" | grep -qE '^  no_misbinding \(all-traces\): falsified' || {
    echo "FAIL: without identity binding, no_misbinding must be falsified" >&2
    exit 1
}
echo "formal verification OK"
