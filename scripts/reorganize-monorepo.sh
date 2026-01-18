#!/bin/bash
# Vauchi Monorepo Reorganization Script
#
# This script moves the code repository into vauchi-code/ subfolder,
# creating a parent directory structure with three separate git repos:
#
# Vauchi/
# ├── vauchi-code/     (main code, git history preserved)
# ├── vauchi-website/  (GitHub Pages site)
# └── vauchi-market/   (private marketing repo)
#
# IMPORTANT: Run this from the Vauchi directory
# BACKUP: Make sure you have pushed all changes before running

set -e

# Check we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d ".git" ]; then
    echo "Error: Run this script from the Vauchi repository root"
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

# Move vauchi-website and vauchi-market to temp
echo "Moving sibling repos to temp..."
if [ -d "vauchi-website" ]; then
    mv vauchi-website "$TEMP_DIR/"
fi
if [ -d "vauchi-market" ]; then
    mv vauchi-market "$TEMP_DIR/"
fi

# Go to parent, rename current dir to vauchi-code
echo "Renaming repository to vauchi-code..."
cd "$PARENT_DIR"
mv "$REPO_NAME" vauchi-code

# Create new parent directory
echo "Creating new parent directory..."
mkdir "$REPO_NAME"

# Move vauchi-code into new parent
mv vauchi-code "$REPO_NAME/"

# Move sibling repos into new parent
echo "Moving sibling repos back..."
if [ -d "$TEMP_DIR/vauchi-website" ]; then
    mv "$TEMP_DIR/vauchi-website" "$REPO_NAME/"
fi
if [ -d "$TEMP_DIR/vauchi-market" ]; then
    mv "$TEMP_DIR/vauchi-market" "$REPO_NAME/"
fi

# Clean up temp directory
rmdir "$TEMP_DIR"

# Create parent README
cat > "$REPO_NAME/README.md" << 'EOF'
# Vauchi

Privacy-focused contact card exchange.

## Repository Structure

This directory contains three separate git repositories:

| Directory | Description | Visibility |
|-----------|-------------|------------|
| `vauchi-code/` | Main application code (Rust, mobile apps, desktop) | Public |
| `vauchi-website/` | GitHub Pages website | Public |
| `vauchi-market/` | Marketing strategy and budget | Private |

## Quick Start

```bash
# Main development
cd vauchi-code
cargo test

# Website development
cd vauchi-website
python -m http.server 8000

# Marketing (team only)
cd vauchi-market
```

## Links

- Website: https://vauchi.app
- GitHub: https://github.com/anthropics/vauchi
- Documentation: vauchi-code/docs/
EOF

echo ""
echo "Reorganization complete!"
echo ""
echo "New structure:"
echo "  $REPO_NAME/"
echo "  ├── vauchi-code/     (your code repository)"
echo "  ├── vauchi-website/  (website repository)"
echo "  └── vauchi-market/   (marketing repository)"
echo ""
echo "Next steps:"
echo "  1. cd $REPO_NAME/vauchi-code"
echo "  2. Update remote URL if needed: git remote set-url origin <new-url>"
echo "  3. Push changes to confirm everything works"
