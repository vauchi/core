# CLAUDE.md - Build Scripts

Build and test scripts tightly coupled to the Rust workspace.

## Rules

- Scripts should be executable (`chmod +x`)
- Use `#!/bin/bash` or `#!/usr/bin/env bash`
- Include usage comments at top of script
- Exit on error: `set -e` (or `set -euo pipefail`)
- Run from `code/` directory

## Key Scripts

| Script | Purpose |
|--------|---------|
| `build-bindings.sh` | Generate UniFFI bindings for mobile |
| `build-android.sh` | Build Android native libs |
| `test-all.sh` | Run full test suite |
| `relay-test.sh` | Integration tests with relay |
| `test-desktop-e2e.sh` | Desktop E2E tests |

## See Also

General dev tools (hooks, feature audit, dev helpers) are in `dev-tools/`.
