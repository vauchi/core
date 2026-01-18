@exchange @proximity
Feature: Contact Card Exchange
  As a Vauchi user
  I want to exchange contact cards with people I meet in person
  So that I can build my contact list through real-world interactions

  Background:
    Given Alice has Vauchi installed with identity "Alice"
    And Bob has Vauchi installed with identity "Bob"
    And both devices are operational

  # QR Code Exchange

  @qr-code
  Scenario: Generate exchange QR code
    Given Alice wants to share her contact card
    When Alice selects "Share Contact"
    Then a QR code should be displayed
    And the QR code should contain Alice's public key
    And the QR code should contain a one-time exchange token
    And the QR code should contain an audio challenge seed

  @qr-code
  Scenario: QR code expiration
    Given Alice has generated an exchange QR code
    When 5 minutes have passed
    Then the QR code should expire
    And Alice should be prompted to generate a new one
    And scanning the expired QR code should fail

  @qr-code
  Scenario: Successful QR code exchange with proximity
    Given Alice is displaying her exchange QR code
    And Bob is physically present with Alice
    When Bob scans Alice's QR code
    And both devices emit and verify ultrasonic audio handshake
    Then the exchange should proceed
    And Bob should receive Alice's contact card
    And Alice should receive Bob's contact card
    And both should see "Exchange Successful"

  @qr-code @proximity-fail
  Scenario: QR code exchange blocked without proximity
    Given Alice is displaying her exchange QR code
    And Bob is scanning remotely via screenshot
    When Bob scans Alice's QR code
    Then ultrasonic audio verification should fail
    And the exchange should be blocked
    And both should see "Proximity verification failed"
    And no contact cards should be exchanged

  @qr-code
  Scenario: QR code exchange with one-way sharing
    Given Alice wants to share but not receive
    And Alice has selected "Share Only" mode
    When Alice displays her QR code
    And Bob scans it with proximity verified
    Then Bob should receive Alice's contact card
    But Alice should not receive Bob's contact card

  # Bluetooth Low Energy (BLE) Exchange

  @ble @mobile
  Scenario: Discover nearby Vauchi users via BLE
    Given Alice has BLE enabled
    And Bob has BLE enabled and is within 2 meters
    When Alice opens the "Nearby" screen
    Then Alice should see Bob in the nearby users list
    And the signal strength should indicate close proximity

  @ble @mobile
  Scenario: Initiate BLE exchange
    Given Alice sees Bob in the nearby users list
    And Bob is within 2 meters (verified by RSSI)
    When Alice taps on Bob to exchange
    And Bob accepts the exchange request
    Then contact cards should be exchanged over BLE
    And both should see "Exchange Successful"

  @ble @mobile @proximity-fail
  Scenario: BLE exchange blocked when too far
    Given Alice sees Bob in the nearby users list
    But Bob is more than 2 meters away
    When Alice attempts to exchange with Bob
    Then the exchange should be blocked
    And Alice should see "Move closer to exchange"

  @ble @mobile
  Scenario: BLE exchange with relay attack prevention
    Given an attacker is relaying BLE signals
    And Alice attempts to exchange with what appears to be Bob
    When the challenge-response verification runs
    Then the relay attack should be detected
    And the exchange should be blocked
    And Alice should see "Security verification failed"

  # NFC Exchange

  @nfc @mobile
  Scenario: NFC contact exchange
    Given Alice and Bob have NFC-capable devices
    And both have NFC enabled
    When Alice and Bob tap their devices together
    Then NFC exchange should initiate
    And public keys should be exchanged
    And both should confirm the exchange
    And contact cards should be exchanged

  @nfc @mobile
  Scenario: NFC exchange timeout
    Given Alice has initiated NFC mode
    When 30 seconds pass without NFC contact
    Then NFC mode should timeout
    And Alice should return to the main screen

  # Exchange Protocol Security

  @security @exchange
  Scenario: X3DH key agreement during exchange
    Given Alice and Bob are performing an exchange
    When the exchange is initiated
    Then X3DH key agreement should establish a shared secret
    And the shared secret should be unique to Alice and Bob
    And the contact cards should be encrypted with the shared secret

  @security @exchange
  Scenario: Exchange creates mutual keys
    Given Alice and Bob have completed an exchange
    Then Alice should have an encryption key for communicating with Bob
    And Bob should have an encryption key for communicating with Alice
    And these keys should be derived from the same shared secret
    And forward secrecy should be established via ratcheting

  @security @exchange
  Scenario: Exchange verifies identity
    Given Alice and Bob are completing an exchange
    When contact cards are received
    Then Alice should verify Bob's card is signed by Bob's public key
    And Bob should verify Alice's card is signed by Alice's public key
    And unsigned or incorrectly signed cards should be rejected

  # Exchange States and Errors

  @exchange-state
  Scenario: Incomplete exchange recovery
    Given Alice and Bob are mid-exchange
    When Alice's device loses connectivity
    Then the exchange should pause
    And both should see "Exchange interrupted"
    And the exchange should be resumable for 60 seconds

  @exchange-state
  Scenario: Resume interrupted exchange
    Given an exchange between Alice and Bob was interrupted
    And less than 60 seconds have passed
    When Alice's device reconnects
    Then the exchange should automatically resume
    And complete successfully

  @exchange-state
  Scenario: Exchange timeout after interruption
    Given an exchange between Alice and Bob was interrupted
    And 60 seconds have passed
    Then the exchange session should expire
    And both should need to start a new exchange

  @exchange-error
  Scenario: Handle malformed QR code
    Given Alice is scanning a QR code
    When the QR code contains invalid data
    Then Alice should see "Invalid QR code"
    And no exchange should be attempted

  @exchange-error
  Scenario: Handle non-Vauchi QR code
    Given Alice is scanning a QR code
    When the QR code is not from Vauchi
    Then Alice should see "Not a Vauchi contact code"
    And no exchange should be attempted

  # Duplicate and Existing Contacts

  @duplicate
  Scenario: Exchange with existing contact
    Given Alice already has Bob in her contacts
    When Alice and Bob perform another exchange
    Then Alice should see "Bob is already in your contacts"
    And Alice should be asked "Update contact?"
    And the existing contact should not be duplicated

  @duplicate
  Scenario: Update existing contact via exchange
    Given Alice has Bob in her contacts with phone "555-1111"
    And Bob has updated his phone to "555-2222"
    When Alice and Bob perform an exchange
    And Alice chooses to update the contact
    Then Alice's contact for Bob should show phone "555-2222"

  @duplicate
  Scenario: Keep existing contact without update
    Given Alice has Bob in her contacts with phone "555-1111"
    And Bob has updated his phone to "555-2222"
    When Alice and Bob perform an exchange
    And Alice chooses not to update
    Then Alice's contact for Bob should still show phone "555-1111"

  # Audio Proximity Verification Details

  @audio @proximity
  Scenario: Ultrasonic audio handshake process
    Given Alice is displaying a QR code
    And Bob has scanned the QR code
    When proximity verification begins
    Then Alice's device should emit an ultrasonic challenge
    And Bob's device should detect the challenge
    And Bob's device should emit a signed response
    And Alice's device should verify the response
    And both devices should confirm proximity

  @audio @proximity
  Scenario: Audio verification works in noisy environment
    Given there is ambient noise in the environment
    When Alice and Bob perform audio proximity verification
    Then the ultrasonic frequencies should not be affected
    And verification should succeed

  @audio @proximity
  Scenario: Audio verification on devices without ultrasonic support
    Given Bob's device cannot emit ultrasonic audio
    When Bob scans Alice's QR code
    Then fallback verification should be offered
    And Alice should manually confirm Bob's presence
    And a warning about reduced security should be shown

  # Cross-Platform Exchange

  @cross-platform
  Scenario Outline: Exchange between different platforms
    Given Alice is using <platform_a>
    And Bob is using <platform_b>
    When they perform a QR code exchange with proximity verification
    Then the exchange should succeed
    And both should have each other's contact cards

    Examples:
      | platform_a | platform_b |
      | iOS        | Android    |
      | iOS        | Desktop    |
      | Android    | Desktop    |
      | Desktop    | iOS        |
      | Desktop    | Android    |

  @cross-platform @desktop
  Scenario: Desktop exchange without audio (requires confirmation)
    Given Alice is using desktop without microphone
    And Bob is using mobile
    When Alice displays QR code on desktop
    And Bob scans it
    Then Bob should be asked to confirm Alice is present
    And Alice should be asked to confirm Bob is present
    And the exchange should complete after both confirmations
