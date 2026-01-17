//! Certificate pinning for TLS connections.
//!
//! This module provides TLS certificate pinning support for relay connections,
//! ensuring that only connections to servers with a known certificate are allowed.

use native_tls::{Certificate, TlsConnector as NativeTlsConnector};
use std::net::TcpStream;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;
use url::Url;

/// Create a TLS connector with certificate pinning.
///
/// The `pinned_cert_pem` should be the PEM-encoded certificate of the relay server.
/// Only connections to servers presenting this exact certificate will be allowed.
pub fn create_pinned_connector(pinned_cert_pem: &str) -> Result<NativeTlsConnector, String> {
    let cert = Certificate::from_pem(pinned_cert_pem.as_bytes())
        .map_err(|e| format!("Invalid certificate: {}", e))?;

    NativeTlsConnector::builder()
        .add_root_certificate(cert)
        .disable_built_in_roots(true) // Only trust the pinned certificate
        .build()
        .map_err(|e| format!("Failed to build TLS connector: {}", e))
}

/// Connect to a WebSocket server with optional certificate pinning.
///
/// If `pinned_cert_pem` is None, uses standard TLS without pinning (for development).
pub fn connect_with_pinning(
    url_str: &str,
    pinned_cert_pem: Option<&str>,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    match pinned_cert_pem {
        Some(pem) => connect_with_pinned_cert(url_str, pem),
        None => {
            // No pinning - use default TLS (for development/testing only)
            let request = url_str
                .into_client_request()
                .map_err(|e| format!("Invalid URL: {}", e))?;
            tungstenite::connect(request)
                .map(|(ws, _)| ws)
                .map_err(|e| format!("Connection failed: {}", e))
        }
    }
}

/// Connect with certificate pinning using manual TLS setup.
fn connect_with_pinned_cert(
    url_str: &str,
    cert_pem: &str,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;

    let host = url.host_str().ok_or("No host in URL")?;
    let port = url
        .port()
        .unwrap_or(if url.scheme() == "wss" { 443 } else { 80 });
    let addr = format!("{}:{}", host, port);

    // Create TCP connection
    let tcp_stream =
        TcpStream::connect(&addr).map_err(|e| format!("TCP connection failed: {}", e))?;

    let connector = create_pinned_connector(cert_pem)?;

    // Perform TLS handshake with pinned certificate
    let tls_stream = connector.connect(host, tcp_stream).map_err(|e| {
        format!(
            "TLS handshake failed (certificate pinning may have rejected the server): {}",
            e
        )
    })?;

    // Upgrade to WebSocket
    let request = url_str
        .into_client_request()
        .map_err(|e| format!("Invalid WebSocket request: {}", e))?;

    let (ws, _) = tungstenite::client(request, MaybeTlsStream::NativeTls(tls_stream))
        .map_err(|e| format!("WebSocket upgrade failed: {}", e))?;

    Ok(ws)
}
