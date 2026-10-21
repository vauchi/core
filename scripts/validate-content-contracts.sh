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

# Runs one named test and passes only if exactly that test ran: a filter
# that matches nothing exits 0 too, which hid a moved test for months.
run_contract_test() {
    package="$1"
    target="$2"
    test_name="$3"
    output=$(cargo test -p "$package" $target -- "$test_name" --exact 2>&1) || {
        echo "$output" | grep -E "error|FAILED|panicked" | head -5
        return 1
    }
    if ! echo "$output" | grep -q "test result: ok. 1 passed"; then
        echo "  $test_name did not run in $package (moved or renamed?)"
        return 1
    fi
}

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
    if VAUCHI_THEMES_PATH="$THEMES_JSON" run_contract_test vauchi-app --lib theme::tests::test_load_real_themes_json; then
        THEME_COUNT=$(python3 -c "import json; print(len(json.load(open('$THEMES_JSON'))))" 2>/dev/null || echo "?")
        echo "  PASS: Core parsed all $THEME_COUNT themes from generated/themes.json"
    else
        echo "  FAIL: themes contract test failed"
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
# The file the CDN publishes, through Core's own validation (#734 D3).
NETWORKS_JSON="${CORE_DIR}/../website/app-files-src/networks.json"
if [ -f "$NETWORKS_JSON" ]; then
    echo "Checking networks contract..."
    if VAUCHI_NETWORKS_PATH="$NETWORKS_JSON" run_contract_test vauchi-core "--features testing --test it" social_network_data_tests::the_cdn_networks_file_passes_core_validation; then
        NET_COUNT=$(python3 -c "import json; print(len(json.load(open('$NETWORKS_JSON'))))" 2>/dev/null || echo "?")
        echo "  PASS: Core validated all $NET_COUNT networks from website/app-files-src/networks.json"
    else
        echo "  FAIL: networks contract test failed"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "  SKIP: website/app-files-src/networks.json not found"
fi

echo ""

# --- CDN Manifest ---
# Validates core can deserialize the manifest format produced by build-manifest.py.
# Uses the inline sample in test_deserialize_build_manifest_output by default.
# Pass VAUCHI_MANIFEST_PATH to test against a real manifest file.
echo "Checking CDN manifest contract..."
MANIFEST_TEST="content::integrity::tests::test_deserialize_build_manifest_output"
if run_contract_test vauchi-app --lib "$MANIFEST_TEST"; then
    echo "  PASS: Core can deserialize build-manifest.py output format"
else
    echo "  FAIL: CDN manifest contract test failed"
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
