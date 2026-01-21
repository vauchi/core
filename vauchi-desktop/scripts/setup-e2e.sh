#!/bin/bash

# E2E Test Setup Script for Vauchi Desktop

set -e

echo "🚀 Setting up E2E tests for Vauchi Desktop..."

# Navigate to desktop app directory
cd "$(dirname "$0")/../ui"

# Install dependencies
echo "📦 Installing Node.js dependencies..."
npm ci

# Install Playwright browsers
echo "🌐 Installing Playwright browsers..."
npx playwright install

# Install system dependencies for Playwright
echo "🔧 Installing Playwright system dependencies..."
npx playwright install-deps

# Build the Tauri app
echo "🏗️ Building Tauri app..."
cd ..
cargo build

echo "✅ E2E test setup complete!"
echo ""
echo "To run tests:"
echo "  npm run test:e2e              # Headless tests"
echo "  npm run test:e2e:headed      # Headed tests (with browser window)"
echo "  npm run test:e2e:debug      # Debug mode"
echo ""
echo "To update browsers:"
echo "  npx playwright install"