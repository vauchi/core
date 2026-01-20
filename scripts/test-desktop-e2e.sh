#!/bin/bash
# Desktop E2E Test Script (Playwright + Tauri CDP)
#
# This script:
# 1. Builds the Tauri desktop app in debug mode
# 2. Starts the app with Chrome DevTools Protocol enabled
# 3. Runs Playwright tests against the running app
# 4. Cleans up

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DESKTOP_DIR="$PROJECT_ROOT/vauchi-desktop"
UI_DIR="$DESKTOP_DIR/ui"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# CDP port
CDP_PORT="${CDP_PORT:-9222}"
TAURI_PID=""

cleanup() {
    echo ""
    echo -e "${YELLOW}Cleaning up...${NC}"

    if [[ -n "$TAURI_PID" ]] && kill -0 "$TAURI_PID" 2>/dev/null; then
        echo "Stopping Tauri app (PID: $TAURI_PID)"
        kill "$TAURI_PID" 2>/dev/null || true
        wait "$TAURI_PID" 2>/dev/null || true
    fi

    # Kill any remaining Tauri processes on the CDP port
    if command -v lsof &>/dev/null; then
        lsof -ti:$CDP_PORT | xargs kill -9 2>/dev/null || true
    fi
}

trap cleanup EXIT

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Desktop E2E Tests (Playwright)     ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

# === Pre-flight Checks ===
echo -e "${BLUE}=== Pre-flight Checks ===${NC}"

if [[ ! -d "$DESKTOP_DIR" ]]; then
    echo -e "${RED}Error: vauchi-desktop directory not found${NC}"
    exit 1
fi

if [[ ! -d "$UI_DIR" ]]; then
    echo -e "${RED}Error: vauchi-desktop/ui directory not found${NC}"
    exit 1
fi

# Check for Node.js
if ! command -v node &>/dev/null; then
    echo -e "${RED}Error: Node.js not found. Install Node.js to run Playwright tests.${NC}"
    exit 1
fi

# Check for Playwright
if [[ ! -f "$UI_DIR/node_modules/.bin/playwright" ]]; then
    echo -e "${YELLOW}Installing Playwright...${NC}"
    (cd "$UI_DIR" && npm install -D @playwright/test)
    (cd "$UI_DIR" && npx playwright install chromium)
fi

# Check if Playwright config exists
if [[ ! -f "$UI_DIR/playwright.config.ts" ]]; then
    echo -e "${YELLOW}Creating Playwright config...${NC}"
    cat > "$UI_DIR/playwright.config.ts" << 'EOF'
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false, // Tauri requires sequential tests
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: 'html',
  timeout: 30000,

  use: {
    // Connect to running Tauri app via CDP
    browserURL: `http://localhost:${process.env.CDP_PORT || 9222}`,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },

  projects: [
    {
      name: 'tauri-webview',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
EOF
fi

# Check if tests directory exists
if [[ ! -d "$UI_DIR/tests/e2e" ]]; then
    echo -e "${YELLOW}Creating tests directory with example test...${NC}"
    mkdir -p "$UI_DIR/tests/e2e"

    cat > "$UI_DIR/tests/e2e/smoke.spec.ts" << 'EOF'
import { test, expect } from '@playwright/test';

test.describe('Vauchi Desktop Smoke Tests', () => {
  test('app loads successfully', async ({ page }) => {
    // Wait for the app to load
    await page.waitForLoadState('domcontentloaded');

    // Check that the app rendered something
    const body = await page.locator('body');
    await expect(body).toBeVisible();
  });

  test('has expected title or heading', async ({ page }) => {
    await page.waitForLoadState('domcontentloaded');

    // Look for Vauchi branding
    const hasVauchi = await page.getByText(/vauchi/i).count();
    expect(hasVauchi).toBeGreaterThan(0);
  });
});
EOF
fi

echo -e "${GREEN}✓${NC} Pre-flight checks passed"

# === Build Frontend ===
echo ""
echo -e "${BLUE}=== Building Frontend ===${NC}"

cd "$UI_DIR"
if [[ ! -d "node_modules" ]]; then
    echo "Installing dependencies..."
    npm install
fi

echo "Building UI..."
npm run build

# === Build Tauri App ===
echo ""
echo -e "${BLUE}=== Building Tauri App ===${NC}"

cd "$DESKTOP_DIR"
echo "Building Tauri in debug mode..."
cargo build -p vauchi-desktop

# === Start Tauri with CDP ===
echo ""
echo -e "${BLUE}=== Starting Tauri App with CDP ===${NC}"

# Find the built binary
TAURI_BIN=""
for bin_path in \
    "$PROJECT_ROOT/target/debug/vauchi-desktop" \
    "$PROJECT_ROOT/target/debug/vauchi-desktop.exe" \
    "$DESKTOP_DIR/src-tauri/target/debug/vauchi-desktop" \
    ; do
    if [[ -f "$bin_path" ]]; then
        TAURI_BIN="$bin_path"
        break
    fi
done

if [[ -z "$TAURI_BIN" ]]; then
    echo -e "${YELLOW}Warning: Built binary not found, using cargo tauri dev${NC}"

    # Use cargo tauri dev with CDP
    # Set environment variable for WebView debugging
    export WEBKIT_WEB_INSPECTOR_SERVER="127.0.0.1:$CDP_PORT"
    export WEBKIT_REMOTE_DEBUGGING_PORT="$CDP_PORT"

    # For Chromium-based WebViews (Windows/Linux)
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$CDP_PORT"

    echo "Starting Tauri dev server with CDP on port $CDP_PORT..."
    (cd "$DESKTOP_DIR" && cargo tauri dev) &
    TAURI_PID=$!
else
    # Set CDP environment and run binary directly
    export WEBKIT_WEB_INSPECTOR_SERVER="127.0.0.1:$CDP_PORT"
    export WEBKIT_REMOTE_DEBUGGING_PORT="$CDP_PORT"
    export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$CDP_PORT"

    echo "Starting $TAURI_BIN with CDP on port $CDP_PORT..."
    "$TAURI_BIN" &
    TAURI_PID=$!
fi

echo "Tauri PID: $TAURI_PID"

# Wait for CDP to be available
echo "Waiting for CDP endpoint..."
MAX_WAIT=30
for i in $(seq 1 $MAX_WAIT); do
    if curl -s "http://localhost:$CDP_PORT/json/version" &>/dev/null; then
        echo -e "${GREEN}✓${NC} CDP endpoint available"
        break
    fi

    if ! kill -0 "$TAURI_PID" 2>/dev/null; then
        echo -e "${RED}Error: Tauri process died${NC}"
        exit 1
    fi

    if [[ $i -eq $MAX_WAIT ]]; then
        echo -e "${RED}Error: CDP endpoint not available after ${MAX_WAIT}s${NC}"
        echo ""
        echo "Note: CDP support varies by platform:"
        echo "  - Linux: Requires WebKitGTK with WebInspector enabled"
        echo "  - macOS: Requires Safari WebDriver"
        echo "  - Windows: WebView2 supports --remote-debugging-port"
        echo ""
        echo "Alternative: Run Playwright against a local dev server"
        exit 1
    fi

    sleep 1
done

# === Run Playwright Tests ===
echo ""
echo -e "${BLUE}=== Running Playwright Tests ===${NC}"

cd "$UI_DIR"
export CDP_PORT

# Run tests
if npx playwright test --reporter=list; then
    echo ""
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    echo -e "${GREEN}All E2E tests passed!${NC}"
    echo -e "${GREEN}═══════════════════════════════════════${NC}"
    EXIT_CODE=0
else
    echo ""
    echo -e "${RED}═══════════════════════════════════════${NC}"
    echo -e "${RED}Some E2E tests failed!${NC}"
    echo -e "${RED}═══════════════════════════════════════${NC}"
    echo ""
    echo "View report: npx playwright show-report"
    EXIT_CODE=1
fi

exit $EXIT_CODE
