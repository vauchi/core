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

**Current: 420 tests** (330 unit + 67 integration + 22 property + 1 E2E)

## Test Types

| Type | Purpose | Location |
|------|---------|----------|
| Unit | Individual functions | `src/**/*.rs` inline |
| Integration | Cross-module workflows | `tests/*.rs` |
| Property | Random input invariants | `tests/property_tests.rs` |
| E2E | Full user scenarios | `tests/integration_tests.rs` |

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

**Tested**: serialization roundtrips, crypto operations, version vectors, visibility rules, device derivation.

## Recommended Additions (Fail Fast)

| Test Type | Value | Why |
|-----------|-------|-----|
| **Fuzz testing** | High | Catches parser crashes from malformed input |
| **Concurrency tests** | High | Catches race conditions in multi-device sync |
| **Protocol compat** | High | Ensures v1/v2 clients can communicate |
| **Migration tests** | High | Prevents data loss on schema changes |
| **Snapshot tests** | Medium | Detects unintended wire format changes |
| **Benchmarks** | Medium | Catches performance regressions |

## Running Tests

```bash
cargo test                              # All tests
cargo test -p webbook-core --lib        # Unit tests only (fast)
cargo test --test property_tests        # Property tests (slow)
cargo tarpaulin --out Html              # Coverage report
```

## TDD Workflow

```
1. Write failing test (RED)
2. Write minimal code (GREEN)
3. Refactor, keep green
4. Commit
```

See `docs/TDD_RULES.md` for details.
