@identity @security
Feature: Identity Management
  As a Vauchi user
  I want to manage my cryptographic identity
  So that I can securely exchange and update contact information

  Background:
    Given the Vauchi application is installed
    And the device has secure storage available

  # First Launch & Identity Creation

  @first-launch
  Scenario: Create new identity on first launch
    Given I am launching Vauchi for the first time
    When the application initializes
    Then a new Ed25519 keypair should be generated
    And an X25519 exchange keypair should be derived
    And the private keys should be stored in secure storage
    And I should see the identity setup screen

  @first-launch
  Scenario: Set display name during identity setup
    Given I have just created a new identity
    And I am on the identity setup screen
    When I enter "Alice Smith" as my display name
    And I confirm the setup
    Then my contact card should have display name "Alice Smith"
    And I should be taken to the main screen

  @first-launch
  Scenario: Display name validation
    Given I am on the identity setup screen
    When I try to set an empty display name
    Then I should see an error "Display name is required"
    And I should not be able to proceed

  # Identity Backup & Recovery

  @backup
  Scenario: Create encrypted identity backup
    Given I have an existing identity
    And I am on the settings screen
    When I select "Backup Identity"
    And I enter backup password "SecureP@ssw0rd!"
    And I confirm the password
    Then an encrypted backup file should be generated
    And the backup should contain my master seed
    And the backup should be encrypted with my password

  @backup
  Scenario: Backup password requirements
    Given I am creating an identity backup
    When I enter password "weak"
    Then I should see an error about password requirements
    And the backup should not be created

  @backup
  Scenario Outline: Password strength validation
    Given I am creating an identity backup
    When I enter password "<password>"
    Then the password strength indicator should show "<strength>"
    And backup creation should be "<allowed>"

    Examples:
      | password          | strength | allowed     |
      | abc               | weak     | not allowed |
      | password123       | weak     | not allowed |
      | MyP@ssw0rd        | medium   | allowed     |
      | C0mpl3x!P@$$w0rd  | strong   | allowed     |

  @recovery
  Scenario: Restore identity from backup
    Given I am launching Vauchi for the first time
    And I have a valid backup file
    When I select "Restore from Backup"
    And I provide the backup file
    And I enter the correct backup password
    Then my identity should be restored
    And my keypairs should be regenerated from the master seed
    And I should see my previous display name

  @recovery
  Scenario: Restore with incorrect password
    Given I am restoring from backup
    When I enter an incorrect password
    Then I should see an error "Incorrect password"
    And my identity should not be restored
    And I should be able to retry

  @recovery
  Scenario: Restore corrupted backup
    Given I am restoring from backup
    And the backup file is corrupted
    When I attempt to restore
    Then I should see an error "Backup file is corrupted or invalid"
    And I should be offered to create a new identity

  # Identity Security

  @security
  Scenario: Private keys never exposed in logs
    Given I have an existing identity
    When any operation involving private keys occurs
    Then private key material should never appear in logs
    And private key material should never appear in crash reports

  @security
  Scenario: Secure memory handling for keys
    Given I am performing cryptographic operations
    When the operation completes
    Then sensitive key material should be zeroed in memory
    And the memory should not be swappable to disk

  @security
  Scenario: Identity verification via public key fingerprint
    Given I have an existing identity
    When I view my identity details
    Then I should see my public key fingerprint
    And the fingerprint should be displayed in a human-readable format
    And I should be able to copy the fingerprint

  # Multi-device Identity

  @multi-device
  Scenario: Generate device linking QR code
    Given I have an existing identity on Device A
    When I select "Link New Device"
    Then a QR code should be displayed
    And the QR code should contain encrypted device linking data
    And the QR code should expire after 5 minutes

  @multi-device
  Scenario: Link second device successfully
    Given I have an existing identity on Device A
    And Device A is displaying a device linking QR code
    And I have Vauchi installed on Device B
    When I scan the QR code with Device B
    And proximity is verified between devices
    Then Device B should receive my identity
    And both devices should share the same public key
    And both devices should appear in my linked devices list

  @multi-device
  Scenario: Device linking requires proximity
    Given I have a device linking QR code
    When someone scans the QR code from a remote location
    Then the linking should fail
    And I should see "Proximity verification failed"
    And the remote device should not receive my identity
