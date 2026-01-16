# Feature Scenarios

Gherkin scenarios defining WebBook behavior. Each scenario should have corresponding tests.

## Status Overview

| Feature | Scenarios | Implemented | Priority | Notes |
|---------|-----------|-------------|----------|-------|
| identity_management | 15 | 15 | P0 | Core |
| contact_card_management | 34 | 34 | P0 | Core |
| contact_exchange | 27 | 12 | P0 | QR done, BLE/NFC planned |
| contacts_management | 40 | 40 | P0 | Core |
| device_management | 30 | 30 | P0 | Core |
| sync_updates | 34 | 34 | P0 | WebSocket relay |
| security | 34 | 34 | P1 | Crypto, signatures |
| visibility_control | 26 | 26 | P1 | Per-contact rules |
| visibility_labels | 41 | 0 | P2 | Post-launch |
| relay_network | 43 | 20 | P2 | Basic relay done |
| social_profile_validation | 33 | 0 | P3 | Low priority |
| **future/** | | | P2-P3 | See `future/README.md` |
| - contact_recovery | 47 | 0 | P2 | Social vouching recovery |
| - tor_mode | 29 | 0 | P3 | Moved to `future/` |
| - hidden_contacts | 36 | 0 | P3 | Moved to `future/` |
| - duress_password | 45 | 0 | P3 | Moved to `future/` |

**Total**: 514 scenarios | **Implemented**: ~245 (~48%)

> **Note**: Future features have been moved to `features/future/` to clearly separate planned-but-unimplemented features from active development. See `features/future/README.md` for details.

## Priority Definitions

- **P0 (Core)**: Required for MVP. Must work before any release.
- **P1 (Security)**: Security-critical features. Required for public launch.
- **P2 (Infrastructure)**: Relay, federation, labels. Enhances reliability.
- **P3 (Post-launch)**: Advanced privacy features. Opt-in, can ship later.

## Feature Descriptions

### P0: Core Features

**identity_management.feature**
- Identity creation, backup, recovery
- Master seed and key derivation
- Code: `webbook-core/src/identity/`

**contact_card_management.feature**
- Create/edit own contact card
- Field types: phone, email, social, address, website, custom
- Social network registry (35+ networks)
- Code: `webbook-core/src/contact_card/`

**contact_exchange.feature**
- QR code generation and scanning (implemented)
- X3DH key exchange (implemented)
- BLE proximity exchange (planned)
- NFC tap exchange (planned)
- Code: `webbook-core/src/exchange/`

**contacts_management.feature**
- Contact list CRUD operations
- Search, filter, favorites
- Blocking and notes
- Code: `webbook-core/src/contact/`

**device_management.feature**
- Multi-device linking via QR
- Device registry with signatures
- Device revocation
- Code: `webbook-core/src/identity/device.rs`, `webbook-core/src/exchange/device_link.rs`

**sync_updates.feature**
- Card update propagation
- Double Ratchet forward secrecy
- Offline queuing and retry
- Code: `webbook-core/src/sync/`, `webbook-core/src/network/`

### P1: Security Features

**security.feature**
- E2E encryption (XChaCha20-Poly1305)
- Signature verification (Ed25519)
- MITM detection, replay prevention
- Code: `webbook-core/src/crypto/`

**visibility_control.feature**
- Per-contact field visibility
- View-as-contact preview
- Code: `webbook-core/src/contact/visibility.rs`

### P2: Infrastructure

**visibility_labels.feature**
- Group contacts by label (Family, Work, etc.)
- Bulk visibility rules
- Status: Designed, not implemented

**relay_network.feature**
- Store-and-forward relay
- Rate limiting, blob expiration
- Federation (planned)
- Code: `webbook-relay/`

**future/contact_recovery.feature**
- Recover contacts after device loss via social vouching
- In-person verification with K contacts (configurable threshold)
- Other contacts verify via mutual contact trust
- Isolated contacts (no mutual vouchers) get warnings
- Planning: `docs/planning/todo/contact-recovery.md`

### P3: Advanced Privacy (Post-Launch)

**social_profile_validation.feature**
- Crowd-sourced profile validation
- OAuth verification (low priority)

> The following P3 features have been moved to `features/future/`:

**future/tor_mode.feature**
- Route traffic through Tor
- Circuit management, bridge support

**future/hidden_contacts.feature**
- Secret gesture/PIN to reveal contacts
- Plausible deniability

**future/duress_password.feature**
- Decoy profile under coercion
- Silent alerts

See `features/future/README.md` for implementation notes.

## Running Scenario Tests

```bash
# Run all tests (includes scenario coverage)
cargo test

# Run tests for specific feature area
cargo test identity
cargo test exchange
cargo test sync
```

## Adding New Scenarios

1. Write scenario in appropriate `.feature` file
2. Write failing test referencing the scenario
3. Implement minimal code to pass
4. Refactor while green

See `docs/TDD_RULES.md` for methodology.
