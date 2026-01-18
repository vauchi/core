# TDD Rules

## Three Laws

1. **No production code without a failing test**
2. **Write only enough test to fail**
3. **Write only enough code to pass**

## Red-Green-Refactor

```
RED     → Write failing test
GREEN   → Minimal code to pass
REFACTOR → Improve, keep tests green
COMMIT  → When all tests pass
```

## Test Types

| Type | Scope | Speed | Coverage |
|------|-------|-------|----------|
| Unit | Single function | < 100ms | 90% min |
| Integration | Multiple components | < 5s | Critical paths |
| E2E | Full system | < 60s | All Gherkin scenarios |

## Naming

```
test_<function>_<scenario>_<expected>

Examples:
- test_encrypt_valid_key_returns_ciphertext
- test_decrypt_wrong_key_fails
```

## Critical Rules

**Crypto** - Never mock. Test with real crypto:
- Roundtrip (encrypt/decrypt, sign/verify)
- Wrong key rejection
- Tampered data rejection

**Gherkin** - Every scenario in `features/` must have a test.

**Coverage** - 90% minimum for vauchi-core.

## Forbidden

- Writing code before tests
- Mocking crypto operations
- `#[ignore]` without tracking issue
- Flaky/non-deterministic tests
- Hardcoded test secrets

## Mocking Strategy

| Component | Approach |
|-----------|----------|
| Crypto | Real (never mock) |
| Network | Mock transport |
| Storage | In-memory DB |
| Time | Mockable clock |

## PR Checklist

- [ ] Tests written before code
- [ ] All Gherkin scenarios covered
- [ ] No ignored tests
- [ ] Coverage ≥ 90%
- [ ] Crypto has security tests
