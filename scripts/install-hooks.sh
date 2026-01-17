#!/bin/bash
# Installs git hooks from scripts/hooks/ to .git/hooks/
# Usage: ./scripts/install-hooks.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
HOOKS_SRC="$SCRIPT_DIR/hooks"
HOOKS_DST="$REPO_ROOT/.git/hooks"

if [[ ! -d "$HOOKS_SRC" ]]; then
    echo "No hooks directory found at $HOOKS_SRC"
    exit 1
fi

echo "Installing git hooks..."

for hook in "$HOOKS_SRC"/*; do
    if [[ -f "$hook" ]]; then
        hook_name=$(basename "$hook")
        dst="$HOOKS_DST/$hook_name"

        # Backup existing hook if it exists and isn't a symlink
        if [[ -f "$dst" && ! -L "$dst" ]]; then
            echo "  Backing up existing $hook_name to $hook_name.backup"
            mv "$dst" "$dst.backup"
        fi

        # Create symlink
        ln -sf "$hook" "$dst"
        chmod +x "$hook"
        echo "  Installed: $hook_name"
    fi
done

echo "Done! Hooks installed to .git/hooks/"
