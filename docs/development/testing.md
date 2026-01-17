# Testing Strategy

## Test Pyramid

```
         ┌─────┐
         │ E2E │     < 5% - Full user scenarios
        ┌┴─────┴┐
        │ Integ │    15% - Component interactions
       ┌┴───────┴┐
       │Property │   5% - Invariants across inputs
      ┌┴─────────┴┐
      │   Unit    │  75% - Fast, isolated
     └─────────────┘
```

**Current: 700+ tests** across unit, integration, property, and E2E suites.

## Test Organization

### Directory Structure

```
webbook-core/tests/
├── common/                    # Shared test utilities
│   ├── mod.rs                 # Module exports
│   ├── helpers.rs             # Setup helpers (create_webbook_with_identity, etc.)
│   ├── fixtures.rs            # Test data (sample cards, passwords, unicode)
│   └── strategies.rs          # Proptest strategies (reusable)
├── e2e/                       # End-to-end tests
│   ├── mod.rs
│   ├── exchange_e2e_test.rs   # Contact exchange workflow
│   ├── backup_e2e_test.rs     # Backup and recovery workflow
│   ├── visibility_e2e_test.rs # Field visibility workflow
│   └── sync_e2e_test.rs       # Multi-user sync workflow
├── integration/               # Integration tests
│   ├── mod.rs
│   ├── identity_workflow_test.rs
│   ├── contact_workflow_test.rs
│   └── sync_workflow_test.rs
├── property_tests.rs          # Property-based tests
├── error_handling_tests.rs    # Error path testing
├── ratchet_error_tests.rs     # Crypto error scenarios
├── edge_cases_tests.rs        # Boundary conditions
├── social_validation_tests.rs # Social profile validation
├── relay_simulation_tests.rs  # Network simulation
├── fuzz_tests.rs              # Fuzzing tests
├── concurrency_tests.rs       # Thread safety tests
├── migration_tests.rs         # Schema migration tests
├── snapshot_tests.rs          # Wire format stability
└── protocol_compatibility_tests.rs # Cross-version compat
```

### Test Categories

| Category | Purpose | Files |
|----------|---------|-------|
| **E2E** | Full user scenarios | `tests/e2e/*.rs` |
| **Integration** | Cross-module workflows | `tests/integration/*.rs` |
| **Property** | Random input invariants | `tests/property_tests.rs` |
| **Error Handling** | Failure modes | `tests/error_handling_tests.rs`, `tests/ratchet_error_tests.rs` |
| **Edge Cases** | Boundary conditions | `tests/edge_cases_tests.rs` |
| **Feature** | Specific features | `tests/social_validation_tests.rs`, `tests/relay_simulation_tests.rs` |
| **Unit** | Individual functions | `src/**/*.rs` inline |

## Property Tests

Test properties that hold for ALL inputs using `proptest`:

```rust
proptest! {
    #[test]
    fn prop_encryption_roundtrip(key in bytes32(), data in vec(any::<u8>(), 1..1000)) {
        prop_assert_eq!(decrypt(&key, &encrypt(&key, &data)?), data);
    }
}
```

**Categories tested**:
- Serialization roundtrips (JSON, binary)
- Cryptographic operations (encrypt/decrypt, sign/verify)
- Data structures (VersionVector, VisibilityRules)
- Device derivation determinism
- Delta computation and application

**Extended tests** (slow, run with `--ignored`):
- `prop_ratchet_many_messages_roundtrip` - 100-500 messages
- `prop_sync_many_deltas_converge` - 20-50 sequential deltas
- `prop_ratchet_bidirectional` - Alternating conversation

## Common Test Utilities

Import shared utilities in test files:

```rust
mod common;
use common::helpers::*;
use common::fixtures::*;
```

### Helpers

```rust
// Create a WebBook with identity
let wb = create_webbook_with_identity("Alice");

// Set up Alice and Bob with mutual contacts
let (alice, bob, secret, bob_id, alice_id) = setup_alice_bob_exchange();

// Set up ratchet states
let (alice_ratchet, bob_ratchet) = setup_ratchets(&shared_secret);

// Three-user setup
let (alice, bob, carol, secrets) = setup_three_users();
```

### Fixtures

```rust
use common::fixtures::{TEST_PASSWORD, WEAK_PASSWORD, MAX_CARD_FIELDS};
use common::fixtures::unicode::{JAPANESE, ARABIC, EMOJI};
use common::fixtures::edge_cases::{EMPTY, SPECIAL_CHARS};
```

### Proptest Strategies

```rust
use common::strategies::*;

proptest! {
    #[test]
    fn my_test(
        name in display_name_strategy(),
        email in email_strategy(),
        key in bytes32_strategy()
    ) {
        // ...
    }
}
```

## Running Tests

```bash
# All tests
cargo test --workspace

# Core library only (fast)
cargo test -p webbook-core --lib

# Specific test suite
cargo test --test e2e                    # E2E tests
cargo test --test integration            # Integration tests
cargo test --test property_tests         # Property tests
cargo test --test error_handling_tests   # Error handling

# Slow tests (marked #[ignore])
cargo test -- --ignored

# Coverage report
cargo tarpaulin --out Html

# With test output
cargo test -- --nocapture
```

## TDD Workflow

```
1. Write failing test (RED)
2. Write minimal code (GREEN)
3. Refactor, keep green
4. Commit
```

See `docs/TDD_RULES.md` for full methodology.

## Coverage Goals

| Metric | Target |
|--------|--------|
| Line coverage | 90%+ |
| Branch coverage | 80%+ |
| Error path coverage | 80%+ |
| Edge case coverage | 85%+ |

## Adding New Tests

1. **Choose the right location**:
   - Unit test? Add to source file with `#[cfg(test)]`
   - Integration? Add to `tests/integration/`
   - E2E? Add to `tests/e2e/`
   - Error handling? Add to `tests/error_handling_tests.rs`

2. **Use shared utilities**:
   - Import from `common::helpers` for setup
   - Use `common::fixtures` for test data
   - Reuse strategies from `common::strategies`

3. **Follow naming conventions**:
   - `test_<feature>_<scenario>` for regular tests
   - `prop_<property>` for property tests

4. **Mark slow tests**:
   ```rust
   #[test]
   #[ignore] // Run with: cargo test -- --ignored
   fn test_slow_operation() { ... }
   ```
