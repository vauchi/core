#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Validate that core's parsers can still load content from sibling repos.
# This catches drift when core changes struct definitions but content repos
# haven't been updated (or vice versa).
#
# Runs as part of core's CI pipeline on MRs that touch parser code.

set -euo pipefail

ERRORS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CORE_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Content Contract Validation ==="
echo ""

# --- Themes ---
# Core reads the generated flat format, not the hierarchical source.
# The .clone-themes CI template runs generate.py to produce it.
THEMES_JSON="${CORE_DIR}/../themes/generated/themes.json"
if [ ! -f "$THEMES_JSON" ]; then
    # Fallback: try generating if source exists
    if [ -f "${CORE_DIR}/../themes/scripts/generate.py" ]; then
        python3 "${CORE_DIR}/../themes/scripts/generate.py" 2>/dev/null || true
    fi
fi
if [ -f "$THEMES_JSON" ]; then
    echo "Checking themes contract..."
    THEME_OUTPUT=$(VAUCHI_THEMES_PATH="$THEMES_JSON" cargo test -p vauchi-core --lib -- theme::tests::test_load_real_themes_json --exact 2>&1)
    THEME_EXIT=$?
    if [ $THEME_EXIT -eq 0 ]; then
        THEME_COUNT=$(python3 -c "import json; print(len(json.load(open('$THEMES_JSON'))))" 2>/dev/null || echo "?")
        echo "  PASS: Core parsed all $THEME_COUNT themes from generated/themes.json"
    else
        echo "  FAIL: themes contract test failed (exit $THEME_EXIT)"
        echo "$THEME_OUTPUT" | grep -E "error|FAILED|panicked" | head -5
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "  SKIP: themes/generated/themes.json not found (run generate.py first)"
fi

echo ""

# --- Locales ---
LOCALES_DIR="${CORE_DIR}/../locales"
if [ -d "$LOCALES_DIR" ]; then
    echo "Checking locales contract..."
    LOCALE_ERRORS=0
    for locale_file in "$LOCALES_DIR"/*.json; do
        [ -f "$locale_file" ] || continue
        lang=$(basename "$locale_file" .json)
        # Validate JSON is parseable as HashMap<String, Value>
        if python3 -c "
import json, sys
with open('$locale_file') as f:
    data = json.load(f)
if not isinstance(data, dict):
    print('  FAIL: $lang.json is not a JSON object')
    sys.exit(1)
non_string = [k for k,v in data.items() if not isinstance(v, str) and k != '_meta']
if non_string:
    print(f'  WARN: $lang.json has {len(non_string)} non-string values (silently skipped by core)')
" 2>/dev/null; then
            echo "  PASS: $lang.json is valid locale format"
        else
            echo "  FAIL: $lang.json cannot be parsed as locale"
            LOCALE_ERRORS=$((LOCALE_ERRORS + 1))
        fi
    done
    if [ $LOCALE_ERRORS -gt 0 ]; then
        ERRORS=$((ERRORS + LOCALE_ERRORS))
    fi
else
    echo "  SKIP: locales/ repo not found at $LOCALES_DIR"
fi

echo ""

# --- Networks ---
NETWORKS_JSON="${CORE_DIR}/vauchi-core/src/social/networks.json"
if [ -f "$NETWORKS_JSON" ]; then
    echo "Checking networks contract..."
    if python3 -c "
import json, sys
with open('$NETWORKS_JSON') as f:
    networks = json.load(f)
errors = 0
for i, n in enumerate(networks):
    for field in ['id', 'name', 'url']:
        if field not in n:
            print(f'  FAIL: Network at index {i} missing required field: {field}')
            errors += 1
sys.exit(1 if errors else 0)
" 2>/dev/null; then
        NET_COUNT=$(python3 -c "import json; print(len(json.load(open('$NETWORKS_JSON'))))" 2>/dev/null || echo "?")
        echo "  PASS: All $NET_COUNT networks have required fields"
    else
        echo "  FAIL: networks.json has contract violations"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "  SKIP: networks.json not found"
fi

echo ""

# --- CDN Manifest ---
# Validates core can deserialize the manifest format produced by build-manifest.py.
# Uses the inline sample in test_deserialize_build_manifest_output by default.
# Pass VAUCHI_MANIFEST_PATH to test against a real manifest file.
echo "Checking CDN manifest contract..."
MANIFEST_TEST="content::integrity::tests::test_deserialize_build_manifest_output"
MANIFEST_OUTPUT=$(cargo test -p vauchi-core --lib -- "$MANIFEST_TEST" --exact 2>&1)
MANIFEST_EXIT=$?
if [ $MANIFEST_EXIT -eq 0 ]; then
    echo "  PASS: Core can deserialize build-manifest.py output format"
else
    # Show the actual error (not hidden behind 2>/dev/null)
    echo "  FAIL: CDN manifest contract test failed (exit $MANIFEST_EXIT)"
    echo "$MANIFEST_OUTPUT" | grep -E "error|FAILED|panicked" | head -5
    echo "  Fix: check content/types.rs vs website/scripts/build-manifest.py"
    ERRORS=$((ERRORS + 1))
fi

echo ""
echo "=== Summary ==="
if [ $ERRORS -gt 0 ]; then
    echo "FAILED: $ERRORS contract violation(s) detected"
    echo "This means core's parsers may not be able to load content from sibling repos."
    echo "Check the parser struct definitions against the content repo schemas."
    exit 1
fi
echo "PASSED: All content contracts valid"
