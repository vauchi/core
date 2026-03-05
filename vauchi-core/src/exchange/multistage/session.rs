// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage exchange session state machine.
//!
//! Manages the full 5-stage atomic QR exchange protocol:
//! `IDLE → ADVERTISING → DISCOVERED → TRANSFERRING → VERIFYING → CONFIRMING → COMPLETE`
//!
//! Each session holds local card data, ephemeral X25519 keys for transport
//! encryption, an Ed25519 identity keypair, and a commitment scheme that
//! ensures atomicity (neither side can decrypt until both reveal keys are exchanged).

use rand::rngs::OsRng;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Secret};
use zeroize::Zeroize;

use super::chunker::{Chunker, ReassemblyBuffer};
use super::commitment::Commitment;
use super::qr_codec::{self, StageQr};
use super::types::{ChunkBitmap, ProtocolState, QrPayload};

/// Maximum raw payload bytes per chunk (before transport encryption overhead).
/// Transport encryption adds 12 (nonce) + 16 (GCM tag) = 28 bytes overhead,
/// so with 500-byte QR chunk budget the usable payload is 472 bytes.
const CHUNK_PAYLOAD_SIZE: usize = 472;

/// HKDF info string for transport key derivation.
const HKDF_INFO: &[u8] = b"vauchi-multistage-v1";

/// Multi-stage exchange session managing the full 5-stage protocol.
///
/// The session progresses through states:
/// 1. **Idle** — created, not yet started
/// 2. **Advertising** — displaying INIT QR with our public keys and commitment hash
/// 3. **Discovered** — scanned peer's INIT, derived transport key (transient)
/// 4. **Transferring** — exchanging encrypted DATA chunks bidirectionally
/// 5. **Verifying** — all chunks exchanged, sending/receiving reveal keys
/// 6. **Confirming** — reveal key verified, sending/receiving confirmation
/// 7. **Complete** — both sides confirmed, peer data available
pub struct MultiStageSession {
    // Local data
    local_card: Vec<u8>,
    display_name: String,
    commitment: Commitment,
    session_id: [u8; 16],

    // Our keys
    identity_pubkey: [u8; 32],
    ephemeral_secret: Option<X25519Secret>,
    ephemeral_public: X25519Public,

    // Peer data (populated after Stage 1)
    peer_pubkey: Option<[u8; 32]>,
    peer_ephemeral: Option<[u8; 32]>,
    peer_commitment_hash: Option<[u8; 32]>,
    peer_session_id: Option<[u8; 16]>,

    // Transport encryption
    transport_key: Option<[u8; 32]>,

    // Outbound chunks (transport-encrypted commitment ciphertext)
    outbound_chunks: Vec<Vec<u8>>,
    outbound_total: u8,

    // Inbound tracking
    inbound_buffer: Option<ReassemblyBuffer>,
    inbound_bitmap: Option<ChunkBitmap>,
    peer_ack_bitmap: Option<ChunkBitmap>,
    peer_chunks_total: Option<u8>,

    // Reveal key received from peer
    peer_reveal_key: Option<[u8; 32]>,

    // Protocol state
    state: ProtocolState,

    // Received peer data (populated at Complete)
    received_data: Option<Vec<u8>>,

    // INIT QR cache (avoid regenerating)
    init_qr_cache: Option<String>,

    // Current outbound chunk index for round-robin display
    current_chunk_idx: u8,
}

impl MultiStageSession {
    /// Create a new session for exchanging the given contact card data.
    pub fn new(local_card: Vec<u8>) -> Self {
        let rng = SystemRandom::new();

        // Generate session ID
        let mut session_id = [0u8; 16];
        rng.fill(&mut session_id).expect("RNG failed");

        // Generate identity keypair (Ed25519-like, but we only need 32 bytes for the INIT QR)
        // For the multi-stage protocol, we use the X25519 public key as the "pubkey" field
        // since that's what matters for key agreement.
        let mut identity_seed = [0u8; 32];
        rng.fill(&mut identity_seed).expect("RNG failed");

        // Generate X25519 ephemeral keypair
        let ephemeral_secret = X25519Secret::random_from_rng(OsRng);
        let ephemeral_public = X25519Public::from(&ephemeral_secret);

        // Create commitment from local card
        let commitment = Commitment::create(&local_card);

        // Use identity seed as a stand-in pubkey for INIT QR
        // (In a full implementation, this would be a proper Ed25519 public key)
        let identity_pubkey = identity_seed;

        MultiStageSession {
            local_card,
            display_name: "Vauchi User".to_string(),
            commitment,
            session_id,
            identity_pubkey,
            ephemeral_secret: Some(ephemeral_secret),
            ephemeral_public,
            peer_pubkey: None,
            peer_ephemeral: None,
            peer_commitment_hash: None,
            peer_session_id: None,
            transport_key: None,
            outbound_chunks: Vec::new(),
            outbound_total: 0,
            inbound_buffer: None,
            inbound_bitmap: None,
            peer_ack_bitmap: None,
            peer_chunks_total: None,
            peer_reveal_key: None,
            state: ProtocolState::Idle,
            received_data: None,
            init_qr_cache: None,
            current_chunk_idx: 0,
        }
    }

    /// Returns the current protocol state.
    pub fn get_state(&self) -> ProtocolState {
        self.state.clone()
    }

    /// Returns the received peer data if the exchange is complete.
    pub fn get_received_data(&self) -> Option<Vec<u8>> {
        self.received_data.clone()
    }

    /// Cancel the session, transitioning to Failed and clearing sensitive data.
    pub fn cancel(&mut self) {
        self.state = ProtocolState::Failed("cancelled".to_string());
        self.clear_sensitive();
    }

    /// Get the QR payload to display based on current state.
    ///
    /// Returns `None` if no QR should be displayed (Complete or Failed).
    pub fn get_display_qr(&mut self) -> Option<QrPayload> {
        match &self.state {
            ProtocolState::Idle => {
                let qr_data = self.build_init_qr();
                self.init_qr_cache = Some(qr_data.clone());
                self.state = ProtocolState::Advertising;
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "M".to_string(),
                    display_duration_ms: 0,
                })
            }
            ProtocolState::Advertising => {
                let qr_data = self
                    .init_qr_cache
                    .clone()
                    .unwrap_or_else(|| self.build_init_qr());
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "M".to_string(),
                    display_duration_ms: 0,
                })
            }
            ProtocolState::Discovered => {
                // Transient state — should move to Transferring quickly
                // Return first data chunk if available
                self.get_data_chunk_qr()
            }
            ProtocolState::Transferring { .. } => self.get_data_chunk_qr(),
            ProtocolState::Verifying => {
                let qr_data =
                    qr_codec::format_verify_qr(&self.session_id, self.commitment.reveal_key());
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "M".to_string(),
                    display_duration_ms: 0,
                })
            }
            ProtocolState::Confirming => {
                // CONF contains hash of our original plaintext card
                let card_hash = self.compute_card_hash(&self.local_card);
                let qr_data = qr_codec::format_confirm_qr(&self.session_id, &card_hash);
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "M".to_string(),
                    display_duration_ms: 0,
                })
            }
            ProtocolState::Complete | ProtocolState::Failed(_) => None,
        }
    }

    /// Process a scanned QR code from the peer.
    ///
    /// Returns the new protocol state after processing.
    pub fn process_scanned_qr(&mut self, raw: &str) -> ProtocolState {
        let parsed = match qr_codec::parse_qr(raw) {
            Ok(p) => p,
            Err(_) => return self.state.clone(),
        };

        match parsed {
            StageQr::Init {
                session_id,
                pubkey,
                ephemeral,
                commitment_hash,
                display_name: _,
            } => self.handle_init(session_id, pubkey, ephemeral, commitment_hash),
            StageQr::Data {
                session_id: _,
                chunk_idx,
                chunk_total,
                ack_bitmap,
                crc: _,
                payload,
            } => self.handle_data(chunk_idx, chunk_total, ack_bitmap, payload),
            StageQr::Verify {
                session_id: _,
                reveal_key,
            } => self.handle_verify(reveal_key),
            StageQr::Confirm {
                session_id: _,
                payload_hash,
            } => self.handle_confirm(payload_hash),
        }
    }

    // --- Private helpers ---

    fn build_init_qr(&self) -> String {
        qr_codec::format_init_qr(
            &self.session_id,
            &self.identity_pubkey,
            self.ephemeral_public.as_bytes(),
            self.commitment.hash(),
            &self.display_name,
        )
    }

    fn handle_init(
        &mut self,
        session_id: [u8; 16],
        pubkey: [u8; 32],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
    ) -> ProtocolState {
        // Only accept INIT while Advertising
        if !matches!(self.state, ProtocolState::Advertising) {
            return self.state.clone();
        }

        // Store peer info
        self.peer_session_id = Some(session_id);
        self.peer_pubkey = Some(pubkey);
        self.peer_ephemeral = Some(ephemeral);
        self.peer_commitment_hash = Some(commitment_hash);

        // Derive transport key via X25519 DH + HKDF
        if let Some(secret) = self.ephemeral_secret.take() {
            let peer_public = X25519Public::from(ephemeral);
            let shared_secret = secret.diffie_hellman(&peer_public);
            let transport_key = self.derive_transport_key(shared_secret.as_bytes());
            self.transport_key = Some(transport_key);
        } else {
            self.state = ProtocolState::Failed("ephemeral secret already consumed".to_string());
            return self.state.clone();
        }

        // Prepare outbound: chunk the commitment ciphertext and transport-encrypt each chunk
        self.prepare_outbound_chunks();

        // Transition to Transferring (skip Discovered for efficiency)
        self.update_transfer_state();
        self.state.clone()
    }

    fn handle_data(
        &mut self,
        chunk_idx: u8,
        chunk_total: u8,
        ack_bitmap_bytes: Vec<u8>,
        encrypted_payload: Vec<u8>,
    ) -> ProtocolState {
        if !matches!(
            self.state,
            ProtocolState::Transferring { .. } | ProtocolState::Discovered
        ) {
            return self.state.clone();
        }

        let transport_key = match &self.transport_key {
            Some(k) => *k,
            None => return self.state.clone(),
        };

        // Initialize inbound buffer on first DATA chunk
        if self.inbound_buffer.is_none() {
            self.inbound_buffer = Some(ReassemblyBuffer::new(chunk_total));
            self.inbound_bitmap = Some(ChunkBitmap::new(chunk_total));
            self.peer_chunks_total = Some(chunk_total);
        }

        // Transport-decrypt the chunk
        if let Some(decrypted) =
            self.transport_decrypt(&transport_key, chunk_idx, &encrypted_payload)
        {
            if let Some(ref mut buffer) = self.inbound_buffer {
                buffer.insert(chunk_idx, decrypted);
            }
            if let Some(ref mut bitmap) = self.inbound_bitmap {
                bitmap.mark_received(chunk_idx);
            }
        }

        // Update peer's ACK bitmap (tells us which of our chunks they've received)
        self.peer_ack_bitmap = Some(ChunkBitmap::from_bytes(
            &ack_bitmap_bytes,
            self.outbound_total,
        ));

        self.update_transfer_state();
        self.state.clone()
    }

    fn handle_verify(&mut self, reveal_key: [u8; 32]) -> ProtocolState {
        if !matches!(self.state, ProtocolState::Verifying) {
            return self.state.clone();
        }

        // Reassemble received chunks into full ciphertext
        let ciphertext = match self.inbound_buffer.as_ref().and_then(|b| b.assemble()) {
            Some(ct) => ct,
            None => {
                self.state = ProtocolState::Failed("incomplete reassembly".to_string());
                return self.state.clone();
            }
        };

        // Verify commitment hash
        let peer_hash = match &self.peer_commitment_hash {
            Some(h) => *h,
            None => {
                self.state = ProtocolState::Failed("missing peer commitment hash".to_string());
                return self.state.clone();
            }
        };

        if !Commitment::verify_hash(&reveal_key, &ciphertext, &peer_hash) {
            self.state = ProtocolState::Failed("commitment verification failed".to_string());
            return self.state.clone();
        }

        // Decrypt with reveal key
        match Commitment::open_with_key(&reveal_key, &ciphertext) {
            Ok(plaintext) => {
                self.received_data = Some(plaintext);
                self.peer_reveal_key = Some(reveal_key);
                self.state = ProtocolState::Confirming;
            }
            Err(_) => {
                self.state = ProtocolState::Failed("decryption failed".to_string());
            }
        }

        self.state.clone()
    }

    fn handle_confirm(&mut self, payload_hash: [u8; 32]) -> ProtocolState {
        if !matches!(self.state, ProtocolState::Confirming) {
            return self.state.clone();
        }

        // Verify: payload_hash should be SHA-256 of peer's original plaintext
        // The peer sends SHA-256(their_plaintext), which should match
        // SHA-256(what we decrypted from their commitment)
        let expected_hash = match &self.received_data {
            Some(data) => self.compute_card_hash(data),
            None => {
                self.state = ProtocolState::Failed("no received data to confirm".to_string());
                return self.state.clone();
            }
        };

        if payload_hash == expected_hash {
            self.state = ProtocolState::Complete;
        } else {
            self.state = ProtocolState::Failed("confirmation mismatch".to_string());
        }

        self.state.clone()
    }

    fn prepare_outbound_chunks(&mut self) {
        let ciphertext = self.commitment.ciphertext();
        let chunker = Chunker::new(ciphertext, CHUNK_PAYLOAD_SIZE);
        let total = chunker.total_chunks();
        self.outbound_total = total;

        let transport_key = match &self.transport_key {
            Some(k) => *k,
            None => return,
        };

        let mut chunks = Vec::with_capacity(total as usize);
        for i in 0..total {
            if let Some(chunk_data) = chunker.chunk(i) {
                let encrypted = self.transport_encrypt(&transport_key, i, chunk_data);
                chunks.push(encrypted);
            }
        }
        self.outbound_chunks = chunks;
    }

    fn get_data_chunk_qr(&mut self) -> Option<QrPayload> {
        if self.outbound_chunks.is_empty() {
            return None;
        }

        // Find the next un-ACK'd chunk to send (round-robin)
        let start = self.current_chunk_idx;
        let total = self.outbound_total;

        for offset in 0..total {
            let idx = (start + offset) % total;

            // Check if peer already ACK'd this chunk
            let already_acked = self
                .peer_ack_bitmap
                .as_ref()
                .map(|b| b.has(idx))
                .unwrap_or(false);

            if !already_acked {
                self.current_chunk_idx = (idx + 1) % total;

                // Build ACK bitmap of what we've received
                let ack_bytes = self
                    .inbound_bitmap
                    .as_ref()
                    .map(|b| b.to_bytes())
                    .unwrap_or_default();

                let chunk_data = &self.outbound_chunks[idx as usize];
                let qr_data =
                    qr_codec::format_data_qr(&self.session_id, idx, total, &ack_bytes, chunk_data);

                return Some(QrPayload {
                    data: qr_data,
                    error_correction: "L".to_string(),
                    display_duration_ms: 500,
                });
            }
        }

        // All chunks ACK'd, return None (shouldn't normally happen — state should advance)
        None
    }

    fn update_transfer_state(&mut self) {
        let all_ours_acked = self
            .peer_ack_bitmap
            .as_ref()
            .map(|b| b.is_complete())
            .unwrap_or(false);

        let all_theirs_received = self
            .inbound_buffer
            .as_ref()
            .map(|b| b.is_complete())
            .unwrap_or(false);

        if all_ours_acked && all_theirs_received {
            self.state = ProtocolState::Verifying;
        } else {
            let chunks_sent = self
                .peer_ack_bitmap
                .as_ref()
                .map(|b| b.received_count())
                .unwrap_or(0);
            let chunks_received = self
                .inbound_buffer
                .as_ref()
                .map(|b| b.received_count())
                .unwrap_or(0);
            let peer_total = self.peer_chunks_total.unwrap_or(0);

            self.state = ProtocolState::Transferring {
                chunks_sent,
                chunks_total: self.outbound_total,
                chunks_received,
                peer_chunks_total: peer_total,
            };
        }
    }

    fn derive_transport_key(&self, shared_secret: &[u8]) -> [u8; 32] {
        use crate::crypto::kdf::HKDF;
        let prk = HKDF::extract(None, shared_secret);
        let okm = HKDF::expand(&prk, HKDF_INFO, 32).expect("HKDF expand failed");
        let mut key = [0u8; 32];
        key.copy_from_slice(&okm);
        key
    }

    fn transport_encrypt(&self, key: &[u8; 32], chunk_idx: u8, plaintext: &[u8]) -> Vec<u8> {
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; 12];
        rng.fill(&mut nonce_bytes).expect("RNG failed");

        let unbound = UnboundKey::new(&AES_256_GCM, key).expect("invalid key");
        let sealing_key = LessSafeKey::new(unbound);
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut in_out = plaintext.to_vec();
        sealing_key
            .seal_in_place_append_tag(nonce, Aad::from(&[chunk_idx]), &mut in_out)
            .expect("encryption failed");

        // nonce || ciphertext+tag
        let mut result = Vec::with_capacity(12 + in_out.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&in_out);
        result
    }

    fn transport_decrypt(
        &self,
        key: &[u8; 32],
        chunk_idx: u8,
        ciphertext: &[u8],
    ) -> Option<Vec<u8>> {
        if ciphertext.len() < 12 + 16 {
            return None;
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce_bytes);

        let unbound = UnboundKey::new(&AES_256_GCM, key).ok()?;
        let opening_key = LessSafeKey::new(unbound);
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        let mut in_out = encrypted.to_vec();
        let plaintext = opening_key
            .open_in_place(nonce, Aad::from(&[chunk_idx]), &mut in_out)
            .ok()?;
        Some(plaintext.to_vec())
    }

    fn compute_card_hash(&self, data: &[u8]) -> [u8; 32] {
        let d = digest(&SHA256, data);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        hash
    }

    fn clear_sensitive(&mut self) {
        if let Some(ref mut key) = self.transport_key {
            key.zeroize();
        }
        self.transport_key = None;
        self.ephemeral_secret = None;
        self.outbound_chunks.clear();
        self.received_data = None;
    }
}

impl Drop for MultiStageSession {
    fn drop(&mut self) {
        if let Some(ref mut key) = self.transport_key {
            key.zeroize();
        }
    }
}
