// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! WebSocket Transport
//!
//! Real transport implementation using tungstenite for WebSocket connections.
//! Supports both native-tls and rustls TLS backends.

use std::net::TcpStream;
use std::time::Duration;

#[cfg(all(feature = "network-native-tls", not(feature = "network-rustls")))]
use native_tls::TlsConnector;

#[cfg(feature = "network-rustls")]
use rustls::pki_types::ServerName;
#[cfg(feature = "network-rustls")]
use std::sync::Arc;

use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use super::error::NetworkError;
use super::message::MessageEnvelope;
use super::protocol::{decode_message, encode_message, read_frame_length, FRAME_HEADER_SIZE};
use super::transport::{ConnectionState, Transport, TransportConfig, TransportResult};

/// WebSocket transport for relay communication.
///
/// Supports both ws:// (plaintext) and wss:// (TLS) connections.
///
/// # Example
///
/// ```ignore
/// use vauchi_core::network::{WebSocketTransport, TransportConfig};
///
/// let mut transport = WebSocketTransport::new();
/// let config = TransportConfig {
///     server_url: "wss://relay.example.com".to_string(),
///     ..Default::default()
/// };
/// transport.connect(&config)?;
/// ```
pub struct WebSocketTransport {
    socket: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    config: TransportConfig,
    state: ConnectionState,
}

impl WebSocketTransport {
    /// Creates a new WebSocket transport.
    pub fn new() -> Self {
        WebSocketTransport {
            socket: None,
            config: TransportConfig::default(),
            state: ConnectionState::Disconnected,
        }
    }

    /// Parses a WebSocket URL into host and port.
    fn parse_url(url: &str) -> Result<(String, u16, bool), NetworkError> {
        let is_tls = url.starts_with("wss://");
        let url_without_scheme = url
            .strip_prefix("wss://")
            .or_else(|| url.strip_prefix("ws://"))
            .ok_or_else(|| {
                NetworkError::ConnectionFailed(
                    "Invalid URL scheme (expected ws:// or wss://)".into(),
                )
            })?;

        // Split host:port/path
        let host_port = url_without_scheme
            .split('/')
            .next()
            .unwrap_or(url_without_scheme);

        let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
            let host = &host_port[..colon_pos];
            let port_str = &host_port[colon_pos + 1..];
            let port: u16 = port_str.parse().map_err(|_| {
                NetworkError::ConnectionFailed(format!("Invalid port: {}", port_str))
            })?;
            (host.to_string(), port)
        } else {
            let default_port = if is_tls { 443 } else { 80 };
            (host_port.to_string(), default_port)
        };

        Ok((host, port, is_tls))
    }

    /// Create a TLS stream using native-tls
    #[cfg(all(feature = "network-native-tls", not(feature = "network-rustls")))]
    fn create_tls_stream(
        host: &str,
        tcp_stream: TcpStream,
    ) -> Result<MaybeTlsStream<TcpStream>, NetworkError> {
        let connector = TlsConnector::new()
            .map_err(|e| NetworkError::ConnectionFailed(format!("TLS error: {}", e)))?;
        let tls_stream = connector
            .connect(host, tcp_stream)
            .map_err(|e| NetworkError::ConnectionFailed(format!("TLS handshake failed: {}", e)))?;
        Ok(MaybeTlsStream::NativeTls(tls_stream))
    }

    /// Create a TLS stream using rustls
    #[cfg(feature = "network-rustls")]
    fn create_tls_stream(
        host: &str,
        tcp_stream: TcpStream,
    ) -> Result<MaybeTlsStream<TcpStream>, NetworkError> {
        // Create root certificate store from webpki roots
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let server_name: ServerName<'_> = host.try_into().map_err(|_| {
            NetworkError::ConnectionFailed(format!("Invalid server name: {}", host))
        })?;

        let tls_conn = rustls::ClientConnection::new(Arc::new(config), server_name.to_owned())
            .map_err(|e| NetworkError::ConnectionFailed(format!("TLS setup failed: {}", e)))?;

        let tls_stream = rustls::StreamOwned::new(tls_conn, tcp_stream);
        Ok(MaybeTlsStream::Rustls(tls_stream))
    }
}

impl Default for WebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for WebSocketTransport {
    fn connect(&mut self, config: &TransportConfig) -> TransportResult<()> {
        if matches!(self.state, ConnectionState::Connected) {
            return Ok(());
        }

        self.state = ConnectionState::Connecting;
        self.config = config.clone();

        let (host, port, is_tls) = Self::parse_url(&config.server_url)?;
        let addr = format!("{}:{}", host, port);

        // Create TCP connection with timeout
        let tcp_stream = TcpStream::connect(&addr).map_err(|e| {
            self.state = ConnectionState::Disconnected;
            NetworkError::ConnectionFailed(e.to_string())
        })?;

        tcp_stream
            .set_read_timeout(Some(Duration::from_millis(config.io_timeout_ms)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        tcp_stream
            .set_write_timeout(Some(Duration::from_millis(config.io_timeout_ms)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        // Wrap in TLS if needed
        let stream: MaybeTlsStream<TcpStream> = if is_tls {
            Self::create_tls_stream(&host, tcp_stream).inspect_err(|_| {
                self.state = ConnectionState::Disconnected;
            })?
        } else {
            MaybeTlsStream::Plain(tcp_stream)
        };

        // WebSocket handshake - use IntoClientRequest for proper HTTP/1.1 request
        let request = config
            .server_url
            .as_str()
            .into_client_request()
            .map_err(|e| {
                self.state = ConnectionState::Disconnected;
                NetworkError::ConnectionFailed(format!("Invalid WebSocket request: {}", e))
            })?;

        let (socket, _response) = tungstenite::client(request, stream).map_err(|e| {
            self.state = ConnectionState::Disconnected;
            NetworkError::ConnectionFailed(format!("WebSocket handshake failed: {}", e))
        })?;

        self.socket = Some(socket);
        self.state = ConnectionState::Connected;

        Ok(())
    }

    fn disconnect(&mut self) -> TransportResult<()> {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.close(None); // Ignore errors on close
        }
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()> {
        let socket = self.socket.as_mut().ok_or(NetworkError::NotConnected)?;

        let encoded = encode_message(message)?;
        let ws_message = Message::Binary(encoded);

        socket.send(ws_message).map_err(|e| {
            // Connection may be broken
            if matches!(
                e,
                tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed
            ) {
                self.state = ConnectionState::Disconnected;
                NetworkError::ConnectionClosed
            } else {
                NetworkError::SendFailed(e.to_string())
            }
        })?;

        // Flush to ensure message is sent
        socket
            .flush()
            .map_err(|e| NetworkError::SendFailed(format!("Flush failed: {}", e)))?;

        Ok(())
    }

    fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        let socket = self.socket.as_mut().ok_or(NetworkError::NotConnected)?;

        // Try to read a message
        match socket.read() {
            Ok(Message::Binary(data)) => {
                // Data includes the length prefix, skip it
                if data.len() < FRAME_HEADER_SIZE {
                    return Err(NetworkError::InvalidMessage("Frame too short".into()));
                }

                let header: [u8; FRAME_HEADER_SIZE] = data[..FRAME_HEADER_SIZE]
                    .try_into()
                    .map_err(|_| NetworkError::InvalidMessage("Invalid header".into()))?;
                let expected_len = read_frame_length(&header);

                if data.len() - FRAME_HEADER_SIZE != expected_len {
                    return Err(NetworkError::InvalidMessage(format!(
                        "Length mismatch: expected {}, got {}",
                        expected_len,
                        data.len() - FRAME_HEADER_SIZE
                    )));
                }

                let envelope = decode_message(&data[FRAME_HEADER_SIZE..])?;
                Ok(Some(envelope))
            }
            Ok(Message::Ping(data)) => {
                // Respond to ping with pong
                let _ = socket.send(Message::Pong(data));
                Ok(None)
            }
            Ok(Message::Pong(_)) => {
                // Ignore pongs
                Ok(None)
            }
            Ok(Message::Close(_)) => {
                self.state = ConnectionState::Disconnected;
                Err(NetworkError::ConnectionClosed)
            }
            Ok(Message::Text(_)) => {
                // We don't use text messages
                Err(NetworkError::InvalidMessage(
                    "Unexpected text message".into(),
                ))
            }
            Ok(Message::Frame(_)) => {
                // Raw frames shouldn't reach here
                Ok(None)
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // No message available (timeout)
                Ok(None)
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                self.state = ConnectionState::Disconnected;
                Err(NetworkError::ConnectionClosed)
            }
            Err(e) => Err(NetworkError::ReceiveFailed(e.to_string())),
        }
    }

    fn has_pending(&self) -> bool {
        // WebSocket doesn't provide a non-blocking check easily
        // Return false; caller should use receive() with timeout
        false
    }
}

// INLINE_TEST_REQUIRED: Tests private parse_url function for URL parsing logic
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url_wss() {
        let (host, port, is_tls) =
            WebSocketTransport::parse_url("wss://relay.example.com").unwrap();
        assert_eq!(host, "relay.example.com");
        assert_eq!(port, 443);
        assert!(is_tls);
    }

    #[test]
    fn test_parse_url_ws() {
        let (host, port, is_tls) = WebSocketTransport::parse_url("ws://localhost:8080").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
        assert!(!is_tls);
    }

    #[test]
    fn test_parse_url_with_path() {
        let (host, port, is_tls) =
            WebSocketTransport::parse_url("wss://relay.example.com:9000/ws").unwrap();
        assert_eq!(host, "relay.example.com");
        assert_eq!(port, 9000);
        assert!(is_tls);
    }

    #[test]
    fn test_parse_url_invalid_scheme() {
        let result = WebSocketTransport::parse_url("http://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_new_transport_disconnected() {
        let transport = WebSocketTransport::new();
        assert_eq!(transport.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_send_without_connect_fails() {
        let mut transport = WebSocketTransport::new();
        let envelope = crate::network::protocol::create_envelope(
            crate::network::message::MessagePayload::Presence(
                crate::network::message::PresenceUpdate {
                    status: crate::network::message::PresenceStatus::Online,
                    message: None,
                },
            ),
        );

        let result = transport.send(&envelope);
        assert!(matches!(result, Err(NetworkError::NotConnected)));
    }

    #[test]
    fn test_receive_without_connect_fails() {
        let mut transport = WebSocketTransport::new();
        let result = transport.receive();
        assert!(matches!(result, Err(NetworkError::NotConnected)));
    }

    #[test]
    fn test_disconnect_when_not_connected_ok() {
        let mut transport = WebSocketTransport::new();
        let result = transport.disconnect();
        assert!(result.is_ok());
        assert_eq!(transport.state(), ConnectionState::Disconnected);
    }
}
