#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Vendors the English catalogue into vauchi-app so consumers that build
# this crate do not each have to clone the locales repo.
#
# Why this exists: vauchi-app's build.rs embeds en.json at compile time
# and, when it cannot find one, silently substitutes a two-key stub
# ("app.name" + "welcome.title") behind a cargo:warning. Every other
# string then renders as `Missing: <key>` — the iOS symptom in
# backlog/2026-07-24-ios-exchange-strings-missing-locale-keys.md. Each
# Rust consumer worked around that by cloning locales in its own CI,
# which put a build input of core into a frontend's pipeline.
#
# build.rs searches the sibling checkout FIRST and this vendored copy
# last, so developers keep live copy and only consumers without a
# checkout fall back to the vendored one.
#
# Usage:
#   vendor-locales.sh                   refresh the vendored copy from ../locales
#   vendor-locales.sh --check           fail if the vendored copy is not locales HEAD
#   vendor-locales.sh --check-revision  fail if the vendored copy is not the
#                                       locales revision its REVISION names
#
# MR pipelines use --check-revision: a locales merge makes every open core
# MR's copy stale at once, through no fault of the MR, and sync:vendor-locales
# refreshes it on main (#432). --check-revision needs that revision in the
# checkout, so unshallow a --depth=1 clone first.
#
# Portability: POSIX shell only (runs in alpine CI).
set -eu

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
CRATE_DIR="$SCRIPT_DIR/../vauchi-app"
VENDOR_DIR="$CRATE_DIR/locales"
SOURCE_DIR="${VAUCHI_LOCALES_DIR:-$SCRIPT_DIR/../../locales}"

CHECK_MODE=0
case "${1:-}" in
    "") ;;
    --check) CHECK_MODE=1 ;;
    --check-revision) CHECK_MODE=2 ;;
    *)
        echo "vendor-locales: unknown argument '$1' (expected --check or --check-revision)" >&2
        exit 2
        ;;
esac

if [ ! -f "$SOURCE_DIR/en.json" ]; then
    if [ "$CHECK_MODE" -ne 0 ]; then
        # Nothing to compare against. Not an error: consumers legitimately
        # build without the checkout — that is what the vendored copy is
        # for. Only a workspace that HAS the source can police drift.
        echo "vendor-locales: no locales checkout at $SOURCE_DIR — skipping drift check"
        exit 0
    fi
    echo "vendor-locales: no locales checkout at $SOURCE_DIR" >&2
    echo "Set VAUCHI_LOCALES_DIR or clone the sibling repo." >&2
    exit 2
fi

SOURCE_REV=$(git -C "$SOURCE_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown)

if [ "$CHECK_MODE" -eq 2 ]; then
    VENDORED_REV=$(cat "$VENDOR_DIR/REVISION" 2>/dev/null || true)
    if [ -z "$VENDORED_REV" ] || [ "$VENDORED_REV" = unknown ]; then
        echo "vendor-locales: $VENDOR_DIR/REVISION does not name a locales revision" >&2
        echo "Run: core/scripts/vendor-locales.sh" >&2
        exit 1
    fi
    if ! git -C "$SOURCE_DIR" rev-parse --verify --quiet "$VENDORED_REV^{commit}" >/dev/null; then
        echo "vendor-locales: locales revision $VENDORED_REV is not in $SOURCE_DIR" >&2
        echo "(a --depth=1 clone needs: git -C $SOURCE_DIR fetch --unshallow origin main)" >&2
        exit 1
    fi
    VENDORED_BLOB=$(git hash-object "$VENDOR_DIR/en.json" 2>/dev/null || echo missing)
    if [ "$VENDORED_BLOB" != "$(git -C "$SOURCE_DIR" rev-parse --verify --quiet "$VENDORED_REV:en.json")" ]; then
        echo "vendor-locales: the vendored en.json is not locales $VENDORED_REV's." >&2
        echo "Run: core/scripts/vendor-locales.sh" >&2
        exit 1
    fi
    if git -C "$SOURCE_DIR" diff --quiet "$VENDORED_REV" HEAD -- en.json; then
        echo "vendor-locales: vendored catalogue is locales $VENDORED_REV, current"
    else
        echo "vendor-locales: vendored catalogue is locales $VENDORED_REV, behind locales $SOURCE_REV;"
        echo "sync:vendor-locales refreshes it on main"
    fi
    exit 0
fi

if [ "$CHECK_MODE" -eq 1 ]; then
    if [ ! -f "$VENDOR_DIR/en.json" ]; then
        echo "vendor-locales: no vendored copy at $VENDOR_DIR/en.json" >&2
        echo "Run: core/scripts/vendor-locales.sh" >&2
        exit 1
    fi
    if ! diff -q "$SOURCE_DIR/en.json" "$VENDOR_DIR/en.json" >/dev/null 2>&1; then
        echo "vendor-locales: the vendored catalogue has drifted from locales." >&2
        echo "A consumer building without a checkout would ship stale copy." >&2
        echo "Run: core/scripts/vendor-locales.sh" >&2
        exit 1
    fi
    echo "vendor-locales: vendored catalogue matches locales ($SOURCE_REV)"
    exit 0
fi

mkdir -p "$VENDOR_DIR"
cp "$SOURCE_DIR/en.json" "$VENDOR_DIR/en.json"

# Provenance travels with the copy: it is not a git checkout, so build.rs
# cannot resolve a revision from it. Without this the build would record
# core's own revision and misreport it as the locales revision.
printf '%s\n' "$SOURCE_REV" > "$VENDOR_DIR/REVISION"

echo "vendor-locales: vendored en.json at $SOURCE_REV → $VENDOR_DIR"
