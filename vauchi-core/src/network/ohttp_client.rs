// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OHTTP Client (RFC 9458)
//!
//! Wraps the `ohttp` crate's client-side API to encrypt requests and decrypt
//! responses. Each [`OhttpClient`] holds a cached gateway key config and
//! creates a fresh `ClientRequest` per call (single-use per RFC 9458).

use ohttp::ClientRequest;

use super::error::NetworkError;

/// Client-side OHTTP encapsulation.
///
/// Holds the encoded gateway key config. Each `encapsulate` call produces a
/// fresh encrypted request and a [`ResponseDecryptor`] that must be used to
/// decrypt the corresponding response.
pub struct OhttpClient {
    /// Encoded key config bytes (fetched from `GET /v2/ohttp-key`).
    encoded_config: Vec<u8>,
}

/// Opaque token returned by [`OhttpClient::encapsulate`].
///
/// Must be used exactly once to decrypt the matching OHTTP response.
/// Wraps `ohttp::ClientResponse` which is not `Clone` or `Copy`.
pub struct ResponseDecryptor {
    inner: ohttp::ClientResponse,
}

impl OhttpClient {
    /// Create a client from the encoded key config bytes.
    ///
    /// The bytes are typically fetched from `GET /v2/ohttp-key` (content-type
    /// `application/ohttp-keys`).
    pub fn new(encoded_config: Vec<u8>) -> Result<Self, NetworkError> {
        // Validate the config is parseable at construction time so we fail
        // early rather than on every request.
        let _ = ClientRequest::from_encoded_config(&encoded_config)
            .map_err(|e| NetworkError::InvalidMessage(format!("invalid OHTTP key config: {e}")))?;
        Ok(Self { encoded_config })
    }

    /// Returns the raw encoded key config bytes.
    pub fn encoded_config(&self) -> &[u8] {
        &self.encoded_config
    }

    /// Encrypt a plaintext request body.
    ///
    /// Returns the encrypted bytes (to send as the OHTTP request body) and a
    /// [`ResponseDecryptor`] that must be used to decrypt the server's response.
    pub fn encapsulate(
        &self,
        plaintext: &[u8],
    ) -> Result<(Vec<u8>, ResponseDecryptor), NetworkError> {
        let client_request =
            ClientRequest::from_encoded_config(&self.encoded_config).map_err(|e| {
                NetworkError::InvalidMessage(format!("failed to parse OHTTP key config: {e}"))
            })?;
        let (encrypted, client_response) = client_request.encapsulate(plaintext).map_err(|e| {
            NetworkError::InvalidMessage(format!("OHTTP encapsulation failed: {e}"))
        })?;
        Ok((
            encrypted,
            ResponseDecryptor {
                inner: client_response,
            },
        ))
    }
}

impl ResponseDecryptor {
    /// Decrypt the OHTTP-encapsulated response from the server.
    ///
    /// Consumes `self` — each decryptor can only be used once.
    pub fn decapsulate(self, encrypted_response: &[u8]) -> Result<Vec<u8>, NetworkError> {
        self.inner.decapsulate(encrypted_response).map_err(|e| {
            NetworkError::InvalidMessage(format!("OHTTP response decryption failed: {e}"))
        })
    }
}

impl std::fmt::Debug for OhttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OhttpClient")
            .field("encoded_config_len", &self.encoded_config.len())
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ResponseDecryptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseDecryptor").finish_non_exhaustive()
    }
}

// INLINE_TEST_REQUIRED: tests validate full encrypt/decrypt roundtrip using
// the ohttp crate's server-side API, which is only available in tests.
#[cfg(test)]
mod tests {
    use super::*;
    use ohttp::{KeyConfig, Server, SymmetricSuite, hpke};

    /// Helper: create a gateway server and return (server, encoded_config).
    fn make_gateway() -> (Server, Vec<u8>) {
        let config = KeyConfig::new(
            0,
            hpke::Kem::X25519Sha256,
            vec![SymmetricSuite::new(
                hpke::Kdf::HkdfSha256,
                hpke::Aead::ChaCha20Poly1305,
            )],
        )
        .expect("KeyConfig::new must succeed");
        let encoded = config.encode().expect("encode must succeed");
        let server = Server::new(config).expect("Server::new must succeed");
        (server, encoded)
    }

    #[test]
    fn test_ohttp_client_creation_with_valid_config() {
        let (_server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded.clone());
        assert!(
            client.is_ok(),
            "client creation must succeed with valid config"
        );
        assert_eq!(client.unwrap().encoded_config(), &encoded);
    }

    #[test]
    fn test_ohttp_client_rejects_invalid_config() {
        let result = OhttpClient::new(b"this is not a valid ohttp config".to_vec());
        assert!(
            result.is_err(),
            "client creation must fail with garbage config"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("invalid OHTTP key config"),
            "error message should mention invalid config, got: {err}"
        );
    }

    #[test]
    fn test_ohttp_client_rejects_empty_config() {
        let result = OhttpClient::new(vec![]);
        assert!(result.is_err(), "empty config must be rejected");
    }

    #[test]
    fn test_ohttp_encrypt_decrypt_roundtrip() {
        let (server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded).expect("client must be created");

        let plaintext = b"test request body";
        let (encrypted, response_decryptor) = client
            .encapsulate(plaintext)
            .expect("encapsulate must succeed");

        assert_ne!(&encrypted, &plaintext[..]);
        assert!(!encrypted.is_empty());

        let (decrypted, srv_response) = server
            .decapsulate(&encrypted)
            .expect("server decapsulate must succeed");
        assert_eq!(&decrypted, plaintext, "server must see original plaintext");

        let response_plain = b"response from gateway";
        let enc_response = srv_response
            .encapsulate(response_plain)
            .expect("server encapsulate must succeed");

        let dec_response = response_decryptor
            .decapsulate(&enc_response)
            .expect("response decapsulate must succeed");
        assert_eq!(
            &dec_response, response_plain,
            "client must recover original response"
        );
    }

    #[test]
    fn test_same_plaintext_produces_different_ciphertexts() {
        let (_server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded).expect("client must be created");

        let (enc1, _dec1) = client.encapsulate(b"identical").expect("enc1");
        let (enc2, _dec2) = client.encapsulate(b"identical").expect("enc2");

        assert_ne!(
            enc1, enc2,
            "same plaintext must produce different ciphertexts (fresh ephemeral keys)"
        );
    }

    #[test]
    fn test_multiple_requests_use_independent_state() {
        let (server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded).expect("client must be created");

        let (enc1, dec1) = client.encapsulate(b"request-1").expect("enc1");
        let (enc2, dec2) = client.encapsulate(b"request-2").expect("enc2");

        assert_ne!(
            enc1, enc2,
            "different plaintexts must produce different ciphertexts"
        );

        let (plain1, srv1) = server.decapsulate(&enc1).expect("dec1");
        assert_eq!(plain1, b"request-1");

        let (plain2, srv2) = server.decapsulate(&enc2).expect("dec2");
        assert_eq!(plain2, b"request-2");

        let resp1 = srv1.encapsulate(b"resp-1").expect("resp1");
        let resp2 = srv2.encapsulate(b"resp-2").expect("resp2");

        assert_eq!(dec1.decapsulate(&resp1).expect("dec-resp1"), b"resp-1");
        assert_eq!(dec2.decapsulate(&resp2).expect("dec-resp2"), b"resp-2");
    }

    #[test]
    fn test_response_decryptor_rejects_garbage() {
        let (_server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded).expect("client must be created");

        let (_encrypted, response_decryptor) = client
            .encapsulate(b"request")
            .expect("encapsulate must succeed");

        let result = response_decryptor.decapsulate(b"not a valid ohttp response");
        assert!(result.is_err(), "garbage response must be rejected");
    }

    #[test]
    fn test_response_decryptor_rejects_mismatched_response() {
        let (server, encoded) = make_gateway();
        let client = OhttpClient::new(encoded).expect("client must be created");

        let (enc1, dec1) = client.encapsulate(b"request-1").expect("enc1");
        let (enc2, _dec2) = client.encapsulate(b"request-2").expect("enc2");

        let (_plain1, _srv1) = server.decapsulate(&enc1).expect("srv-dec1");
        let (_plain2, srv2) = server.decapsulate(&enc2).expect("srv-dec2");

        let resp2 = srv2.encapsulate(b"resp-2").expect("srv-enc2");

        // dec1 should NOT be able to decrypt resp2 (wrong session)
        let result = dec1.decapsulate(&resp2);
        assert!(
            result.is_err(),
            "decryptor must reject response from a different OHTTP session"
        );
    }
}
