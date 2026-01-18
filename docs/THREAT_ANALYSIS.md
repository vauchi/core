# Vauchi Threat Analysis

This document analyzes potential threats to Vauchi and the mitigations in place.

## System Context

Vauchi is a **contact card exchange** system, not a messaging app:

- **Traffic pattern**: Generated when users physically meet (QR exchange) or when a user updates their contact info (small update to all contacts)
- **Data type**: Contact cards (name, phone, email, address, social handles)
- **Exchange model**: In-person only via QR code scan
- **Update frequency**: Infrequent (users rarely change phone numbers, emails, etc.)
- **Update size**: Small (delta of changed fields, typically < 1KB)

---

## Threat Model Overview

### Assets to Protect

| Asset | Sensitivity | Description |
|-------|-------------|-------------|
| Contact card data | High | Personal info (phone, email, address) |
| Identity keys | Critical | Long-term signing and encryption keys |
| Social graph | High | Who knows whom (contact relationships) |
| Update metadata | Medium | When someone changed their info |
| Master seed | Critical | Root secret for key derivation |

### Adversary Types

| Adversary | Capability | Motivation |
|-----------|------------|------------|
| **Passive network observer** | Sees encrypted traffic | Mass surveillance, profiling |
| **Malicious relay operator** | Runs a relay node | Data harvesting, traffic analysis |
| **Compromised device** | Full access to one device | Targeted attack, theft |
| **Physical attacker** | Steals/seizes device | Law enforcement, theft |
| **Malicious contact** | Is a legitimate contact | Social engineering, stalking |
| **Platform provider** | Controls app distribution | Backdoor insertion |

---

## Threat Categories

### 1. Confidentiality Threats

#### T1.1: Contact Data Exposure to Relay

**Threat**: Relay operator reads contact card contents.

**Mitigation**:
- All data is E2E encrypted with AES-256-GCM
- Relay only sees encrypted blobs
- Relay cannot decrypt without recipient's private key

**Residual Risk**: None for content. Metadata exposed (see T2.x).

#### T1.2: Contact Data Exposure to Network Observer

**Threat**: ISP/government intercepts traffic.

**Mitigation**:
- E2E encryption regardless of transport
- TLS for relay connections (defense in depth)
- Optional Tor support for IP hiding

**Residual Risk**: Traffic timing/volume analysis possible.

#### T1.3: Key Extraction from Device

**Threat**: Attacker extracts keys from stolen device.

**Mitigation**:
- Master seed encrypted with user password (PBKDF2)
- Keys derived on-demand, not stored in plaintext
- Zeroize sensitive memory on drop

**Residual Risk**: Weak passwords can be brute-forced. Device encryption (OS-level) recommended.

#### T1.4: Forward Secrecy Compromise

**Threat**: Future key compromise reveals past messages.

**Mitigation**:
- Double Ratchet protocol provides forward secrecy
- Each message uses a new encryption key
- Compromising current key doesn't reveal past updates

**Residual Risk**: None for properly implemented ratchet.

---

### 2. Metadata / Traffic Analysis Threats

#### T2.1: Social Graph Inference by Relay

**Threat**: Relay operator maps who contacts whom.

**Mitigation**:
- Recipient IDs are pseudonymous (public key hashes)
- Sender ID is included in encrypted payload, not visible to relay
- No accounts, no registration, no email/phone linking

**Residual Risk**:
- Relay sees which pseudonymous IDs receive messages
- Long-term correlation of IDs is possible
- **Low impact**: Unlike messaging, contact updates are rare

#### T2.2: Update Timing Correlation

**Threat**: Observer notes when Alice updates, when Bob receives.

**Mitigation**:
- Updates batch naturally (all contacts notified simultaneously)
- Relay adds random jitter (planned)
- Low update frequency makes correlation harder

**Residual Risk**:
- If Alice has few contacts, timing reveals her update pattern
- **Low impact**: Knowing "Alice changed her email" has limited value

#### T2.3: Network Location Tracking

**Threat**: ISP/relay sees user's IP address.

**Mitigation**:
- Tor support (optional) hides IP from relay
- Relay doesn't log IPs by default

**Residual Risk**: Without Tor, IP is visible to relay operator.

#### T2.4: Physical Meeting Detection

**Threat**: Observer detects that Alice and Bob met (QR exchange).

**Mitigation**:
- QR exchange is local (Bluetooth/camera), no network traffic during scan
- Initial key agreement uses relay but exchange could be offline
- No location data transmitted

**Residual Risk**:
- If both devices sync immediately after meeting, timing correlation possible
- **Mitigation**: Delay sync by random interval after exchange

---

### 3. Integrity Threats

#### T3.1: Message Tampering by Relay

**Threat**: Relay modifies encrypted blob contents.

**Mitigation**:
- AEAD encryption (AES-GCM) detects tampering
- Message signatures (Ed25519) verify sender
- Tampered messages are rejected

**Residual Risk**: None. Tampering is cryptographically detected.

#### T3.2: Replay Attacks

**Threat**: Relay replays old update to confuse recipient.

**Mitigation**:
- Double Ratchet includes message counters
- Replayed messages have wrong counter, rejected
- Version numbers on cards detect stale data

**Residual Risk**: None for ratchet-protected channels.

#### T3.3: Message Deletion by Relay

**Threat**: Relay drops messages, preventing updates.

**Mitigation**:
- Delivery acknowledgments track what was received
- Unacknowledged updates are retried
- Federation allows fallback to other relays

**Residual Risk**:
- Determined attacker could block all relays
- User would see "sync failed" error

#### T3.4: Impersonation Attack

**Threat**: Attacker claims to be a known contact.

**Mitigation**:
- Contacts identified by public key, not name
- Key exchange happens in-person via QR
- Out-of-band verification (fingerprint comparison)

**Residual Risk**: Social engineering (fake person with QR code).

---

### 4. Availability Threats

#### T4.1: Relay Denial of Service

**Threat**: Attacker floods relay, blocking legitimate users.

**Mitigation**:
- Per-client rate limiting
- Proof-of-work for anonymous clients (planned)
- Multiple relay federation for redundancy

**Residual Risk**: Sustained DDoS can overwhelm resources.

#### T4.2: Storage Exhaustion Attack

**Threat**: Attacker fills relay storage with garbage.

**Mitigation**:
- Message size limits (1MB max)
- Rate limiting per client
- Automatic expiration (90 days)
- Federation allows offloading

**Residual Risk**:
- Large-scale attack from many IPs
- SQLite handles disk efficiently

#### T4.3: Key Loss / Device Loss

**Threat**: User loses device, loses all contacts.

**Mitigation**:
- Encrypted backup with password
- Seed phrase recovery (planned)
- Multi-device sync (planned)

**Residual Risk**: Lost backup + forgotten password = permanent loss.

---

### 5. Privacy Threats (Beyond Confidentiality)

#### T5.1: Contact Cannot Be Removed

**Threat**: Unwanted contact keeps receiving updates.

**Mitigation**:
- User can block contacts (revoke visibility)
- Blocked contacts receive empty updates
- Eventually excluded from future updates

**Residual Risk**:
- Blocked contact retains old data they already received
- Cannot "unsend" previously shared info

#### T5.2: Field Visibility Bypass

**Threat**: Contact sees fields they shouldn't.

**Mitigation**:
- Visibility enforced client-side before encryption
- Hidden fields never included in encrypted payload
- Server cannot override visibility

**Residual Risk**: None if implementation is correct.

#### T5.3: Coerced Disclosure

**Threat**: User forced to reveal contacts under duress.

**Mitigation**:
- Plausible deniability not currently supported
- Hidden contacts feature (planned)
- Duress password (planned) - shows fake contact list

**Residual Risk**: Device seizure reveals true contact list.

#### T5.4: Backup Exposure

**Threat**: Cloud backup includes unencrypted data.

**Mitigation**:
- Local storage is encrypted with device key
- Backups are password-encrypted
- App excludes itself from cloud backup by default

**Residual Risk**: User manually exports unencrypted data.

---

### 6. Implementation Threats

#### T6.1: Cryptographic Implementation Flaws

**Threat**: Bugs in crypto code leak keys or plaintext.

**Mitigation**:
- Use audited libraries (`ring` crate)
- No custom cryptography
- Extensive test coverage

**Residual Risk**: Library vulnerabilities. Monitor advisories.

#### T6.2: Memory Safety Issues

**Threat**: Buffer overflows, use-after-free leak data.

**Mitigation**:
- Rust's memory safety guarantees
- `zeroize` crate for sensitive data
- No unsafe code in core crypto paths

**Residual Risk**: Bugs in dependencies with `unsafe`.

#### T6.3: Side-Channel Attacks

**Threat**: Timing attacks reveal key bits.

**Mitigation**:
- `ring` uses constant-time operations
- No branching on secret values

**Residual Risk**:
- Cache timing attacks on shared hardware
- Mobile devices have weaker isolation

#### T6.4: Supply Chain Attack

**Threat**: Malicious dependency introduced.

**Mitigation**:
- Minimal dependency tree
- Cargo.lock pins versions
- Review updates before merging

**Residual Risk**: Sophisticated supply chain attacks are hard to detect.

---

### 7. Relay-Specific Threats

#### T7.1: Rogue Relay in Federation

**Threat**: Malicious relay joins federation, harvests data.

**Mitigation**:
- Mutual TLS authentication between relays
- Relay identity verification
- E2E encryption means relay sees nothing useful

**Residual Risk**: Metadata (recipient IDs, timing) visible to any relay.

#### T7.2: Relay Database Breach

**Threat**: Attacker dumps relay SQLite database.

**Mitigation**:
- Database contains only encrypted blobs
- No plaintext, no keys, no user accounts
- Blobs are useless without recipient keys

**Residual Risk**:
- Pseudonymous recipient IDs exposed
- Message timestamps exposed

#### T7.3: Relay Operator Subpoena

**Threat**: Law enforcement demands user data.

**Mitigation**:
- Relay holds no decryptable content
- No user accounts to identify
- Operator can only provide encrypted blobs

**Residual Risk**:
- IP logs (if enabled) could be demanded
- Recommendation: don't log IPs

---

### 8. Device Linking Threats

#### T8.1: QR Code Shoulder Surfing

**Threat**: Attacker photographs device link QR code and links their own device.

**Mitigation**:
- Link QR expires after 10 minutes
- Physical proximity required (must be in same location as victim)
- Attacker would need to scan QR *and* complete linking before victim notices

**Residual Risk**:
- Brief exposure window if user leaves QR displayed unattended
- Recommendation: Cancel linking flow if stepping away

#### T8.2: Man-in-the-Middle During Device Link

**Threat**: Attacker intercepts link QR, relays modified version to victim's new device.

**Mitigation**:
- Link QR is signed by identity key
- New device verifies signature before proceeding
- Attacker cannot forge signature without master seed

**Residual Risk**: None if signature verification is implemented correctly.

#### T8.3: Rogue Device Remains After Revocation

**Threat**: Revoked device continues to receive updates.

**Mitigation**:
- Device revocation is broadcast to all contacts
- Contacts remove revoked device from their recipient lists
- Registry version numbers prevent rollback to pre-revocation state

**Residual Risk**:
- Offline contacts may not receive revocation immediately
- Revoked device retains data received before revocation

#### T8.4: Device Registry Tampering

**Threat**: Attacker adds unauthorized device to registry.

**Mitigation**:
- Registry is signed by identity signing key
- Signature verified before registry updates are accepted
- Only devices with master seed can derive valid keys

**Residual Risk**: None for properly signed registries.

#### T8.5: Seed Exposure During Device Link

**Threat**: Master seed intercepted during device linking transfer.

**Mitigation**:
- Seed encrypted with random link_key
- Link key transmitted via QR (local camera, not network)
- Encrypted response sent over secure channel (TLS + E2E)

**Residual Risk**:
- Compromised existing device exposes seed to attacker
- Recommendation: Verify device linking in trusted environment

#### T8.6: Device Key Collision

**Threat**: Two devices derive same device_id, causing conflicts.

**Mitigation**:
- Device ID derived from master_seed + device_index
- Indices assigned sequentially and uniquely
- HKDF collision is cryptographically infeasible

**Residual Risk**: None. HKDF is collision-resistant.

#### T8.7: Excessive Device Linking

**Threat**: Attacker links many devices to exhaust resources or create confusion.

**Mitigation**:
- Maximum 10 devices per identity
- User must explicitly approve each device link
- Device registry shows all linked devices

**Residual Risk**:
- 10 malicious devices could be linked if attacker has physical access repeatedly
- Recommendation: Review linked devices periodically

#### T8.8: Device Fingerprint Spoofing

**Threat**: Attacker's device pretends to be victim's existing device.

**Mitigation**:
- Each device has unique device_id derived from master_seed + index
- Device exchange keys are unique per device
- Cannot spoof without knowing master_seed

**Residual Risk**: None. Device identity is cryptographically bound.

---

## Risk Summary Matrix

| Threat | Likelihood | Impact | Mitigation Effectiveness | Residual Risk |
|--------|------------|--------|-------------------------|---------------|
| T1.1 Data exposure to relay | High | Critical | Strong (E2E) | None |
| T2.1 Social graph inference | Medium | Medium | Partial (pseudonyms) | Low |
| T2.2 Update timing correlation | Low | Low | Partial | Very Low |
| T3.1 Message tampering | Medium | High | Strong (AEAD+sig) | None |
| T3.4 Impersonation | Low | High | Strong (in-person) | Social eng. |
| T4.1 Relay DoS | Medium | Medium | Moderate (rate limit) | Sustained attack |
| T5.1 Cannot remove contact | Medium | Medium | Moderate (block) | Past data |
| T6.1 Crypto bugs | Low | Critical | Strong (audited lib) | Library bugs |
| T7.2 Relay DB breach | Medium | Low | Strong (E2E) | Metadata only |
| T8.1 QR shoulder surfing | Low | Critical | Moderate (expiry) | Brief window |
| T8.3 Rogue device after revocation | Low | Medium | Strong (broadcast) | Offline delay |
| T8.5 Seed exposure during link | Low | Critical | Strong (encrypted) | Compromised device |

---

## Recommendations

### For Users

1. Use a strong password for identity backup
2. Enable device encryption (OS-level)
3. Verify contact fingerprints for sensitive relationships
4. Use Tor mode for enhanced privacy (when available)
5. Regularly backup encrypted identity

### For Relay Operators

1. Don't log IP addresses
2. Enable TLS (reverse proxy)
3. Set appropriate rate limits
4. Monitor for abuse patterns
5. Keep software updated

### For Developers

1. Never implement custom cryptography
2. Update dependencies regularly
3. Run security-focused fuzzing
4. Consider formal verification for critical paths
5. External security audit before 1.0 release

---

## Comparison to Messaging Apps

| Aspect | Vauchi | Messaging Apps |
|--------|---------|----------------|
| Traffic volume | Very low (rare updates) | High (continuous) |
| Message sensitivity | Contact info | Conversations |
| Timing analysis value | Low | High |
| Social graph value | Medium | High |
| Metadata exposure | Recipient ID only | Sender + recipient |
| Forward secrecy | Yes (Double Ratchet) | Varies |
| Relay knowledge | Encrypted blobs only | Often plaintext |

**Key insight**: Vauchi's threat model benefits from infrequent, small updates. Traffic analysis yields less information than with messaging apps because there's less traffic to analyze.

---

### 9. Contact Recovery Threats

Contact recovery allows users who lost all devices to reconnect with contacts through social vouching. This introduces new attack surfaces.

#### T9.1: Recovery Impersonation Attack

**Threat**: Attacker social-engineers K contacts to vouch for them as the victim.

**Mitigation**:
- Requires in-person meeting with each voucher
- Human recognition is the trust anchor
- K vouchers needed (default: 3), harder than fooling 1 person
- Contacts prompted with victim's stored photo/name for verification

**Residual Risk**:
- Sophisticated attacker could potentially fool K contacts
- Especially risky if contacts don't know victim well
- **Impact**: Attacker gains access to victim's contact relationships

#### T9.2: Social Graph Leakage via Vouchers

**Threat**: Recovery proof reveals voucher identities, exposing partial social graph.

**Mitigation**:
- Only voucher public keys are revealed (not names)
- Verifiers may not know all vouchers
- Accepted tradeoff for recovery functionality

**Residual Risk**:
- Partial graph leakage is inherent to the design
- Contacts who verify can see who else vouched
- **Impact**: Medium - reveals some relationships

#### T9.3: Conflicting Recovery Claims

**Threat**: Attacker creates competing recovery proof for same old identity.

**Mitigation**:
- Relay detects conflicts (multiple proofs for same old_pk)
- Contacts warned: "Multiple recovery claims detected"
- Contacts advised to verify directly with victim

**Residual Risk**:
- User confusion during legitimate recovery
- Attacker could delay legitimate recovery
- **Impact**: DoS on recovery process

#### T9.4: Stale Recovery Proof Replay

**Threat**: Old recovery proof used after victim legitimately recovered.

**Mitigation**:
- Recovery proofs expire after 90 days
- Revocation mechanism: victim can sign revocation with old key (if recovered)
- New recovery with different new_pk invalidates old proof

**Residual Risk**:
- 90-day window before automatic expiration
- Requires victim to actively revoke if they recover old device
- **Impact**: Potential for confusion, not compromise

#### T9.5: Voucher Coercion

**Threat**: Attacker forces contacts to vouch under duress.

**Mitigation**:
- Requires coercing K separate people
- In-person meeting required for each voucher
- Coercion of multiple people is operationally difficult

**Residual Risk**:
- Targeted attack with resources could coerce multiple people
- No technical defense against physical coercion
- **Impact**: Critical if successful, but very difficult

#### T9.6: Isolated Contact Exploitation

**Threat**: Attacker exploits contacts who don't know any vouchers (David's case).

**Mitigation**:
- Clear warning: "You don't know any of the vouchers"
- Multiple options: meet in person, verify out-of-band, reject
- "Accept anyway" requires explicit confirmation with strong warning
- User can set personal verification threshold

**Residual Risk**:
- Users may ignore warnings and accept anyway
- Social pressure to accept ("Why don't you trust me?")
- **Impact**: Medium - isolated contacts may accept impersonator

#### T9.7: Relay Metadata About Recovery

**Threat**: Relay learns that recovery events are happening.

**Mitigation**:
- Proof stored under hash(old_pk), not identity
- Batch queries hide specific interest
- Relay cannot correlate old_pk to real identity

**Residual Risk**:
- Relay sees: recovery proof uploaded, queries for specific hashes
- Cannot determine who is recovering or who is querying
- **Impact**: Low - metadata only, no content

#### T9.8: Misconfigured Low Threshold

**Threat**: User sets threshold too low (e.g., 1), easy to attack.

**Mitigation**:
- Default threshold is 3
- Minimum threshold is 1 (cannot be 0)
- UI warns when setting threshold below 3
- Verifiers see the threshold in recovery proof

**Residual Risk**:
- User may intentionally set low threshold
- Single voucher = single point of failure
- **Impact**: High if threshold is 1 and single contact is compromised

#### T9.9: Voucher Timestamp Manipulation

**Threat**: Attacker uses old vouchers with manipulated timestamps.

**Mitigation**:
- Voucher timestamps are signed by voucher's key
- Recovery claim has timestamp, voucher must be within 48 hours
- Clock skew tolerance is limited

**Residual Risk**: None if implementation validates timestamps correctly.

#### T9.10: Self-Vouching Attack

**Threat**: Attacker creates fake contacts, then vouches for self.

**Mitigation**:
- Vouchers must be *existing* contacts of victim
- Voucher's public key must be in victim's contact list (verified by other contacts)
- Cannot vouch with newly created identity

**Residual Risk**:
- Requires attacker to have been legitimate contact before attack
- Long-term sleeper attack theoretically possible
- **Impact**: Requires significant preparation

---

## Risk Summary Matrix

| Threat | Likelihood | Impact | Mitigation Effectiveness | Residual Risk |
|--------|------------|--------|-------------------------|---------------|
| T1.1 Data exposure to relay | High | Critical | Strong (E2E) | None |
| T2.1 Social graph inference | Medium | Medium | Partial (pseudonyms) | Low |
| T2.2 Update timing correlation | Low | Low | Partial | Very Low |
| T3.1 Message tampering | Medium | High | Strong (AEAD+sig) | None |
| T3.4 Impersonation | Low | High | Strong (in-person) | Social eng. |
| T4.1 Relay DoS | Medium | Medium | Moderate (rate limit) | Sustained attack |
| T5.1 Cannot remove contact | Medium | Medium | Moderate (block) | Past data |
| T6.1 Crypto bugs | Low | Critical | Strong (audited lib) | Library bugs |
| T7.2 Relay DB breach | Medium | Low | Strong (E2E) | Metadata only |
| T8.1 QR shoulder surfing | Low | Critical | Moderate (expiry) | Brief window |
| T8.3 Rogue device after revocation | Low | Medium | Strong (broadcast) | Offline delay |
| T8.5 Seed exposure during link | Low | Critical | Strong (encrypted) | Compromised device |
| T9.1 Recovery impersonation | Low | Critical | Moderate (K vouchers) | Social eng. |
| T9.2 Graph leakage via vouchers | High | Medium | Accepted tradeoff | Partial graph |
| T9.5 Voucher coercion | Very Low | Critical | Moderate (K people) | Resourced attacker |
| T9.6 Isolated contact exploitation | Medium | Medium | Moderate (warnings) | User ignores |

---

## Recommendations

### For Users

1. Use a strong password for identity backup
2. Enable device encryption (OS-level)
3. Verify contact fingerprints for sensitive relationships
4. Use Tor mode for enhanced privacy (when available)
5. Regularly backup encrypted identity
6. **Set recovery threshold to at least 3**
7. **Don't accept recovery claims without mutual contact verification or out-of-band confirmation**

### For Relay Operators

1. Don't log IP addresses
2. Enable TLS (reverse proxy)
3. Set appropriate rate limits
4. Monitor for abuse patterns
5. Keep software updated
6. **Set recovery proof expiration (default: 90 days)**

### For Developers

1. Never implement custom cryptography
2. Update dependencies regularly
3. Run security-focused fuzzing
4. Consider formal verification for critical paths
5. External security audit before 1.0 release
6. **Validate voucher timestamps strictly**
7. **Implement conflict detection for recovery proofs**

---

## Comparison to Messaging Apps

| Aspect | Vauchi | Messaging Apps |
|--------|---------|----------------|
| Traffic volume | Very low (rare updates) | High (continuous) |
| Message sensitivity | Contact info | Conversations |
| Timing analysis value | Low | High |
| Social graph value | Medium | High |
| Metadata exposure | Recipient ID only | Sender + recipient |
| Forward secrecy | Yes (Double Ratchet) | Varies |
| Relay knowledge | Encrypted blobs only | Often plaintext |
| **Recovery model** | Social vouching (K contacts) | Phone number / cloud backup |

**Key insight**: Vauchi's threat model benefits from infrequent, small updates. Traffic analysis yields less information than with messaging apps because there's less traffic to analyze.

**Recovery comparison**: Unlike phone-number-based recovery (vulnerable to SIM swap) or cloud backup (vulnerable to account compromise), social vouching requires physical presence with multiple contacts.

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2024-01 | Initial threat analysis |
| 1.1 | 2026-01 | Added device linking threats (T8.x) |
| 1.2 | 2026-01 | Added contact recovery threats (T9.x) |
