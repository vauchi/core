# Relay Network

## Voluntary Relay Network

For users behind restrictive NATs or firewalls:

```
┌─────────────────────────────────────────────────────────┐
│                    RELAY NODE                            │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │              Encrypted Blob Store                │    │
│  │   (No access to plaintext, just routing)         │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│  Features:                                               │
│  - Store-and-forward encrypted messages                 │
│  - No user accounts required                            │
│  - Rate limiting to prevent abuse                       │
│  - Automatic blob expiration (90 days)                  │
│  - Tor-friendly (.onion addresses)                      │
│                                                          │
│  Contribution Model:                                     │
│  - Docker image for easy deployment                     │
│  - Bandwidth tracking (optional donation prompt)        │
│  - No special privileges for contributors               │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

## Relay Federation

Relays can offload messages to peers when storage is full:

```
┌──────────────┐                      ┌──────────────┐
│   Relay A    │                      │   Relay B    │
│  (85% full)  │ ──── offload ──────► │  (40% full)  │
└──────────────┘                      └──────────────┘
       │                                     │
       │  forwarding hint                    │
       ▼                                     ▼
┌──────────────┐                      ┌──────────────┐
│    Client    │ ◄─── "blob at B" ────│              │
│              │ ──── fetch blob ────►│              │
└──────────────┘                      └──────────────┘
```

## Federation Features

- **Peer Discovery**: Relays find each other via registry/DHT
- **Capacity Exchange**: Relays share storage status periodically
- **Message Offloading**: Full relays transfer blobs to peers with capacity
- **Forwarding Hints**: Original relay tells clients where blob moved
- **TTL Preservation**: Transferred blobs keep original expiration
- **Graceful Shutdown**: Relay can offload all blobs before maintenance

## Security

- Mutual TLS authentication between relays
- Blob integrity verified on transfer
- E2E encryption preserved (relays can't read content)
