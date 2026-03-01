// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Noise NK Initiator
//!
//! Provides inner transport encryption between client and relay using the
//! Noise NK pattern (`Noise_NK_25519_ChaChaPoly_BLAKE2s`).
//!
//! NK handshake flow:
//!   Pre-message: <- s (responder's static key known to initiator)
//!   Message 1: -> e, es (initiator sends ephemeral, DH with responder static)
//!   Message 2: <- e, ee (responder sends ephemeral, DH between ephemerals)
//!
//! This is defense-in-depth: if TLS is compromised, routing metadata
//! (recipient_id, message types) remains encrypted.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use snow::{Builder, TransportState};

/// Noise protocol pattern for relay-client communication.
pub const NOISE_PATTERN: &str = "Noise_NK_25519_ChaChaPoly_BLAKE2s";

/// Magic bytes identifying a v2 (Noise-encrypted) connection.
/// First byte is 0x00 (invalid JSON start), followed by "V2".
pub const V2_MAGIC: [u8; 3] = [0x00, b'V', b'2'];

/// Error type for Noise operations.
#[derive(Debug)]
pub enum NoiseError {
    Handshake(String),
    Encrypt(String),
    Decrypt(String),
}

impl std::fmt::Display for NoiseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoiseError::Handshake(msg) => write!(f, "Noise handshake error: {}", msg),
            NoiseError::Encrypt(msg) => write!(f, "Noise encrypt error: {}", msg),
            NoiseError::Decrypt(msg) => write!(f, "Noise decrypt error: {}", msg),
        }
    }
}

/// Noise NK initiator (client side).
///
/// Creates the initial handshake message (-> e, es) and, after receiving
/// the responder's reply (<- e, ee), transitions to transport mode.
pub struct NoiseInitiator {
    state: snow::HandshakeState,
}

/// Noise transport state for encrypting/decrypting messages after handshake.
pub struct NoiseTransport {
    state: TransportState,
}

impl NoiseInitiator {
    /// Creates a new NK initiator targeting the given relay public key.
    ///
    /// Returns the initiator and the 48-byte handshake message (-> e, es)
    /// to send to the relay.
    pub fn new(relay_pubkey: &[u8; 32]) -> Result<(Self, Vec<u8>), NoiseError> {
        let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
        let mut state = builder
            .remote_public_key(relay_pubkey)
            .build_initiator()
            .map_err(|e| NoiseError::Handshake(e.to_string()))?;

        // Write initiator's message (-> e, es)
        let mut handshake_msg = vec![0u8; 65535];
        let len = state
            .write_message(&[], &mut handshake_msg)
            .map_err(|e| NoiseError::Handshake(e.to_string()))?;
        handshake_msg.truncate(len);

        Ok((NoiseInitiator { state }, handshake_msg))
    }

    /// Processes the responder's handshake reply (<- e, ee) and transitions
    /// to transport mode.
    pub fn finalize(mut self, response: &[u8]) -> Result<NoiseTransport, NoiseError> {
        // Read responder's message (<- e, ee)
        let mut read_buf = vec![0u8; 65535];
        self.state
            .read_message(response, &mut read_buf)
            .map_err(|e| NoiseError::Handshake(e.to_string()))?;

        // Transition to transport mode
        let state = self
            .state
            .into_transport_mode()
            .map_err(|e| NoiseError::Handshake(e.to_string()))?;

        Ok(NoiseTransport { state })
    }
}

/// Prepends V2 magic bytes to a handshake message for wire transmission.
pub fn build_v2_message(handshake_bytes: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(V2_MAGIC.len() + handshake_bytes.len());
    msg.extend_from_slice(&V2_MAGIC);
    msg.extend_from_slice(handshake_bytes);
    msg
}

/// Parses a relay's Noise NK public key from a URL fragment.
///
/// The relay public key is distributed in the URL fragment:
/// `wss://relay.example.com#<base64url-encoded-32-byte-X25519-pubkey>`
///
/// Returns `None` if no fragment, invalid base64, or wrong key length.
pub fn parse_relay_noise_pubkey(url: &str) -> Option<[u8; 32]> {
    let fragment = url.rsplit_once('#')?.1;
    if fragment.is_empty() {
        return None;
    }

    let decoded = URL_SAFE_NO_PAD.decode(fragment).ok()?;
    if decoded.len() != 32 {
        return None;
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Some(key)
}

impl NoiseTransport {
    /// Encrypts plaintext into a Noise transport message.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        // Noise adds 16-byte MAC
        let mut buf = vec![0u8; plaintext.len() + 16];
        let len = self
            .state
            .write_message(plaintext, &mut buf)
            .map_err(|e| NoiseError::Encrypt(e.to_string()))?;
        buf.truncate(len);
        Ok(buf)
    }

    /// Decrypts a Noise transport message.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        let mut buf = vec![0u8; ciphertext.len()];
        let len = self
            .state
            .read_message(ciphertext, &mut buf)
            .map_err(|e| NoiseError::Decrypt(e.to_string()))?;
        buf.truncate(len);
        Ok(buf)
    }
}

// INLINE_TEST_REQUIRED: Tests access private Noise protocol internals (keypair generation, handshake state)
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: generate a relay keypair for testing.
    fn generate_test_relay_keypair() -> ([u8; 32], [u8; 32]) {
        let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
        let keypair = builder.generate_keypair().unwrap();
        let mut private = [0u8; 32];
        let mut public = [0u8; 32];
        private.copy_from_slice(&keypair.private);
        public.copy_from_slice(&keypair.public);
        (private, public)
    }

    /// Helper: build a snow responder for testing interop.
    fn build_test_responder(private_key: &[u8; 32]) -> snow::HandshakeState {
        let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
        builder
            .local_private_key(private_key)
            .build_responder()
            .unwrap()
    }

    #[test]
    fn test_handshake_message_is_48_bytes() {
        let (_priv, pub_key) = generate_test_relay_keypair();
        let (_initiator, handshake_msg) = NoiseInitiator::new(&pub_key).unwrap();

        // NK pattern message 1: ephemeral key (32 bytes) + encrypted empty payload (16-byte tag)
        assert_eq!(handshake_msg.len(), 48);
    }

    // @scenario: noise_protocol.feature:V2 connection uses magic bytes prefix
    #[test]
    fn test_v2_message_has_correct_magic() {
        let (_priv, pub_key) = generate_test_relay_keypair();
        let (_initiator, handshake_msg) = NoiseInitiator::new(&pub_key).unwrap();

        let v2_msg = build_v2_message(&handshake_msg);

        assert_eq!(v2_msg.len(), 3 + 48);
        assert_eq!(&v2_msg[..3], &V2_MAGIC);
        assert_eq!(&v2_msg[3..], &handshake_msg);
    }

    // @scenario: noise_protocol.feature:Messages encrypted after handshake
    #[test]
    fn test_full_handshake_and_transport() {
        let (priv_key, pub_key) = generate_test_relay_keypair();

        // Initiator creates handshake
        let (initiator, handshake_msg) = NoiseInitiator::new(&pub_key).unwrap();

        // Responder processes handshake
        let mut responder = build_test_responder(&priv_key);
        let mut read_buf = vec![0u8; 65535];
        responder
            .read_message(&handshake_msg, &mut read_buf)
            .unwrap();

        // Responder sends reply
        let mut response = vec![0u8; 65535];
        let response_len = responder.write_message(&[], &mut response).unwrap();
        response.truncate(response_len);

        let mut responder_transport = responder.into_transport_mode().unwrap();

        // Initiator finalizes
        let mut client_transport = initiator.finalize(&response).unwrap();

        // Client -> Relay
        let plaintext = b"Hello from client";
        let ct = client_transport.encrypt(plaintext).unwrap();
        let mut dec_buf = vec![0u8; ct.len()];
        let dec_len = responder_transport.read_message(&ct, &mut dec_buf).unwrap();
        dec_buf.truncate(dec_len);
        assert_eq!(dec_buf, plaintext);

        // Relay -> Client
        let msg2 = b"Hello from relay";
        let mut ct2 = vec![0u8; msg2.len() + 16];
        let ct2_len = responder_transport.write_message(msg2, &mut ct2).unwrap();
        ct2.truncate(ct2_len);
        let dec2 = client_transport.decrypt(&ct2).unwrap();
        assert_eq!(dec2, msg2);
    }

    #[test]
    fn test_wrong_relay_key_fails() {
        let (priv_key, _pub_key) = generate_test_relay_keypair();
        let (_wrong_priv, wrong_pub) = generate_test_relay_keypair();

        // Client uses wrong public key
        let (initiator, handshake_msg) = NoiseInitiator::new(&wrong_pub).unwrap();

        // Relay tries to process with its actual key — MAC check fails
        let mut responder = build_test_responder(&priv_key);
        let mut read_buf = vec![0u8; 65535];
        let result = responder.read_message(&handshake_msg, &mut read_buf);
        assert!(result.is_err(), "Handshake should fail with wrong key");

        // Also test that initiator.finalize fails with garbage response
        let result = initiator.finalize(&[0u8; 48]);
        assert!(result.is_err());
    }

    #[test]
    fn test_corrupted_ciphertext_fails() {
        let (priv_key, pub_key) = generate_test_relay_keypair();

        let (initiator, handshake_msg) = NoiseInitiator::new(&pub_key).unwrap();
        let mut responder = build_test_responder(&priv_key);
        let mut read_buf = vec![0u8; 65535];
        responder
            .read_message(&handshake_msg, &mut read_buf)
            .unwrap();
        let mut response = vec![0u8; 65535];
        let response_len = responder.write_message(&[], &mut response).unwrap();
        response.truncate(response_len);
        let _responder_transport = responder.into_transport_mode().unwrap();

        let mut client_transport = initiator.finalize(&response).unwrap();

        let ct = client_transport.encrypt(b"secret data").unwrap();
        let mut corrupted = ct.clone();
        corrupted[5] ^= 0xff;

        // Decryption should fail on corrupted data
        let result = client_transport.decrypt(&corrupted);
        assert!(result.is_err());
    }

    // @scenario: noise_protocol.feature:Relay public key parsed from URL fragment
    #[test]
    fn test_parse_relay_noise_pubkey_valid() {
        let (_priv, pub_key) = generate_test_relay_keypair();
        let encoded = URL_SAFE_NO_PAD.encode(pub_key);
        let url = format!("wss://relay.example.com#{}", encoded);

        let parsed = parse_relay_noise_pubkey(&url);
        assert_eq!(parsed, Some(pub_key));
    }

    // @scenario: noise_protocol.feature:URL without fragment has no Noise key
    #[test]
    fn test_parse_relay_noise_pubkey_no_fragment() {
        assert_eq!(parse_relay_noise_pubkey("wss://relay.example.com"), None);
    }

    #[test]
    fn test_parse_relay_noise_pubkey_empty_fragment() {
        assert_eq!(parse_relay_noise_pubkey("wss://relay.example.com#"), None);
    }

    // @scenario: noise_protocol.feature:Invalid Noise key in URL fragment is rejected
    #[test]
    fn test_parse_relay_noise_pubkey_invalid_base64() {
        assert_eq!(
            parse_relay_noise_pubkey("wss://relay.example.com#not-valid-!!"),
            None
        );
    }

    // @scenario: noise_protocol.feature:Wrong-length key in URL fragment is rejected
    #[test]
    fn test_parse_relay_noise_pubkey_wrong_length() {
        // Encode only 16 bytes instead of 32
        let short = URL_SAFE_NO_PAD.encode([0u8; 16]);
        let url = format!("wss://relay.example.com#{}", short);
        assert_eq!(parse_relay_noise_pubkey(&url), None);
    }

    #[test]
    fn test_parse_relay_noise_pubkey_with_path() {
        let (_priv, pub_key) = generate_test_relay_keypair();
        let encoded = URL_SAFE_NO_PAD.encode(pub_key);
        let url = format!("wss://relay.example.com/ws#{}", encoded);

        let parsed = parse_relay_noise_pubkey(&url);
        assert_eq!(parsed, Some(pub_key));
    }

    #[test]
    fn test_multiple_messages_sequential() {
        let (priv_key, pub_key) = generate_test_relay_keypair();

        let (initiator, handshake_msg) = NoiseInitiator::new(&pub_key).unwrap();
        let mut responder = build_test_responder(&priv_key);
        let mut read_buf = vec![0u8; 65535];
        responder
            .read_message(&handshake_msg, &mut read_buf)
            .unwrap();
        let mut response = vec![0u8; 65535];
        let response_len = responder.write_message(&[], &mut response).unwrap();
        response.truncate(response_len);
        let mut responder_transport = responder.into_transport_mode().unwrap();
        let mut client_transport = initiator.finalize(&response).unwrap();

        // Send multiple messages in sequence
        for i in 0..10 {
            let msg = format!("message {}", i);
            let ct = client_transport.encrypt(msg.as_bytes()).unwrap();

            let mut dec_buf = vec![0u8; ct.len()];
            let dec_len = responder_transport.read_message(&ct, &mut dec_buf).unwrap();
            dec_buf.truncate(dec_len);
            assert_eq!(dec_buf, msg.as_bytes());
        }
    }
}
