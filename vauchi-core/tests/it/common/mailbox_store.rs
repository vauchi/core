// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! In-memory mailbox behavior for local OHTTP delivery tests. This is not
//! a relay implementation: TTL, federation, quotas and auth are not modeled.

use base64::Engine;
use std::num::NonZeroUsize;
use std::sync::Arc;
use vauchi_core::clock::Clock;
use vauchi_protocol::v2::{FetchedBlob, V2AckRequest, V2FetchRequest, V2SendRequest};

pub(super) struct MailboxStore {
    clock: Arc<dyn Clock>,
    page_size: NonZeroUsize,
    sequence: u64,
    blobs: Vec<FetchedBlob>,
}

impl MailboxStore {
    pub(super) fn new(clock: Arc<dyn Clock>, page_size: usize) -> Self {
        Self {
            clock,
            page_size: NonZeroUsize::new(page_size)
                .expect("test mailbox page size must be positive"),
            sequence: 0,
            blobs: Vec::new(),
        }
    }

    pub(super) fn reply(&mut self, action: &str, request: serde_json::Value) -> serde_json::Value {
        let reply = match action {
            "send" => serde_json::from_value(request).map(|request| self.send(request)),
            "fetch" => serde_json::from_value(request).map(|request| self.fetch(request)),
            "ack" => serde_json::from_value(request).map(|request| self.acknowledge(request)),
            _ => {
                return serde_json::json!({ "status": "error", "error": "unsupported mailbox action" });
            }
        };
        reply.unwrap_or_else(
            |_| serde_json::json!({ "status": "error", "error": "invalid mailbox request" }),
        )
    }

    fn send(&mut self, request: V2SendRequest) -> serde_json::Value {
        if request.recipient_id.len() != 64
            || !request
                .recipient_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return serde_json::json!({ "status": "error", "error": "recipient_id must be 64 hex characters" });
        }
        if base64::engine::general_purpose::STANDARD
            .decode(&request.ciphertext)
            .is_err()
        {
            return serde_json::json!({ "status": "error", "error": "ciphertext must be valid base64" });
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("test mailbox id exhausted");
        let blob_id = format!("{:064x}", self.sequence);
        self.blobs.push(FetchedBlob {
            blob_id: blob_id.clone(),
            ciphertext: request.ciphertext,
            created_at: self.clock.unix_seconds(),
            mailbox_token: Some(request.recipient_id),
            origin_hint: request.origin_hint,
        });
        serde_json::json!({ "status": "ok", "blob_id": blob_id })
    }

    fn fetch(&self, request: V2FetchRequest) -> serde_json::Value {
        // Match handle_fetch_logic's limit so planner tests cannot pass with
        // batches the application relay rejects (private#691).
        if !(1..=100).contains(&request.mailbox_tokens.len()) {
            return serde_json::json!({ "status": "error", "error": "mailbox_tokens must contain 1-100 entries" });
        }
        let mut matching: Vec<_> = self
            .blobs
            .iter()
            .filter(|blob| {
                blob.mailbox_token
                    .as_ref()
                    .is_some_and(|token| request.mailbox_tokens.contains(token))
            })
            .collect();
        matching.sort_by_key(|blob| blob.created_at);
        let truncated = matching.len() > self.page_size.get();
        matching.truncate(self.page_size.get());
        serde_json::json!({ "status": "ok", "blobs": matching, "truncated": truncated })
    }

    fn acknowledge(&mut self, request: V2AckRequest) -> serde_json::Value {
        let index = self.blobs.iter().position(|blob| {
            blob.blob_id == request.blob_id
                && blob.mailbox_token.as_deref() == Some(request.recipient_id.as_str())
        });
        let acknowledged = if let Some(index) = index {
            self.blobs.remove(index);
            true
        } else {
            false
        };
        serde_json::json!({ "status": "ok", "acknowledged": acknowledged })
    }
}
