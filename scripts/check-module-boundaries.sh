#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

# Module boundary lint — enforces dependency direction rules at CI time.
# Catches imports that encode wrong dependency arrows between modules.
# Replaces the need for crate extraction to enforce boundaries.
#
# Exit 0: all boundaries clean
# Exit 1: violations found (prints each with file:line)
#
# Add new rules by appending to the RULES array. Format:
#   "source_glob|forbidden_pattern|explanation"

set -euo pipefail

CORE_SRC="vauchi-core/src"

# Each rule: source glob | grep -E pattern | human explanation
# The pattern matches `use crate::...` import lines.
RULES=(
  # storage must not depend on api layer (use crate::types:: for shared types)
  "$CORE_SRC/storage/*.rs|use crate::api::|storage/ must not import from api/ (use crate::types:: for shared types)"

  # storage must not depend on top-level UX modules (types already in types.rs)
  "$CORE_SRC/storage/*.rs|use crate::onboarding::|storage/ must not import from onboarding (use crate::types::)"
  "$CORE_SRC/storage/*.rs|use crate::aha_moments::|storage/ must not import from aha_moments (use crate::types::)"
  "$CORE_SRC/storage/*.rs|use crate::demo_contact::|storage/ must not import from demo_contact (use crate::types::)"

  # crypto must not depend on exchange (prevents circular dependency)
  "$CORE_SRC/crypto/*.rs|use crate::exchange::|crypto/ must not import from exchange/ (circular dep)"

  # No pub use re-exports of types.rs types from domain modules
  # (types should be imported directly from crate::types::, not through shim re-exports)
  "$CORE_SRC/onboarding.rs|pub use crate::types::|onboarding.rs must not re-export types (use, not pub use)"
  "$CORE_SRC/aha_moments.rs|pub use crate::types::|aha_moments.rs must not re-export types (use, not pub use)"
  "$CORE_SRC/demo_contact.rs|pub use crate::types::|demo_contact.rs must not re-export types (use, not pub use)"
  "$CORE_SRC/api/duress.rs|pub use crate::types::|api/duress.rs must not re-export types (use, not pub use)"
  "$CORE_SRC/api/emergency.rs|pub use crate::types::|api/emergency.rs must not re-export types (use, not pub use)"
)

violations=0

for rule in "${RULES[@]}"; do
  IFS='|' read -r glob pattern explanation <<< "$rule"

  # shellcheck disable=SC2086
  # Match only actual use statements, not comments or strings
  matches=$(grep -rn -E "^\s*$pattern" $glob 2>/dev/null || true)

  if [ -n "$matches" ]; then
    echo "VIOLATION: $explanation"
    echo "$matches" | sed 's/^/  /'
    echo
    violations=$((violations + 1))
  fi
done

if [ "$violations" -gt 0 ]; then
  echo "Found $violations module boundary violation(s)."
  echo "See: _private/docs/planning/todo/2026-03-10-core-modularization-plan.md"
  exit 1
else
  echo "Module boundaries OK ($((${#RULES[@]})) rules checked)"
fi
