#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Runs Core's formal checks (#757): the Kani parser proofs, the Tamarin
# protocol models and the Quint model of the device-link machines. Each
# model also runs a control that must fail, or a weakened lemma or
# invariant would pass unnoticed.
#
# Usage: [FORMAL_SET=gate|extended|all] ci/formal-verification.sh
#        (from the core repo root)
# Tools are cached under $FORMAL_HOME (default: ./.formal).
#
# FORMAL_SET picks what runs. `gate` is the merge-request set: Kani, the
# wipe tests, the Tamarin models that answer in minutes and the
# device-link Quint verifies, about 45 minutes together. `extended` is
# the rest: the Bluetooth handshake lemmas, one capped prover run each,
# and the Apalache verifies of the Bluetooth and multi-stage machines,
# which together pushed one job past its hour (private#842). `all`, the
# default for a local run, is both.
set -eu

FORMAL_SET=${FORMAL_SET:-all}
case "$FORMAL_SET" in
    gate | extended | all) ;;
    *)
        echo "FAIL: FORMAL_SET must be gate, extended or all, not '$FORMAL_SET'" >&2
        exit 2
        ;;
esac
wants() { [ "$FORMAL_SET" = all ] || [ "$FORMAL_SET" = "$1" ]; }

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
if wants gate; then
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
fi # wants gate

# ── Wipe on drop: Miri and release (#817) ─────────────────────────────
# Kani cannot model zeroize's barrier; these tests read a dropped value's
# storage back. Miri shows each read is defined behaviour; release shows the
# optimizer keeps the wipe. Kani's own nightly carries Miri too.
if wants gate; then
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
fi # wants gate

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

# One lemma per prover run under a heap cap: ble_handshake.spthy proved
# as a whole exhausts memory (private#842).
tamarin_lemma_expect() {
    l_model=$1
    l_lemma=$2
    l_verdict=$3
    l_flags=$4
    l_out=$(tamarin-prover +RTS -M8G -RTS --prove="$l_lemma" $l_flags "$l_model" 2>&1) || {
        printf '%s\n' "$l_out"
        exit 1
    }
    echo "$l_model --prove=$l_lemma $l_flags:"
    printf '%s\n' "$l_out" | grep -E "^  $l_lemma \((exists|all)-trace" || true
    printf '%s\n' "$l_out" | grep -qE "^  $l_lemma \((exists|all)-traces?\): $l_verdict" || {
        echo "FAIL: Tamarin lemma $l_lemma is not $l_verdict ($l_model --prove=$l_lemma $l_flags)" >&2
        exit 1
    }
}
models=vauchi-core/verification
if wants gate; then
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
tamarin_expect "$models/social_recovery.spthy" verified "-DGROUP_OF_TWO" \
    backup_needs_both_guardians a_group_mate_can_stand_in
tamarin_expect "$models/social_recovery.spthy" falsified "-DGROUP_OF_TWO -DONE_GUARDIAN" \
    backup_needs_both_guardians
tamarin_expect "$models/social_recovery.spthy" verified "-DTHREE_GROUPS" \
    executable backup_needs_both_guardians a_group_mate_can_stand_in
tamarin_expect "$models/social_recovery.spthy" falsified "-DTHREE_GROUPS -DONE_GUARDIAN" \
    backup_needs_both_guardians
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
tamarin_expect "$models/ble_advertisement.spthy" verified "" \
    executable scanner_connects_to_the_displayer_it_scanned
tamarin_expect "$models/ble_advertisement.spthy" falsified "-DNO_SCAN_BINDING" \
    scanner_connects_to_the_displayer_it_scanned
tamarin_diff_expect "$models/ble_advertisement.spthy" verified "-DUNLINKABILITY"
tamarin_diff_expect "$models/ble_advertisement.spthy" falsified "-DUNLINKABILITY -DKEY_DERIVED_TOKEN"
tamarin_diff_expect "$models/mailbox_token.spthy" verified ""
tamarin_diff_expect "$models/mailbox_token.spthy" falsified "-DSTABLE_TOKEN"
tamarin_expect "$models/link_exchange.spthy" falsified "" \
    contact_only_after_approval
tamarin_expect "$models/link_exchange.spthy" verified "-DPIN_CONFIRMED" \
    executable contact_only_after_approval
fi # wants gate
# The six capped runs took 18 minutes on CI (job 17540297472): extended.
if wants extended; then
tamarin_lemma_expect "$models/ble_handshake.spthy" executable verified ""
tamarin_lemma_expect "$models/ble_handshake.spthy" session_key_secrecy verified ""
tamarin_lemma_expect "$models/ble_handshake.spthy" scanner_binds_the_displayer verified ""
tamarin_lemma_expect "$models/ble_handshake.spthy" scanner_binds_the_displayer falsified "-DRADIO_ONLY"
tamarin_lemma_expect "$models/ble_handshake.spthy" displayer_binds_the_scanner falsified ""
tamarin_lemma_expect "$models/ble_handshake.spthy" displayer_binds_the_scanner falsified "-DQR_PRIVATE"
fi # wants extended

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
    q_model=$1
    q_main=$2
    q_verdict=$3
    q_mode=$4
    q_invariant=$5
    # Both violations the model found took 12 steps; 15 keeps a margin
    # without Apalache's cost at 25, which ran 33 minutes on CI. The
    # simulator's Rust evaluator needs glibc 2.39, newer than this image's.
    if [ "$q_mode" = verify ]; then
        set -- verify --max-steps=15
    else
        set -- run --backend=typescript --max-steps=40 --max-samples=20000 --seed=7
    fi
    q_out=$(HOME=$quint_home quint "$@" --main="$q_main" --invariant="$q_invariant" \
        "vauchi-app/verification/$q_model" 2>&1) || true
    echo "$q_model $q_main $q_mode $q_invariant:"
    printf '%s\n' "$q_out" | grep -E '^\[(ok|violation)\]' || true
    printf '%s\n' "$q_out" | grep -q "^\[$q_verdict\]" || {
        printf '%s\n' "$q_out" | tail -20
        echo "FAIL: Quint invariant $q_invariant is not $q_verdict ($q_main $q_mode)" >&2
        exit 1
    }
}
if wants gate; then
quint_expect device_link_machines.qnt reliable_relay ok verify no_device_registered_for_a_failed_joiner
quint_expect device_link_machines.qnt reliable_relay ok verify joiner_seed_implies_registered
# Without the host's expiry check the host registers a device whose
# joiner gave up; a lost response still does, which #808 leaves open.
quint_expect device_link_machines.qnt before_808 violation run no_device_registered_for_a_failed_joiner
quint_expect device_link_machines.qnt lossy_relay violation run no_device_registered_for_a_failed_joiner
fi # wants gate
# Each Apalache verify runs for about 80 s on CI; the eight below are the
# extended set (private#842).
if wants extended; then
# The BLE chrome's stall deadline left the handshake machine holding the
# link (private#842): three violations before #843, none now that a failure
# and a Retry cancel the machine.
for q_inv in no_live_machine_behind_a_failed_screen a_retry_starts_from_no_machine \
    a_contact_is_persisted_only_on_a_live_screen; do
    quint_expect ble_exchange_machine.qnt before_843 violation run "$q_inv"
    quint_expect ble_exchange_machine.qnt failure_cancels_the_machine ok verify "$q_inv"
done
# A multi-stage phase never outlives its budget; random simulation cannot
# draw the 13 silent ticks the sentinel needs, so both run under Apalache.
quint_expect multi_stage_machine.qnt main_today ok verify no_phase_outlives_its_budget
quint_expect multi_stage_machine.qnt no_step_timeout violation verify no_phase_outlives_its_budget
fi # wants extended
echo "formal verification OK ($FORMAL_SET)"
