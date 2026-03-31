// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    // Skip header generation when cross-compiling — the C header is
    // platform-independent and checked in at include/vauchi.h.
    // cbindgen's `cargo metadata` call conflicts with cross-compile
    // toolchains (e.g., cargo-xwin sets lld-link flags that the host
    // rustc rejects). Validated by lint:cabi-header in CI.
    let target = std::env::var("TARGET").unwrap_or_default();
    let host = std::env::var("HOST").unwrap_or_default();
    if target != host {
        return;
    }

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    match cbindgen::generate(&crate_dir) {
        Ok(bindings) => {
            bindings.write_to_file("include/vauchi.h");
        }
        Err(e) => {
            println!("cargo:warning=cbindgen failed: {}", e);
        }
    }
}
