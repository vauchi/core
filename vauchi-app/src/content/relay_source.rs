// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The relay, over OHTTP, as the content source the binding layers use
//! (vauchi/private#733 D8, D11).

use vauchi_core::network::HttpTransport;

impl super::ContentSource for HttpTransport {
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String> {
        self.fetch_content(path).map_err(|e| e.to_string())
    }
}
