#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Lets main's build:tests use the test archives its merge train car built.
# A fast-forward train lands exactly the commit the car tested, and main's
# build:tests compiled that commit again under the cold-rebuild-tree lock
# (vauchi/private#1399). The archives stay on the runner host, so reuse is
# hard links into this pipeline's directory; anything uncertain (no car for
# the commit, another host, a pruned or unusual directory) makes main build
# as before.
#
# Usage: sh ci/train-archives.sh record   at the end of a car's build:tests
#        sh ci/train-archives.sh reuse    at the start of main's; exits 0
#                                         only when the archives are in place

set -eu

cache=$HOME/.cache/vauchi-ci
archives=$cache/archives/$CI_PROJECT_NAME
index_dir=$cache/archives-by-sha/$CI_PROJECT_NAME
index=$index_dir/$CI_COMMIT_SHA

case "${1:-}" in
record)
    mkdir -p "$index_dir"
    # Archives older than three hours are pruned by build:tests; so are
    # the entries that point at them.
    find "$index_dir" -type f -mmin +180 -exec rm -f {} + 2>/dev/null || true
    printf '%s\n' "$CI_PIPELINE_ID" > "$index.$CI_PIPELINE_ID"
    mv "$index.$CI_PIPELINE_ID" "$index"
    echo "train-archives: recorded pipeline $CI_PIPELINE_ID's archives for $CI_COMMIT_SHA"
    ;;
reuse)
    if [ ! -f "$index" ]; then
        echo "train-archives: no train car built $CI_COMMIT_SHA on this host; building"
        exit 1
    fi
    car=$(cat "$index")
    case "$car" in
        '' | *[!0-9]*)
            echo "train-archives: $index does not hold a pipeline id; building"
            exit 1
            ;;
    esac
    source_dir=$archives/$car
    target_dir=$archives/$CI_PIPELINE_ID
    if [ ! -d "$source_dir" ]; then
        echo "train-archives: pipeline $car's archives are gone; building"
        exit 1
    fi
    if [ "$(cat "$source_dir/built-on" 2>/dev/null || true)" != "$(hostname)" ]; then
        echo "train-archives: pipeline $car's archives were not built on $(hostname); building"
        exit 1
    fi
    rm -rf "$target_dir.tmp" "$target_dir"
    mkdir -p "$target_dir.tmp"
    for file in "$source_dir"/* "$source_dir"/.[!.]*; do
        [ -e "$file" ] || continue
        if [ ! -f "$file" ] || ! ln "$file" "$target_dir.tmp/"; then
            rm -rf "$target_dir.tmp"
            echo "train-archives: could not link $file; building"
            exit 1
        fi
    done
    mv "$target_dir.tmp" "$target_dir"
    echo "train-archives: reused train car pipeline $car's archives for $CI_COMMIT_SHA"
    ls -la "$target_dir"
    ;;
*)
    echo "usage: $0 record|reuse" >&2
    exit 2
    ;;
esac
