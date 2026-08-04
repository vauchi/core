#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
TMPDIR_ROOT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_ROOT"' EXIT HUP INT TERM

EXPECTED_TYPES='MobileContact
MobileContactCard
MobileContactField
MobileExchangeResult
MobileExchangeState
MobileRecoveryClaim
MobileRecoveryProgress
MobileRecoveryVerification
MobileRecoveryVoucher
MobileSocialNetwork
MobileSyncResult
MobileSyncStatus
MobileVisibilityLabel
MobileVisibilityLabelDetail
MobileFieldType
MobileError
VauchiPlatform'

write_binding() {
    file=$1
    minimum_lines=$2

    printf '%s\n' "$EXPECTED_TYPES" > "$file"
    awk -v minimum_lines="$minimum_lines" '
        BEGIN {
            for (line = 1; line <= minimum_lines; line++) {
                print "// generated binding filler"
            }
        }
    ' >> "$file"
}

make_fixture() {
    name=$1
    swift_export=$2
    kotlin_export=$3
    cabi_export=$4
    fixture="$TMPDIR_ROOT/$name"

    mkdir -p \
        "$fixture/project/scripts" \
        "$fixture/project/target/bindings/ios/generated" \
        "$fixture/project/target/bindings/android/kotlin/uniffi/vauchi_platform" \
        "$fixture/project/vauchi-cabi/include"
    cp "$ROOT/scripts/validate-bindings.sh" "$fixture/project/scripts/"

    swift_file="$fixture/project/target/bindings/ios/generated/vauchi_platform.swift"
    kotlin_file="$fixture/project/target/bindings/android/kotlin/uniffi/vauchi_platform/vauchi_platform.kt"
    header_file="$fixture/project/vauchi-cabi/include/vauchi.h"
    write_binding "$swift_file" 3500
    write_binding "$kotlin_file" 5000
    : > "$header_file"

    if [ "$swift_export" = present ]; then
        printf '%s\n' 'public func presentationContractFixtureJson() -> String {' >> "$swift_file"
    fi
    if [ "$kotlin_export" = present ]; then
        printf '%s\n' 'fun presentationContractFixtureJson(): kotlin.String {' >> "$kotlin_file"
    fi
    if [ "$cabi_export" = present ]; then
        printf '%s\n' 'char *vauchi_presentation_contract_fixture(void);' >> "$header_file"
    fi

    printf '%s\n' "$fixture"
}

assert_passes() {
    fixture=$1
    if ! bash "$fixture/project/scripts/validate-bindings.sh" > "$fixture/output" 2>&1; then
        echo "FAIL: validator rejected a complete presentation binding contract" >&2
        cat "$fixture/output" >&2
        exit 1
    fi
}

assert_fails() {
    fixture=$1
    description=$2
    if bash "$fixture/project/scripts/validate-bindings.sh" > "$fixture/output" 2>&1; then
        echo "FAIL: validator accepted $description" >&2
        exit 1
    fi
}

complete=$(make_fixture complete present present present)
missing_language_exports=$(make_fixture missing-language-exports absent absent present)
missing_cabi_export=$(make_fixture missing-cabi-export present present absent)

assert_passes "$complete"
assert_fails "$missing_language_exports" "bindings without the presentation fixture functions"
assert_fails "$missing_cabi_export" "a C header without the presentation fixture export"

echo "PASS: binding validation enforces the generic presentation contract"
