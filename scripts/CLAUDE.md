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
| `build-bindings.sh` | Generate UniFFI bindings for iOS/Android. Outputs to `target/bindings/` |
| `package-xcframework.sh` | Package iOS bindings into distributable XCFramework zip |
| `package-android.sh` | Package Android bindings into distributable zip |
| `publish-packages.sh` | Upload packages to GitLab Generic Packages registry |
| `trigger-downstream.sh` | Trigger vauchi-mobile-swift and vauchi-mobile-android CI |
| `validate-bindings.sh` | Verify generated bindings have all expected types |
| `test-all.sh` | Run core + mobile test suite (local dev convenience) |

## CI Release Flow

```
build-bindings.sh → package-*.sh → publish-packages.sh → trigger-downstream.sh
```

## See Also

General dev tools (hooks, feature audit, dev helpers) are in `../dev-tools/`.
