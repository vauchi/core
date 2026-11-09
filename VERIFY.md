<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Verify it yourself

Vauchi's security claims are written to be falsified. Each one in the
[claims chapter](https://vauchi.app/spec/claims.html) names the tests and
formal models behind it. This page tells you where those models are, how
to run them, and what they do not cover.

## In 30 seconds: what CI says

Open the
[scheduled pipelines on `main`](https://gitlab.com/vauchi/core/-/pipelines?ref=main&source=schedule)
and look at three jobs in the newest one:

| Job | What it runs |
| --- | --- |
| `test:formal-code` | Kani proofs of the parsers, and the key-wipe tests under Miri |
| `test:formal-models` | Tamarin protocol models and the Quint machines |
| `test:formal-extended` | The slow ones: Bluetooth handshake lemmas, Apalache on two machines |

All three run every night on `main`, where a failure turns the pipeline
red. The code and model jobs also run, and block, on every merge request
that changes what they check.

## Where the models are

| Tool | Files | What they model |
| --- | --- | --- |
| [Tamarin](https://tamarin-prover.com/) | `vauchi-core/verification/*.spthy` (16) | Exchange, device link, card updates, recovery, mailbox tokens, duress cover traffic |
| [Quint](https://quint-lang.org/) | `vauchi-app/verification/*.qnt` (6) | Exchange and device-link state machines, guardian voting and succession |
| [Kani](https://model-checking.github.io/kani/) | `vauchi-core/src/**/*proofs.rs` (6 files) | Parsers of bytes from outside the device never panic and read each field where the spec puts it; the Shamir recovery arithmetic never panics |

Every model also runs with a deliberate weakening (a `-D` flag or an
alternative module) that **must** make it fail. A model that cannot fail
proves nothing; if a control passes, CI goes red. The exact list of
models, flags and expected verdicts is the script itself,
[`ci/formal-verification.sh`](ci/formal-verification.sh): each
`tamarin_expect` line is one claim you can check.

## Run everything

Linux x86-64, from a fresh clone. The script fetches pinned versions of
Tamarin, Maude, Quint, a Java runtime and Kani into `.formal/` on first
run. CI uses the image below; using it too avoids differences between
hosts.

```sh
git clone https://gitlab.com/vauchi/core.git && cd core
docker run --rm -v "$PWD":/core -w /core rust:1.93.1-bookworm \
    sh -c 'FORMAL_SET=models sh ci/formal-verification.sh'
```

`FORMAL_SET` picks the set: `models` (Tamarin and Quint), `code` (Kani
and the wipe tests), `extended` (the slow models), or `all` (the default,
`code` and `models`). CI's own timings: about twenty minutes for
`models`, sixteen for `code`. From a fresh clone with the command above,
`models` took 18 minutes on 8 CPUs and ended `formal verification OK
(models)` (2026-11-09).

## Run one model

Once the first run has installed the tools:

```sh
export PATH="$PWD/.formal/bin:$PATH"
tamarin-prover --prove vauchi-core/verification/qr_exchange.spthy
tamarin-prover --prove -DNO_IDENTITY_BINDING vauchi-core/verification/qr_exchange.spthy
```

The first must report `verified` for `session_key_secrecy` and
`no_misbinding`. The second must report `no_misbinding` as `falsified`:
without the identity binding, an attacker re-signs the peer's ephemeral
key.

## What this does not show

- **The models are written by hand.** No tool checks that a model matches
  the Rust code; the tests named next to each claim are what tie the two
  together.
- **Not modelled**: the double ratchet, the backup format, the relay's HTTP
  API and the Oblivious HTTP path (covered by tests), and lengths and
  timing. The full list is in
  [Formal Verification](https://vauchi.app/docs/developers/formal-verification.html#what-is-not-modelled).
- **Not claimed**: the spec lists what Vauchi does not protect against,
  such as the relay correlating one fetcher's requests by timing, and key
  agreement that is not post-quantum. See the claims chapter.
- **No external audit** has been done.

## Found something?

A model that passes when it should fail, a claim its evidence does not
support, or a gap this page does not mention: report it as described in
[SECURITY.md](SECURITY.md).
