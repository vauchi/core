# CLAUDE.md - Build Scripts

Build and test scripts for vauchi-core and vauchi-mobile.

## Rules

- Scripts should be executable (`chmod +x`)
- Use `#!/bin/bash` or `#!/usr/bin/env bash`
- Include usage comments at top of script
- Exit on error: `set -e` (or `set -euo pipefail`)
- Run from `core/` directory

## Key Scripts

| Script | Purpose |
|--------|---------|
| `build-bindings.sh` | Generate UniFFI bindings for iOS/Android |
| `build-android.sh` | Build Android native libs |
| `validate-bindings.sh` | Verify generated bindings are complete |
| `test-all.sh` | Run core + mobile test suite |

## See Also

General dev tools (hooks, feature audit, dev helpers) are in `../dev-tools/`.
