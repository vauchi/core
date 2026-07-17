// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rust -> native log bridge for Windows (C#/P-Invoke) and Linux-Qt (C++).
//!
//! Routes the existing `log::warn!`/`log::error!` call sites in
//! `vauchi-app` (BLE/exchange/sync failure paths) to stderr. These
//! consumers have no Rust `main()` of their own to call
//! `env_logger::init()` from directly, so the backend install is exposed
//! as a C ABI call the host app makes at startup — mirrors
//! `vauchi-platform`'s `init_mobile_logging()` for the UniFFI consumers.

/// Install the stderr log backend. Safe to call more than once — only the
/// first call takes effect.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_cabi_init_logging() {
    let _ = std::panic::catch_unwind(|| {
        let _ = env_logger::try_init();
    });
}

// INLINE_TEST_REQUIRED: cdylib crate-type prevents integration tests in tests/ directory
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_logging_does_not_panic_when_called_repeatedly() {
        unsafe {
            vauchi_cabi_init_logging();
            vauchi_cabi_init_logging();
            vauchi_cabi_init_logging();
        }
    }
}
