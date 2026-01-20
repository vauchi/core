#!/bin/bash
# Run all tests across the Vauchi project
#
# This script runs:
# 1. Rust workspace tests (vauchi-core, vauchi-relay, vauchi-cli, vauchi-tui, vauchi-desktop)
# 2. Desktop Playwright E2E tests
# 3. Android unit + instrumented tests
# 4. iOS unit + UI tests
# 5. Coverage report generation

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Track results
PASSED=0
FAILED=0
SKIPPED=0

run_test() {
    local name="$1"
    local cmd="$2"
    local skip_if_missing="${3:-}"

    echo ""
    echo -e "${BLUE}=== $name ===${NC}"

    if [[ -n "$skip_if_missing" && ! -d "$skip_if_missing" ]]; then
        echo -e "${YELLOW}SKIPPED: Directory not found: $skip_if_missing${NC}"
        ((SKIPPED++))
        return 0
    fi

    if eval "$cmd"; then
        echo -e "${GREEN}PASSED: $name${NC}"
        ((PASSED++))
    else
        echo -e "${RED}FAILED: $name${NC}"
        ((FAILED++))
        return 1
    fi
}

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║        Vauchi Test Suite               ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Project root: $PROJECT_ROOT"
echo "Workspace root: $WORKSPACE_ROOT"

cd "$PROJECT_ROOT"

# --- Rust Tests ---
run_test "Rust Workspace Tests" "cargo test --workspace"

run_test "Rust Clippy Lint" "cargo clippy --workspace -- -D warnings"

run_test "Rust Format Check" "cargo fmt --all -- --check"

# --- TUI Tests (ratatui-testlib) ---
if cargo test -p vauchi-tui --no-run 2>/dev/null; then
    run_test "TUI Tests" "cargo test -p vauchi-tui"
else
    echo -e "${YELLOW}SKIPPED: vauchi-tui has no tests yet${NC}"
    ((SKIPPED++))
fi

# --- Desktop Tauri Rust Tests ---
if cargo test -p vauchi-desktop --no-run 2>/dev/null; then
    run_test "Desktop Tauri Tests" "cargo test -p vauchi-desktop"
else
    echo -e "${YELLOW}SKIPPED: vauchi-desktop has no tests yet${NC}"
    ((SKIPPED++))
fi

# --- Desktop Playwright E2E ---
if [[ -f "$SCRIPT_DIR/test-desktop-e2e.sh" ]]; then
    run_test "Desktop Playwright E2E" "$SCRIPT_DIR/test-desktop-e2e.sh"
else
    echo -e "${YELLOW}SKIPPED: test-desktop-e2e.sh not found${NC}"
    ((SKIPPED++))
fi

# --- Android Tests ---
ANDROID_DIR="$WORKSPACE_ROOT/android"
if [[ -d "$ANDROID_DIR" && -f "$ANDROID_DIR/gradlew" ]]; then
    run_test "Android Unit Tests" "(cd '$ANDROID_DIR' && ./gradlew test)" "$ANDROID_DIR"

    # Instrumented tests require a device/emulator
    if adb devices 2>/dev/null | grep -q "device$"; then
        run_test "Android Instrumented Tests" "(cd '$ANDROID_DIR' && ./gradlew connectedAndroidTest)" "$ANDROID_DIR"
    else
        echo -e "${YELLOW}SKIPPED: Android instrumented tests (no device/emulator)${NC}"
        ((SKIPPED++))
    fi
else
    echo -e "${YELLOW}SKIPPED: Android directory not found or no gradlew${NC}"
    ((SKIPPED++))
fi

# --- iOS Tests ---
IOS_DIR="$WORKSPACE_ROOT/ios"
if [[ -d "$IOS_DIR" && "$(uname)" == "Darwin" ]]; then
    # Check if xcodebuild is available
    if command -v xcodebuild &>/dev/null; then
        run_test "iOS Unit Tests" "(cd '$IOS_DIR' && xcodebuild test -scheme Vauchi -destination 'platform=iOS Simulator,name=iPhone 15' -quiet)" "$IOS_DIR"
    else
        echo -e "${YELLOW}SKIPPED: xcodebuild not found (macOS only)${NC}"
        ((SKIPPED++))
    fi
else
    echo -e "${YELLOW}SKIPPED: iOS tests (not on macOS or directory missing)${NC}"
    ((SKIPPED++))
fi

# --- Coverage Report ---
echo ""
echo -e "${BLUE}=== Coverage Report ===${NC}"
if command -v cargo-llvm-cov &>/dev/null; then
    cargo llvm-cov --workspace --html --output-dir "$PROJECT_ROOT/target/coverage"
    echo -e "${GREEN}Coverage report: $PROJECT_ROOT/target/coverage/html/index.html${NC}"
else
    echo -e "${YELLOW}SKIPPED: cargo-llvm-cov not installed${NC}"
    echo "Install with: cargo install cargo-llvm-cov"
    ((SKIPPED++))
fi

# --- Summary ---
echo ""
echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║             Test Summary               ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""
echo -e "${GREEN}Passed:  $PASSED${NC}"
echo -e "${RED}Failed:  $FAILED${NC}"
echo -e "${YELLOW}Skipped: $SKIPPED${NC}"
echo ""

if [[ $FAILED -gt 0 ]]; then
    echo -e "${RED}Some tests failed!${NC}"
    exit 1
else
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
fi
