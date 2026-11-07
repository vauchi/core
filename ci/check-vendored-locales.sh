#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# The copy consumers build against must not drift from locales, or a build
# without a checkout ships stale copy. The only commit that can ship is one
# that cuts a release, so that commit needs locales HEAD (--check), which is
# all that keeps stale copy out of a release: this job does not run on tags,
# and auto-tag:version runs behind it.
#
# Every other pipeline only needs the copy to be exactly the revision it
# names. A locales merge stales every open MR at once (#432) and arrives on
# main before sync:vendor-locales refreshes it (private#557), and failing main
# for that holds up its dev tag through no fault of the commit (private#1112).
#
# A tag lookup that cannot answer counts as a release: the strict check is the
# safe side.
set -eu

locales_dir=${VAUCHI_LOCALES_DIR:-../locales}
export VAUCHI_LOCALES_DIR="$locales_dir"

version=$(grep -A5 '^\[workspace\.package\]' Cargo.toml | grep '^version' | sed 's/.*"\(.*\)"/\1/')

may_lag_locales() {
    case "${CI_PIPELINE_SOURCE:-}" in
        merge_request_event) return 0 ;;
        pipeline) [ "${CONTENT_TRIGGER_REPO:-}" = locales ] ;;
        *) git ls-remote --tags origin "refs/tags/v$version" 2>/dev/null | grep -q . ;;
    esac
}

if may_lag_locales; then
    if [ "$(git -C "$locales_dir" rev-parse --is-shallow-repository)" = true ]; then
        git -C "$locales_dir" fetch --quiet --unshallow origin main
    fi
    exec sh scripts/vendor-locales.sh --check-revision
fi

echo "vendor-locales: this commit cuts release v$version, so it needs locales HEAD"
exec sh scripts/vendor-locales.sh --check
