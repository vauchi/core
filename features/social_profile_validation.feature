@social @validation @trust
Feature: Social Profile Validation
  As a WebBook user
  I want to verify that social profiles belong to my contacts
  So that I can trust the social links they share are authentic

  Background:
    Given I have an existing identity as "Alice"
    And I have a contact "Bob" in my contacts
    And Bob has a social field for "twitter" with value "@bob_smith"

  # Viewing Validation Status

  @view-status
  Scenario: View unvalidated social profile
    Given no one has validated Bob's Twitter profile
    When I view Bob's contact card
    Then the Twitter field should show validation score 0
    And the field should be marked as "unverified"

  @view-status
  Scenario: View partially validated social profile
    Given 2 contacts have validated Bob's Twitter profile
    When I view Bob's contact card
    Then the Twitter field should show validation score 2
    And the field should be marked as "partially verified"

  @view-status
  Scenario: View highly validated social profile
    Given 5 or more contacts have validated Bob's Twitter profile
    When I view Bob's contact card
    Then the Twitter field should show validation score 5+
    And the field should be marked as "verified"

  # Validating Social Profiles

  @validate
  Scenario: Validate a contact's social profile
    Given I am viewing Bob's contact card
    And Bob has a Twitter profile "@bob_smith"
    When I tap "Verify" on the Twitter field
    And I confirm "I recognize this as Bob's Twitter account"
    Then my validation should be recorded
    And Bob's Twitter validation score should increase by 1
    And Bob should be notified that I validated their profile

  @validate
  Scenario: Cannot validate own social profile
    Given I have a Twitter field "@alice_wonder"
    When I view my own contact card
    Then I should not see a "Verify" option on my Twitter field

  @validate
  Scenario: Cannot validate same profile twice
    Given I have already validated Bob's Twitter profile
    When I view Bob's contact card
    Then the Twitter field should show "You verified this"
    And the "Verify" button should be disabled

  @validate
  Scenario: Revoke validation
    Given I have validated Bob's Twitter profile
    When I tap "Revoke verification" on the Twitter field
    And I confirm the revocation
    Then my validation should be removed
    And Bob's Twitter validation score should decrease by 1

  # Validation Propagation

  @propagation
  Scenario: Validation is stored locally
    Given I validate Bob's Twitter profile
    Then the validation should be stored in my local database
    And the validation should be linked to Bob's contact ID
    And the validation should include my signature

  @propagation
  Scenario: Validation count syncs from contacts
    Given Bob has 3 validations for his Twitter profile
    When Bob updates his contact card
    Then I should receive the updated validation count
    And I should see "3 people verified this"

  @propagation
  Scenario: Validation details are privacy-preserving
    Given Bob has 3 validations for his Twitter profile
    When I view the validation details
    Then I should see the count of validations
    But I should NOT see who validated (unless they are my contacts)

  # Trust Levels

  @trust-levels
  Scenario Outline: Validation score determines trust level
    Given Bob's Twitter profile has <count> validations
    When I view Bob's contact card
    Then the Twitter field should show trust level "<level>"
    And the visual indicator should be "<indicator>"

    Examples:
      | count | level              | indicator    |
      | 0     | unverified         | grey         |
      | 1     | low confidence     | yellow       |
      | 2-4   | partial confidence | light green  |
      | 5+    | high confidence    | green        |

  @trust-levels
  Scenario: Trust level considers validator relationship
    Given Bob's Twitter profile has 2 validations
    And one validator is my direct contact "Carol"
    When I view Bob's contact card
    Then the Twitter field should show "Verified by Carol and 1 other"
    And validations from my contacts should be weighted higher

  # Multiple Social Profiles

  @multiple
  Scenario: Each social field has independent validation
    Given Bob has Twitter "@bob_smith" with 3 validations
    And Bob has GitHub "bobsmith" with 1 validation
    When I view Bob's contact card
    Then Twitter should show validation score 3
    And GitHub should show validation score 1
    And each field should have its own verify button

  @multiple
  Scenario: Validate multiple profiles for same contact
    Given Bob has Twitter and GitHub profiles
    When I validate Bob's Twitter profile
    And I validate Bob's GitHub profile
    Then both validations should be recorded separately
    And both profiles should show my verification

  # Validation on Profile Changes

  @profile-change
  Scenario: Validation resets when profile value changes
    Given Bob's Twitter "@bob_smith" has 5 validations
    When Bob changes his Twitter to "@bob_new_handle"
    Then the validation count should reset to 0
    And previous validators should be notified of the change
    And they should be prompted to re-verify

  @profile-change
  Scenario: Validation persists when other fields change
    Given Bob's Twitter "@bob_smith" has 5 validations
    When Bob updates his email address
    Then Bob's Twitter validation count should remain 5

  # Edge Cases

  @edge-cases
  Scenario: Validation for contact with no social profiles
    Given Bob has no social fields
    When I view Bob's contact card
    Then I should not see any validation options

  @edge-cases
  Scenario: New contact inherits existing validations
    Given Bob has Twitter with 3 validations from others
    When I add Bob as a new contact
    Then I should see Bob's Twitter with validation score 3
    And I should be able to add my own validation

  @edge-cases
  Scenario: Blocked contact's validation is ignored
    Given I have blocked "Mallory"
    And Mallory has validated Bob's Twitter profile
    When calculating Bob's Twitter validation score for me
    Then Mallory's validation should not be counted

  # Validation Incentives

  @incentives
  Scenario: View my validation contributions
    Given I have validated 10 social profiles for various contacts
    When I view my validation history
    Then I should see a list of profiles I've validated
    And I should see when I validated each one

  @incentives
  Scenario: Receive notification when validation is appreciated
    Given I validated Bob's Twitter profile
    When a new contact adds Bob and sees my validation
    Then I may receive a "validation was helpful" signal
    And my reputation as a reliable validator may increase

  # Security

  @security
  Scenario: Validations are cryptographically signed
    Given I validate Bob's Twitter profile
    Then my validation should be signed with my identity key
    And Bob should be able to verify the signature
    And tampering with the validation should be detectable

  @security
  Scenario: Cannot forge validations
    Given an attacker tries to create fake validations
    Then the system should reject unsigned validations
    And the system should reject validations with invalid signatures

  @security
  Scenario: Sybil attack resistance
    Given an attacker creates multiple fake identities
    And they all validate a malicious profile
    When I view the validation score
    Then validations from non-contacts should be weighted less
    And validations from verified exchange contacts should be weighted more
