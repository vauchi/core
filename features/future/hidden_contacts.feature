@privacy @hidden @plausible-deniability @opt-in
Feature: Hidden Contacts
  As a Vauchi user in a sensitive situation
  I want to hide certain contacts from casual view
  So that I have plausible deniability if someone looks at my phone

  Note: This is an OPT-IN feature. Hidden contacts functionality is
  disabled by default and must be explicitly enabled in Privacy settings.

  Background:
    Given I have an existing identity
    And I have multiple contacts

  # Opt-in Default State

  @opt-in @default
  Scenario: Hidden contacts feature is disabled by default
    Given I have just installed the app
    When I view my contacts
    Then there should be no "Hide contact" option
    And hidden contacts feature should be OFF
    And no secret gesture should be configured

  @opt-in @default
  Scenario: Enable hidden contacts feature
    Given hidden contacts feature is disabled
    When I go to Settings > Privacy > Hidden Contacts
    And I toggle "Enable hidden contacts"
    And I set up a secret gesture or PIN
    Then hidden contacts feature should be enabled
    And "Hide contact" option should appear in contact menus

  @opt-in @default
  Scenario: Must configure access method when enabling
    Given I am enabling hidden contacts feature
    When I toggle the feature ON
    Then I must configure an access method before proceeding:
      | method         | description                    |
      | Secret gesture | Swipe/tap pattern              |
      | Separate PIN   | PIN different from app lock    |
      | App password   | Reuse existing app password    |
    And I cannot skip this step

  @opt-in @default
  Scenario: Disable hidden contacts feature
    Given hidden contacts feature is enabled
    And I have hidden contacts
    When I go to Settings > Privacy > Hidden Contacts
    And I toggle "Enable hidden contacts" OFF
    Then I should be warned "X contacts will be unhidden"
    And after confirmation, all hidden contacts become visible
    And the feature should be disabled

  # Hiding Contacts

  @hide
  Scenario: Hide a contact
    Given I have a contact "Alice"
    When I long-press on Alice's contact
    And I select "Hide contact"
    And I confirm the action
    Then Alice should be moved to hidden contacts
    And Alice should not appear in the main contact list
    And Alice should still receive my updates

  @hide
  Scenario: Hide contact from contact details
    Given I am viewing contact details for "Bob"
    When I open the contact menu
    And I select "Hide this contact"
    Then Bob should be hidden
    And I should be returned to the contact list
    And Bob should not be visible

  @hide
  Scenario: Hide multiple contacts at once
    Given I have selected contacts "Alice", "Bob", and "Carol"
    When I choose "Hide selected"
    Then all three contacts should be hidden
    And they should not appear in the main list

  @hide
  Scenario: Cannot hide all contacts
    Given I have only one visible contact
    When I try to hide that contact
    Then I should see a warning "At least one contact must remain visible"
    And the contact should not be hidden
    Unless I explicitly confirm hiding all

  # Accessing Hidden Contacts

  @access
  Scenario: Access hidden contacts via gesture
    Given I have hidden contacts
    When I perform the secret gesture (configurable)
    Then the hidden contacts view should appear
    And I should see all hidden contacts

  @access
  Scenario: Access hidden contacts via settings
    Given I have hidden contacts
    And app password is enabled
    When I go to Settings > Hidden Contacts
    And I enter my app password
    Then I should see the hidden contacts list

  @access
  Scenario: Hidden contacts require authentication
    Given I have hidden contacts
    When I try to access hidden contacts
    Then I should be prompted for authentication
    And authentication can be:
      | method      | description              |
      | App password| The main app password    |
      | Biometric   | Fingerprint or face      |
      | PIN         | Separate hidden PIN      |

  @access
  Scenario: Configure access method for hidden contacts
    When I go to Settings > Hidden Contacts > Access Method
    Then I should be able to choose:
      | option           | description                        |
      | Same as app      | Use app password/biometric         |
      | Separate PIN     | Different PIN for hidden contacts  |
      | Gesture only     | Secret gesture, no password        |

  # Unhiding Contacts

  @unhide
  Scenario: Unhide a contact
    Given "Alice" is a hidden contact
    When I access hidden contacts
    And I select Alice
    And I choose "Unhide"
    Then Alice should appear in the main contact list
    And Alice should be removed from hidden contacts

  @unhide
  Scenario: Unhide all contacts
    Given I have multiple hidden contacts
    When I access hidden contacts
    And I choose "Unhide all"
    And I confirm the action
    Then all hidden contacts should become visible
    And the hidden contacts list should be empty

  # Notifications and Updates

  @notifications
  Scenario: Hidden contact updates are silent
    Given "Alice" is a hidden contact
    When Alice updates her contact card
    And the update is received
    Then no notification should be shown
    And no badge should indicate new updates
    And the update should be applied silently

  @notifications
  Scenario: Configure hidden contact notifications
    When I go to Settings > Hidden Contacts > Notifications
    Then I should be able to choose:
      | option         | description                          |
      | Silent         | No notifications at all              |
      | Subtle         | Generic "app update" notification    |
      | Normal         | Show notification but hide name      |

  @notifications
  Scenario: View pending updates from hidden contacts
    Given I have hidden contacts with updates
    When I access the hidden contacts view
    Then I should see which hidden contacts have updates
    And I should be able to view the updates

  # Syncing Behavior

  @sync
  Scenario: Hidden contacts sync normally
    Given "Bob" is a hidden contact
    When I update my contact card
    Then Bob should receive the update
    And the sync should work identically to visible contacts
    And nothing should indicate Bob is hidden

  @sync
  Scenario: Exchange with someone who will be hidden
    Given I am exchanging contacts with someone
    When the exchange completes
    Then I should be asked to categorize the contact
    And "Hide immediately" should be an option
    And the contact can be hidden before appearing in the list

  # Search and Discovery

  @search
  Scenario: Hidden contacts excluded from search
    Given "Alice" is a hidden contact
    When I search for "Alice" in the main contact list
    Then Alice should not appear in results
    And no indication of hidden matches should be shown

  @search
  Scenario: Search within hidden contacts
    Given I have multiple hidden contacts
    When I access hidden contacts
    And I search for a name
    Then only hidden contacts should be searched
    And matching hidden contacts should appear

  # Plausible Deniability

  @deniability
  Scenario: No trace of hidden contacts in main UI
    Given I have hidden contacts
    When someone views my contact list
    Then they should see no indication of hidden contacts
    And contact count should not reveal hidden contacts
    And settings should not obviously show "hidden contacts"

  @deniability
  Scenario: Hidden contacts setting is itself hidden
    Given hidden contacts feature is enabled
    When someone browses my settings
    Then "Hidden contacts" should not be obviously visible
    And access should require the secret gesture or deep navigation

  @deniability
  Scenario: Storage does not reveal hidden status
    Given I have hidden contacts
    When examining the local database
    Then hidden contacts should be stored encrypted
    And it should not be obvious which contacts are hidden
    And hidden status should be part of encrypted blob

  # Visibility Labels Integration

  @labels
  Scenario: Hidden contacts can have visibility labels
    Given "Alice" is a hidden contact
    And I have visibility labels configured
    When I access Alice in hidden contacts
    Then I should be able to assign labels to Alice
    And Alice's visibility should work normally

  @labels
  Scenario: Hide entire label group
    Given I have a label "Sensitive"
    And the label contains contacts
    When I choose to hide the entire label
    Then all contacts in "Sensitive" should be hidden
    And the label itself should be hidden

  # Backup and Restore

  @backup
  Scenario: Hidden contacts included in backup
    Given I have hidden contacts
    When I create an encrypted backup
    Then hidden contacts should be included
    And hidden status should be preserved
    And backup should not reveal which contacts are hidden

  @backup
  Scenario: Restore preserves hidden status
    Given I have a backup with hidden contacts
    When I restore from the backup
    Then hidden contacts should remain hidden
    And I should be able to access them via secret gesture

  # Edge Cases

  @edge-cases
  Scenario: Hide contact during exchange
    Given I am completing a contact exchange
    And I realize I want this contact hidden
    When the exchange completes
    And I immediately select "Hide"
    Then the contact should never appear in the main list
    And it should go directly to hidden contacts

  @edge-cases
  Scenario: Contact blocked and hidden
    Given "Eve" is both blocked and hidden
    When I access hidden contacts
    Then Eve should appear with a "blocked" indicator
    And I should be able to unblock or unhide independently

  @edge-cases
  Scenario: All contacts hidden
    Given I have hidden all my contacts
    When someone views my contact list
    Then they should see an empty list
    And this should appear as if I have no contacts
    And "Add your first contact" prompt may show

  @edge-cases
  Scenario: Receive exchange from hidden contact
    Given "Alice" is a hidden contact
    And Alice has a new device
    When Alice tries to exchange contacts with me again
    Then I should be notified in the hidden contacts area
    And the exchange should update Alice's keys
    And Alice should remain hidden

  # Secret Gesture Configuration

  @gesture
  Scenario: Configure secret gesture
    Given I am in hidden contacts settings
    When I choose to set up a secret gesture
    Then I should be able to define a gesture pattern
    And the gesture should be practiced for confirmation
    And the gesture should be remembered

  @gesture
  Scenario: Change secret gesture
    Given I have a secret gesture configured
    When I want to change it
    Then I should authenticate first
    And I should be able to set a new gesture
    And the old gesture should stop working

  @gesture
  Scenario: Secret gesture options
    When I configure the secret gesture
    Then I should be able to choose from:
      | gesture type     | description                      |
      | Swipe pattern    | Specific swipe sequence          |
      | Tap pattern      | Tap specific areas in order      |
      | Shake pattern    | Device shake sequence            |
      | Pull to reveal   | Pull down past normal refresh    |

  @gesture
  Scenario: Gesture works from any screen
    Given I have configured a secret gesture
    When I perform the gesture from any app screen
    Then hidden contacts should be revealed
    And I should be able to dismiss them to return
