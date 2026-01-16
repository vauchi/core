@privacy @duress @plausible-deniability @security
Feature: Duress Password
  As a WebBook user who may be coerced to unlock my phone
  I want a secondary password that shows a fake contact list
  So that I can comply with demands while protecting sensitive contacts

  Note: This feature requires app password to be enabled first.
  The duress password unlocks a decoy profile that looks legitimate.

  Background:
    Given I have an existing identity
    And I have app password enabled
    And I have contacts in my real profile

  # Prerequisites

  @prerequisite
  Scenario: App password required for duress mode
    Given app password is not enabled
    When I try to enable duress password
    Then I should see "App password required"
    And I should be prompted to set up app password first
    And duress password option should be disabled

  @prerequisite
  Scenario: Enable app password first
    Given app password is not enabled
    When I enable app password
    And I set a strong password
    Then duress password option should become available
    And I should see "Duress mode available" in security settings

  # Setting Up Duress Password

  @setup
  Scenario: Enable duress password
    Given app password is enabled
    When I navigate to Security settings
    And I select "Duress password"
    And I authenticate with my real password
    Then I should be prompted to create a duress password
    And the duress password must be different from my real password

  @setup
  Scenario: Duress password must differ from real password
    Given I am setting up duress password
    When I enter the same password as my real password
    Then I should see "Duress password must be different"
    And the setup should not proceed

  @setup
  Scenario: Configure duress password
    Given I am setting up duress password
    When I enter a valid duress password
    And I confirm the duress password
    Then duress mode should be enabled
    And I should see a confirmation
    And I should be prompted to set up the decoy profile

  @setup
  Scenario: Duress password strength requirements
    Given I am setting up duress password
    Then the duress password should meet requirements:
      | requirement        | description                    |
      | Minimum length     | At least 8 characters          |
      | Different from app | Must not match app password    |
      | Memorable          | User should be able to recall it under stress |

  # Decoy Profile Setup

  @decoy-profile
  Scenario: Create decoy profile after duress setup
    Given duress password is enabled
    And I have not set up the decoy profile
    When I access decoy profile settings
    Then I should be able to add decoy contacts
    And I should set a decoy display name
    And I should configure decoy contact card

  @decoy-profile
  Scenario: Add decoy contacts
    Given I am configuring the decoy profile
    When I add a decoy contact
    Then I should enter:
      | field        | description                    |
      | Name         | Decoy contact name             |
      | Phone        | Optional phone number          |
      | Email        | Optional email                 |
    And the contact should be saved to the decoy profile
    And it should not be a real WebBook contact

  @decoy-profile
  Scenario: Decoy contacts are static
    Given I have decoy contacts configured
    Then decoy contacts should not sync
    And decoy contacts should not receive updates
    And decoy contacts are just display data

  @decoy-profile
  Scenario: Make decoy profile believable
    Given I am configuring the decoy profile
    Then I should consider:
      | aspect           | recommendation                           |
      | Contact count    | Have a realistic number (5-20)           |
      | Names            | Use common, believable names             |
      | Recency          | Some should have recent "activity"       |
      | Variety          | Mix of friends, family, work contacts    |

  @decoy-profile
  Scenario: Import decoy contacts from phone
    Given I am configuring the decoy profile
    When I choose "Import from phone contacts"
    Then I should be able to select contacts to copy
    And selected contacts become static decoy entries
    And they are disconnected from real phone contacts

  # Using Duress Password

  @duress-login
  Scenario: Login with duress password shows decoy
    Given duress password is enabled
    And I have a decoy profile configured
    When I am at the app login screen
    And I enter the duress password
    Then I should see the decoy profile
    And I should see decoy contacts
    And no indication of duress mode should be visible

  @duress-login
  Scenario: Decoy profile looks like real app
    Given I am logged in with duress password
    Then the app should look completely normal
    And I should see my decoy contact card
    And I should be able to browse decoy contacts
    And all features should appear to work

  @duress-login
  Scenario: Duress mode actions are fake
    Given I am in duress mode
    When I try to sync
    Then a fake sync animation should play
    And no real network activity should occur
    And "Sync complete" should be shown

  @duress-login
  Scenario: Cannot access real contacts in duress mode
    Given I am in duress mode
    When I browse the app
    Then I should only see decoy contacts
    And real contacts should be completely inaccessible
    And hidden contacts should not be accessible

  # Duress Mode Behavior

  @behavior
  Scenario: Exchange in duress mode is fake
    Given I am in duress mode
    When I try to exchange contacts
    Then a QR code should be displayed
    But scanning it should not work
    And no real exchange should occur

  @behavior
  Scenario: Edits in duress mode are discarded
    Given I am in duress mode
    When I edit my decoy contact card
    And I save the changes
    Then changes should appear saved
    But they should be discarded on logout
    And real data should be unaffected

  @behavior
  Scenario: No trace of duress mode after logout
    Given I was in duress mode
    When I logout or close the app
    And I login with my real password
    Then I should see my real contacts
    And there should be no trace of duress activity
    And real data should be intact

  @behavior
  Scenario: Duress mode does not affect real data
    Given I am in duress mode
    When I perform any action
    Then real contacts should not be modified
    And real encryption keys should not be accessed
    And real sync state should not change

  # Silent Features in Duress Mode

  @silent
  Scenario: Optional silent alert in duress mode
    Given duress password is enabled
    And I have configured a silent alert
    When I login with duress password
    Then a silent alert should be triggered
    And the alert could:
      | action              | description                    |
      | Send SMS            | To a trusted contact           |
      | Send location       | Last known GPS coordinates     |
      | Log timestamp       | Record when duress occurred    |

  @silent
  Scenario: Configure silent alert
    Given duress password is enabled
    When I configure silent alert
    Then I should be able to set:
      | option              | description                    |
      | Emergency contact   | Phone number to alert          |
      | Message content     | Pre-written message            |
      | Include location    | Whether to send GPS            |
      | Alert method        | SMS, email, or silent log      |

  @silent
  Scenario: Silent alert requires network
    Given silent alert is configured
    And I login with duress password
    But there is no network connection
    Then the alert should be queued
    And it should be sent when connectivity returns
    And this should happen invisibly

  # Distinguishing Passwords

  @passwords
  Scenario: Passwords processed identically at UI level
    Given duress password is enabled
    When observing the login process
    Then entering either password should look the same
    And timing should be similar
    And no visual difference should reveal which was entered

  @passwords
  Scenario: Wrong password behavior
    Given duress password is enabled
    When I enter a wrong password (neither real nor duress)
    Then I should see "Incorrect password"
    And this should be indistinguishable from other failures

  @passwords
  Scenario: Cannot use biometric for duress
    Given duress password is enabled
    And biometric login is enabled
    When biometric authentication is used
    Then it should always unlock the real profile
    And duress mode requires password entry

  # Changing Duress Settings

  @settings
  Scenario: Change duress password
    Given duress password is enabled
    When I go to Security settings
    And I authenticate with my real password
    And I select "Change duress password"
    Then I should be able to set a new duress password
    And the old duress password should stop working

  @settings
  Scenario: Disable duress password
    Given duress password is enabled
    When I go to Security settings
    And I authenticate with my real password
    And I disable duress password
    Then the duress password should stop working
    And decoy profile should be deleted
    And I should see confirmation

  @settings
  Scenario: Cannot access duress settings in duress mode
    Given I am in duress mode
    When I go to Security settings
    Then I should see fake settings
    And "Duress password" option should not appear
    And real settings should be inaccessible

  # Decoy Profile Management

  @decoy-management
  Scenario: Edit decoy profile from real mode
    Given I am logged in with real password
    When I go to Security > Duress settings
    And I select "Edit decoy profile"
    Then I should be able to modify decoy contacts
    And I should be able to update decoy card

  @decoy-management
  Scenario: Preview decoy profile
    Given duress password is enabled
    When I select "Preview decoy profile"
    Then I should see what the decoy profile looks like
    And I should be able to verify it looks realistic
    And I should be clearly in preview mode (not duress)

  @decoy-management
  Scenario: Delete all decoy data
    Given duress password is enabled
    When I choose to reset decoy profile
    And I confirm the action
    Then all decoy contacts should be deleted
    And the decoy card should be reset
    And I should be prompted to set up again

  # Edge Cases

  @edge-cases
  Scenario: App password changed affects duress
    Given duress password is enabled
    When I change my app password
    Then duress password should remain unchanged
    And both passwords should continue to work
    And they should remain different

  @edge-cases
  Scenario: Forgot duress password
    Given I cannot remember my duress password
    When I login with real password
    Then I can access duress settings
    And I can reset the duress password
    And no data is lost

  @edge-cases
  Scenario: Backup includes duress profile
    Given duress password is enabled
    When I create an encrypted backup
    Then the backup should include duress settings
    And decoy profile should be included
    And restore should preserve duress configuration

  @edge-cases
  Scenario: Duress mode timeout
    Given I am in duress mode
    And the app has been inactive for 5 minutes
    When the app locks due to timeout
    Then I should see the normal login screen
    And either password will work
    And no indication of previous duress mode

  @edge-cases
  Scenario: Duress mode with notification
    Given I am in duress mode
    And a real update arrives in the background
    Then no notification should be shown
    And the update should be processed silently
    And it will be visible in real mode later

  # Believability

  @believability
  Scenario: Decoy app has full functionality appearance
    Given I am in duress mode
    Then all app features should appear to work:
      | feature          | behavior                         |
      | Sync             | Fake animation, "success"        |
      | Exchange         | Shows QR (non-functional)        |
      | Settings         | Fake settings that "save"        |
      | Contact details  | Shows decoy contact info         |
      | Edit card        | Appears to save (discarded)      |

  @believability
  Scenario: Decoy profile has realistic timestamps
    Given I have decoy contacts
    Then decoy contacts should show:
      | field            | behavior                         |
      | Added date       | Realistic past dates             |
      | Last updated     | Staggered, believable dates      |
      | Last seen        | Some recent, some older          |

  @believability
  Scenario: Settings reflect decoy state
    Given I am in duress mode
    When I view settings
    Then settings should show decoy identity name
    And "contacts count" should match decoy count
    And storage usage should be plausible

  # Security Considerations

  @security
  Scenario: Real keys never loaded in duress mode
    Given I am in duress mode
    Then real encryption keys should not be in memory
    And real identity seed should not be accessed
    And real contacts database should not be decrypted

  @security
  Scenario: Duress detection should be impossible
    Given an attacker is examining the app
    When they compare duress mode to real mode
    Then behavior should be indistinguishable
    And storage patterns should look similar
    And network timing should be similar

  @security
  Scenario: Multiple duress passwords not supported
    Given duress password is enabled
    When I try to add another duress password
    Then this should not be possible
    And only one duress password is supported
    And this prevents complexity attacks
