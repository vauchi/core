# WebBook Relay

Lightweight WebSocket relay server for WebBook - stores and forwards encrypted blobs between clients.

## Overview

The relay server is a zero-knowledge message broker. It:

- Accepts WebSocket connections from WebBook clients
- Stores encrypted messages for offline recipients
- Forwards messages when recipients connect
- Automatically expires old messages (24 hours default)
- Rate limits clients to prevent abuse

**Privacy**: The server only sees encrypted blobs. It cannot read message contents, identify contacts, or access any user data.

## Installation

```bash
cargo build -p webbook-relay --release
```

The binary will be at `target/release/webbook-relay`.

## Usage

```bash
# Start with defaults (port 8080)
webbook-relay

# Or run via cargo
cargo run -p webbook-relay
```

The server listens on `0.0.0.0:8080` by default.

## Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `WEBBOOK_PORT` | `8080` | WebSocket listen port |
| `WEBBOOK_MAX_BLOB_SIZE` | `65536` | Maximum blob size in bytes |
| `WEBBOOK_BLOB_TTL` | `86400` | Blob expiration time in seconds |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

## Protocol

The relay uses a simple JSON protocol over WebSocket binary frames.

### Message Format

Messages are length-prefixed JSON:

```
[4 bytes: length][JSON payload]
```

### Message Types

**Handshake** (client → server):
```json
{
  "version": 1,
  "message_id": "uuid",
  "timestamp": 1234567890,
  "payload": {
    "type": "Handshake",
    "client_id": "hex-encoded-public-key"
  }
}
```

**EncryptedUpdate** (client → server):
```json
{
  "version": 1,
  "message_id": "uuid",
  "timestamp": 1234567890,
  "payload": {
    "type": "EncryptedUpdate",
    "recipient_id": "hex-encoded-public-key",
    "sender_id": "hex-encoded-public-key",
    "ciphertext": [encrypted bytes]
  }
}
```

**Acknowledgment** (server → client):
```json
{
  "version": 1,
  "message_id": "uuid",
  "timestamp": 1234567890,
  "payload": {
    "type": "Acknowledgment",
    "message_id": "original-message-id",
    "status": "ReceivedByRelay"
  }
}
```

## Architecture

```
webbook-relay/
├── src/
│   ├── main.rs       # Server entry point
│   ├── config.rs     # Configuration management
│   ├── handler.rs    # WebSocket connection handler
│   ├── storage.rs    # In-memory blob storage
│   └── rate_limit.rs # Per-client rate limiting
```

### Components

- **Handler**: Manages WebSocket connections, parses messages, routes to storage
- **Storage**: Thread-safe in-memory store with automatic TTL expiration
- **Rate Limiter**: Token bucket algorithm per client ID

## Deployment

### Docker (planned)

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build -p webbook-relay --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/webbook-relay /usr/local/bin/
EXPOSE 8080
CMD ["webbook-relay"]
```

### Systemd

```ini
[Unit]
Description=WebBook Relay Server
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/webbook-relay
Restart=always
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

## Security Considerations

- **No Authentication**: The relay is open by design; security comes from E2E encryption
- **Rate Limiting**: Prevents abuse and DoS
- **No Persistence**: In-memory storage; restart clears all data
- **TLS**: Deploy behind a reverse proxy (nginx, caddy) for TLS termination

## License

MIT
