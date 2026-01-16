# Contact Recovery via Social Vouching

**Status**: Planned
**Priority**: P2
**Feature File**: `features/future/contact_recovery.feature`

## Overview

Enable users to recover their contact relationships after losing all devices through a social vouching mechanism. Users coordinate with old contacts via external channels (phone, WhatsApp, email, real-life meetings) and collect in-person vouchers until a threshold is met.

## Problem Statement

When a user loses their last device:
- Their identity (master seed, keys) is gone
- Their contact list is gone
- Their contacts still have them listed, but with an orphaned public key
- No way to reconnect without starting fresh

**Goal**: Allow recovery of contact *relationships* (not keys or data) through social proof.

## Design Principles

1. **No pre-designation required** - Any contact can vouch, no special setup needed
2. **In-person verification** - Vouchers must verify the person physically
3. **Distributed trust** - Each contact decides based on their own trust network
4. **Minimal relay knowledge** - Relay stores opaque blobs, learns nothing
5. **Graceful degradation** - Isolated contacts get warnings, not blocked

## Architecture

### Data Structures

```rust
/// Recovery claim shown as QR code
#[derive(Serialize, Deserialize)]
pub struct RecoveryClaim {
    pub claim_type: String,        // "recovery_claim"
    pub old_pk: [u8; 32],          // Public key of lost identity
    pub new_pk: [u8; 32],          // Public key of new identity
    pub timestamp: u64,            // Claim generation time
}

/// Voucher created by a contact
#[derive(Serialize, Deserialize)]
pub struct RecoveryVoucher {
    pub old_pk: [u8; 32],          // Public key being recovered
    pub new_pk: [u8; 32],          // New public key
    pub voucher_pk: [u8; 32],      // Voucher's public key
    pub timestamp: u64,            // Voucher creation time
    pub signature: [u8; 64],       // Ed25519 signature
}

impl RecoveryVoucher {
    /// Creates a signed voucher
    pub fn create(
        old_pk: [u8; 32],
        new_pk: [u8; 32],
        voucher_signing_key: &SigningKeyPair,
        timestamp: u64,
    ) -> Self {
        let mut data = Vec::new();
        data.extend_from_slice(&old_pk);
        data.extend_from_slice(&new_pk);
        data.extend_from_slice(&voucher_signing_key.public_key());
        data.extend_from_slice(&timestamp.to_le_bytes());

        let signature = voucher_signing_key.sign(&data);

        Self {
            old_pk,
            new_pk,
            voucher_pk: voucher_signing_key.public_key(),
            timestamp,
            signature,
        }
    }

    /// Verifies the voucher signature
    pub fn verify(&self) -> bool {
        let mut data = Vec::new();
        data.extend_from_slice(&self.old_pk);
        data.extend_from_slice(&self.new_pk);
        data.extend_from_slice(&self.voucher_pk);
        data.extend_from_slice(&self.timestamp.to_le_bytes());

        verify_signature(&self.voucher_pk, &data, &self.signature)
    }
}

/// Complete recovery proof with multiple vouchers
#[derive(Serialize, Deserialize)]
pub struct RecoveryProof {
    pub old_pk: [u8; 32],
    pub new_pk: [u8; 32],
    pub threshold: u32,
    pub vouchers: Vec<RecoveryVoucher>,
    pub created_at: u64,
    pub expires_at: u64,
}

impl RecoveryProof {
    /// Validates the proof has sufficient valid vouchers
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.vouchers.len() < self.threshold as usize {
            return Err(RecoveryError::InsufficientVouchers);
        }

        // Check for duplicates
        let mut seen_vouchers = HashSet::new();
        for voucher in &self.vouchers {
            if !seen_vouchers.insert(voucher.voucher_pk) {
                return Err(RecoveryError::DuplicateVoucher);
            }
            if !voucher.verify() {
                return Err(RecoveryError::InvalidSignature);
            }
            if voucher.old_pk != self.old_pk || voucher.new_pk != self.new_pk {
                return Err(RecoveryError::MismatchedKeys);
            }
        }

        Ok(())
    }
}

/// User's recovery settings
#[derive(Serialize, Deserialize)]
pub struct RecoverySettings {
    /// How many vouchers needed to create a recovery proof
    pub recovery_threshold: u32,      // Default: 3

    /// How many mutual contacts must vouch for auto-acceptance
    pub verification_threshold: u32,  // Default: 2
}

impl Default for RecoverySettings {
    fn default() -> Self {
        Self {
            recovery_threshold: 3,
            verification_threshold: 2,
        }
    }
}
```

### Vouching Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                        VOUCHING FLOW                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. Alice generates recovery claim QR                            │
│     ┌─────────────────────────────────────┐                     │
│     │  RecoveryClaim {                    │                     │
│     │    old_pk: "abc...",                │                     │
│     │    new_pk: "xyz...",                │                     │
│     │    timestamp: now()                 │                     │
│     │  }                                  │                     │
│     └─────────────────────────────────────┘                     │
│                           │                                      │
│                           ▼                                      │
│  2. Bob scans QR, app looks up old_pk                           │
│     ┌─────────────────────────────────────┐                     │
│     │  Found: "Alice" (pk: abc...)        │                     │
│     │  "This person claims to be Alice"   │                     │
│     │  [Vouch] [Cancel]                   │                     │
│     └─────────────────────────────────────┘                     │
│                           │                                      │
│                           ▼                                      │
│  3. Bob verifies in person, taps Vouch                          │
│     ┌─────────────────────────────────────┐                     │
│     │  RecoveryVoucher {                  │                     │
│     │    old_pk: "abc...",                │                     │
│     │    new_pk: "xyz...",                │                     │
│     │    voucher_pk: Bob's pk,            │                     │
│     │    signature: sign(...)             │                     │
│     │  }                                  │                     │
│     └─────────────────────────────────────┘                     │
│                           │                                      │
│                           ▼                                      │
│  4. Bob's contact updated, voucher sent to Alice                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Discovery Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                       DISCOVERY FLOW                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. Alice creates recovery proof (threshold met)                 │
│     ┌─────────────────────────────────────┐                     │
│     │  RecoveryProof {                    │                     │
│     │    old_pk, new_pk, threshold: 3,    │                     │
│     │    vouchers: [Bob, Charlie, Betty]  │                     │
│     │  }                                  │                     │
│     └─────────────────────────────────────┘                     │
│                           │                                      │
│                           ▼                                      │
│  2. Alice uploads to relay                                       │
│     Key: hash(old_pk)                                           │
│     Value: serialized RecoveryProof                             │
│                           │                                      │
│                           ▼                                      │
│  3. John's app syncs, queries relay                             │
│     Query: [hash(contact1_pk), hash(contact2_pk), ...]          │
│                           │                                      │
│                           ▼                                      │
│  4. Relay returns matching proofs                                │
│     Response: { hash(alice_old_pk): RecoveryProof }             │
│                           │                                      │
│                           ▼                                      │
│  5. John's app verifies and prompts                             │
│     - Checks old_pk matches stored contact                      │
│     - Counts mutual contact vouchers                            │
│     - Shows appropriate UI based on trust level                 │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Verification Logic

```rust
pub enum VerificationResult {
    /// Sufficient mutual contacts vouched
    HighConfidence {
        mutual_vouchers: Vec<String>,  // Display names
        total_vouchers: usize,
    },

    /// Some mutual contacts, but below threshold
    MediumConfidence {
        mutual_vouchers: Vec<String>,
        required: u32,
        total_vouchers: usize,
    },

    /// No mutual contacts (David's case)
    LowConfidence {
        total_vouchers: usize,
    },
}

impl RecoveryProof {
    /// Verify recovery proof against local contacts
    pub fn verify_for_contact(
        &self,
        my_contacts: &[Contact],
        settings: &RecoverySettings,
    ) -> VerificationResult {
        // Find mutual contacts who vouched
        let my_contact_pks: HashSet<_> = my_contacts
            .iter()
            .map(|c| c.public_key())
            .collect();

        let mutual_vouchers: Vec<_> = self.vouchers
            .iter()
            .filter(|v| my_contact_pks.contains(&v.voucher_pk))
            .collect();

        let mutual_count = mutual_vouchers.len() as u32;
        let total = self.vouchers.len();

        if mutual_count >= settings.verification_threshold {
            VerificationResult::HighConfidence {
                mutual_vouchers: mutual_vouchers
                    .iter()
                    .filter_map(|v| {
                        my_contacts
                            .iter()
                            .find(|c| c.public_key() == v.voucher_pk)
                            .map(|c| c.display_name().to_string())
                    })
                    .collect(),
                total_vouchers: total,
            }
        } else if mutual_count > 0 {
            VerificationResult::MediumConfidence {
                mutual_vouchers: /* same as above */,
                required: settings.verification_threshold,
                total_vouchers: total,
            }
        } else {
            VerificationResult::LowConfidence {
                total_vouchers: total,
            }
        }
    }
}
```

### Relay API

```rust
/// Recovery-related relay endpoints
impl RelayServer {
    /// Store a recovery proof
    /// POST /recovery/{hash_of_old_pk}
    pub fn store_recovery_proof(
        &self,
        key: [u8; 32],        // hash(old_pk)
        proof: RecoveryProof,
    ) -> Result<(), RelayError> {
        // Validate proof structure (not signatures - relay doesn't have contacts)
        if proof.vouchers.len() < proof.threshold as usize {
            return Err(RelayError::InvalidProof);
        }

        // Check for existing proof (conflict detection)
        if let Some(existing) = self.get_recovery_proof(&key)? {
            if existing.new_pk != proof.new_pk {
                // Conflict! Store both for clients to handle
                self.store_conflicting_proof(key, proof)?;
                return Ok(());
            }
        }

        // Store with expiration
        let expires_at = now() + Duration::days(90);
        self.storage.put_with_expiry(
            format!("recovery:{}", hex::encode(key)),
            proof,
            expires_at,
        )
    }

    /// Query for recovery proofs
    /// POST /recovery/batch
    pub fn batch_query_recovery(
        &self,
        keys: Vec<[u8; 32]>,  // hashes of contact public keys
    ) -> HashMap<[u8; 32], RecoveryProof> {
        keys.into_iter()
            .filter_map(|key| {
                self.get_recovery_proof(&key)
                    .ok()
                    .flatten()
                    .map(|proof| (key, proof))
            })
            .collect()
    }
}
```

## Security Analysis

### Threat Model

| Threat | Attack Vector | Mitigation |
|--------|--------------|------------|
| Impersonation | Attacker claims to be Alice | Requires K in-person vouches from real contacts |
| Social engineering | Trick contacts into vouching | In-person verification, human recognition |
| Compromised relay | Relay modifies proofs | Signatures verify end-to-end |
| Sybil attack | Create fake vouching contacts | Vouchers must be existing contacts of victim |
| Replay attack | Reuse old vouchers | Timestamps + claim freshness check |
| Graph leakage | Learn who knows whom | Voucher list reveals some relationships (accepted tradeoff) |

### Trust Assumptions

1. **K contacts are honest** - If K contacts vouch, at least one verified in-person
2. **In-person verification works** - Humans can recognize each other
3. **Relay is honest-but-curious** - Stores data correctly but may observe metadata
4. **Contacts protect their keys** - Vouchers can't be forged without contact's private key

### Privacy Considerations

**What the relay learns:**
- A recovery proof exists for hash(old_pk)
- The proof contains N vouchers
- When proof was uploaded

**What the relay does NOT learn:**
- Who old_pk or new_pk belong to
- Who the vouchers are (just their public keys)
- Who queries for the proof
- The social graph

**What verifying contacts learn:**
- The voucher list (reveals some of recoverer's contacts)
- This is an acceptable tradeoff for recovery functionality

## Implementation Plan

### Phase 1: Core Data Structures
- [ ] `RecoveryClaim` struct and QR generation
- [ ] `RecoveryVoucher` struct with signing/verification
- [ ] `RecoveryProof` struct with validation
- [ ] `RecoverySettings` with defaults
- [ ] Storage schema for vouchers and settings

### Phase 2: Vouching Flow
- [ ] Recovery claim QR code generation
- [ ] QR scanning and contact lookup
- [ ] Voucher creation UI
- [ ] Voucher collection and storage
- [ ] Contact update after vouching

### Phase 3: Proof Creation and Upload
- [ ] Threshold checking
- [ ] Proof aggregation
- [ ] Relay upload endpoint
- [ ] Proof expiration handling

### Phase 4: Discovery and Verification
- [ ] Batch query implementation
- [ ] Periodic background checking
- [ ] Mutual contact detection
- [ ] Verification result UI (high/medium/low confidence)

### Phase 5: Acceptance Flow
- [ ] Accept/reject/remind UI
- [ ] Contact record update
- [ ] New key exchange initiation
- [ ] Contact card refresh

### Phase 6: Edge Cases
- [ ] Conflict detection (multiple claims)
- [ ] Proof revocation
- [ ] Voucher expiration
- [ ] Isolated contact handling (David's case)

## Testing Strategy

### Unit Tests
- Voucher signature creation and verification
- Proof validation (threshold, duplicates, signatures)
- Verification result calculation
- Relay storage and retrieval

### Integration Tests
- Full vouching flow (QR → vouch → collect)
- Discovery flow (upload → query → verify)
- Multi-device scenarios
- Conflict handling

### Scenario Tests
- All scenarios in `features/future/contact_recovery.feature`
- Focus on security edge cases
- Isolated contact flows

## Open Questions

1. **Proof update frequency**: When Alice gets more vouchers, how often should she update the proof on relay?

2. **Conflict resolution**: If two conflicting proofs exist, should we prefer the one with more vouchers? More recent? Let user decide?

3. **Voucher revocation**: Can a voucher be revoked? (e.g., Bob vouched but now wants to retract)

4. **Cross-app verification**: Could vouchers from other apps (Signal, WhatsApp) be accepted? (Probably out of scope)

5. **Backup option**: Should there be an optional password-protected contact list backup for users who want it?

## Related Documents

- `features/future/contact_recovery.feature` - Gherkin scenarios
- `docs/architecture/decisions.md` - ADR-018 (to be added)
- `docs/THREAT_ANALYSIS.md` - Security considerations
- `docs/architecture/device-linking.md` - Multi-device context
