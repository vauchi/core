# Vauchi Privacy Policy

**Last Updated:** January 2026

## Overview

Vauchi is a privacy-focused contact card exchange application. This privacy policy explains how we handle your data. The short version: **your data stays on your device, encrypted, and under your control.**

## Data Collection

### What We Collect

**On Your Device (Local Storage):**
- Your identity (cryptographic keypair, display name)
- Your contact card (fields you choose to add: email, phone, etc.)
- Contacts you've exchanged with (their public cards)
- Visibility rules (which contacts can see which fields)
- Device registry (for multi-device sync)

**On Our Relay Server:**
- Temporary encrypted message envelopes (deleted after delivery or 7 days)
- Connection metadata (IP address, connection timestamps) for rate limiting
- No message content is ever readable by the server

### What We Don't Collect

- We do not collect analytics or telemetry
- We do not track your location
- We do not access your device contacts, photos, or other apps
- We do not use advertising identifiers
- We do not sell or share your data with third parties

## Data Storage

### Local-First Architecture

All your personal data is stored locally on your device:

- **Encryption at Rest:** Your data is encrypted using AES-256-GCM with a key stored in your device's secure enclave (iOS Keychain / Android KeyStore)
- **No Cloud Backup by Default:** Your data is not automatically backed up to any cloud service
- **You Control Exports:** You can create encrypted backups manually, protected by a password you choose

### Relay Server

The relay server facilitates real-time sync between your devices and contacts. It operates as a message broker:

- Messages are end-to-end encrypted before leaving your device
- The server cannot decrypt message contents
- Messages are deleted immediately after delivery or after 7 days if undelivered
- Server logs contain only connection metadata, not message content
- Rate limiting data (IP-based) is retained for 24 hours

## Data Sharing

### With Your Contacts

When you exchange contact cards with someone:
- You explicitly choose which fields they can see
- You can change visibility settings at any time
- Changes sync automatically to their device

### With Third Parties

We do not share your data with any third parties. Period.

### With Law Enforcement

If required by law, we can only provide:
- Connection metadata (IP addresses, timestamps)
- Encrypted message envelopes (which we cannot decrypt)

We cannot provide your contact card content, contact list, or any decrypted data because we do not have access to it.

## Data Security

### Cryptographic Protections

- **Identity Keys:** Ed25519 signing keys, never leave your device
- **Encryption:** X25519 key exchange + AES-256-GCM for all messages
- **Key Derivation:** Argon2id for password-based encryption (backups)
- **Forward Secrecy:** Double Ratchet protocol for ongoing conversations

### Platform Security

- **iOS:** Keys stored in Secure Enclave via Keychain
- **Android:** Keys stored in Hardware-backed KeyStore
- **Desktop:** Keys encrypted with OS-level secure storage

### Certificate Pinning

Mobile apps use certificate pinning to prevent man-in-the-middle attacks against the relay server connection.

## Your Rights

### Access Your Data

All your data is stored locally on your device. You can view it directly in the app.

### Export Your Data

You can export an encrypted backup of all your data at any time from Settings > Backup.

### Delete Your Data

- **Single Device:** Uninstalling the app removes all local data
- **All Devices:** Revoke all linked devices, then uninstall
- **Contact Removal:** You can remove any contact, which removes their data from your device

### Data Portability

Encrypted backups can be imported on any device where you install Vauchi.

## Account Recovery

Vauchi uses a social recovery system instead of traditional account recovery:

- There is no "forgot password" because there is no central account
- Recovery requires vouchers from contacts you've previously exchanged with
- This ensures only you (verified by your social network) can recover your identity

## Children's Privacy

Vauchi is not intended for children under 13. We do not knowingly collect data from children under 13. If you believe a child has provided us with data, please contact us.

## Changes to This Policy

We may update this privacy policy from time to time. We will notify you of significant changes through the app or our website. Continued use of Vauchi after changes constitutes acceptance of the updated policy.

## Open Source

Vauchi is open source software. You can inspect exactly how your data is handled by reviewing our source code at: https://github.com/anthropics/vauchi

## Contact Us

For privacy-related questions or concerns:

- Email: privacy@vauchi.app
- GitHub Issues: https://github.com/anthropics/vauchi/issues

---

## Summary

| Question | Answer |
|----------|--------|
| Do you store my contacts on your servers? | No, only on your device |
| Can you read my messages? | No, they're end-to-end encrypted |
| Do you sell my data? | No, never |
| Do you use tracking/analytics? | No |
| Can I delete my data? | Yes, uninstall the app |
| Is my data backed up automatically? | No, you control backups |
| What if I lose my device? | Use social recovery with your contacts |
