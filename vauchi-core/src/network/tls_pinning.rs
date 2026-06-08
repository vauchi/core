// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TLS Certificate Pinning for ureq/rustls
//!
//! Custom `rustls::ServerCertVerifier` that performs standard WebPKI
//! validation followed by SPKI SHA-256 pin verification.
//!
//! Provides `build_pinned_agent()` to create a `ureq::Agent` with pinning.

use std::io::{Read, Write};
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, Transport,
    TransportAdapter,
};

use super::pinning::{PinnedCertificate, verify_pin};

/// Wraps standard WebPKI validation and adds SPKI SHA-256 pin checking.
///
/// Fail-closed: if pins are configured and none match, the connection is
/// rejected even if CA validation succeeds.
#[derive(Debug)]
struct PinningVerifier {
    inner: Arc<WebPkiServerVerifier>,
    pins: Vec<PinnedCertificate>,
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        // 1. Standard CA validation (chain, expiry, hostname)
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;

        // 2. SPKI pin check on the leaf certificate
        if !verify_pin(end_entity.as_ref(), &self.pins) {
            return Err(Error::General(
                "certificate pin verification failed: server SPKI does not match any pinned hash"
                    .into(),
            ));
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Build a `rustls::ClientConfig` with SPKI pin verification.
fn build_pinned_tls_config(pins: &[PinnedCertificate]) -> Result<rustls::ClientConfig, Error> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());

    let root_store =
        rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let webpki_verifier =
        WebPkiServerVerifier::builder_with_provider(Arc::new(root_store), provider.clone())
            .build()
            .map_err(|e| Error::General(format!("failed to build WebPKI verifier: {e}")))?;

    let pinning_verifier = PinningVerifier {
        inner: webpki_verifier,
        pins: pins.to_vec(),
    };

    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::General(format!("TLS protocol versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(pinning_verifier))
        .with_no_client_auth())
}

/// ureq `Connector` that performs TLS with a pinned `rustls::ClientConfig`.
///
/// Mirrors ureq's built-in `RustlsConnector` but uses our pre-built
/// `ClientConfig` with `PinningVerifier` instead of building one from
/// ureq's `TlsConfig`.
#[derive(Debug)]
struct PinningTlsConnector {
    tls_config: Arc<rustls::ClientConfig>,
}

impl<In: Transport> Connector<In> for PinningTlsConnector {
    type Out = Either<In, PinnedTlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(transport) = chained else {
            return Err(ureq::Error::Tls(
                "PinningTlsConnector requires a chained transport",
            ));
        };

        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }

        let name: ServerName<'_> = details
            .uri
            .authority()
            .expect("uri authority for tls")
            .host()
            .try_into()
            .map_err(|_| ureq::Error::Tls("invalid server name for TLS pinning"))?;

        let conn = ClientConnection::new(self.tls_config.clone(), name.to_owned())
            .map_err(|_| ureq::Error::Tls("TLS handshake init failed (pin verifier)"))?;

        let stream = StreamOwned {
            conn,
            sock: TransportAdapter::new(transport.boxed()),
        };

        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );

        Ok(Some(Either::B(PinnedTlsTransport { buffers, stream })))
    }
}

/// TLS transport using our pinned rustls config.
///
/// Identical in structure to ureq's internal `RustlsTransport`.
struct PinnedTlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl std::fmt::Debug for PinnedTlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedTlsTransport").finish_non_exhaustive()
    }
}

impl Transport for PinnedTlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}

/// Build a `ureq::Agent` with SPKI certificate pinning.
///
/// When `pins` is non-empty, every TLS connection verifies the server's
/// SPKI against the pinned hashes after standard CA validation.
/// Fail-closed on mismatch.
///
/// When `pins` is empty, returns a standard agent (no pinning).
pub fn build_pinned_agent(
    pins: &[PinnedCertificate],
    timeout: std::time::Duration,
    proxy: Option<ureq::Proxy>,
) -> Result<ureq::Agent, super::error::NetworkError> {
    use super::error::NetworkError;
    use ureq::unversioned::resolver::DefaultResolver;
    use ureq::unversioned::transport::{Connector, TcpConnector};

    // Build pinned TLS config
    let tls_config = build_pinned_tls_config(pins)
        .map_err(|e| NetworkError::ConnectionFailed(format!("TLS pin config: {e}")))?;

    let mut config_builder = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false);

    if let Some(proxy) = proxy {
        config_builder = config_builder.proxy(Some(proxy));
    }

    let config = config_builder.build();

    // Chain: TCP → PinningTLS (proxy handled by ureq config, not connector)
    let connector = TcpConnector::default().chain(PinningTlsConnector {
        tls_config: Arc::new(tls_config),
    });

    Ok(ureq::Agent::with_parts(
        config,
        connector,
        DefaultResolver::default(),
    ))
}
