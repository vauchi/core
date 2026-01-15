@device @multi-device
Feature: Device Management
  As a WebBook user
  I want to use WebBook on multiple devices
  So that I can access and manage my contacts from any device

  Background:
    Given I have an existing identity on my primary device (Device A)
    And my identity has been set up with display name "Alice"

  # Device Linking

  @link
  Scenario: Generate device linking code
    Given I am on Device A
    When I go to Settings > Link New Device
    Then a QR code should be displayed
    And the QR code should contain encrypted linking data
    And a numeric code should be shown as fallback
    And the code should expire after 5 minutes

  @link
  Scenario: Link new device via QR code
    Given Device A is showing a linking QR code
    And I have WebBook installed on Device B (new)
    When I select "Link to Existing Identity" on Device B
    And I scan the QR code from Device A
    And Device A and B are in proximity
    Then Device B should receive my identity
    And Device B should sync all my data
    And Device B should appear in my linked devices list

  @link
  Scenario: Link new device via numeric code
    Given Device A is showing a linking code "123-456-789"
    And I have WebBook on Device B without camera
    When I enter "123-456-789" on Device B
    And I confirm the device fingerprint on Device A
    Then Device B should be linked to my identity
    And my data should sync to Device B

  @link
  Scenario: Linking requires proximity verification
    Given Device A is showing a linking QR code
    When someone scans it from a remote location
    Then proximity verification should fail
    And the device should not be linked
    And a security warning should be shown on Device A

  @link
  Scenario: Maximum devices reached
    Given I have 10 linked devices
    When I try to link an 11th device
    Then I should see "Maximum devices reached"
    And I should be prompted to unlink a device first

  # Device Synchronization

  @sync
  Scenario: New device receives full state
    Given I have contacts and a contact card on Device A
    When Device B is newly linked
    Then Device B should receive my complete contact card
    And Device B should receive all my contacts
    And Device B should receive all visibility settings

  @sync
  Scenario: Changes sync between devices
    Given Device A and Device B are linked
    When I update my phone number on Device A
    Then Device B should receive the update
    And both devices should show the same phone number

  @sync
  Scenario: Bidirectional sync
    Given Device A and Device B are linked
    When I add a field on Device A
    And I add a different field on Device B
    Then both fields should appear on both devices

  @sync
  Scenario: Offline changes sync when reconnected
    Given Device B is offline
    When I make changes on Device A
    And Device B comes back online
    Then changes should sync to Device B
    And both devices should be in sync

  @sync
  Scenario: Conflict resolution between devices
    Given Device A and Device B are both offline
    When I update my email to "a@test.com" on Device A
    And I update my email to "b@test.com" on Device B
    And both come online
    Then the later change should win
    And both devices should converge to the same value

  # Viewing Linked Devices

  @view
  Scenario: View list of linked devices
    Given I have 3 linked devices
    When I go to Settings > Linked Devices
    Then I should see all 3 devices listed
    And each should show device name and type
    And each should show last sync time
    And the current device should be marked

  @view
  Scenario: View device details
    Given I have linked "iPhone 15 Pro" as Device B
    When I view details for Device B
    Then I should see the device name "iPhone 15 Pro"
    And I should see the device type "iOS"
    And I should see when it was linked
    And I should see last activity time

  @view
  Scenario: Rename a device
    Given Device B is named "iPhone 15 Pro"
    When I rename Device B to "Work Phone"
    Then Device B should appear as "Work Phone"
    And the name should sync to other devices

  # Unlinking Devices

  @unlink
  Scenario: Unlink a device remotely
    Given Device B is linked
    When I select "Remove Device" for Device B on Device A
    And I confirm the removal
    Then Device B should be unlinked
    And Device B should no longer receive updates
    And Device B should be notified of removal

  @unlink
  Scenario: Unlink current device
    Given I am on Device B
    When I select "Unlink This Device"
    And I confirm
    Then Device B should be unlinked
    And Device B should return to initial setup
    And my data should be removed from Device B

  @unlink
  Scenario: Unlinked device data wiped
    Given Device B has been unlinked
    Then all identity data should be deleted from Device B
    And all contact data should be deleted
    And Device B should show the welcome screen

  @unlink
  Scenario: Cannot unlink last device
    Given Device A is my only linked device
    When I try to unlink Device A
    Then I should see "Cannot unlink your only device"
    And unlinking should be prevented

  # Device Security

  @security
  Scenario: Device-specific keys
    Given Device A and Device B are linked
    Then each device should have its own encryption key
    And the device key should be derived from master seed
    And compromise of one device key should not compromise others

  @security
  Scenario: Lost device revocation
    Given Device B has been lost or stolen
    When I mark Device B as "Lost" on Device A
    Then Device B should be immediately unlinked
    And Device B's device key should be revoked
    And contacts should be notified if necessary

  @security
  Scenario: Verify device during linking
    Given I am linking Device B
    Then Device A should display Device B's fingerprint
    And Device B should display Device A's fingerprint
    And I should confirm they match before completing

  @security
  Scenario: Prevent unauthorized device linking
    Given an attacker has access to a linking QR code
    But the attacker is not physically present
    Then proximity verification should block the attack
    And the device should not be linked

  # Platform Support

  @platform
  Scenario Outline: Link devices across platforms
    Given I have Device A running <platform_a>
    When I link Device B running <platform_b>
    Then linking should succeed
    And sync should work between platforms

    Examples:
      | platform_a | platform_b |
      | iOS        | Android    |
      | iOS        | macOS      |
      | iOS        | Windows    |
      | iOS        | Linux      |
      | Android    | macOS      |
      | Android    | Windows    |
      | macOS      | Windows    |

  @platform @desktop
  Scenario: Desktop device without camera uses code
    Given Device B is a desktop without camera
    When I try to link Device B
    Then I should be offered numeric code entry
    And I should be able to type the code from Device A

  # Device-Specific Settings

  @settings
  Scenario: Device-specific notification settings
    Given I have Device A and Device B linked
    When I disable notifications on Device A
    Then notifications should be off on Device A
    But notifications should remain on for Device B

  @settings
  Scenario: Device-specific sync settings
    Given I have Device A (mobile) and Device B (desktop)
    When I set "WiFi only sync" on Device A
    Then Device A should respect WiFi-only
    But Device B should sync on any connection

  @settings
  Scenario: Some settings sync across devices
    Given I update my display name on Device A
    Then the display name should sync to Device B
    And visibility rules should sync to Device B
    And contact groups should sync to Device B

  # Activity Monitoring

  @activity
  Scenario: View device activity
    Given I have multiple linked devices
    When I view device activity
    Then I should see which device made recent changes
    And I should see timestamps of activity
    And I should see types of activity (sync, exchange, etc.)

  @activity
  Scenario: Suspicious device activity alert
    Given Device B shows activity from unusual location
    When the system detects the anomaly
    Then I should be alerted on other devices
    And I should be offered to review and potentially revoke Device B

  # Recovery

  @recovery
  Scenario: Recover using any linked device
    Given I have Device A and Device B linked
    And I lose Device A
    When I set up new Device C
    Then I should be able to link it using Device B
    And Device C should receive all my data

  @recovery
  Scenario: Transfer primary device role
    Given Device A is my primary device
    When Device A is about to be replaced
    Then I should be able to designate Device B as primary
    And Device B should handle all primary device functions
