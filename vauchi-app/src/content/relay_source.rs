// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The relay, over OHTTP, as the content source the binding layers use
//! (vauchi/private#733 D8, D11).

use vauchi_core::network::HttpTransport;

impl super::ContentSource for HttpTransport {
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String> {
        self.fetch_content(path).map_err(|e| e.to_string())
    }
}

/// The relay of `vauchi`'s configuration, over OHTTP, as a content source.
/// May fetch the gateway's signed key first when none is held.
pub fn relay_content_source(vauchi: &vauchi_core::api::Vauchi) -> super::ContentSourceHandle {
    let relay = &vauchi.config().relay;
    let transport =
        vauchi.build_action_relay_transport(&relay.server_url, relay.connect_timeout_ms);
    super::ContentSourceHandle(std::sync::Arc::new(transport))
}
