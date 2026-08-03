<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Fuzz Targets

12 fuzz targets covering protocol parsing, crypto boundaries, and data
serialization.

## Targets

| Target | Module | What it fuzzes |
|--------|--------|---------------|
| `fuzz_decode_message` | `network::protocol` | Wire protocol message decoding |
| `fuzz_qr_payload` | `exchange::qr` | QR code payload parsing |
| `fuzz_unpad` | `crypto` | PKCS7 unpadding |
| `fuzz_delta_decode` | `sync` | Delta-encoded contact card decoding |
| `fuzz_recovery_claim` | `recovery` | Recovery claim parsing |
| `fuzz_nfc_payload` | `exchange::nfc` | NFC payload parsing (VNFC magic bytes) |
| `fuzz_ble_payload` | `exchange::ble` | BLE advertisement payload parsing |
| `fuzz_ble_advertisement` | `exchange::ble` | BLE advertisement frame parsing |
| `fuzz_exchange_payload` | `exchange` | Generic exchange payload parsing |
| `fuzz_encrypted_exchange` | `exchange` | Encrypted exchange envelope parsing |
| `fuzz_ratchet_state` | `crypto::ratchet` | Ratchet state deserialization |
| `fuzz_shamir_reconstruct` | `backup::key_shard` | Arbitrary shard parsing plus a guaranteed valid Shamir reconstruction path |

## Running Locally

```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Run a single target (runs until crash or Ctrl-C)
cd vauchi-core/fuzz
cargo fuzz run fuzz_decode_message

# Run with time limit (5 minutes)
cargo fuzz run fuzz_qr_payload -- -max_total_time=300

# Run all targets (5 minutes each)
for target in $(cargo fuzz list); do
  echo "=== $target ==="
  cargo fuzz run "$target" -- -max_total_time=300
done
```

To exercise guardian shard reconstruction for one million iterations:

```bash
cd vauchi-core/fuzz
cargo +nightly fuzz run fuzz_shamir_reconstruct -- -runs=1000000
```

## CI

The `fuzz:nightly` scheduled pipeline job runs all 12 targets for 5 minutes each. Crashes are
saved as artifacts for 30 days. Enable by setting `FUZZ_ENABLED=true` on the schedule.

## Adding a New Target

1. Create `fuzz_targets/fuzz_<name>.rs`
2. Add `[[bin]]` entry to `Cargo.toml`
3. Update the target list in `.gitlab-ci.yml` `fuzz:nightly` job
4. Update this README
