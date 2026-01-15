# Test-Driven Development Rules for WebBook

## Overview

This document defines the strict Test-Driven Development (TDD) rules for the WebBook project. These rules are mandatory for all contributors and will be enforced through code review and CI/CD pipelines.

---

## The Three Laws of TDD

### Law 1: Write No Production Code Without a Failing Test
You are **not allowed** to write any production code unless it is to make a failing unit test pass.

### Law 2: Write Only Enough Test to Fail
You are **not allowed** to write more of a unit test than is sufficient to fail, and compilation failures count as failures.

### Law 3: Write Only Enough Production Code to Pass
You are **not allowed** to write more production code than is sufficient to pass the currently failing test.

---

## The Red-Green-Refactor Cycle

Every code change must follow this cycle:

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                  │
│    1. RED        Write a failing test                           │
│       │          - Test must fail for the right reason          │
│       │          - Test describes expected behavior             │
│       ▼                                                         │
│    2. GREEN      Write minimal code to pass                     │
│       │          - Simplest possible implementation             │
│       │          - No over-engineering                          │
│       ▼                                                         │
│    3. REFACTOR   Improve the code                               │
│       │          - All tests must still pass                    │
│       │          - Improve design without changing behavior     │
│       │                                                         │
│       └──────────────────► Back to RED                         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Test Categories & Requirements

### 1. Unit Tests

**Scope**: Individual functions, methods, and classes in isolation

**Rules**:
- Must run in < 100ms each
- Must not access network, file system, or databases
- Must use mocks/stubs for external dependencies
- Must be deterministic (same input = same output)
- One assertion per test (where practical)

**Naming Convention**:
```
test_<function_name>_<scenario>_<expected_result>

Examples:
- test_validate_phone_valid_format_returns_true
- test_encrypt_field_null_input_throws_error
- test_sign_message_valid_key_produces_valid_signature
```

**Coverage Requirement**: **Minimum 90%** line coverage for core library

### 2. Integration Tests

**Scope**: Multiple components working together

**Rules**:
- Must run in < 5 seconds each
- May access test databases or mock servers
- Must clean up after themselves
- Must be isolated from other integration tests

**Naming Convention**:
```
test_integration_<feature>_<scenario>

Examples:
- test_integration_contact_exchange_qr_flow
- test_integration_sync_update_propagation
- test_integration_visibility_rule_application
```

**Coverage Requirement**: All critical paths must have integration tests

### 3. End-to-End (E2E) Tests

**Scope**: Full system behavior from user perspective

**Rules**:
- Map directly to Gherkin scenarios
- Must run in isolated test environments
- May be slower but must complete in < 60 seconds
- Must be reliable (no flaky tests allowed)

**Implementation**: Use Cucumber/Behave frameworks to execute Gherkin features

### 4. Security Tests

**Scope**: Cryptographic operations, access control, data protection

**Rules**:
- **Mandatory** for all crypto functions
- Must verify correct algorithm usage
- Must test failure modes (wrong keys, tampered data)
- Must verify no plaintext leakage

**Examples**:
```rust
#[test]
fn test_encrypt_decrypt_roundtrip() {
    let key = generate_key();
    let plaintext = b"sensitive data";
    let ciphertext = encrypt(&key, plaintext).unwrap();
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    assert_eq!(plaintext, &decrypted[..]);
}

#[test]
fn test_decrypt_wrong_key_fails() {
    let key1 = generate_key();
    let key2 = generate_key();
    let ciphertext = encrypt(&key1, b"data").unwrap();
    assert!(decrypt(&key2, &ciphertext).is_err());
}

#[test]
fn test_signature_verification_tampered_data_fails() {
    let keypair = generate_keypair();
    let message = b"original message";
    let signature = sign(&keypair.private, message);
    let tampered = b"tampered message";
    assert!(!verify(&keypair.public, tampered, &signature));
}
```

### 5. Property-Based Tests

**Scope**: Testing invariants across random inputs

**Rules**:
- Use for crypto, serialization, parsing
- Must define clear properties
- Run with sufficient iterations (min 1000)

**Examples**:
```rust
#[quickcheck]
fn prop_encrypt_decrypt_roundtrip(data: Vec<u8>) -> bool {
    let key = generate_key();
    let ciphertext = encrypt(&key, &data).unwrap();
    let decrypted = decrypt(&key, &ciphertext).unwrap();
    data == decrypted
}

#[quickcheck]
fn prop_serialization_roundtrip(card: ContactCard) -> bool {
    let bytes = serialize(&card).unwrap();
    let restored: ContactCard = deserialize(&bytes).unwrap();
    card == restored
}
```

---

## Mandatory Test Checklist

Before any pull request can be merged, the following must be true:

### For All Code Changes

- [ ] All new functions have corresponding unit tests
- [ ] All edge cases are tested (null, empty, max values)
- [ ] All error conditions are tested
- [ ] No production code was written before a failing test
- [ ] All tests pass locally
- [ ] CI pipeline is green

### For Cryptographic Code

- [ ] Roundtrip tests (encrypt/decrypt, sign/verify)
- [ ] Wrong key rejection tests
- [ ] Tampered data rejection tests
- [ ] Key generation randomness tests
- [ ] No hardcoded keys in tests (use generated ones)
- [ ] Constant-time operation tests (where applicable)

### For Data Model Changes

- [ ] Serialization roundtrip tests
- [ ] Schema migration tests (if applicable)
- [ ] Validation tests for all constraints
- [ ] Edge case tests (empty, max size, special chars)

### For Network Code

- [ ] Connection failure handling tests
- [ ] Timeout handling tests
- [ ] Retry logic tests
- [ ] Rate limiting tests
- [ ] Invalid response handling tests

### For UI Code

- [ ] Component render tests
- [ ] User interaction tests
- [ ] Error state display tests
- [ ] Loading state tests
- [ ] Accessibility tests

---

## Gherkin-to-Test Mapping

Every Gherkin scenario must be implemented as an automated test.

### Mapping Process

1. **Feature File** → **Test File**
   ```
   features/contact_exchange.feature → tests/e2e/contact_exchange_test.rs
   ```

2. **Scenario** → **Test Function**
   ```gherkin
   Scenario: Successful QR code exchange with proximity
   ```
   Maps to:
   ```rust
   #[tokio::test]
   async fn test_successful_qr_code_exchange_with_proximity() {
       // Given Alice is displaying her exchange QR code
       let alice = TestUser::new("Alice").await;
       let qr = alice.generate_exchange_qr().await;

       // And Bob is physically present with Alice
       let bob = TestUser::new("Bob").await;
       let proximity = MockProximityVerifier::verified();

       // When Bob scans Alice's QR code
       let scan_result = bob.scan_qr(&qr).await;

       // And both devices emit and verify ultrasonic audio handshake
       let handshake = proximity.verify(&alice, &bob).await;
       assert!(handshake.is_success());

       // Then the exchange should proceed
       // And Bob should receive Alice's contact card
       assert!(bob.has_contact(&alice.public_id()).await);

       // And Alice should receive Bob's contact card
       assert!(alice.has_contact(&bob.public_id()).await);

       // And both should see "Exchange Successful"
       assert_eq!(alice.last_status(), Status::ExchangeSuccessful);
       assert_eq!(bob.last_status(), Status::ExchangeSuccessful);
   }
   ```

### Step Definition Library

Create reusable step definitions:

```rust
// tests/steps/exchange_steps.rs

pub async fn given_user_is_displaying_exchange_qr(user: &TestUser) -> QRCode {
    user.generate_exchange_qr().await
}

pub async fn given_users_are_in_proximity(user_a: &TestUser, user_b: &TestUser) -> ProximityContext {
    ProximityContext::establish(user_a, user_b).await
}

pub async fn when_user_scans_qr(user: &TestUser, qr: &QRCode) -> ScanResult {
    user.scan_qr(qr).await
}

pub async fn then_user_has_contact(user: &TestUser, contact_id: &PublicId) {
    assert!(user.has_contact(contact_id).await,
        "Expected {} to have contact {}", user.name(), contact_id);
}
```

---

## Test Infrastructure Requirements

### Mocking Strategy

| Component | Mocking Approach |
|-----------|------------------|
| Crypto | Use real crypto (never mock security) |
| Network | Mock at transport layer |
| Storage | In-memory test database |
| Time | Mockable clock interface |
| Randomness | Seeded RNG for reproducibility |
| Hardware (BLE/NFC) | Device-level mocks |

### Test Fixtures

```rust
// tests/fixtures/mod.rs

pub fn sample_contact_card() -> ContactCard {
    ContactCard {
        id: "test-id-12345".into(),
        display_name: "Test User".into(),
        fields: vec![
            ContactField::phone("Mobile", "+1-555-123-4567"),
            ContactField::email("Work", "test@example.com"),
        ],
        ..Default::default()
    }
}

pub fn sample_keypair() -> KeyPair {
    // Use deterministic seed for reproducibility
    KeyPair::from_seed(&[42u8; 32])
}

pub async fn setup_test_users(count: usize) -> Vec<TestUser> {
    let mut users = Vec::with_capacity(count);
    for i in 0..count {
        users.push(TestUser::new(&format!("User{}", i)).await);
    }
    users
}
```

### Test Database

```rust
pub fn create_test_db() -> Database {
    Database::in_memory()
}

pub async fn with_test_db<F, Fut, T>(test: F) -> T
where
    F: FnOnce(Database) -> Fut,
    Fut: Future<Output = T>,
{
    let db = create_test_db();
    let result = test(db.clone()).await;
    db.cleanup().await;
    result
}
```

---

## CI/CD Pipeline Requirements

### Pre-Commit Hooks

```bash
#!/bin/bash
# .git/hooks/pre-commit

# Run unit tests
cargo test --lib || exit 1

# Check coverage threshold
coverage=$(cargo tarpaulin --out Json | jq '.coverage')
if (( $(echo "$coverage < 90" | bc -l) )); then
    echo "Coverage below 90%: $coverage"
    exit 1
fi

# Check for skipped tests
if grep -r "#\[ignore\]" src/ tests/; then
    echo "Found ignored tests - please fix or remove"
    exit 1
fi
```

### CI Pipeline Stages

```yaml
stages:
  - lint
  - unit-tests
  - integration-tests
  - e2e-tests
  - security-scan
  - coverage-report

unit-tests:
  script:
    - cargo test --lib
  coverage: '/^(\d+\.\d+)% coverage/'
  rules:
    - if: $CI_COMMIT_BRANCH
  artifacts:
    reports:
      junit: target/test-results.xml

integration-tests:
  script:
    - cargo test --test integration_*
  needs: [unit-tests]

e2e-tests:
  script:
    - cargo test --test e2e_*
  needs: [integration-tests]

coverage-gate:
  script:
    - |
      coverage=$(cat coverage.json | jq '.coverage')
      if (( $(echo "$coverage < 90" | bc -l) )); then
        echo "FAILED: Coverage $coverage% is below 90%"
        exit 1
      fi
  needs: [unit-tests]
```

---

## Test Documentation Requirements

### Every Test Must Have

1. **Clear description** of what is being tested
2. **Arrange-Act-Assert** structure
3. **Single responsibility** (test one thing)

### Example

```rust
/// Tests that encrypting and decrypting data with the same key
/// produces the original plaintext.
///
/// This is a critical security test ensuring our encryption
/// implementation is correct.
#[test]
fn test_encrypt_decrypt_roundtrip_produces_original_data() {
    // Arrange
    let key = generate_symmetric_key();
    let original_data = b"Hello, World!";

    // Act
    let ciphertext = encrypt(&key, original_data)
        .expect("Encryption should succeed");
    let decrypted = decrypt(&key, &ciphertext)
        .expect("Decryption should succeed");

    // Assert
    assert_eq!(
        original_data.to_vec(),
        decrypted,
        "Decrypted data should match original"
    );
}
```

---

## Forbidden Practices

### Never Do These

1. **Skip tests to meet deadlines**
   - Tests are not optional
   - No code ships without tests

2. **Write tests after code**
   - Tests must come first
   - Violations will be caught in code review

3. **Use `#[ignore]` without explanation**
   - Ignored tests need tracking issues
   - Must be resolved before release

4. **Mock security-critical code**
   - Real crypto must be tested
   - Mocks hide real bugs

5. **Hardcode test data**
   - Use factories and builders
   - Tests must be maintainable

6. **Test implementation details**
   - Test behavior, not internals
   - Tests should survive refactoring

7. **Write flaky tests**
   - Tests must be deterministic
   - Flaky tests are bugs

8. **Copy-paste test code**
   - Extract common code to helpers
   - DRY applies to tests too

---

## Enforcement

### Code Review Checklist

Reviewers must verify:

- [ ] Tests written before implementation
- [ ] All scenarios from Gherkin are covered
- [ ] Test names are descriptive
- [ ] No test code in production files
- [ ] Coverage threshold maintained
- [ ] No ignored tests without issues
- [ ] Security tests for crypto code

### Automated Enforcement

```yaml
# .github/workflows/tdd-check.yml

name: TDD Compliance

on: [pull_request]

jobs:
  check-tdd:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          fetch-depth: 0

      - name: Check test-first commits
        run: |
          # Get commits in this PR
          commits=$(git log origin/main..HEAD --oneline)

          # For each commit, verify tests exist
          for commit in $commits; do
            # Check that test changes came before/with prod changes
            ./scripts/verify-tdd-commit.sh $commit
          done

      - name: Coverage gate
        run: |
          cargo tarpaulin --out Json
          coverage=$(jq '.coverage' tarpaulin-report.json)
          if (( $(echo "$coverage < 90" | bc -l) )); then
            exit 1
          fi
```

---

## Getting Started with TDD

### New Feature Workflow

1. **Read the Gherkin feature** for the scenario you're implementing
2. **Write a failing E2E test** that maps to the scenario
3. **Write failing unit tests** for each component you'll need
4. **Implement the smallest piece** to make one test pass
5. **Refactor** while keeping tests green
6. **Repeat** until the E2E test passes
7. **Submit PR** with tests and implementation together

### Example: Implementing Contact Card Creation

```rust
// Step 1: Write failing E2E test
#[tokio::test]
async fn test_e2e_create_contact_card_with_phone() {
    let app = TestApp::launch().await;
    app.setup_identity("Alice").await;

    app.add_field(FieldType::Phone, "Mobile", "+1-555-1234").await;

    let card = app.get_my_card().await;
    assert!(card.has_field("Mobile"));
    assert_eq!(card.get_field("Mobile").value(), "+1-555-1234");
}

// Step 2: Write failing unit tests for ContactCard
#[test]
fn test_contact_card_add_field() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::phone("Mobile", "+1-555-1234");

    card.add_field(field.clone());

    assert!(card.fields().contains(&field));
}

#[test]
fn test_contact_field_phone_creation() {
    let field = ContactField::phone("Mobile", "+1-555-1234");

    assert_eq!(field.field_type(), FieldType::Phone);
    assert_eq!(field.label(), "Mobile");
    assert_eq!(field.value(), "+1-555-1234");
}

// Step 3: Implement to make tests pass
// ... (implementation code)
```

---

## Resources

- [Test-Driven Development by Example](https://www.amazon.com/Test-Driven-Development-Kent-Beck/dp/0321146530) - Kent Beck
- [Growing Object-Oriented Software, Guided by Tests](https://www.amazon.com/Growing-Object-Oriented-Software-Guided-Tests/dp/0321503627) - Freeman & Pryce
- [The Art of Unit Testing](https://www.manning.com/books/the-art-of-unit-testing-third-edition) - Roy Osherove
- [Cucumber Documentation](https://cucumber.io/docs/cucumber/)
- [Property-Based Testing with QuickCheck](https://hypothesis.works/articles/quickcheck-in-every-language/)
