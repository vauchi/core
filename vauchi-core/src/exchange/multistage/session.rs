// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage exchange session state machine.
//!
//! Manages the full 5-stage atomic QR exchange protocol:
//! `IDLE → ADVERTISING → DISCOVERED → TRANSFERRING → VERIFYING → CONFIRMING → COMPLETE → FINALIZED`
//!
//! Each session holds local card data, ephemeral X25519 keys for transport
//! encryption, an Ed25519 identity keypair, and a commitment scheme that
//! ensures atomicity (neither side can decrypt until both reveal keys are exchanged).
//!
//! Resilience features:
//! - Wall-clock timeouts with progress extension (not tick-based)
//! - COMBO QR (VRFY+CONF+RDYY) display in Complete state for single-scan finalization
//! - Adaptive QR display durations per stage
//! - Graduated retry: Complete → RetryReady → Failed (one auto-retry)
//! - FAIL QR type for immediate peer abort notification
//! - Scan acknowledgment via RDYY payload

use std::time::{Duration, Instant};

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Secret};
use zeroize::Zeroize;

use super::chunker::{Chunker, ReassemblyBuffer};
use super::commitment::Commitment;
use super::qr_codec::{self, StageQr};
use super::types::{ChunkBitmap, ProtocolState, QrPayload};

/// Maximum raw payload bytes per chunk (before transport encryption overhead).
/// Transport encryption adds 12 (nonce) + 16 (Poly1305 tag) = 28 bytes overhead,
/// so with 500-byte QR chunk budget the usable payload is 472 bytes.
const CHUNK_PAYLOAD_SIZE: usize = 472;

/// HKDF info string for transport key derivation.
const HKDF_INFO: &[u8] = b"vauchi-multistage-v1";

/// Wall-clock timeout for the RDYY phase (Complete + RetryReady).
/// From device testing: Samsung S7 finalizes ~18s after Pixel.
/// 30s base gives comfortable margin for slow devices.
const RDYY_BASE_TIMEOUT: Duration = Duration::from_secs(30);

/// How much extra time is granted when the peer shows progress (e.g. scan detected).
const RDYY_PROGRESS_EXTENSION: Duration = Duration::from_secs(10);

/// Absolute maximum time in the RDYY phase (prevents infinite extension).
const RDYY_MAX_TIMEOUT: Duration = Duration::from_secs(45);

/// How long to broadcast FAIL QR after entering Failed state.
const FAIL_BROADCAST_DURATION: Duration = Duration::from_secs(5);

/// How long to continue broadcasting RDYY after Finalized (grace period for peer).
/// MUST be >= RDYY_MAX_TIMEOUT so the fast device never stops before the slow
/// device's timeout expires. The user sees "Contact exchanged!" immediately
/// (save on Finalized) while QRs continue in the background.
const FINALIZED_GRACE_DURATION: Duration = Duration::from_secs(60);

/// Base display durations per QR type (jitter added at runtime).
/// Tuned for <5s total exchange on typical hardware.
/// Shorter = more scan opportunities per second = faster convergence.
const DISPLAY_MS_INIT: u32 = 600;
const DISPLAY_MS_DATA: u32 = 250;
const DISPLAY_MS_VRFY: u32 = 400;
const DISPLAY_MS_CONF: u32 = 400;
const DISPLAY_MS_RDYY: u32 = 500;
const DISPLAY_MS_FAIL: u32 = 400;

/// Add ±20% jitter to prevent synchronization lock between two devices.
/// When both devices cycle QRs at identical cadence, they can stay in phase
/// and always scan during the other's transition — missing every QR.
/// Jitter breaks the phase alignment.
fn jittered(base_ms: u32) -> u32 {
    let jitter_range = base_ms / 5; // ±20%
    let jitter: u32 = crate::crypto::random_bytes::<4>()
        .iter()
        .fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
        % (jitter_range * 2 + 1);
    base_ms - jitter_range + jitter
}

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

    // Display cycle counter — every Nth cycle, re-show INIT so a slower
    // peer still in Advertising can discover us.
    display_cycle: u32,

    // Wall-clock timestamps for timeout management.
    phase_entered_at: Option<Instant>,
    last_progress_at: Option<Instant>,

    // Track whether we've already used the auto-retry.
    retry_used: bool,

    // When Failed, continue broadcasting FAIL QR until this deadline.
    fail_broadcast_until: Option<Instant>,

    // Our relay metadata (included in our INIT QR)
    our_relay_url: Option<String>,
    our_relay_noise_pubkey: Option<[u8; 32]>,

    // Peer's relay metadata (extracted from their INIT QR)
    peer_relay_url: Option<String>,
    peer_relay_noise_pubkey: Option<[u8; 32]>,
}

impl MultiStageSession {
    /// Create a new session for exchanging the given contact card data.
    pub fn new(local_card: Vec<u8>) -> Self {
        Self::new_with_relay(local_card, None, None)
    }

    /// Create a new session with optional relay metadata.
    ///
    /// The relay URL and Noise NK pubkey will be included in our INIT QR
    /// so the peer can route future messages to our relay.
    pub fn new_with_relay(
        local_card: Vec<u8>,
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
    ) -> Self {
        // Generate session ID
        let session_id: [u8; 16] = crate::crypto::random_bytes();

        // Generate identity keypair (Ed25519-like, but we only need 32 bytes for the INIT QR)
        // For the multi-stage protocol, we use the X25519 public key as the "pubkey" field
        // since that's what matters for key agreement.
        let identity_seed: [u8; 32] = crate::crypto::random_bytes();

        // Generate X25519 ephemeral keypair
        let ephemeral_secret = X25519Secret::random_from_rng(OsRng);
        let ephemeral_public = X25519Public::from(&ephemeral_secret);

        // Create commitment from local card, binding our relay metadata (T1.7)
        let commitment_context =
            Self::build_commitment_context(relay_url.as_deref(), relay_noise_pubkey.as_ref());
        let commitment = Commitment::create_with_context(&local_card, &commitment_context);

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
            display_cycle: 0,
            phase_entered_at: None,
            last_progress_at: None,
            retry_used: false,
            fail_broadcast_until: None,
            our_relay_url: relay_url,
            our_relay_noise_pubkey: relay_noise_pubkey,
            peer_relay_url: None,
            peer_relay_noise_pubkey: None,
        }
    }

    /// Returns the current protocol state.
    pub fn get_state(&self) -> ProtocolState {
        self.state.clone()
    }

    /// Returns the received peer data if the exchange is finalized.
    ///
    /// Data is only available in the `Finalized` state — both sides must
    /// have exchanged READY QRs to confirm mutual completion. This ensures
    /// atomicity: neither side persists a contact unless both succeeded.
    pub fn get_received_data(&self) -> Option<Vec<u8>> {
        if matches!(self.state, ProtocolState::Finalized) {
            self.received_data.clone()
        } else {
            None
        }
    }

    /// Returns the transport key derived during the ECDH key exchange.
    ///
    /// Available after the Discovered state (once both sides have exchanged
    /// ephemeral public keys). Used by the mobile layer to derive the shared
    /// secret for the double ratchet after exchange completion.
    pub fn get_transport_key(&self) -> Option<[u8; 32]> {
        self.transport_key
    }

    /// Returns the peer's relay URL (available after INIT exchange).
    pub fn peer_relay_url(&self) -> Option<&str> {
        self.peer_relay_url.as_deref()
    }

    /// Returns the peer's relay Noise NK public key (available after INIT exchange).
    pub fn peer_relay_noise_pubkey(&self) -> Option<[u8; 32]> {
        self.peer_relay_noise_pubkey
    }

    /// Cancel the session, transitioning to Failed and clearing sensitive data.
    pub fn cancel(&mut self) {
        self.state = ProtocolState::Failed("cancelled".to_string());
        self.clear_sensitive();
    }

    /// Get the QR payload to display based on current state.
    ///
    /// Returns `None` after the finalized grace period expires or after
    /// the FAIL broadcast window closes.
    pub fn get_display_qr(&mut self) -> Option<QrPayload> {
        match &self.state {
            ProtocolState::Idle => {
                let qr_data = self.build_init_qr();
                self.init_qr_cache = Some(qr_data.clone());
                self.state = ProtocolState::Advertising;
                // Use "L" error correction for INIT/INID — produces less dense QR
                // that scans faster on older cameras (Samsung S7).
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "L".to_string(),
                    display_duration_ms: jittered(DISPLAY_MS_INIT),
                })
            }
            ProtocolState::Advertising => {
                let qr_data = self
                    .init_qr_cache
                    .clone()
                    .unwrap_or_else(|| self.build_init_qr());
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "L".to_string(),
                    display_duration_ms: jittered(DISPLAY_MS_INIT),
                })
            }
            ProtocolState::Discovered => {
                // Transient state — should move to Transferring quickly
                // Return first data chunk if available
                self.display_cycle += 1;
                self.get_data_chunk_qr()
            }
            ProtocolState::Transferring { .. } => {
                self.display_cycle += 1;
                // Every 4th cycle, re-show INIT so a slower peer still in
                // Advertising can discover us (fixes the race condition where
                // one device transitions to Transferring before the other
                // scans our INIT).
                if self.display_cycle.is_multiple_of(4) {
                    let qr_data = self
                        .init_qr_cache
                        .clone()
                        .unwrap_or_else(|| self.build_init_qr());
                    Some(QrPayload {
                        data: qr_data,
                        error_correction: "M".to_string(),
                        display_duration_ms: jittered(DISPLAY_MS_DATA), // S3: adaptive
                    })
                } else {
                    self.get_data_chunk_qr()
                }
            }
            ProtocolState::Verifying => {
                self.display_cycle += 1;
                let qr_data =
                    qr_codec::format_verify_qr(&self.session_id, self.commitment.reveal_key());
                let qr = QrPayload {
                    data: qr_data,
                    error_correction: "M".to_string(),
                    display_duration_ms: jittered(DISPLAY_MS_VRFY), // S3: adaptive
                };
                // If we stashed the peer's reveal key (received VRFY while still
                // Transferring), process it now that we've generated our own VRFY
                // for the peer to scan.
                self.try_process_stashed_reveal_key();
                Some(qr)
            }
            ProtocolState::Confirming => {
                self.display_cycle += 1;
                // Every 3rd cycle, re-show VRFY so a slower peer still in
                // Verifying can process our reveal key before seeing CONF.
                if self.display_cycle.is_multiple_of(3) {
                    let qr_data =
                        qr_codec::format_verify_qr(&self.session_id, self.commitment.reveal_key());
                    Some(QrPayload {
                        data: qr_data,
                        error_correction: "M".to_string(),
                        display_duration_ms: jittered(DISPLAY_MS_VRFY),
                    })
                } else {
                    // CONF contains hash of our original plaintext card
                    let card_hash = self.compute_card_hash(&self.local_card);
                    let qr_data = qr_codec::format_confirm_qr(&self.session_id, &card_hash);
                    Some(QrPayload {
                        data: qr_data,
                        error_correction: "M".to_string(),
                        display_duration_ms: jittered(DISPLAY_MS_CONF),
                    })
                }
            }
            ProtocolState::Complete | ProtocolState::RetryReady => {
                // S1: Wall-clock timeout with progress extension.
                let now = Instant::now();
                let entered = *self.phase_entered_at.get_or_insert(now);

                // Compute deadline: base + extension from last progress, capped at max.
                let progress_deadline = self
                    .last_progress_at
                    .map(|p| p + RDYY_PROGRESS_EXTENSION)
                    .unwrap_or(entered + RDYY_BASE_TIMEOUT);
                let deadline = progress_deadline
                    .max(entered + RDYY_BASE_TIMEOUT)
                    .min(entered + RDYY_MAX_TIMEOUT);

                if now > deadline {
                    // S4: Graduated retry — try once more before failing.
                    if !self.retry_used {
                        self.retry_used = true;
                        self.state = ProtocolState::RetryReady;
                        self.phase_entered_at = Some(now);
                        self.last_progress_at = None;
                        self.display_cycle = 0;
                        // Fall through to display RDYY
                    } else {
                        // Both attempts exhausted — fail with FAIL broadcast.
                        self.state =
                            ProtocolState::Failed("peer did not confirm readiness".to_string());
                        self.fail_broadcast_until = Some(now + FAIL_BROADCAST_DURATION);
                        return self.get_fail_qr();
                    }
                }

                self.display_cycle += 1;

                // Interleave DATA with COMBO: if our outbound chunks aren't
                // fully ACK'd, the peer still needs our DATA. Show DATA every
                // 3rd cycle so the peer can receive chunks while also getting
                // COMBO for the stages it's ready for.
                let all_acked = self
                    .peer_ack_bitmap
                    .as_ref()
                    .map(|b| b.is_complete())
                    .unwrap_or(false);
                if !all_acked && self.display_cycle.is_multiple_of(3) {
                    self.get_data_chunk_qr().or_else(|| self.get_combo_qr())
                } else {
                    self.get_combo_qr()
                }
            }
            ProtocolState::Finalized => {
                // Continue broadcasting COMBO for a grace period so the peer
                // can scan it and also finalize.
                let now = Instant::now();
                let entered = *self.phase_entered_at.get_or_insert(now);
                if now.duration_since(entered) > FINALIZED_GRACE_DURATION {
                    return None;
                }
                // Also interleave DATA if peer hasn't ACK'd all chunks.
                let all_acked = self
                    .peer_ack_bitmap
                    .as_ref()
                    .map(|b| b.is_complete())
                    .unwrap_or(false);
                if !all_acked && self.display_cycle.is_multiple_of(3) {
                    self.display_cycle += 1;
                    self.get_data_chunk_qr().or_else(|| self.get_combo_qr())
                } else {
                    self.display_cycle += 1;
                    self.get_combo_qr()
                }
            }
            ProtocolState::Failed(_) => {
                // S5: Broadcast FAIL QR so peer aborts immediately.
                if self
                    .fail_broadcast_until
                    .is_some_and(|until| Instant::now() <= until)
                {
                    return self.get_fail_qr();
                }
                None
            }
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
                relay_url,
                relay_noise_pubkey,
            } => self.handle_init(
                session_id,
                pubkey,
                ephemeral,
                commitment_hash,
                relay_url,
                relay_noise_pubkey,
            ),
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
            StageQr::Ready {
                session_id: _,
                ack_hash,
            } => self.handle_ready(ack_hash),
            StageQr::Combo {
                session_id: _,
                reveal_key,
                payload_hash,
                ack_hash,
            } => self.handle_combo(reveal_key, payload_hash, ack_hash),
            StageQr::Inid {
                session_id,
                pubkey,
                ephemeral,
                commitment_hash,
                display_name: _,
                relay_url,
                relay_noise_pubkey,
                ciphertext,
            } => self.handle_inid(
                session_id,
                pubkey,
                ephemeral,
                commitment_hash,
                relay_url,
                relay_noise_pubkey,
                ciphertext,
            ),
            StageQr::Fail { session_id: _ } => self.handle_fail(),
        }
    }

    // --- Private helpers ---

    fn build_init_qr(&self) -> String {
        // INID (INIT+Data) disabled for now — the combined QR is too dense for
        // older cameras (Samsung S7). The COMBO QR still optimizes the RDYY phase.
        // TODO: re-enable when QR scanning reliability improves (bigger screens,
        // better cameras, or binary QR mode).
        //
        // let ciphertext = self.commitment.ciphertext();
        // if ciphertext.len() <= CHUNK_PAYLOAD_SIZE {
        //     return qr_codec::format_inid_qr(...);
        // }
        qr_codec::format_init_qr_with_relay(
            &self.session_id,
            &self.identity_pubkey,
            self.ephemeral_public.as_bytes(),
            self.commitment.hash(),
            &self.display_name,
            self.our_relay_url.as_deref(),
            self.our_relay_noise_pubkey.as_ref(),
        )
    }

    fn handle_init(
        &mut self,
        session_id: [u8; 16],
        pubkey: [u8; 32],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
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
        self.peer_relay_url = relay_url;
        self.peer_relay_noise_pubkey = relay_noise_pubkey;

        // Derive transport key via X25519 DH + HKDF
        match self.ephemeral_secret.take() {
            Some(secret) => {
                let peer_public = X25519Public::from(ephemeral);
                let shared_secret = secret.diffie_hellman(&peer_public);
                if !shared_secret.was_contributory() {
                    self.state = ProtocolState::Failed("non-contributory DH output".to_string());
                    return self.state.clone();
                }
                let transport_key = self.derive_transport_key(shared_secret.as_bytes());
                self.transport_key = Some(transport_key);
            }
            _ => {
                self.state = ProtocolState::Failed("ephemeral secret already consumed".to_string());
                return self.state.clone();
            }
        }

        // Prepare outbound: chunk the commitment ciphertext and transport-encrypt each chunk
        self.prepare_outbound_chunks();

        // Transition to Transferring (skip Discovered for efficiency)
        self.update_transfer_state();
        self.state.clone()
    }

    /// Handle INID (INIT+Data) — processes INIT fields and stores embedded ciphertext.
    /// Eliminates the DATA phase for small payloads (1 chunk).
    #[allow(clippy::too_many_arguments)]
    fn handle_inid(
        &mut self,
        session_id: [u8; 16],
        pubkey: [u8; 32],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
        ciphertext: Vec<u8>,
    ) -> ProtocolState {
        // Process the INIT portion first
        let state = self.handle_init(
            session_id,
            pubkey,
            ephemeral,
            commitment_hash,
            relay_url,
            relay_noise_pubkey,
        );

        // If INIT processing failed, return the error state
        if matches!(state, ProtocolState::Failed(_)) {
            return state;
        }

        // Store ciphertext directly (already commitment-encrypted).
        // Set up inbound tracking as if we received all DATA chunks.
        self.inbound_buffer = Some(ReassemblyBuffer::from_complete(ciphertext));
        self.inbound_bitmap = Some({
            let mut b = ChunkBitmap::new(1);
            b.mark_received(0);
            b
        });
        self.peer_chunks_total = Some(1);
        // All chunks received — should advance to Verifying
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
        // Accept DATA in Transferring, Discovered, or Verifying.
        // In Verifying, we still need to process the peer's ACK bitmap updates
        // so they learn which of their chunks we received (asymmetric payload sizes
        // can cause one side to reach Verifying before the other finishes transfer).
        if !matches!(
            self.state,
            ProtocolState::Transferring { .. }
                | ProtocolState::Discovered
                | ProtocolState::Verifying
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

        // Only advance transfer state if not already past Transferring.
        // In Verifying, we accept DATA only for the ACK bitmap update.
        if !matches!(self.state, ProtocolState::Verifying) {
            self.update_transfer_state();
        }
        self.state.clone()
    }

    fn handle_verify(&mut self, reveal_key: [u8; 32]) -> ProtocolState {
        // Accept VRFY in Verifying or Transferring.
        // With asymmetric payload sizes one side may reach Verifying while the other
        // is still Transferring. Receiving VRFY while Transferring means the peer
        // has all our chunks and has moved to Verifying; we stash the reveal key and
        // fast-track to Verifying so we can send our own VRFY in the next display cycle.
        if matches!(self.state, ProtocolState::Transferring { .. }) {
            self.peer_reveal_key = Some(reveal_key);
            // Fast-track: peer sending VRFY proves they have all our chunks.
            // Move to Verifying so we send our own VRFY to the peer.
            self.state = ProtocolState::Verifying;
            return self.state.clone();
        }

        if !matches!(self.state, ProtocolState::Verifying) {
            return self.state.clone();
        }

        // Process the reveal key (either just received, or stashed earlier)
        self.process_reveal_key(reveal_key)
    }

    /// Attempt to verify and decrypt peer data using the given reveal key.
    /// Called from handle_verify and from try_process_stashed_reveal_key.
    fn process_reveal_key(&mut self, reveal_key: [u8; 32]) -> ProtocolState {
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

        // Build peer's commitment context from their relay metadata (T1.7)
        let peer_context = Self::build_commitment_context(
            self.peer_relay_url.as_deref(),
            self.peer_relay_noise_pubkey.as_ref(),
        );

        if !Commitment::verify_hash_with_context(
            &reveal_key,
            &ciphertext,
            &peer_hash,
            &peer_context,
        ) {
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

    /// If we received the peer's reveal key while still Transferring, it was
    /// stashed in `peer_reveal_key` without decryption. Once we reach Verifying
    /// and have displayed our own VRFY, process the stashed key.
    fn try_process_stashed_reveal_key(&mut self) {
        if self.received_data.is_some() {
            return; // Already processed
        }
        if let Some(key) = self.peer_reveal_key {
            self.process_reveal_key(key);
        }
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

        if bool::from(payload_hash.ct_eq(&expected_hash)) {
            self.state = ProtocolState::Complete;
            // S1: Start wall-clock timer for the RDYY phase.
            self.phase_entered_at = Some(Instant::now());
            self.last_progress_at = None;
            self.display_cycle = 0;
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
                    display_duration_ms: jittered(DISPLAY_MS_DATA),
                });
            }
        }

        // All our chunks are ACK'd, but we may still need to send our ACK bitmap
        // so the peer knows which of their chunks we've received. Resend chunk 0
        // as a carrier for the updated bitmap.
        let ack_bytes = self
            .inbound_bitmap
            .as_ref()
            .map(|b| b.to_bytes())
            .unwrap_or_default();

        let chunk_data = &self.outbound_chunks[0];
        let qr_data = qr_codec::format_data_qr(&self.session_id, 0, total, &ack_bytes, chunk_data);

        Some(QrPayload {
            data: qr_data,
            error_correction: "L".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_DATA),
        })
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
        let nonce_bytes: [u8; 12] = crate::crypto::random_bytes();

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

        let payload = Payload {
            msg: plaintext,
            aad: &[chunk_idx],
        };
        let ciphertext = cipher.encrypt(nonce, payload).expect("encryption failed");

        // nonce || ciphertext+tag
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
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

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);

        let payload = Payload {
            msg: encrypted,
            aad: &[chunk_idx],
        };
        cipher.decrypt(nonce, payload).ok()
    }

    fn compute_card_hash(&self, data: &[u8]) -> [u8; 32] {
        let d = Sha256::digest(data);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        hash
    }

    /// Build commitment context from relay metadata (T1.7).
    ///
    /// Both sides must use the same context construction when creating and
    /// verifying the commitment hash. The context binds relay fields into
    /// the commitment so a MitM cannot swap relay URLs in the INIT QR.
    fn build_commitment_context(
        relay_url: Option<&str>,
        relay_noise_pubkey: Option<&[u8; 32]>,
    ) -> Vec<u8> {
        // Length-delimited encoding prevents ambiguity between
        // (url_A || pubkey_A) and (url_B || pubkey_B) where url_A is a
        // prefix of url_B. Without delimiters, SHA-256 pre-images could collide.
        let mut context = Vec::new();
        if let Some(url) = relay_url {
            let len = (url.len() as u32).to_be_bytes();
            context.extend_from_slice(&len);
            context.extend_from_slice(url.as_bytes());
        }
        if let Some(pk) = relay_noise_pubkey {
            context.extend_from_slice(pk);
        }
        context
    }

    fn handle_ready(&mut self, ack_hash: [u8; 32]) -> ProtocolState {
        // Accept READY in Complete or RetryReady states.
        if !matches!(
            self.state,
            ProtocolState::Complete | ProtocolState::RetryReady
        ) {
            return self.state.clone();
        }

        // S6: Any RDYY scan is progress — extend the deadline.
        self.last_progress_at = Some(Instant::now());

        // Verify ack_hash matches our computation.
        let expected = self.compute_ready_hash();
        if bool::from(ack_hash.ct_eq(&expected)) {
            self.state = ProtocolState::Finalized;
            // Reset for the finalized grace period (wall-clock based).
            self.phase_entered_at = Some(Instant::now());
            self.display_cycle = 0;
        }
        // Ignore mismatched READY (could be from a different exchange)

        self.state.clone()
    }

    /// Handle a FAIL QR from the peer — abort immediately.
    fn handle_fail(&mut self) -> ProtocolState {
        // Don't overwrite Finalized — if we already succeeded, ignore peer's failure.
        if matches!(self.state, ProtocolState::Finalized) {
            return self.state.clone();
        }
        // Don't overwrite existing failure.
        if matches!(self.state, ProtocolState::Failed(_)) {
            return self.state.clone();
        }
        self.state = ProtocolState::Failed("peer reported failure".to_string());
        self.state.clone()
    }

    /// Generate a FAIL QR payload for broadcasting failure to peer.
    fn get_fail_qr(&self) -> Option<QrPayload> {
        let qr_data = qr_codec::format_fail_qr(&self.session_id);
        Some(QrPayload {
            data: qr_data,
            error_correction: "L".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_FAIL),
        })
    }

    /// Generate a COMBO QR containing VRFY + CONF + RDYY.
    /// One scan by the peer can advance through all remaining stages at once.
    fn get_combo_qr(&self) -> Option<QrPayload> {
        let ack_hash = self.compute_ready_hash();
        let card_hash = self.compute_card_hash(&self.local_card);
        let qr_data = qr_codec::format_combo_qr(
            &self.session_id,
            self.commitment.reveal_key(),
            &card_hash,
            &ack_hash,
        );
        Some(QrPayload {
            data: qr_data,
            // COMBO is the densest QR (~172 chars). Use Q (25% error recovery)
            // instead of M (15%) — compensates for brightness asymmetry and
            // camera-screen distance at the scanning margin.
            error_correction: "Q".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_RDYY),
        })
    }

    /// Handle a COMBO QR from the peer — process VRFY + CONF + RDYY in one shot.
    /// Allows jumping from Verifying/Confirming/Complete straight to Finalized.
    ///
    /// SAFETY: Only processes VRFY if we have all inbound chunks. If we're still
    /// Transferring without complete data, the COMBO is treated as a stashed
    /// reveal key (same as receiving a standalone VRFY during Transferring).
    fn handle_combo(
        &mut self,
        reveal_key: [u8; 32],
        payload_hash: [u8; 32],
        ack_hash: [u8; 32],
    ) -> ProtocolState {
        // If still Transferring, stash the reveal key but don't chain further.
        // We can't process CONF/RDYY without the actual data.
        if matches!(self.state, ProtocolState::Transferring { .. }) {
            // handle_verify will stash the reveal key if chunks aren't complete
            self.handle_verify(reveal_key);
            // Don't chain — we need more DATA chunks first
            return self.state.clone();
        }

        // In Verifying: process reveal key → should move to Confirming
        if matches!(self.state, ProtocolState::Verifying) {
            self.handle_verify(reveal_key);
        }

        // In Confirming: process payload hash → should move to Complete
        if matches!(self.state, ProtocolState::Confirming) {
            self.handle_confirm(payload_hash);
        }

        // In Complete/RetryReady: process ack hash → should move to Finalized
        if matches!(
            self.state,
            ProtocolState::Complete | ProtocolState::RetryReady
        ) {
            self.handle_ready(ack_hash);
        }

        self.state.clone()
    }

    /// Compute the READY acknowledgment hash.
    ///
    /// SHA-256(min(our_session_id, peer_session_id) || max(our_session_id, peer_session_id))
    ///
    /// Using sorted session IDs ensures both sides compute the same hash
    /// regardless of who initiated the exchange.
    fn compute_ready_hash(&self) -> [u8; 32] {
        let peer_sid = self.peer_session_id.unwrap_or([0u8; 16]);
        let (first, second) = if self.session_id <= peer_sid {
            (self.session_id, peer_sid)
        } else {
            (peer_sid, self.session_id)
        };

        let mut input = Vec::with_capacity(32);
        input.extend_from_slice(&first);
        input.extend_from_slice(&second);
        let d = Sha256::digest(&input);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        hash
    }

    fn clear_sensitive(&mut self) {
        if let Some(ref mut key) = self.transport_key {
            key.zeroize();
        }
        if let Some(ref mut key) = self.peer_reveal_key {
            key.zeroize();
        }
        if let Some(ref mut key) = self.our_relay_noise_pubkey {
            key.zeroize();
        }
        if let Some(ref mut key) = self.peer_relay_noise_pubkey {
            key.zeroize();
        }
        self.transport_key = None;
        self.peer_reveal_key = None;
        self.ephemeral_secret = None;
        self.outbound_chunks.clear();
        self.inbound_buffer = None;
        self.received_data = None;
    }
}

impl Drop for MultiStageSession {
    fn drop(&mut self) {
        self.clear_sensitive();
    }
}
