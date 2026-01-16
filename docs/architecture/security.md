# Security Considerations

## Threat Model

| Threat | Mitigation |
|--------|------------|
| Remote contact harvesting | Physical proximity verification required |
| Man-in-the-middle | End-to-end encryption with verified keys |
| Server compromise | Server only sees encrypted blobs |
| Device theft | Local encryption with device key |
| Metadata leakage | Minimal metadata, random padding |
| Replay attacks | Timestamps and nonces in all messages |
| Key compromise | Forward secrecy via ratcheting |
| Device link hijacking | Signed QR codes, 10-minute expiry |
| Rogue device after revocation | Revocation broadcast to all contacts |
| Device registry tampering | Registry signed by identity key |
| Recovery impersonation | K in-person vouchers required |
| Social graph via recovery | Accepted tradeoff, voucher PKs only |
| Isolated contact exploitation | Clear warnings, multiple verification options |

For detailed threat analysis (35+ threats), see [THREAT_ANALYSIS.md](../THREAT_ANALYSIS.md).

## Data Classification

- **Highly Sensitive**: Private keys, master seed
- **Sensitive**: Contact information, visibility rules
- **Semi-Public**: Public keys, relay routing info

## Security Priorities

1. **Private keys** never leave the device unencrypted
2. **All data** encrypted at rest and in transit
3. **Physical proximity** required for exchange
4. **Forward secrecy** via Double Ratchet
5. **No metadata** leakage to relays
6. **Open source** for community audit

## Audit Requirements

- Regular security audits of crypto implementation
- Open source for community review
- Bug bounty program
