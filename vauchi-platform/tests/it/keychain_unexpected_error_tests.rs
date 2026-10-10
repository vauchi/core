// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A keychain callback that fails in a way the platform did not declare
//! reaches Core as a keychain failure instead of aborting the process
//! (vauchi/private#580). Seen in Android CI: a keychain call made while the
//! JVM was exiting came back as an unexpected callback error, UniFFI found no
//! way to turn it into a `KeychainError`, panicked across the FFI boundary,
//! and the process aborted (exit 134).

use uniffi::{LiftReturn, UnexpectedUniFFICallbackError};
use vauchi_platform::{KeychainError, UniFfiTag};

// @internal
#[test]
fn an_unexpected_keychain_failure_is_an_operation_failure() {
    let lifted = <Result<Option<Vec<u8>>, KeychainError> as LiftReturn<UniFfiTag>>::handle_callback_unexpected_error(
        UnexpectedUniFFICallbackError {
            reason: "java.lang.IllegalStateException: /data/user/0/app.vauchi/files".into(),
        },
    );

    let Err(KeychainError::OperationFailed { msg }) = lifted else {
        panic!("expected OperationFailed, got {lifted:?}");
    };
    assert!(
        !msg.contains("/data/"),
        "the platform's message travelled: {msg}"
    );
}
