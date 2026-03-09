// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
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
