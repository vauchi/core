#!/bin/bash
# WebBook Monorepo Reorganization Script
#
# This script moves the code repository into webbook-code/ subfolder,
# creating a parent directory structure with three separate git repos:
#
# WebBook/
# ├── webbook-code/     (main code, git history preserved)
# ├── webbook-website/  (GitHub Pages site)
# └── webbook-market/   (private marketing repo)
#
# IMPORTANT: Run this from the WebBook directory
# BACKUP: Make sure you have pushed all changes before running

set -e

# Check we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d ".git" ]; then
    echo "Error: Run this script from the WebBook repository root"
    exit 1
fi

# Check for uncommitted changes
if [ -n "$(git status --porcelain)" ]; then
    echo "Error: You have uncommitted changes. Commit or stash them first."
    exit 1
fi

echo "Starting reorganization..."

# Create temporary directory
TEMP_DIR=$(mktemp -d)
echo "Using temp directory: $TEMP_DIR"

# Save the current location
ORIGINAL_DIR=$(pwd)
PARENT_DIR=$(dirname "$ORIGINAL_DIR")
REPO_NAME=$(basename "$ORIGINAL_DIR")

# Move webbook-website and webbook-market to temp
echo "Moving sibling repos to temp..."
if [ -d "webbook-website" ]; then
    mv webbook-website "$TEMP_DIR/"
fi
if [ -d "webbook-market" ]; then
    mv webbook-market "$TEMP_DIR/"
fi

# Go to parent, rename current dir to webbook-code
echo "Renaming repository to webbook-code..."
cd "$PARENT_DIR"
mv "$REPO_NAME" webbook-code

# Create new parent directory
echo "Creating new parent directory..."
mkdir "$REPO_NAME"

# Move webbook-code into new parent
mv webbook-code "$REPO_NAME/"

# Move sibling repos into new parent
echo "Moving sibling repos back..."
if [ -d "$TEMP_DIR/webbook-website" ]; then
    mv "$TEMP_DIR/webbook-website" "$REPO_NAME/"
fi
if [ -d "$TEMP_DIR/webbook-market" ]; then
    mv "$TEMP_DIR/webbook-market" "$REPO_NAME/"
fi

# Clean up temp directory
rmdir "$TEMP_DIR"

# Create parent README
cat > "$REPO_NAME/README.md" << 'EOF'
# WebBook

Privacy-focused contact card exchange.

## Repository Structure

This directory contains three separate git repositories:

| Directory | Description | Visibility |
|-----------|-------------|------------|
| `webbook-code/` | Main application code (Rust, mobile apps, desktop) | Public |
| `webbook-website/` | GitHub Pages website | Public |
| `webbook-market/` | Marketing strategy and budget | Private |

## Quick Start

```bash
# Main development
cd webbook-code
cargo test

# Website development
cd webbook-website
python -m http.server 8000

# Marketing (team only)
cd webbook-market
```

## Links

- Website: https://webbook.app
- GitHub: https://github.com/anthropics/webbook
- Documentation: webbook-code/docs/
EOF

echo ""
echo "Reorganization complete!"
echo ""
echo "New structure:"
echo "  $REPO_NAME/"
echo "  ├── webbook-code/     (your code repository)"
echo "  ├── webbook-website/  (website repository)"
echo "  └── webbook-market/   (marketing repository)"
echo ""
echo "Next steps:"
echo "  1. cd $REPO_NAME/webbook-code"
echo "  2. Update remote URL if needed: git remote set-url origin <new-url>"
echo "  3. Push changes to confirm everything works"
