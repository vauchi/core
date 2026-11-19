#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Prints what build:tests finds in its runner slot's target directory
# before it compiles. A warm directory makes the job 4 to 6 minutes, a
# cold one 13 and more, and three cold builds on 2026-11-19 followed no
# job that cleans the directory (vauchi/private#1305). The lines tell an
# emptied directory (no units, or only young ones) from a full one that
# cargo rebuilt anyway (many old units), which is a changed compiler,
# flag or path rather than a deletion. It never fails the job.
#
# Usage: ci/target-state.sh   (from the core repo root)

target=${CARGO_TARGET_DIR:-target}
units=$target/debug/.fingerprint

echo "target-state: slot $(pwd) on $(hostname), $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
echo "target-state: $(rustc --version 2>&1)"
echo "target-state: RUSTFLAGS=${RUSTFLAGS:-}"
if [ ! -d "$units" ]; then
    echo "target-state: $units is missing: this build starts empty"
    exit 0
fi
echo "target-state: $(find "$units" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ') units in $units, $(du -sh "$target" 2>/dev/null | cut -f1) in $target"
echo "target-state: units not touched for a day: $(find "$units" -mindepth 1 -maxdepth 1 -type d -mtime +0 | wc -l | tr -d ' ')"
echo "target-state: newest and oldest unit:"
# shellcheck disable=SC2012  # unit names are cargo's crate-hash names
ls -lt "$units" | sed -n '2p'
# shellcheck disable=SC2012
ls -ltr "$units" | sed -n '2p'
ls -ld "$target" "$target/debug" "$target/.rustc_info.json" 2>&1
exit 0
