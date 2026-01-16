@visibility @labels @privacy
Feature: Visibility Labels
  As a WebBook user
  I want to organize my contacts into labels like "family", "friends", "professional"
  So that I can easily control what contact information different groups of people can see

  Background:
    Given I have an existing identity as "Alice"
    And I have the following fields on my contact card:
      | type   | label        | value              |
      | phone  | Personal     | +1-555-111-1111    |
      | phone  | Work         | +1-555-222-2222    |
      | email  | Personal     | alice@personal.com |
      | email  | Work         | alice@work.com     |
      | address| Home         | 123 Main St        |
    And I have contacts "Bob", "Carol", "Dave", and "Eve"

  # Label Management

  @label-create
  Scenario: Create a new visibility label
    When I create a new label named "Family"
    Then the label "Family" should be created
    And the label should have no contacts assigned
    And the label should have no fields associated

  @label-create
  Scenario: Default labels are suggested on first use
    Given I have not created any labels yet
    When I open the label management screen
    Then I should see suggested labels:
      | label        |
      | Family       |
      | Friends      |
      | Professional |
    And I should be able to create them with one tap

  @label-create
  Scenario: Create custom label with any name
    When I create a new label named "University Colleagues"
    Then the label "University Colleagues" should be created
    And it should appear in my labels list

  @label-create
  Scenario: Cannot create duplicate label names
    Given I have a label "Friends"
    When I try to create another label named "Friends"
    Then I should see an error "Label already exists"
    And only one "Friends" label should exist

  @label-rename
  Scenario: Rename an existing label
    Given I have a label "Work"
    When I rename the label "Work" to "Colleagues"
    Then the label should be named "Colleagues"
    And all contacts in the label should remain
    And all field associations should remain

  @label-rename
  Scenario: Cannot rename to existing label name
    Given I have labels "Friends" and "Family"
    When I try to rename "Friends" to "Family"
    Then I should see an error "Label name already in use"
    And the label should remain named "Friends"

  @label-delete
  Scenario: Delete a label
    Given I have a label "Temporary"
    And Bob and Carol are in label "Temporary"
    When I delete the label "Temporary"
    And I confirm the deletion
    Then the label "Temporary" should not exist
    And Bob and Carol should remain in my contacts
    And field visibility should fall back to default

  @label-delete
  Scenario: Cancel label deletion
    Given I have a label "Important"
    When I attempt to delete the label "Important"
    And I cancel the deletion
    Then the label "Important" should still exist

  # Assigning Contacts to Labels

  @assign-contact
  Scenario: Add a contact to a label
    Given I have a label "Family"
    When I add Bob to label "Family"
    Then Bob should be a member of "Family"
    And Bob should see fields associated with "Family"

  @assign-contact
  Scenario: Add multiple contacts to a label at once
    Given I have a label "Close Friends"
    When I add Bob, Carol, and Dave to label "Close Friends"
    Then all three should be members of "Close Friends"

  @assign-contact
  Scenario: Remove a contact from a label
    Given Bob is in label "Friends"
    When I remove Bob from label "Friends"
    Then Bob should not be a member of "Friends"
    And Bob's visibility should fall back to per-contact settings or defaults

  @assign-contact
  Scenario: Contact in multiple labels
    Given I have labels "Friends" and "Colleagues"
    When I add Carol to both "Friends" and "Colleagues"
    Then Carol should be a member of both labels
    And Carol should see fields from both labels (union of visibility)

  @assign-contact
  Scenario: View all labels for a contact
    Given Carol is in labels "Friends" and "Colleagues"
    When I view Carol's contact details
    Then I should see that Carol belongs to "Friends" and "Colleagues"
    And I should be able to edit her label memberships

  # Associating Fields with Labels

  @field-label
  Scenario: Associate a field with a label
    Given I have a label "Family"
    When I set my "Personal" phone to be visible to label "Family"
    Then contacts in "Family" should see my "Personal" phone
    And contacts not in "Family" should not see it

  @field-label
  Scenario: Associate field with multiple labels
    Given I have labels "Family" and "Close Friends"
    When I set my "Home" address to be visible to "Family" and "Close Friends"
    Then contacts in either label should see my "Home" address
    And contacts in neither label should not see it

  @field-label
  Scenario: Remove field from label visibility
    Given my "Personal" email is visible to label "Family"
    When I remove "Family" from "Personal" email visibility
    Then contacts in "Family" should no longer see "Personal" email
    Unless they have per-contact visibility override

  @field-label
  Scenario: View which labels can see a field
    Given my "Personal" phone is visible to labels "Family" and "Close Friends"
    When I view visibility settings for "Personal" phone
    Then I should see "Family" and "Close Friends" listed
    And I should see how many contacts are in each label

  # Label-based Visibility in Action

  @visibility-effect
  Scenario: Contact sees fields based on label membership
    Given I have a label "Family" containing Bob
    And my "Personal" phone is visible to label "Family"
    And my "Work" phone is visible to all
    When Bob views my contact card
    Then Bob should see my "Personal" phone
    And Bob should see my "Work" phone

  @visibility-effect
  Scenario: Non-member does not see label-restricted fields
    Given I have a label "Family" containing Bob
    And Carol is not in "Family"
    And my "Personal" phone is visible only to label "Family"
    When Carol views my contact card
    Then Carol should not see my "Personal" phone

  @visibility-effect
  Scenario: Adding contact to label grants visibility
    Given my "Personal" email is visible to label "Family"
    And Dave is not in "Family"
    And Dave is online
    When I add Dave to label "Family"
    Then Dave should receive an update
    And Dave should now see my "Personal" email

  @visibility-effect
  Scenario: Removing contact from label revokes visibility
    Given my "Personal" email is visible only to label "Family"
    And Eve is in "Family"
    And Eve is online
    When I remove Eve from label "Family"
    Then Eve should receive an update
    And Eve should no longer see my "Personal" email

  # Per-Contact Visibility Override

  @override
  Scenario: Grant visibility to contact not in label
    Given my "Home" address is visible to label "Family"
    And Dave is not in "Family"
    When I grant "Home" address visibility specifically to Dave
    Then Dave should see my "Home" address
    And Dave does not need to be in "Family"

  @override
  Scenario: Revoke visibility from contact in label
    Given my "Personal" phone is visible to label "Friends"
    And Carol is in "Friends"
    When I specifically hide "Personal" phone from Carol
    Then Carol should not see my "Personal" phone
    Despite being in the "Friends" label

  @override
  Scenario: Per-contact override takes precedence over label
    Given my "Work" email is visible to label "Colleagues"
    And Bob is in "Colleagues"
    And I have specifically hidden "Work" email from Bob
    When Bob views my contact card
    Then Bob should not see my "Work" email
    Because per-contact settings override label settings

  @override
  Scenario: View effective visibility for a contact
    Given Carol is in labels "Family" and "Friends"
    And I have some per-contact overrides for Carol
    When I view Carol's effective visibility
    Then I should see which fields Carol can see
    And I should see which visibility comes from labels vs overrides
    And I should be able to modify either

  @override
  Scenario: Clear per-contact overrides
    Given I have custom per-contact visibility for Dave
    When I clear Dave's per-contact overrides
    Then Dave's visibility should be determined only by his labels
    And Dave's visibility should be determined by default settings

  # Labels are Local

  @local-only
  Scenario: Labels are not shared with contacts
    Given I have Bob in label "Annoying People"
    When Bob views my contact information
    Then Bob should have no indication of any label
    And the label name should never be transmitted to Bob

  @local-only
  Scenario: Labels exist only on my devices
    Given I have labels "Family", "Friends", and "Work"
    When Bob exchanges contacts with me
    Then Bob should not receive any label information
    And my labels should remain private to me

  @local-only
  Scenario: Labels sync across my own devices only
    Given I have labels on Device A
    When I link Device B to my identity
    Then Device B should have the same labels
    And contacts in each label should be synced
    And label-based visibility settings should be synced

  # Quick Actions with Labels

  @quick-assign
  Scenario: Assign label during contact exchange
    Given I have labels "Family", "Friends", and "Professional"
    When I complete a contact exchange with "Frank"
    Then I should be prompted to assign Frank to a label
    And I should be able to skip the assignment

  @quick-assign
  Scenario: Assign label from contact list
    Given I am viewing my contacts list
    When I long-press on Bob's contact
    Then I should see an option to manage labels
    And I should be able to add or remove Bob from labels

  @quick-assign
  Scenario: Bulk assign contacts to label
    Given I have selected Bob, Carol, and Dave
    When I choose "Add to label"
    And I select "Work"
    Then all three contacts should be added to "Work"

  # Label Visibility Templates

  @template
  Scenario: Configure default fields for a label
    Given I have a label "Professional"
    When I configure "Professional" to show only work fields by default
    Then the label should have associated default visibility:
      | field          | visible |
      | Work phone     | yes     |
      | Work email     | yes     |
      | Personal phone | no      |
      | Personal email | no      |

  @template
  Scenario: Apply label template to new contact
    Given I have a label "Professional" with configured visibility
    When I add Eve to label "Professional"
    Then Eve should automatically see the fields configured for "Professional"
    Unless I have specific overrides for Eve

  # Edge Cases

  @edge-cases
  Scenario: Delete contact removes from all labels
    Given Dave is in labels "Friends" and "Colleagues"
    When I delete Dave from my contacts
    Then Dave should be removed from all labels
    And all visibility rules for Dave should be deleted

  @edge-cases
  Scenario: Block contact removes from all labels
    Given Eve is in labels "Friends" and "Family"
    When I block Eve
    Then Eve should be removed from all labels
    And Eve should not see any of my fields
    And Eve should receive an empty contact card update

  @edge-cases
  Scenario: Unblock contact does not restore labels
    Given I have blocked Eve who was in "Friends"
    When I unblock Eve
    Then Eve should not automatically be in "Friends" again
    And I should be prompted to assign labels if desired

  @edge-cases
  Scenario: Label with no contacts still exists
    Given I have a label "Future Team" with no contacts
    When I view my labels
    Then "Future Team" should be displayed
    And I should be able to configure its visibility settings

  @edge-cases
  Scenario: Maximum number of labels
    Given I have created 50 labels
    When I try to create another label
    Then I should see an error "Maximum number of labels reached"
    And I should be suggested to delete unused labels

  # Label Statistics

  @stats
  Scenario: View label statistics
    Given I have labels with various contacts
    When I open the label management screen
    Then I should see each label with contact count
    And I should see which labels have visibility rules configured

  @stats
  Scenario: View which contacts are not in any label
    When I view the unlabeled contacts list
    Then I should see all contacts not assigned to any label
    And I should be able to bulk-assign them
