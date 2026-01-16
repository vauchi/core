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
| `RELAY_LISTEN_ADDR` | `0.0.0.0:8080` | Address to listen on |
| `RELAY_MAX_MESSAGE_SIZE` | `1048576` | Maximum message size in bytes (1 MB) |
| `RELAY_BLOB_TTL_SECS` | `7776000` | Blob expiration time in seconds (90 days) |
| `RELAY_RATE_LIMIT` | `60` | Messages per minute per client |
| `RELAY_CLEANUP_INTERVAL` | `3600` | Cleanup interval in seconds (1 hour) |
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |

**Note:** The 90-day TTL allows users who sync infrequently to still receive updates.
However, current storage is in-memory and does not survive server restarts.

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
- **In-Memory Storage**: Server restart clears all pending messages (persistence planned for production)
- **TLS**: Deploy behind a reverse proxy (nginx, caddy) for TLS termination

## Storage Considerations

The default 90-day TTL enables users who rarely open the app to still receive contact updates.

**Current limitations:**
- In-memory storage (lost on restart)
- Memory usage grows with pending messages

**For production deployments**, consider:
- Adding persistent storage (SQLite/RocksDB)
- Setting `RELAY_BLOB_TTL_SECS` based on expected user activity
- Monitoring memory usage

## License

MIT
