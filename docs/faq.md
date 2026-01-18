# Vauchi FAQ

Frequently asked questions about Vauchi.

---

## Privacy & Security

### Is my data encrypted?

**Yes, comprehensively:**

- **At rest:** All data on your device is encrypted with AES-256-GCM using a key stored in your device's secure hardware (iOS Keychain / Android KeyStore)
- **In transit:** All communication uses end-to-end encryption (X25519 + AES-256-GCM)
- **Backups:** Protected with Argon2id key derivation and AES-256-GCM

### Can the relay server read my contacts?

**No.** The relay server only sees encrypted message envelopes. It cannot:
- Decrypt any message content
- See your contact list
- Read your contact card fields
- Associate your identity with your data

The relay is essentially a "dumb pipe" that passes encrypted blobs between devices.

### What data does the relay server store?

Only:
- Encrypted message envelopes (deleted after delivery or 7 days)
- Connection metadata for rate limiting (IP address, timestamps - deleted after 24 hours)

### Is Vauchi truly private?

Yes. Vauchi is designed with privacy as the core principle:
- Local-first architecture (your data lives on your device)
- End-to-end encryption (we can't read your data even if we wanted to)
- No analytics or tracking
- No cloud accounts
- Open source (you can verify our claims)

### Can someone intercept my exchanges?

Exchanges use cryptographic protocols that prevent interception:
- QR codes contain signed public keys
- Mobile apps use certificate pinning
- All subsequent communication is encrypted with keys derived from the exchange

---

## Identity & Account

### What happens if I lose my device?

You have several options:

1. **Another linked device:** Continue using Vauchi normally
2. **Backup:** Restore your identity from an encrypted backup
3. **Social recovery:** Get vouchers from contacts who can verify your identity
4. **Start fresh:** Create a new identity and re-exchange with contacts

### How does social recovery work?

Social recovery uses your real-world relationships to verify your identity:

1. You create a "recovery claim" on a new device
2. You share this claim with trusted contacts
3. Each contact creates a "voucher" confirming they recognize you
4. After collecting enough vouchers, your identity is restored
5. The number of required vouchers depends on your settings

This prevents both:
- You being locked out (contacts can vouch for you)
- Someone else stealing your identity (they'd need to fool multiple contacts)

### Can I change my display name?

Yes, go to Settings and edit your display name. The change syncs to all your devices and contacts see the update.

### Can I have multiple identities?

Each app installation has one identity. For multiple identities:
- Use different devices
- Or reinstall the app (this deletes the previous identity)

---

## Contacts & Exchange

### How do I exchange contacts?

1. Meet the person in real life (or video call)
2. Open the Exchange screen
3. Show them your QR code to scan
4. Scan their QR code
5. Done! You're now contacts

### Why do I need to meet in person?

In-person exchange ensures:
- You're connecting with who you think you are
- No man-in-the-middle can intercept the exchange
- The trust relationship is established in the real world

### Can I exchange contacts remotely?

Yes, via video call - share your screen showing the QR code while the other person scans it, then switch. However, in-person is more secure because you can verify identity directly.

### What's contact verification for?

Verification adds an extra layer of trust:
- You compare cryptographic fingerprints in person
- This confirms the person's cryptographic identity matches their real identity
- Verified contacts display a badge
- Useful for high-security situations

### Can I remove a contact?

Yes:
1. Go to Contacts
2. Select the contact
3. Tap Delete/Remove
4. Confirm

This removes them from your device. They still have your data (whatever was visible to them), but won't receive future updates.

### Can I block someone?

Hide all fields from them:
1. Open their contact details
2. Toggle all fields to hidden
3. They'll see you as a contact with no information

You can also remove them entirely.

---

## Multi-Device

### Can I use Vauchi on multiple devices?

Yes! Vauchi supports multi-device sync:
1. Set up Vauchi on your first device
2. Go to Settings > Devices > Link New Device
3. Follow the linking process on your second device
4. Both devices now share the same identity

### How many devices can I link?

Up to 5 devices can be linked to one identity.

### How do I migrate to a new phone?

**Method 1: Device Linking**
1. On old phone: Settings > Devices > Link New Device
2. On new phone: Install Vauchi and join existing identity
3. Once synced, you can uninstall from old phone

**Method 2: Backup & Restore**
1. On old phone: Create an encrypted backup
2. On new phone: Install Vauchi and restore from backup

### What if I lose access to all devices?

Use social recovery:
1. Install Vauchi on a new device
2. Create a recovery claim with your old public ID
3. Get vouchers from contacts
4. Complete recovery

Or, if you have a backup:
1. Install Vauchi
2. Restore from your backup code + password

---

## Backup & Restore

### How do backups work?

1. You create a backup with a password you choose
2. Vauchi encrypts all your data using that password
3. You receive a backup code (Base64 encoded data)
4. To restore: backup code + password = your identity

### What's included in a backup?

- Your identity (cryptographic keys)
- Your contact card (all fields)
- All contacts and their cards
- Visibility settings
- Device information

### How secure are backups?

Very secure:
- Argon2id key derivation (resistant to brute force)
- AES-256-GCM encryption
- Without the password, the backup is useless
- We recommend passphrases (4+ random words)

### I forgot my backup password. Can you recover it?

No. The encryption is designed so that only you can decrypt your backup. This is a security feature, not a bug. Without the password, the backup cannot be decrypted by anyone, including us.

---

## Visibility & Sharing

### How do I control what each contact sees?

1. Open a contact's detail page
2. Scroll to "What They Can See"
3. Toggle individual fields on/off

### Do contacts know when I hide fields?

They see fields disappear from your card, but don't receive a notification. It appears as if you removed the field.

### Can I share different info with different contacts?

Yes! That's the core feature:
- Work contacts: Show work email, hide personal phone
- Family: Show everything
- Acquaintances: Show only basic info

### What's the default visibility for new fields?

New fields are visible to everyone (all contacts) by default. You can change this immediately after adding or later.

---

## Technical

### What's the relay server for?

The relay server:
- Routes encrypted messages between your devices
- Enables real-time sync
- Stores messages temporarily if a device is offline
- Cannot read any message content

Think of it like a post office that handles sealed envelopes.

### Does Vauchi work offline?

Partially:
- You can view all your data offline
- You can make changes offline
- Changes sync when you're back online
- You cannot exchange contacts offline (needs camera + network)

### What encryption does Vauchi use?

- **Signing:** Ed25519
- **Key Exchange:** X25519 (Curve25519)
- **Symmetric Encryption:** AES-256-GCM
- **Key Derivation:** Argon2id (for passwords)
- **Forward Secrecy:** Double Ratchet protocol

All cryptography uses the `ring` library (Rust) - the same used by major browsers and security software.

### Is Vauchi open source?

Yes! The complete source code is available at:
https://github.com/anthropics/vauchi

You can:
- Inspect how your data is handled
- Verify our security claims
- Contribute improvements
- Run your own relay server

### Can I run my own relay server?

Yes! See the deployment documentation:
- Docker image available
- Helm chart for Kubernetes
- systemd service for VPS
- nginx/Caddy configs for TLS

---

## Troubleshooting

### My contacts don't see my updates

1. Check your internet connection
2. Ensure sync is working (Settings > check last sync time)
3. Verify the field is visible to that contact
4. Ask them to manually refresh

### The QR scanner doesn't work

1. Check camera permissions
2. Ensure adequate lighting
3. Clean your camera lens
4. Try adjusting distance to the QR code
5. Restart the app

### Sync seems stuck

1. Check internet connectivity
2. Try manual sync (pull to refresh or Settings > Sync)
3. Check if the relay server is reachable
4. Restart the app

### I can't create a backup (password rejected)

The password strength checker requires:
- At least 8 characters
- Sufficient entropy (randomness)
- Not a common password

Try a passphrase: 4+ random words like "correct horse battery staple"

---

## Platform-Specific

### iOS: Where are my keys stored?

In the iOS Keychain, protected by the Secure Enclave. Keys require device unlock to access.

### Android: Is my data hardware-protected?

On devices with hardware security modules (most modern phones), keys are stored in the Hardware-backed KeyStore, which is tamper-resistant.

### Desktop: How secure is local storage?

Keys are encrypted using OS-level secure storage:
- macOS: Keychain
- Windows: Credential Manager
- Linux: Secret Service (if available) or encrypted file

---

## Still Have Questions?

- **GitHub Issues:** https://github.com/anthropics/vauchi/issues
- **Email:** support@vauchi.app
- **Documentation:** See the full user guide at docs/user-guide.md
