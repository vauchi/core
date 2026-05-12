// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tracing-flame layer setup for profiling vauchi-core directly.
//!
//! Enabled via the `flame` cargo feature. Captures the `#[instrument]`
//! spans declared on core entry points (identity, sync, network,
//! crypto, avatar) and writes them to a `.folded` file consumable by
//! `inferno-flamegraph`.
//!
//! Output: `$VAUCHI_FLAME_OUT` or
//! `<CARGO_MANIFEST_DIR>/artifacts/flame/core-<ts>.folded`.
//!
//! Writes are unbuffered so traces survive `process::exit()` from
//! libtest, which skips static destructors (and would therefore skip
//! a buffered `FlushGuard::drop`).

use std::fs::File;
use std::path::PathBuf;

use tracing_flame::FlameLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_FILTER: &str = "info,vauchi_core=trace";

/// Install the global subscriber with a flame layer. Idempotent: a
/// second call is a no-op.
pub fn init_layer() {
    let path = output_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("flame: create {} failed: {e}", parent.display()));
    }
    let file = File::create(&path)
        .unwrap_or_else(|e| panic!("flame: open {} failed: {e}", path.display()));
    let flame_layer = FlameLayer::new(file);

    let filter = std::env::var("VAUCHI_FLAME_FILTER")
        .ok()
        .and_then(|s| EnvFilter::try_new(s).ok())
        .unwrap_or_else(|| EnvFilter::new(DEFAULT_FILTER));

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(flame_layer)
        .try_init();
    match result {
        Ok(_) => eprintln!("[flame] writing folded trace -> {}", path.display()),
        Err(e) => eprintln!(
            "[flame] WARNING: subscriber install failed ({e}); another subscriber is already active"
        ),
    }
}

fn output_path() -> PathBuf {
    if let Ok(p) = std::env::var("VAUCHI_FLAME_OUT") {
        return PathBuf::from(p);
    }
    let ts = crate::clock::ambient_now_secs();
    let base = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("artifacts/flame")
        .join(format!("core-{ts}.folded"))
}
