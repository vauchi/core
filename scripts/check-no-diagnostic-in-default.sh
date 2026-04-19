#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

# Guard: no `diagnostic_*` UniFFI symbols in default-feature binding surface.
#
# Rationale: diagnostics must only ship in test/debug builds, never in
# production. The `diagnostic-scanner` feature flag is the intended gate.
# This script enforces that gate by checking the UniFFI binding output of
# a default-feature build — any `diagnostic_` symbol that escapes the
# gate is a regression.
#
# Transition allowlist (removed in 0.20.0 per Phase 9 of the plan):
#   - diagnostic_scan_qr: deprecated production alias; will be removed
#     after Android/iOS consumers migrate to scan_qr.
#
# Exit 0: binding surface clean (or only allowlisted symbols present)
# Exit 1: forbidden diagnostic_* symbol found
#
# See: _private/docs/planning/todo/2026-04-19-diagnostics-out-of-production-plan.md

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Transition allowlist — remove entries when the referenced deprecation
# is retired. Each line is an exact function name (no wildcards).
ALLOWLIST=(
  "diagnostic_scan_qr"
)

# Regenerate Kotlin bindings with default features (no diagnostic-scanner).
# We use Kotlin as the reference because it's the most compact surface
# and unambiguously lists every `pub` UniFFI export.
BINDINGS_DIR=$(mktemp -d)
trap 'rm -rf "$BINDINGS_DIR"' EXIT

echo "Generating default-feature Kotlin bindings into $BINDINGS_DIR ..."
(
  cd "$PROJECT_ROOT"
  cargo build -p vauchi-platform --quiet
  cargo run --quiet --bin uniffi-bindgen -p vauchi-platform -- generate \
    --library "target/debug/libvauchi_platform.dylib" \
    --language kotlin \
    --out-dir "$BINDINGS_DIR" \
    --no-format 2>/dev/null \
  || cargo run --quiet --bin uniffi-bindgen -p vauchi-platform -- generate \
    --library "target/debug/libvauchi_platform.so" \
    --language kotlin \
    --out-dir "$BINDINGS_DIR" \
    --no-format
)

# Extract function/type names starting with `diagnostic` (case insensitive,
# matches Kotlin-style `diagnosticXxx` camelCase derived from Rust
# snake_case `diagnostic_xxx`).
# UniFFI Kotlin bindings wrap identifiers in backticks:
#   fun `diagnosticScanQr`(...)
# and generated .kt files live under uniffi/<crate>/ — recurse with find.
# Note: `\b` word boundary is a GNU-grep/sed extension; we avoid it so the
# script works on macOS (BSD sed).
FOUND=$(find "$BINDINGS_DIR" -name '*.kt' -exec \
  grep -hE 'fun `diagnostic[A-Z][a-zA-Z]*`\(' {} \; 2>/dev/null \
  | sed -nE 's/.*fun `(diagnostic[A-Za-z]*)`\(.*/\1/p' \
  | sort -u || true)

# Camel-case → snake_case translation to compare against allowlist.
# Kotlin: diagnosticScanQr ← Rust: diagnostic_scan_qr.
# Uses sed + tr for portability (BSD sed on macOS has no `\L` lowercase
# case conversion; GNU-sed-only behavior would fail silently here).
to_snake() {
  echo "$1" | sed -E 's/([A-Z])/_\1/g' | tr '[:upper:]' '[:lower:]'
}

VIOLATIONS=()
for name in $FOUND; do
  snake=$(to_snake "$name")
  allowed=false
  for entry in "${ALLOWLIST[@]}"; do
    if [[ "$snake" == "$entry" ]]; then
      allowed=true
      break
    fi
  done
  if ! $allowed; then
    VIOLATIONS+=("$snake (Kotlin: $name)")
  fi
done

if [[ ${#VIOLATIONS[@]} -eq 0 ]]; then
  echo -e "${GREEN}PASS${NC}: no unexpected diagnostic_* symbols in default-feature bindings"
  if [[ -n "$FOUND" ]]; then
    echo "  Allowlisted (transition):"
    for name in $FOUND; do
      echo "    - $(to_snake "$name") (Kotlin: $name)"
    done
  fi
  exit 0
fi

echo -e "${RED}FAIL${NC}: diagnostic_* symbols leaked into default-feature bindings:"
for v in "${VIOLATIONS[@]}"; do
  echo "  - $v"
done
echo
echo -e "${YELLOW}Hint${NC}: gate the offending UniFFI export with"
echo "  #[cfg(feature = \"diagnostic-scanner\")]"
echo "in vauchi-platform/src/diagnostic.rs (and its re-export in lib.rs)."
echo "If a symbol is a legitimate production alias during a transition,"
echo "add it to the ALLOWLIST in this script with a plan reference."
exit 1
