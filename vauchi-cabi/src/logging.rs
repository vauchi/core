// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rust -> stderr `tracing` bridge for Windows (C#/P-Invoke) and Linux-Qt
//! (C++).
//!
//! Routes `vauchi-app`'s `tracing::warn!`/`tracing::error!` call sites —
//! the canonical client logging facade per ADR-067 — to stderr. These
//! consumers have no Rust `main()` to install a subscriber, so the install
//! is exposed as a C ABI call the host makes at startup — mirrors
//! `vauchi-platform`'s `init_mobile_logging()`. Dev-only: installs a
//! subscriber solely under the `dev-logging` cargo feature; a release
//! binary installs none, so events are dropped.

/// Install the stderr `tracing` subscriber. Safe to call more than once —
/// only the first call takes effect.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_cabi_init_logging() {
    #[cfg(feature = "dev-logging")]
    let _ = std::panic::catch_unwind(|| {
        use tracing_subscriber::prelude::*;
        // Info default (matches the mobile os_log/Logcat backends and
        // logging-rules.md's "Info level only"); RUST_LOG overrides.
        // try_init is a no-op if a subscriber is already installed.
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .with(filter)
            .try_init();
    });
}

// INLINE_TEST_REQUIRED: cdylib crate-type prevents integration tests in tests/ directory
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn init_logging_does_not_panic_when_called_repeatedly() {
        // allow(zero_assertions): repeated init must not panic — there is
        // no observable state to assert beyond that.
        unsafe {
            vauchi_cabi_init_logging();
            vauchi_cabi_init_logging();
            vauchi_cabi_init_logging();
        }
    }

    // Shipped-binary posture: with `dev-logging` off, init installs no
    // subscriber, so `tracing`'s global max level stays `OFF` and
    // vauchi-app's `tracing::` events are dropped. Relies on nextest
    // process-per-test isolation for the global dispatcher.
    // @internal
    #[cfg(not(feature = "dev-logging"))]
    #[test]
    fn release_build_installs_no_log_backend() {
        unsafe {
            vauchi_cabi_init_logging();
        }
        assert_eq!(
            tracing::level_filters::LevelFilter::current(),
            tracing::level_filters::LevelFilter::OFF
        );
    }

    // Dev-logging posture: with `dev-logging` on, init installs the stderr
    // subscriber at Info (fmt layer + "info" EnvFilter default), so
    // vauchi-app's `tracing::warn!` failure-path events surface. Relies on
    // nextest process-per-test isolation for the global dispatcher.
    // @internal
    #[cfg(feature = "dev-logging")]
    #[test]
    fn dev_logging_build_installs_backend_at_info() {
        unsafe {
            vauchi_cabi_init_logging();
        }
        assert!(
            tracing::level_filters::LevelFilter::current()
                >= tracing::level_filters::LevelFilter::INFO,
            "dev-logging subscriber must surface Info/Warn, got {:?}",
            tracing::level_filters::LevelFilter::current()
        );
    }
}
