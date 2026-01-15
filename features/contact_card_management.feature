@contact-card
Feature: Contact Card Management
  As a WebBook user
  I want to manage my contact card with various types of information
  So that I can share relevant contact details with others

  Background:
    Given I have an existing identity
    And I am logged into WebBook
    And I have a contact card with display name "Alice Smith"

  # Adding Contact Fields

  @add-field @phone
  Scenario: Add a phone number field
    Given I am viewing my contact card
    When I add a new field of type "phone"
    And I set the label to "Mobile"
    And I set the value to "+1-555-123-4567"
    And I save the field
    Then my contact card should have a phone field labeled "Mobile"
    And the phone field should have value "+1-555-123-4567"

  @add-field @email
  Scenario: Add an email field
    Given I am viewing my contact card
    When I add a new field of type "email"
    And I set the label to "Work"
    And I set the value to "alice@company.com"
    And I save the field
    Then my contact card should have an email field labeled "Work"
    And the email field should have value "alice@company.com"

  @add-field @social
  Scenario Outline: Add social media fields
    Given I am viewing my contact card
    When I add a new field of type "<social_type>"
    And I set the label to "<label>"
    And I set the value to "<value>"
    And I save the field
    Then my contact card should have a social field of type "<social_type>"
    And the field should be labeled "<label>"
    And the field should have value "<value>"

    Examples:
      | social_type       | label      | value              |
      | social_twitter    | Twitter    | @alicesmith        |
      | social_instagram  | Instagram  | alice.smith        |
      | social_linkedin   | LinkedIn   | linkedin.com/in/as |
      | social_github     | GitHub     | alicesmith         |
      | social_facebook   | Facebook   | alice.smith.123    |

  @add-field @address
  Scenario: Add a physical address field
    Given I am viewing my contact card
    When I add a new field of type "address"
    And I set the label to "Home"
    And I set the value to "123 Main St, City, ST 12345"
    And I save the field
    Then my contact card should have an address field labeled "Home"
    And the address field should have value "123 Main St, City, ST 12345"

  @add-field @website
  Scenario: Add a website field
    Given I am viewing my contact card
    When I add a new field of type "website"
    And I set the label to "Personal Site"
    And I set the value to "https://alicesmith.com"
    And I save the field
    Then my contact card should have a website field labeled "Personal Site"

  @add-field @custom
  Scenario: Add a custom field
    Given I am viewing my contact card
    When I add a new field of type "custom"
    And I set the label to "Signal"
    And I set the value to "+1-555-987-6543"
    And I save the field
    Then my contact card should have a custom field labeled "Signal"

  # Field Validation

  @validation @phone
  Scenario Outline: Phone number validation
    Given I am adding a phone field
    When I enter "<phone_number>" as the value
    Then the validation should "<result>"
    And I should see message "<message>"

    Examples:
      | phone_number      | result   | message                           |
      | +1-555-123-4567   | pass     |                                   |
      | 555-123-4567      | pass     |                                   |
      | +44 20 7946 0958  | pass     |                                   |
      | not-a-phone       | fail     | Please enter a valid phone number |
      |                   | fail     | Phone number is required          |

  @validation @email
  Scenario Outline: Email validation
    Given I am adding an email field
    When I enter "<email>" as the value
    Then the validation should "<result>"

    Examples:
      | email                 | result |
      | alice@example.com     | pass   |
      | alice+tag@example.com | pass   |
      | alice@sub.example.com | pass   |
      | invalid-email         | fail   |
      | @example.com          | fail   |
      | alice@               | fail   |

  @validation @size
  Scenario: Field value size limit
    Given I am adding a field
    When I enter a value exceeding 1000 characters
    Then I should see an error "Value exceeds maximum length"
    And the field should not be saved

  # Editing Contact Fields

  @edit-field
  Scenario: Edit an existing field value
    Given my contact card has a phone field "Mobile" with value "+1-555-123-4567"
    When I edit the "Mobile" phone field
    And I change the value to "+1-555-999-8888"
    And I save the changes
    Then the "Mobile" phone field should have value "+1-555-999-8888"
    And the last modified timestamp should be updated

  @edit-field
  Scenario: Edit a field label
    Given my contact card has an email field labeled "Work"
    When I edit the label to "Office"
    And I save the changes
    Then the email field should be labeled "Office"
    And the value should remain unchanged

  @edit-field
  Scenario: Cancel editing preserves original values
    Given my contact card has a phone field "Mobile" with value "+1-555-123-4567"
    When I edit the "Mobile" phone field
    And I change the value to "+1-555-999-8888"
    And I cancel the edit
    Then the "Mobile" phone field should still have value "+1-555-123-4567"

  # Removing Contact Fields

  @remove-field
  Scenario: Remove a field from contact card
    Given my contact card has a phone field "Mobile"
    When I remove the "Mobile" phone field
    And I confirm the removal
    Then my contact card should not have a field labeled "Mobile"

  @remove-field
  Scenario: Cancel field removal
    Given my contact card has a phone field "Mobile"
    When I attempt to remove the "Mobile" phone field
    And I cancel the removal
    Then my contact card should still have the "Mobile" phone field

  @remove-field
  Scenario: Remove field updates contacts
    Given my contact card has a phone field "Mobile" visible to contact "Bob"
    And Bob has my contact card
    When I remove the "Mobile" phone field
    Then Bob should receive an update
    And Bob should no longer see the "Mobile" field on my card

  # Display Name Management

  @display-name
  Scenario: Update display name
    Given my display name is "Alice Smith"
    When I change my display name to "Alice S."
    And I save the changes
    Then my contact card should have display name "Alice S."

  @display-name
  Scenario: Display name cannot be empty
    Given my display name is "Alice Smith"
    When I try to change my display name to ""
    Then I should see an error "Display name cannot be empty"
    And my display name should remain "Alice Smith"

  @display-name
  Scenario: Display name length limit
    Given I am editing my display name
    When I enter a name longer than 100 characters
    Then I should see an error "Display name too long"

  # Avatar Management

  @avatar
  Scenario: Add avatar to contact card
    Given my contact card has no avatar
    When I add an avatar image
    Then my contact card should display the avatar
    And the avatar should be under 64KB

  @avatar
  Scenario: Avatar image too large
    Given I am adding an avatar
    When I select an image larger than 64KB
    Then the image should be automatically compressed
    And the compressed avatar should be under 64KB

  @avatar
  Scenario: Remove avatar from contact card
    Given my contact card has an avatar
    When I remove the avatar
    Then my contact card should not display an avatar

  # Contact Card Limits

  @limits
  Scenario: Maximum number of fields
    Given my contact card has 24 fields
    When I try to add another field
    Then the field should be added successfully
    And my contact card should have 25 fields

  @limits
  Scenario: Exceed maximum fields
    Given my contact card has 25 fields
    When I try to add another field
    Then I should see an error "Maximum number of fields reached"
    And the field should not be added

  @limits
  Scenario: Contact card size limit
    Given my contact card is approaching the 64KB limit
    When I try to add a field that would exceed the limit
    Then I should see an error "Contact card size limit exceeded"
    And the field should not be added

  # Ordering Fields

  @ordering
  Scenario: Reorder contact fields
    Given my contact card has fields in order: "Mobile", "Email", "Twitter"
    When I drag "Twitter" to the first position
    Then my contact card fields should be in order: "Twitter", "Mobile", "Email"

  @ordering
  Scenario: Field order persists after restart
    Given I have reordered my fields to: "Twitter", "Mobile", "Email"
    When I restart the application
    Then my contact card fields should still be in order: "Twitter", "Mobile", "Email"
