@visibility @privacy
Feature: Visibility Control
  As a Vauchi user
  I want to control which contacts can see which of my contact fields
  So that I can share different information with different people

  Background:
    Given I have an existing identity as "Alice"
    And I have the following fields on my contact card:
      | type   | label        | value              |
      | phone  | Personal     | +1-555-111-1111    |
      | phone  | Work         | +1-555-222-2222    |
      | email  | Personal     | alice@personal.com |
      | email  | Work         | alice@work.com     |
    And I have contacts "Bob", "Carol", and "Dave"

  # Default Visibility

  @default
  Scenario: New fields default to visible to all contacts
    When I add a new phone field "Mobile" with value "+1-555-333-3333"
    Then the visibility for "Mobile" should be "all contacts"
    And Bob, Carol, and Dave should all be able to see the "Mobile" field

  @default
  Scenario: New contacts see default-visible fields
    Given all my fields are set to "visible to all"
    When I exchange contacts with "Eve"
    Then Eve should see all four of my contact fields

  # Setting Individual Visibility

  @individual
  Scenario: Hide a field from a specific contact
    Given my "Personal" phone is visible to all
    When I set "Personal" phone visibility to hide from "Dave"
    Then Bob should see my "Personal" phone
    And Carol should see my "Personal" phone
    But Dave should not see my "Personal" phone

  @individual
  Scenario: Show a field only to specific contacts
    Given my "Work" phone is visible to all
    When I set "Work" phone visibility to "only Bob and Carol"
    Then Bob should see my "Work" phone
    And Carol should see my "Work" phone
    But Dave should not see my "Work" phone

  @individual
  Scenario: Make a field private (visible to none)
    Given my "Personal" email is visible to all
    When I set "Personal" email visibility to "no one"
    Then Bob should not see my "Personal" email
    And Carol should not see my "Personal" email
    And Dave should not see my "Personal" email

  # Group Visibility

  @groups
  Scenario: Create a visibility group
    When I create a visibility group named "Close Friends"
    And I add "Bob" and "Carol" to "Close Friends"
    Then "Close Friends" should contain Bob and Carol
    And "Close Friends" should not contain Dave

  @groups
  Scenario: Apply visibility group to a field
    Given I have a visibility group "Work Contacts" containing Carol and Dave
    When I set "Work" email visibility to group "Work Contacts"
    Then Carol should see my "Work" email
    And Dave should see my "Work" email
    But Bob should not see my "Work" email

  @groups
  Scenario: Add contact to group updates their visibility
    Given "Work" email is visible only to group "Work Contacts"
    And Bob is not in "Work Contacts"
    When I add Bob to "Work Contacts"
    Then Bob should now see my "Work" email
    And Bob should receive an update with the "Work" email field

  @groups
  Scenario: Remove contact from group updates their visibility
    Given "Work" email is visible only to group "Work Contacts"
    And Carol is in "Work Contacts"
    When I remove Carol from "Work Contacts"
    Then Carol should no longer see my "Work" email
    And Carol should receive an update removing the "Work" email field

  # Visibility Changes Propagation

  @propagation
  Scenario: Granting visibility sends update to contact
    Given my "Personal" phone is hidden from Dave
    And Dave is online
    When I make "Personal" phone visible to Dave
    Then Dave should receive an encrypted update
    And Dave should now see my "Personal" phone number

  @propagation
  Scenario: Revoking visibility sends update to contact
    Given my "Personal" phone is visible to Dave
    And Dave is online
    When I hide "Personal" phone from Dave
    Then Dave should receive an encrypted update
    And my "Personal" phone should be removed from Dave's view
    And Dave should see the updated contact card

  @propagation
  Scenario: Visibility change when contact is offline
    Given my "Personal" phone is visible to Dave
    And Dave is offline
    When I hide "Personal" phone from Dave
    Then the update should be queued for Dave
    And when Dave comes online
    Then Dave should receive the update
    And my "Personal" phone should be removed from Dave's view

  # Visibility and New Contacts

  @new-contact
  Scenario: Set visibility before exchange
    Given I have marked "Personal" phone as "exchange with explicit consent only"
    When I exchange contacts with "Eve"
    Then I should be prompted to choose visibility for Eve
    And Eve should only see fields I approve

  @new-contact
  Scenario: Apply template visibility to new contact
    Given I have a visibility template "Professional"
    And "Professional" template shows only work fields
    When I exchange with Eve and apply "Professional" template
    Then Eve should see my "Work" phone
    And Eve should see my "Work" email
    But Eve should not see my "Personal" phone
    But Eve should not see my "Personal" email

  # Visibility Rules Persistence

  @persistence
  Scenario: Visibility settings persist after app restart
    Given I have set custom visibility for multiple fields
    When I restart the application
    Then all visibility settings should be preserved
    And Bob, Carol, and Dave should see the same fields as before

  @persistence
  Scenario: Visibility settings sync across my devices
    Given I have custom visibility settings on Device A
    When I link Device B to my identity
    Then Device B should have the same visibility settings
    And changes on either device should sync to the other

  # Visibility Verification

  @verification
  Scenario: View what a specific contact can see
    Given I have various visibility settings
    When I select "View as Bob"
    Then I should see my contact card as Bob sees it
    And hidden fields should not be displayed

  @verification
  Scenario: Visibility audit shows all contacts for a field
    Given my "Personal" phone has custom visibility
    When I view visibility details for "Personal" phone
    Then I should see a list of contacts who can see it
    And I should see a list of contacts who cannot see it

  # Edge Cases

  @edge-cases
  Scenario: Delete contact removes their visibility rules
    Given I have custom visibility rules for Dave
    When I delete Dave from my contacts
    Then all visibility rules for Dave should be removed
    And if I later re-add Dave, default visibility should apply

  @edge-cases
  Scenario: Block contact removes all their visibility
    Given Dave can see all my fields
    When I block Dave
    Then Dave should not be able to see any of my fields
    And Dave should receive an update with an empty contact card
    And Dave should not receive future updates

  @edge-cases
  Scenario: Unblock contact restores previous visibility
    Given I have blocked Dave
    And Dave previously could see my "Work" fields
    When I unblock Dave
    Then Dave should be able to see my "Work" fields again
    And Dave should receive an update with the restored fields

  # Privacy Protection

  @privacy
  Scenario: Contact cannot determine hidden field existence
    Given I have a "Personal" phone hidden from Dave
    When Dave views my contact card
    Then Dave should not see the "Personal" phone field
    And Dave should have no indication that the field exists
    And the data sent to Dave should not contain the hidden field

  @privacy
  Scenario: Encrypted updates reveal nothing about hidden fields
    Given I have fields hidden from Dave
    When I update a field that Dave cannot see
    Then Dave should not receive any update
    And no network traffic should go to Dave for this update

  @privacy
  Scenario: Visibility changes are atomic
    Given I am making multiple visibility changes
    When I save the changes
    Then all changes should be applied atomically
    And contacts should receive a single update
    And there should be no intermediate inconsistent state visible

  # Bulk Operations

  @bulk
  Scenario: Set visibility for all fields at once
    Given I have 10 contact fields
    When I select "Set all to visible for Bob only"
    Then all 10 fields should be visible only to Bob
    And other contacts should not see any fields

  @bulk
  Scenario: Reset all visibility to default
    Given I have various custom visibility settings
    When I select "Reset all to default"
    And I confirm the action
    Then all fields should be visible to all contacts
    And all contacts should receive updates
