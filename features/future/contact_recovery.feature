Feature: Contact Recovery
  As a user who lost their device
  I want to recover my contact relationships through social vouching
  So that I can reconnect with my contacts without pre-designated recovery contacts

  Background:
    Given the Vauchi app is installed
    And the user has an identity

  # ============================================================
  # Configuration
  # ============================================================

  @recovery @configuration
  Scenario: Default recovery threshold
    When a new identity is created
    Then the default recovery threshold should be 3 vouchers
    And the default verification threshold should be 2 mutual contacts

  @recovery @configuration
  Scenario: Configure recovery threshold
    Given I have an identity
    When I set my recovery threshold to 5 vouchers
    Then my recovery threshold should be 5
    And contacts will need 5 vouchers to verify my recovery

  @recovery @configuration
  Scenario: Configure verification threshold
    Given I have an identity
    When I set my verification threshold to 3 mutual contacts
    Then recoveries I receive will require 3 mutual contact vouchers
    And recoveries with fewer mutual vouchers will show a warning

  @recovery @configuration
  Scenario: Recovery threshold limits
    Given I have an identity
    When I try to set my recovery threshold to 0
    Then the operation should fail with "Recovery threshold must be at least 1"
    When I try to set my recovery threshold to 20
    Then the operation should fail with "Recovery threshold cannot exceed 10"

  # ============================================================
  # Identity Loss and New Identity Creation
  # ============================================================

  @recovery @identity
  Scenario: Create new identity after device loss
    Given Alice had an identity with public key "pk_old"
    And Alice had contacts Bob, Charlie, John, Betty, and David
    When Alice loses her device
    And Alice installs the app on a new device
    And Alice creates a new identity
    Then Alice has a new public key "pk_new"
    And Alice has no contacts on the new device
    And Alice can initiate recovery claiming "pk_old"

  @recovery @identity
  Scenario: Remember old identity fingerprint
    Given Alice had an identity with fingerprint "ABCD-1234-EFGH-5678"
    When Alice creates a new identity on a new device
    Then Alice can enter her old fingerprint manually
    And the app stores the recovery claim for "pk_old"

  # ============================================================
  # In-Person Vouching Process
  # ============================================================

  @recovery @vouching
  Scenario: Generate recovery claim QR code
    Given Alice has a new identity with public key "pk_new"
    And Alice claims her old identity was "pk_old"
    When Alice generates a recovery QR code
    Then the QR code contains:
      | field      | value          |
      | type       | recovery_claim |
      | old_pk     | pk_old         |
      | new_pk     | pk_new         |
      | timestamp  | current_time   |

  @recovery @vouching
  Scenario: Voucher recognizes old contact
    Given Bob has Alice as a contact with public key "pk_old"
    When Bob scans Alice's recovery QR code claiming "pk_old"
    Then Bob's app shows "Alice claims to have lost their device"
    And Bob's app shows Alice's stored display name and photo
    And Bob is prompted to verify this is really Alice in person

  @recovery @vouching
  Scenario: Voucher does not recognize the claimed identity
    Given Bob does not have "pk_old" as a contact
    When Bob scans a recovery QR code claiming "pk_old"
    Then Bob's app shows "Unknown identity - you have no contact with this public key"
    And Bob cannot create a voucher

  @recovery @vouching
  Scenario: Create voucher after in-person verification
    Given Bob has Alice as a contact with public key "pk_old"
    And Bob scans Alice's recovery QR code for "pk_old" -> "pk_new"
    When Bob confirms he has verified Alice in person
    And Bob taps "Vouch for Recovery"
    Then a voucher is created containing:
      | field       | value                                |
      | old_pk      | pk_old                               |
      | new_pk      | pk_new                               |
      | voucher_pk  | Bob's public key                     |
      | timestamp   | current_time                         |
      | signature   | Ed25519 signature of above fields    |
    And the voucher is sent to Alice's new identity

  @recovery @vouching
  Scenario: Voucher establishes contact with new identity
    Given Bob vouches for Alice's recovery from "pk_old" to "pk_new"
    Then Bob's contact record for Alice is updated to "pk_new"
    And a new key exchange is initiated between Bob and Alice
    And Bob and Alice can communicate using the new shared key

  @recovery @vouching
  Scenario: Collect multiple vouchers
    Given Alice has recovery threshold of 3
    And Alice has collected vouchers from:
      | voucher |
      | Bob     |
      | Charlie |
    When Alice meets Betty in person
    And Betty vouches for Alice's recovery
    Then Alice has 3 vouchers
    And the recovery threshold is met

  # ============================================================
  # Recovery Proof Creation and Distribution
  # ============================================================

  @recovery @proof
  Scenario: Create recovery proof when threshold met
    Given Alice has recovery threshold of 3
    And Alice has collected 3 vouchers from Bob, Charlie, and Betty
    When the recovery threshold is met
    Then a recovery proof is automatically created containing:
      | field     | value                              |
      | old_pk    | Alice's old public key             |
      | new_pk    | Alice's new public key             |
      | threshold | 3                                  |
      | vouchers  | [Bob's voucher, Charlie's voucher, Betty's voucher] |

  @recovery @proof
  Scenario: Upload recovery proof to relay
    Given Alice has a valid recovery proof
    When Alice uploads the recovery proof
    Then the relay stores the proof under key hash(old_pk)
    And the relay does not learn Alice's identity
    And the proof is retrievable by anyone who knows old_pk

  @recovery @proof
  Scenario: Recovery proof expiration
    Given Alice uploads a recovery proof
    Then the proof has a default expiration of 90 days
    And after 90 days the relay deletes the proof
    And Alice must create a new proof if needed

  @recovery @proof
  Scenario: Continue collecting vouchers after threshold
    Given Alice has recovery threshold of 3
    And Alice has created a recovery proof with 3 vouchers
    When Alice meets David and David vouches
    Then the recovery proof is updated with 4 vouchers
    And the updated proof is uploaded to the relay
    And more contacts may be able to verify via mutual contacts

  # ============================================================
  # Discovery by Other Contacts
  # ============================================================

  @recovery @discovery
  Scenario: Contact discovers recovery proof via relay
    Given Alice has uploaded a recovery proof
    And John has Alice as a contact with public key "pk_old"
    When John's app syncs with the relay
    Then John's app queries for recovery proofs for all contacts
    And John's app finds the recovery proof for "pk_old"
    And John is notified about Alice's recovery claim

  @recovery @discovery
  Scenario: Batch query for recovery proofs
    Given John has 50 contacts
    When John's app checks for recovery proofs
    Then John's app sends a single batch query with 50 hashes
    And the relay cannot determine which specific contact John is checking
    And John receives any matching recovery proofs

  @recovery @discovery
  Scenario: No recovery proof found
    Given John has Alice as a contact
    And Alice has not uploaded a recovery proof
    When John's app checks for recovery proofs
    Then no recovery notification is shown for Alice

  # ============================================================
  # Verification - Mutual Contacts
  # ============================================================

  @recovery @verification @mutual
  Scenario: Verify recovery with mutual contacts
    Given John has verification threshold of 2 mutual contacts
    And John has Alice as a contact
    And John also has Betty as a contact
    When John receives Alice's recovery proof vouched by Bob, Charlie, and Betty
    Then John's app identifies Betty as a mutual contact
    And John's app shows:
      """
      Alice has recovered their identity.

      Vouched by:
        - Bob (unknown to you)
        - Charlie (unknown to you)
        - Betty (contact of yours) ✓

      Mutual contacts vouching: 1 of 2 required
      """
    And John is warned that verification threshold is not met

  @recovery @verification @mutual
  Scenario: Automatic verification with sufficient mutual contacts
    Given John has verification threshold of 2 mutual contacts
    And John has Alice, Bob, and Charlie as contacts
    When John receives Alice's recovery proof vouched by Bob, Charlie, and Betty
    Then John's app identifies Bob and Charlie as mutual contacts
    And John's app shows:
      """
      Alice has recovered their identity.

      Vouched by:
        - Bob (contact of yours) ✓
        - Charlie (contact of yours) ✓
        - Betty (unknown to you)

      Mutual contacts vouching: 2 of 2 required ✓
      """
    And John can confidently accept the recovery

  @recovery @verification @mutual
  Scenario: High trust with many mutual contacts
    Given Eve has verification threshold of 2 mutual contacts
    And Eve knows Alice, Bob, Charlie, Betty, and David
    When Eve receives Alice's recovery proof vouched by Bob, Charlie, Betty, and David
    Then Eve's app shows all 4 vouchers are mutual contacts
    And the recovery is marked as "High confidence"

  # ============================================================
  # Verification - Isolated Contacts (David's Case)
  # ============================================================

  @recovery @verification @isolated
  Scenario: Isolated contact receives recovery proof
    Given David has verification threshold of 2 mutual contacts
    And David has Alice as his only contact
    And David does not know Bob, Charlie, or Betty
    When David receives Alice's recovery proof vouched by Bob, Charlie, and Betty
    Then David's app shows:
      """
      ⚠️ Recovery Request

      Alice claims to have recovered their identity.

      Vouched by:
        - Bob (unknown to you)
        - Charlie (unknown to you)
        - Betty (unknown to you)

      ⚠️ Warning: You don't know any of the vouchers

      This could be legitimate - Alice may have friend groups
      you're not part of. But it could also be an impersonator.
      """

  @recovery @verification @isolated
  Scenario: Isolated contact options
    Given David receives a recovery proof with no mutual contact vouchers
    Then David is presented with options:
      | option                 | security | description                           |
      | Meet Alice in Person   | Highest  | Verify directly and become a voucher  |
      | Verify Another Way     | High     | Call or text Alice to confirm         |
      | Accept Anyway          | Lower    | Trust the unknown vouchers            |
      | Reject                 | Safe     | Decline the recovery                  |
      | Remind Me Later        | Neutral  | Check again in 7 days                 |

  @recovery @verification @isolated
  Scenario: Isolated contact accepts with warning
    Given David has no mutual contacts with the vouchers
    When David chooses "Accept Anyway"
    Then David is shown a final warning:
      """
      You are accepting a recovery without mutual contact verification.

      If this is an impersonator, they will have access to communicate
      with you as if they were Alice.

      Are you sure?
      """
    And David must confirm to proceed

  @recovery @verification @isolated
  Scenario: Isolated contact verifies out of band
    Given David has no mutual contacts with the vouchers
    When David chooses "Verify Another Way"
    Then David's app shows:
      """
      Contact Alice through another channel to verify:

      Ask Alice to confirm their new identity fingerprint:
      WXYZ-5678-ABCD-1234

      If Alice confirms this fingerprint, tap "Verified"
      """
    And David can mark as verified after out-of-band confirmation

  @recovery @verification @isolated
  Scenario: Isolated contact meets in person and vouches
    Given David has no mutual contacts with the vouchers
    When David chooses "Meet Alice in Person"
    And David meets Alice and scans her recovery QR code
    And David vouches for Alice
    Then David becomes a voucher
    And Alice's recovery proof is updated with David's voucher
    And other isolated contacts who know David can now verify

  # ============================================================
  # Acceptance and Reconnection
  # ============================================================

  @recovery @acceptance
  Scenario: Accept recovery and reconnect
    Given John accepts Alice's recovery
    Then John's contact record for Alice is updated:
      | field      | old_value | new_value |
      | public_key | pk_old    | pk_new    |
    And a new X3DH key exchange is initiated
    And John and Alice establish a new shared secret
    And the old shared secret is discarded

  @recovery @acceptance
  Scenario: Contact card is refreshed after recovery
    Given John accepts Alice's recovery
    And the new key exchange completes
    When Alice sends her current contact card
    Then John receives Alice's updated contact card
    And John's stored card for Alice is refreshed

  @recovery @acceptance
  Scenario: Reject recovery
    Given John receives Alice's recovery proof
    When John rejects the recovery
    Then John's contact for Alice remains unchanged
    And John is not notified again for this recovery proof
    And John can manually reconsider later in settings

  @recovery @acceptance
  Scenario: Remind me later
    Given John receives Alice's recovery proof
    When John chooses "Remind Me Later"
    Then the notification is dismissed
    And John is reminded after 7 days
    And John can adjust the reminder period

  # ============================================================
  # Edge Cases and Security
  # ============================================================

  @recovery @security
  Scenario: Reject recovery from unknown identity
    Given John receives a recovery proof for "pk_unknown"
    And John does not have "pk_unknown" as a contact
    Then the recovery proof is ignored
    And no notification is shown

  @recovery @security
  Scenario: Reject recovery with insufficient vouchers
    Given Alice has recovery threshold of 3
    And Alice has only collected 2 vouchers
    When Alice tries to create a recovery proof
    Then the operation fails with "Insufficient vouchers (2 of 3 required)"

  @recovery @security
  Scenario: Reject duplicate vouchers
    Given Alice is collecting vouchers
    And Bob has already vouched for Alice
    When Bob tries to vouch again
    Then the duplicate voucher is rejected
    And Alice still has 1 voucher from Bob

  @recovery @security
  Scenario: Voucher timestamp validation
    Given Alice generates a recovery claim at time T
    When Bob vouches 48 hours later
    Then the voucher is rejected as expired
    And Alice must generate a fresh recovery claim

  @recovery @security
  Scenario: Detect conflicting recovery claims
    Given Alice uploads a recovery proof for "pk_old" -> "pk_new_1"
    When an attacker uploads a recovery proof for "pk_old" -> "pk_new_2"
    Then contacts see a conflict warning:
      """
      ⚠️ Multiple recovery claims detected for Alice

      Claim 1: pk_new_1 (3 vouchers)
      Claim 2: pk_new_2 (2 vouchers)

      This may indicate an attack. Verify with Alice directly.
      """

  @recovery @security
  Scenario: Revoke recovery proof
    Given Alice has uploaded a recovery proof
    And Alice later recovers her old device
    When Alice signs a revocation with her old private key
    Then the recovery proof is invalidated
    And contacts are notified the recovery was revoked

  @recovery @security
  Scenario: Cannot vouch for own recovery
    Given Alice claims recovery from "pk_old" to "pk_new"
    When Alice tries to vouch for herself (using pk_new)
    Then the self-voucher is rejected

  @recovery @security
  Scenario: Voucher must have existing relationship
    Given Eve does not have Alice as a contact
    When Eve tries to scan Alice's recovery QR code
    Then Eve cannot vouch
    And Eve sees "You must be an existing contact to vouch"

  # ============================================================
  # Data Recovery Limitations
  # ============================================================

  @recovery @limitations
  Scenario: What is recovered
    Given John accepts Alice's recovery
    Then the following is recovered:
      | data                    | recovered |
      | Contact relationship    | Yes       |
      | Ability to communicate  | Yes       |
      | Alice's contact card    | Yes (re-sent by Alice) |

  @recovery @limitations
  Scenario: What is NOT recovered
    Given John accepts Alice's recovery
    Then the following is NOT recovered:
      | data                        | reason                          |
      | Old encryption keys         | Lost with device                |
      | Old message history         | Cannot decrypt without keys     |
      | Visibility rules Alice set  | Lost with device                |
      | Notes Alice wrote           | Lost with device                |
      | Old shared secret           | Replaced with new exchange      |

  # ============================================================
  # Multi-Device Considerations
  # ============================================================

  @recovery @multi-device
  Scenario: Recovery when user has multiple devices
    Given Alice has 2 devices linked
    And Alice loses device 1 but still has device 2
    Then Alice does not need recovery
    And Alice can revoke device 1 from device 2
    And contacts are notified of device revocation

  @recovery @multi-device
  Scenario: Recovery after losing all devices
    Given Alice had 2 linked devices
    And Alice loses both devices
    Then Alice must use social recovery
    And Alice creates new identity on new device
    And the old linked devices are implicitly revoked

  # ============================================================
  # Relay Behavior
  # ============================================================

  @recovery @relay
  Scenario: Relay stores recovery proof privately
    Given Alice uploads a recovery proof
    Then the relay stores the proof under hash(pk_old)
    And the relay cannot read the proof contents (not encrypted, but opaque)
    And the relay does not learn Alice's identity or contacts

  @recovery @relay
  Scenario: Relay returns recovery proof on query
    Given Alice has uploaded a recovery proof
    When any client queries for hash(pk_old)
    Then the relay returns the recovery proof
    And the relay does not log who queried

  @recovery @relay
  Scenario: Relay handles proof expiration
    Given Alice uploaded a recovery proof 91 days ago
    And the proof expiration is 90 days
    When John's app queries for the proof
    Then the relay returns "not found"
    And Alice must create a new recovery proof if needed

  @recovery @relay
  Scenario: Relay rate limits recovery queries
    Given a client makes 1000 recovery proof queries in 1 minute
    Then the relay rate limits the client
    And returns "too many requests" error
