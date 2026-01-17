#!/bin/bash
# Checks for inline tests in src/ directories
# Allows exceptions marked with: // INLINE_TEST_REQUIRED: <reason>
#
# Usage: ./scripts/check_inline_tests.sh
# Returns: 0 if no violations, 1 if violations found

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

VIOLATIONS=0
EXCEPTIONS=0

echo "Checking for inline tests in src/ directories..."
echo ""

# Find all Rust files in src/ directories
while IFS= read -r file; do
    # Skip if file doesn't exist (glob found nothing)
    [[ -f "$file" ]] || continue

    # Check for #[cfg(test)] in the file
    if grep -q '#\[cfg(test)\]' "$file"; then
        # Check if there's an exception marker within 5 lines before the #[cfg(test)]
        if grep -B5 '#\[cfg(test)\]' "$file" | grep -q 'INLINE_TEST_REQUIRED:'; then
            echo -e "${YELLOW}EXCEPTION:${NC} $file"
            # Extract the reason
            reason=$(grep -B5 '#\[cfg(test)\]' "$file" | grep 'INLINE_TEST_REQUIRED:' | sed 's/.*INLINE_TEST_REQUIRED://' | head -1)
            echo "           Reason:$reason"
            ((EXCEPTIONS++))
        else
            echo -e "${RED}VIOLATION:${NC} $file"
            echo "           Found #[cfg(test)] without INLINE_TEST_REQUIRED marker"
            ((VIOLATIONS++))
        fi
    fi

    # Also check for mod tests declarations (common pattern)
    if grep -q 'mod tests' "$file"; then
        # Only flag if it's actually a test module (has #[test] nearby or cfg(test))
        if grep -A20 'mod tests' "$file" | grep -q '#\[test\]'; then
            if ! grep -B5 'mod tests' "$file" | grep -q 'INLINE_TEST_REQUIRED:'; then
                # Avoid double-counting if already caught by cfg(test) check
                if ! grep -q '#\[cfg(test)\]' "$file"; then
                    echo -e "${RED}VIOLATION:${NC} $file"
                    echo "           Found test module without INLINE_TEST_REQUIRED marker"
                    ((VIOLATIONS++))
                fi
            fi
        fi
    fi
done < <(find . -path "*/src/*.rs" -type f ! -path "./target/*" ! -path "./.test_refactor_backup/*")

echo ""
echo "================================"
echo -e "Violations: ${RED}$VIOLATIONS${NC}"
echo -e "Exceptions: ${YELLOW}$EXCEPTIONS${NC}"
echo "================================"

if [[ $VIOLATIONS -gt 0 ]]; then
    echo ""
    echo "To fix violations, either:"
    echo "  1. Move tests to the tests/ directory (preferred)"
    echo "  2. Add exception marker above #[cfg(test)]:"
    echo "     // INLINE_TEST_REQUIRED: <reason why inline is necessary>"
    echo ""
    exit 1
fi

echo -e "${GREEN}All inline tests are properly documented or moved to tests/${NC}"
exit 0
