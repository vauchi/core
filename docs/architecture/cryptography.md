# Cryptography

## User Identity

- Each user generates a **Ed25519 keypair** on first launch
- Public key serves as the user's unique identifier
- Private key never leaves the device (stored in secure enclave when available)

## Key Derivation

```
Master Seed (256-bit)
    │
    ├── Identity Keypair (Ed25519) - for signing
    │
    ├── Exchange Keypair (X25519) - for key exchange
    │
    └── Per-Contact Symmetric Keys (derived via X3DH)
```

## Encryption Scheme

- **Contact Card Encryption**: XChaCha20-Poly1305 with per-contact keys
- **Key Exchange**: X3DH (Extended Triple Diffie-Hellman) for initial exchange
- **Forward Secrecy**: Double Ratchet algorithm for update propagation

## Implementation

WebBook uses the `ring` crate (audited, production-ready) for all cryptographic operations:

- **Signing**: Ed25519 for identity and message signatures
- **Encryption**: AES-256-GCM with random nonces
- **Key Derivation**: PBKDF2 for password-derived keys
- **Memory Safety**: Sensitive data (seeds, keys) zeroed on drop via `zeroize`
