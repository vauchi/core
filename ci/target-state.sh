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
# The first reading found a slot with no units at all, and nothing in the
# pipeline said who emptied it. So the job also leaves a note outside the
# target directory when it ends, and the next start prints it: an empty
# start then names the last job that saw the directory full, and the
# window in which it was emptied.
#
# Usage: ci/target-state.sh         before the first compile
#        ci/target-state.sh leave   after the last one
# Both from the core repo root.

target=${CARGO_TARGET_DIR:-target}
units=$target/debug/.fingerprint
notes=${TARGET_STATE_DIR:-$HOME/.cache/vauchi-ci/target-seen}
note=$notes/$(pwd | tr -c 'A-Za-z0-9\n' '_')

unit_count() {
    if [ -d "$units" ]; then
        find "$units" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' '
    else
        echo 0
    fi
}

if [ "${1:-}" = leave ]; then
    mkdir -p "$notes" &&
        echo "job ${CI_JOB_ID:-none} (${CI_JOB_NAME:-none}, pipeline ${CI_PIPELINE_ID:-none}) left $(unit_count) units at $(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$note"
    exit 0
fi

echo "target-state: slot $(pwd) on $(hostname), $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
echo "target-state: $(rustc --version 2>&1)"
echo "target-state: RUSTFLAGS=${RUSTFLAGS:-}"
if [ -f "$note" ]; then
    echo "target-state: before this job, $(cat "$note")"
else
    echo "target-state: no note from an earlier job in this slot"
fi
# A checkout younger than the note means the runner made the slot anew.
ls -ld . .git 2>&1
if [ ! -d "$units" ]; then
    echo "target-state: $units is missing: this build starts empty"
    exit 0
fi
echo "target-state: $(unit_count) units in $units, $(du -sh "$target" 2>/dev/null | cut -f1) in $target"
echo "target-state: units not touched for a day: $(find "$units" -mindepth 1 -maxdepth 1 -type d -mtime +0 | wc -l | tr -d ' ')"
echo "target-state: newest and oldest unit:"
# shellcheck disable=SC2012  # unit names are cargo's crate-hash names
ls -lt "$units" | sed -n '2p'
# shellcheck disable=SC2012
ls -ltr "$units" | sed -n '2p'
ls -ld "$target" "$target/debug" "$target/.rustc_info.json" 2>&1
exit 0
