#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# e2e's smoke suite against a Core merge request's own commit
# (vauchi/private#1032, D8), with the failure attributed
# (vauchi/private#1034): e2e's pinned set is built against the MR's Core;
# if that fails, the same is tried against the target branch's Core.
# When the target fails too, the pinned set is behind Core (a Core minor
# moved and e2e's bump has not landed), not this MR's doing: exit 3,
# which the job allows as a warning.
#
# Usage: ci/e2e-smoke.sh <e2e-checkout> <mr-core-dir> <target-core-dir>
# Exit:  0 the MR passes; 1 only the MR fails; 3 the target fails too.

set -eu

# e2e's Core device calls this Core in-process and no CLI is built, so a
# Core change the CLI has yet to follow can still merge; the CLI's own
# pipeline runs the suite on the CLI at its pin bump (vauchi/private#1118).
export E2E_DEVICE_BACKEND=core

e2e=${1:?usage: e2e-smoke.sh <e2e-checkout> <mr-core-dir> <target-core-dir>}
mr_core=${2:?mr core dir}
target_core=${3:?target core dir}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# E2E_SMOKE_RELAY_SHA (private#1183): a dependency Core and the relay both
# link (libsqlite3-sys) moves in only one of them per build, so the MR
# that moves it cannot resolve against e2e's pinned relay. It names the
# relay commit that moved with it, for the MR's attempt only; the
# target's attempt keeps e2e's pins, so an MR failure still exits 1.
# Remove it from .gitlab-ci.yml when e2e pins that relay.
relay_override=${E2E_SMOKE_RELAY_SHA:-}
if [ -n "$relay_override" ]; then
    case "$relay_override" in
        *[!0-9a-f]*) valid=no ;;
        *) [ "${#relay_override}" -eq 40 ] && valid=yes || valid=no ;;
    esac
    if [ "$valid" != yes ]; then
        echo "ERROR: E2E_SMOKE_RELAY_SHA must be a full 40-hex relay commit, got '$relay_override'" >&2
        exit 2
    fi
fi

point_relay_at() { # copy sha
    pinned=$(sed -n 's/^RELAY_SHA=//p' "$1/ci/pins.env")
    echo "relay override: $2 (e2e pins $pinned)"
    sed "/^vauchi-relay = /s/rev = \"[0-9a-f]*\"/rev = \"$2\"/" "$1/Cargo.toml" > "$1/Cargo.toml.new" &&
        mv "$1/Cargo.toml.new" "$1/Cargo.toml" &&
        sed "s/^RELAY_SHA=.*/RELAY_SHA=$2/" "$1/ci/pins.env" > "$1/ci/pins.env.new" &&
        mv "$1/ci/pins.env.new" "$1/ci/pins.env"
}

# One attempt in a fresh copy of the e2e checkout, so the rewritten
# manifest and the build of one attempt never leak into the other.
smoke_against() { # core-dir name
    copy="$work/$2"
    cp -R "$e2e" "$copy"
    if [ "$2" = mr ] && [ -n "$relay_override" ]; then
        point_relay_at "$copy" "$relay_override" || return 1
    fi
    sh "$copy/ci/point-core-at.sh" "$1" "$copy/Cargo.toml" &&
        E2E_CORE_DIR="$1" sh "$copy/ci/build-pinned-binaries.sh" "$copy/.bin" &&
        (cd "$copy" && E2E_BIN_DIR="$copy/.bin" cargo nextest run --profile ci \
            --run-ignored default -j 4 -- smoke_)
}

if smoke_against "$mr_core" mr; then
    echo "e2e smoke passes against this merge request's Core"
    exit 0
fi

echo "e2e smoke failed against this merge request's Core; trying the target branch's Core"
if smoke_against "$target_core" target; then
    echo "FAIL: e2e smoke passes against the target branch and fails against this merge request" >&2
    exit 1
fi

echo "WARN: e2e smoke fails against the target branch's Core too: e2e's pinned set is behind Core." >&2
echo "      Not this merge request's doing; land e2e's Core bump (ci/pins.env and Cargo.toml)." >&2
exit 3
