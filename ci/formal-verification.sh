#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs Core's formal checks (#757): the Kani parser proofs, the Tamarin
# protocol models and the Quint model of the device-link machines. Each
# model also runs a control that must fail, or a weakened lemma or
# invariant would pass unnoticed.
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
# These stub format! or gf_mul, which CBMC cannot finish within the limits;
# each harness says why its stub keeps the proof meaningful.
for harness in \
    cek_wrapped_payload_decode_never_panics_and_reads_each_field_at_its_offset \
    interpolation_never_panics_and_refuses_a_zero_or_repeated_x \
    recover_byte_never_panics \
    reconstruct_refuses_more_shares_than_guardians; do
    cargo kani -p vauchi-core -Z stubbing --harness "$harness"
done

# ── Wipe on drop: Miri and release (#817) ─────────────────────────────
# Kani cannot model zeroize's barrier; these tests read a dropped value's
# storage back. Miri shows each read is defined behaviour; release shows the
# optimizer keeps the wipe. Kani's own nightly carries Miri too.
MIRI_TOOLCHAIN=nightly-2026-08-21
rustup toolchain install "$MIRI_TOOLCHAIN" --profile minimal --component miri,rust-src
# The image has neither clang nor mold, the linker .cargo/config.toml names;
# Kani links through its own driver, these cargo runs do not. The linker
# env override takes precedence over the config. The rustflags one does
# not: CARGO_TARGET_*_RUSTFLAGS appends to the config's array, and mold
# stayed (job 17451589766); RUSTFLAGS makes cargo ignore target rustflags.
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc
RUSTFLAGS="--cfg rust_analyzer"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER RUSTFLAGS
for wipe_test in wipe_on_drop_tests heap_wipe_on_drop_tests; do
    cargo +"$MIRI_TOOLCHAIN" miri test -p vauchi-core --no-default-features --features testing \
        --test "$wipe_test"
    cargo test --release -p vauchi-core --features testing --test "$wipe_test"
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
# tamarin_expect <model> <verdict> "<flags>" <lemma>...: every lemma must
# reach <verdict> (verified or falsified) with those -D flags.
tamarin_expect() {
    t_model=$1
    t_verdict=$2
    t_flags=$3
    shift 3
    # shellcheck disable=SC2086 # the flags are separate words
    t_out=$(tamarin-prover --prove $t_flags "$t_model" 2>&1) || {
        printf '%s\n' "$t_out"
        exit 1
    }
    echo "$t_model $t_flags:"
    printf '%s\n' "$t_out" | grep -E '^  [a-z_]+ \((exists|all)-trace' || true
    for t_lemma in "$@"; do
        printf '%s\n' "$t_out" | grep -qE "^  $t_lemma \((exists|all)-traces?\): $t_verdict" || {
            echo "FAIL: Tamarin lemma $t_lemma is not $t_verdict ($t_model $t_flags)" >&2
            exit 1
        }
    done
}

# tamarin_diff_expect <model> <verdict> "<flags>": the model's two sides
# (`diff(...)`) must be observationally equivalent (verified) or
# distinguishable (falsified) with those -D flags.
tamarin_diff_expect() {
    d_model=$1
    d_verdict=$2
    d_flags=$3
    # shellcheck disable=SC2086 # the flags are separate words
    d_out=$(tamarin-prover --prove --diff $d_flags "$d_model" 2>&1) || {
        printf '%s\n' "$d_out"
        exit 1
    }
    echo "$d_model --diff $d_flags:"
    printf '%s\n' "$d_out" | grep -E '^  DiffLemma:' || true
    printf '%s\n' "$d_out" | grep -qE "^  DiffLemma:  Observational_equivalence : $d_verdict" || {
        echo "FAIL: Tamarin observational equivalence is not $d_verdict ($d_model --diff $d_flags)" >&2
        exit 1
    }
}

models=vauchi-core/verification
tamarin_expect "$models/qr_exchange.spthy" verified "" \
    executable session_key_secrecy no_misbinding
# Without identity binding an attacker re-signs the peer's ephemeral key.
tamarin_expect "$models/qr_exchange.spthy" falsified "-DNO_IDENTITY_BINDING" \
    no_misbinding
tamarin_expect "$models/device_link.spthy" verified "" \
    executable seed_secrecy initiator_agreement joiner_agreement \
    qr_leak_exposes_seed qr_leak_lets_attacker_choose_joiner_identity
tamarin_expect "$models/device_link.spthy" verified "-DHONEST_RELAY" \
    executable seed_secrecy initiator_agreement joiner_agreement \
    photographed_qr_alone_keeps_the_seed
# Without the code comparison the attacker's own request gets the seed.
tamarin_expect "$models/device_link.spthy" falsified "-DHONEST_RELAY -DNO_CODE_CHECK" \
    photographed_qr_alone_keeps_the_seed
# Reciprocity confirmation: signature and asymmetric tokens each stop a
# reflected confirmation; only the signature covers a leaked shared key.
tamarin_expect "$models/reciprocity.spthy" verified "" \
    executable confirmed_means_the_peer_confirmed \
    a_leaked_shared_key_also_needs_the_identity_key
tamarin_expect "$models/reciprocity.spthy" falsified "-DNO_SIGNATURE" \
    a_leaked_shared_key_also_needs_the_identity_key
tamarin_expect "$models/reciprocity.spthy" falsified "-DNO_SIGNATURE -DSYMMETRIC_TOKENS" \
    confirmed_means_the_peer_confirmed
# Recovery acceptance (#796): both rekey designs; the remote one cannot
# authenticate John to Alice, which the exists-trace lemma records.
tamarin_expect "$models/recovery_acceptance.spthy" verified "" \
    executable accept_needs_the_real_recovery john_key_secrecy \
    a_stranger_can_rekey_with_the_recovering_user
tamarin_expect "$models/recovery_acceptance.spthy" verified "-DIN_PERSON" \
    executable accept_needs_the_real_recovery john_key_secrecy \
    alice_rekeys_only_with_an_accepting_contact
# Without #797's check the stolen device and one corrupted helper suffice.
tamarin_expect "$models/recovery_acceptance.spthy" falsified "-DNO_LOST_KEY_CHECK" \
    accept_needs_the_real_recovery
# Social recovery, then rekeying online with every old contact (#796):
# the guardian backup gives Alice each contact's identity key, so both
# sides authenticate although the thief holds the shared keys.
tamarin_expect "$models/social_recovery.spthy" verified "" \
    executable backup_needs_both_guardians john_rekeys_only_with_the_real_alice \
    alice_rekeys_only_with_the_real_john new_key_secrecy
tamarin_expect "$models/social_recovery.spthy" falsified "-DONE_GUARDIAN" \
    backup_needs_both_guardians
tamarin_expect "$models/social_recovery.spthy" falsified "-DNO_PROOF_CHECK" \
    john_rekeys_only_with_the_real_alice
tamarin_expect "$models/social_recovery.spthy" falsified "-DNO_ANSWER_SIGNATURE" \
    alice_rekeys_only_with_the_real_john
tamarin_expect "$models/social_recovery.spthy" falsified "-DUNBOUND_ANSWER" \
    alice_rekeys_only_with_the_real_john
# Card updates (#757): the signature binds the recipient, and the version
# floor stops a relay that only reorders from rolling a card back.
tamarin_expect "$models/card_update.spthy" verified "" \
    executable update_comes_from_the_sender_for_this_recipient \
    each_update_is_applied_once changes_stay_secret
tamarin_expect "$models/card_update.spthy" falsified "-DNO_RECIPIENT_BINDING" \
    update_comes_from_the_sender_for_this_recipient
tamarin_expect "$models/card_update.spthy" falsified "-DNO_VERSION_FLOOR" \
    no_rollback

# Duress cover (#811, ADR-032): a coercer watching the wire cannot tell a
# duress unlock that sends cover from a normal unlock that sends their
# edit — unless they also extract the ratchet keys, the design's limit.
tamarin_diff_expect "$models/duress_cover.spthy" verified ""
tamarin_diff_expect "$models/duress_cover.spthy" falsified "-DCOERCER_HOLDS_RATCHET"

# ── Quint ─────────────────────────────────────────────────────────────
# The device-link host and joiner machines (#757, #808). `quint verify`
# runs Apalache, which needs Java 17+; Quint fetches Apalache and its
# evaluator into $HOME/.quint, so HOME points into the cache.
NODE_VERSION=22.20.0
JRE_VERSION=21.0.8_9
QUINT_VERSION=0.33.0
if [ ! -x "$FORMAL_HOME/node/bin/node" ]; then
    scratch=$(mktemp -d)
    fetch "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.gz" "$scratch/node.tar.gz"
    mkdir -p "$FORMAL_HOME/node"
    tar -xzf "$scratch/node.tar.gz" -C "$FORMAL_HOME/node" --strip-components=1
    rm -rf "$scratch"
fi
if [ ! -x "$FORMAL_HOME/jre/bin/java" ]; then
    scratch=$(mktemp -d)
    jre_tag=$(printf '%s' "$JRE_VERSION" | sed 's/_/%2B/')
    fetch "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-$jre_tag/OpenJDK21U-jre_x64_linux_hotspot_$JRE_VERSION.tar.gz" "$scratch/jre.tar.gz"
    mkdir -p "$FORMAL_HOME/jre"
    tar -xzf "$scratch/jre.tar.gz" -C "$FORMAL_HOME/jre" --strip-components=1
    rm -rf "$scratch"
fi
JAVA_HOME=$FORMAL_HOME/jre
PATH="$FORMAL_HOME/node/bin:$JAVA_HOME/bin:$FORMAL_HOME/quint/node_modules/.bin:$PATH"
export JAVA_HOME PATH
if ! quint --version 2>/dev/null | grep -qx "$QUINT_VERSION"; then
    npm install --prefix "$FORMAL_HOME/quint" --no-audit --no-fund \
        "@informalsystems/quint@$QUINT_VERSION"
fi
quint_home=$FORMAL_HOME/quint-home
mkdir -p "$quint_home"
# quint_expect <main> <ok|violation> <run|verify> <invariant>: exhaustive
# `verify` for the claims, the simulator for controls that must break.
quint_expect() {
    q_main=$1
    q_verdict=$2
    q_mode=$3
    q_invariant=$4
    # Both violations the model found took 12 steps; 15 keeps a margin
    # without Apalache's cost at 25, which ran 33 minutes on CI. The
    # simulator's Rust evaluator needs glibc 2.39, newer than this image's.
    if [ "$q_mode" = verify ]; then
        set -- verify --max-steps=15
    else
        set -- run --backend=typescript --max-steps=40 --max-samples=20000 --seed=7
    fi
    q_out=$(HOME=$quint_home quint "$@" --main="$q_main" --invariant="$q_invariant" \
        vauchi-app/verification/device_link_machines.qnt 2>&1) || true
    echo "device_link_machines.qnt $q_main $q_mode $q_invariant:"
    printf '%s\n' "$q_out" | grep -E '^\[(ok|violation)\]' || true
    printf '%s\n' "$q_out" | grep -q "^\[$q_verdict\]" || {
        printf '%s\n' "$q_out" | tail -20
        echo "FAIL: Quint invariant $q_invariant is not $q_verdict ($q_main $q_mode)" >&2
        exit 1
    }
}
quint_expect reliable_relay ok verify no_device_registered_for_a_failed_joiner
quint_expect reliable_relay ok verify joiner_seed_implies_registered
# Without the host's expiry check the host registers a device whose
# joiner gave up; a lost response still does, which #808 leaves open.
quint_expect before_808 violation run no_device_registered_for_a_failed_joiner
quint_expect lossy_relay violation run no_device_registered_for_a_failed_joiner
echo "formal verification OK"
