# Sync & Update Propagation

## Decentralized Sync Architecture

Using **libp2p** with DHT (Distributed Hash Table):

1. **Discovery**: Contacts find each other via DHT using public key hashes
2. **Connection**: Direct P2P connection when both online
3. **Relay**: Use relay nodes when direct connection impossible (NAT, etc.)
4. **Sync Protocol**:
   - CRDT-based (Conflict-free Replicated Data Types) for eventual consistency
   - Only encrypted deltas transmitted
   - Merkle tree for efficient change detection

## Update Flow

```
┌──────────────┐                         ┌──────────────┐
│   User A     │                         │   User B     │
│  (updater)   │                         │  (contact)   │
└──────┬───────┘                         └──────┬───────┘
       │                                        │
       │  1. Modify contact field               │
       │                                        │
       │  2. Check visibility rules             │
       │     (B can see this field?)            │
       │                                        │
       │  3. Encrypt update with                │
       │     A-B shared key                     │
       │                                        │
       │  4. Sign update                        │
       │                                        │
       │──────── 5. Push to DHT ───────────────►│
       │         (or direct P2P)                │
       │                                        │
       │                          6. Receive    │
       │                             encrypted  │
       │                             update     │
       │                                        │
       │                          7. Verify     │
       │                             signature  │
       │                                        │
       │                          8. Decrypt &  │
       │                             apply      │
       │                                        │
```

## Current Implementation

The MVP uses WebSocket relay transport with Double Ratchet for forward secrecy. DHT-based discovery is planned for future versions.
