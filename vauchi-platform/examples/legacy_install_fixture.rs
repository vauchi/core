// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Writes the installs from before vauchi/private#580 that the shells'
//! upgrade tests start from: data under a storage key the shell kept, and
//! nothing in the keychain. The shells check these in, because the
//! constructor that wrote such an install is not on their binding surface.
//!
//! `cargo run -p vauchi-platform --example legacy_install_fixture -- <dir>`
//! writes `<dir>/empty` (no identity yet) and `<dir>/before` (identity
//! "Before", one imported contact, Carol). Both are under [`LEGACY_KEY`].

use vauchi_platform::PlatformAppEngineTestHelpers;
use vauchi_platform::{DomainCommand, PlatformAppEngine};

/// The shell-held storage key both installs are under. A fixed key, so the
/// shells can hand it over without regenerating the files.
const LEGACY_KEY: [u8; 32] = [0x58; 32];

const CAROL: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:before-580\r\nFN:Carol\r\nEND:VCARD\r\n";

fn open(dir: &std::path::Path) -> std::sync::Arc<PlatformAppEngine> {
    PlatformAppEngine::open_under_shell_key(
        dir.to_string_lossy().into_owned(),
        "https://relay.test".into(),
        LEGACY_KEY.to_vec(),
    )
    .expect("open the legacy install")
}

fn main() {
    let out = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: legacy_install_fixture <dir>"),
    );
    drop(open(&out.join("empty")));
    let before = open(&out.join("before"));
    before
        .dispatch_domain_command(DomainCommand::CreateIdentity {
            display_name: "Before".into(),
        })
        .expect("create identity");
    before
        .dispatch_domain_command(DomainCommand::ImportContactsFromVcf {
            data: CAROL.as_bytes().to_vec(),
        })
        .expect("import Carol");
}
