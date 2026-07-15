#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
TMPDIR_ROOT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_ROOT"' EXIT HUP INT TERM

mkdir -p "$TMPDIR_ROOT/core/scripts" "$TMPDIR_ROOT/bin"
cp "$ROOT/scripts/build-bindings.sh" "$TMPDIR_ROOT/core/scripts/"

cat > "$TMPDIR_ROOT/bin/uname" <<'EOF'
#!/bin/sh
printf '%s\n' Darwin
EOF

cat > "$TMPDIR_ROOT/bin/rustup" <<'EOF'
#!/bin/sh
if [ "${1:-}" = show ]; then
    printf '%s\n' test-toolchain
fi
EOF

cat > "$TMPDIR_ROOT/bin/cargo" <<'EOF'
#!/bin/sh
set -eu
printf '%s\t%s\t%s\t%s\t%s\n' \
    "$*" \
    "${IPHONEOS_DEPLOYMENT_TARGET:-}" \
    "${CFLAGS_aarch64_apple_ios:-}" \
    "${CFLAGS_aarch64_apple_ios_sim:-}" \
    "${CFLAGS_x86_64_apple_ios:-}" >> "$CARGO_LOG"

case " $* " in
    *' build '*)
        for target in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
            mkdir -p "target/$target/release"
            : > "target/$target/release/libvauchi_platform.a"
        done
        ;;
    *' run '*)
        while [ "$#" -gt 0 ]; do
            if [ "$1" = --out-dir ]; then
                mkdir -p "$2"
                : > "$2/vauchi_platform.swift"
                : > "$2/vauchi_platformFFI.h"
                break
            fi
            shift
        done
        ;;
esac
EOF

cat > "$TMPDIR_ROOT/bin/lipo" <<'EOF'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
    if [ "$1" = -output ]; then
        : > "$2"
        exit 0
    fi
    shift
done
exit 1
EOF

chmod +x "$TMPDIR_ROOT/bin/"*
CARGO_LOG="$TMPDIR_ROOT/cargo.log"
export CARGO_LOG
PATH="$TMPDIR_ROOT/bin:$PATH" \
    bash "$TMPDIR_ROOT/core/scripts/build-bindings.sh" --ios >/dev/null

if awk -F '\t' '$2 != "" { found = 1 } END { exit !found }' "$CARGO_LOG"; then
    echo "FAIL: iOS deployment target leaked into host Cargo builds" >&2
    exit 1
fi

if ! awk -F '\t' '
    $3 !~ /-miphoneos-version-min=10[.]0/ ||
    $4 !~ /-mios-simulator-version-min=10[.]0/ ||
    $5 !~ /-mios-simulator-version-min=10[.]0/ { exit 1 }
    END { if (NR == 0) exit 1 }
' "$CARGO_LOG"; then
    echo "FAIL: an iOS target is missing its deployment flag" >&2
    exit 1
fi

echo "PASS: iOS deployment flags are target-scoped"
