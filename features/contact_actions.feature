@contact-actions
Feature: Open Contact Info in External Applications
  As a Vauchi user
  I want to tap on contact information to open it in the appropriate app
  So that I can quickly call, email, or navigate to my contacts

  Background:
    Given I have an existing identity as "Alice"
    And I am logged into Vauchi
    And I have a contact "Bob" in my contacts list

  # ============================================================
  # Phone Number Actions
  # ============================================================

  @phone @tel
  Scenario: Tap phone number opens dialer
    Given Bob has a phone field "Mobile" with value "+1-555-123-4567"
    When I view Bob's contact details
    And I tap on the phone number "+1-555-123-4567"
    Then the system dialer should open
    And the number "+1-555-123-4567" should be pre-filled

  @phone @tel
  Scenario: Phone number with international format
    Given Bob has a phone field "International" with value "+44 20 7946 0958"
    When I tap on the phone number
    Then the system dialer should open with "tel:+442079460958"

  @phone @tel
  Scenario Outline: Various phone number formats are normalized for dialer
    Given Bob has a phone field with value "<display_value>"
    When I tap on the phone number
    Then the dialer should receive URI "<expected_uri>"

    Examples:
      | display_value     | expected_uri          |
      | +1-555-123-4567   | tel:+1-555-123-4567   |
      | (555) 123-4567    | tel:(555)123-4567     |
      | 555.123.4567      | tel:555.123.4567      |
      | +44 20 7946 0958  | tel:+442079460958     |

  @phone @sms
  Scenario: Long press phone number shows action menu
    Given Bob has a phone field "Mobile" with value "+1-555-123-4567"
    When I long-press on the phone number
    Then I should see an action menu with options:
      | option           |
      | Call             |
      | Send SMS         |
      | Copy to Clipboard|

  @phone @sms
  Scenario: Send SMS to phone number
    Given Bob has a phone field "Mobile" with value "+1-555-123-4567"
    When I long-press on the phone number
    And I select "Send SMS"
    Then the system messaging app should open
    And the recipient should be "+1-555-123-4567"

  # ============================================================
  # Email Actions
  # ============================================================

  @email @mailto
  Scenario: Tap email opens mail client
    Given Bob has an email field "Work" with value "bob@company.com"
    When I view Bob's contact details
    And I tap on the email "bob@company.com"
    Then the default mail application should open
    And the "To" field should be "bob@company.com"

  @email @mailto
  Scenario: Email with special characters
    Given Bob has an email field with value "bob+work@company.com"
    When I tap on the email
    Then the mail client should open with properly encoded URI "mailto:bob+work@company.com"

  @email @mailto
  Scenario: Long press email shows action menu
    Given Bob has an email field "Work" with value "bob@company.com"
    When I long-press on the email
    Then I should see an action menu with options:
      | option           |
      | Compose Email    |
      | Copy to Clipboard|

  # ============================================================
  # Website Actions
  # ============================================================

  @website @https
  Scenario: Tap website opens browser
    Given Bob has a website field "Personal" with value "https://bobsmith.com"
    When I tap on the website
    Then the default web browser should open
    And it should navigate to "https://bobsmith.com"

  @website @https
  Scenario: Website without protocol prefix
    Given Bob has a website field with value "bobsmith.com"
    When I tap on the website
    Then the browser should open with "https://bobsmith.com"

  @website @http
  Scenario: HTTP website preserves protocol
    Given Bob has a website field with value "http://legacy-site.com"
    When I tap on the website
    Then the browser should open with "http://legacy-site.com"

  @website
  Scenario: Long press website shows action menu
    Given Bob has a website field with value "https://bobsmith.com"
    When I long-press on the website
    Then I should see an action menu with options:
      | option             |
      | Open in Browser    |
      | Copy URL           |
      | Share              |

  # ============================================================
  # Social Media Actions
  # ============================================================

  @social @profile-url
  Scenario Outline: Tap social media opens profile
    Given Bob has a social field "<network>" with value "<username>"
    And the social network registry provides profile URL template "<url_template>"
    When I tap on the social field
    Then the browser or app should open with "<expected_url>"

    Examples:
      | network   | username    | url_template                        | expected_url                         |
      | twitter   | @bobsmith   | https://twitter.com/{username}      | https://twitter.com/bobsmith         |
      | github    | bobsmith    | https://github.com/{username}       | https://github.com/bobsmith          |
      | linkedin  | in/bobsmith | https://linkedin.com/{username}     | https://linkedin.com/in/bobsmith     |
      | instagram | bob.smith   | https://instagram.com/{username}    | https://instagram.com/bob.smith      |
      | mastodon  | @bob@mas.to | https://{instance}/@{user}          | https://mas.to/@bob                  |

  @social @deep-link @android
  Scenario: Social media opens native app if installed (Android)
    Given I am using the Android app
    And the Twitter app is installed
    And Bob has a social field "twitter" with value "@bobsmith"
    When I tap on the social field
    Then the Twitter app should open to Bob's profile
    And the browser should not open

  @social @deep-link @ios
  Scenario: Social media opens native app if installed (iOS)
    Given I am using the iOS app
    And the Instagram app is installed
    And Bob has a social field "instagram" with value "bob.smith"
    When I tap on the social field
    Then the Instagram app should open to Bob's profile

  @social @fallback
  Scenario: Social media falls back to browser when app not installed
    Given the Twitter app is not installed
    And Bob has a social field "twitter" with value "@bobsmith"
    When I tap on the social field
    Then the web browser should open
    And it should navigate to "https://twitter.com/bobsmith"

  @social
  Scenario: Long press social field shows action menu
    Given Bob has a social field "github" with value "bobsmith"
    When I long-press on the social field
    Then I should see an action menu with options:
      | option             |
      | Open Profile       |
      | Copy Username      |
      | Copy Profile URL   |
      | Share              |

  # ============================================================
  # Address Actions
  # ============================================================

  @address @maps
  Scenario: Tap address opens maps application
    Given Bob has an address field "Home" with value "123 Main St, San Francisco, CA 94102"
    When I tap on the address
    Then the default maps application should open
    And it should show the location "123 Main St, San Francisco, CA 94102"

  @address @maps @android
  Scenario: Address opens Google Maps on Android
    Given I am using the Android app
    And Bob has an address field with value "123 Main St, City, ST 12345"
    When I tap on the address
    Then the intent should use "geo:0,0?q=123+Main+St,+City,+ST+12345"

  @address @maps @ios
  Scenario: Address opens Apple Maps on iOS
    Given I am using the iOS app
    And Bob has an address field with value "123 Main St, City, ST 12345"
    When I tap on the address
    Then Maps app should open with the address query

  @address @maps @desktop
  Scenario: Address opens web maps on desktop
    Given I am using the desktop app
    And Bob has an address field with value "123 Main St, City, ST 12345"
    When I tap on the address
    Then the browser should open with a maps URL
    And the address should be URL-encoded in the query

  @address
  Scenario: Long press address shows action menu
    Given Bob has an address field with value "123 Main St, City"
    When I long-press on the address
    Then I should see an action menu with options:
      | option             |
      | Open in Maps       |
      | Get Directions     |
      | Copy Address       |
      | Share              |

  @address @directions
  Scenario: Get directions to address
    Given Bob has an address field with value "123 Main St, City"
    When I long-press on the address
    And I select "Get Directions"
    Then the maps app should open in directions mode
    And the destination should be "123 Main St, City"

  # ============================================================
  # Custom Field Actions
  # ============================================================

  @custom @heuristic
  Scenario: Custom field with phone-like value offers call
    Given Bob has a custom field "Signal" with value "+1-555-987-6543"
    When I tap on the custom field
    Then the system should detect it as a phone number
    And the dialer should open with the number

  @custom @heuristic
  Scenario: Custom field with email-like value offers email
    Given Bob has a custom field "Alternate" with value "bob.alt@email.com"
    When I tap on the custom field
    Then the system should detect it as an email
    And the mail client should open

  @custom @heuristic
  Scenario: Custom field with URL-like value offers browser
    Given Bob has a custom field "Portfolio" with value "https://portfolio.bob.com"
    When I tap on the custom field
    Then the browser should open with the URL

  @custom @no-action
  Scenario: Custom field with plain text shows copy option
    Given Bob has a custom field "Notes" with value "Met at conference"
    When I tap on the custom field
    Then only the copy to clipboard action should be available

  @custom
  Scenario: Long press custom field shows contextual menu
    Given Bob has a custom field "Signal" with value "+1-555-987-6543"
    When I long-press on the custom field
    Then I should see an action menu with options:
      | option            |
      | Call              |
      | Send SMS          |
      | Copy to Clipboard |

  # ============================================================
  # Error Handling
  # ============================================================

  @error @no-handler
  Scenario: No application available for action
    Given Bob has an email field with value "bob@company.com"
    And no email application is configured on the device
    When I tap on the email
    Then I should see a message "No email app available"
    And I should be offered to copy the email to clipboard

  @error @no-handler @android
  Scenario: No handler found shows fallback on Android
    Given I am using the Android app
    And Bob has a phone field with value "+1-555-123-4567"
    And no dialer app is available
    When I tap on the phone number
    Then I should see "No app available to handle this action"
    And the number should be copied to clipboard automatically

  @error @no-handler @ios
  Scenario: canOpenURL returns false on iOS
    Given I am using the iOS app
    And Bob has a phone field with value "+1-555-123-4567"
    And the tel: scheme cannot be opened
    When I tap on the phone number
    Then I should see "Cannot open phone dialer"
    And I should be offered to copy the number

  @error @malformed
  Scenario: Malformed phone number shows error
    Given Bob has a phone field with value "not-a-valid-number"
    When I tap on the phone field
    Then I should see "Invalid phone number format"
    And the value should be copied to clipboard as fallback

  @error @network
  Scenario: Network error when opening social profile
    Given Bob has a social field "twitter" with value "@bobsmith"
    And the device has no network connectivity
    When I tap on the social field
    Then the browser should still attempt to open
    And the browser will show its own network error

  # ============================================================
  # Cross-Platform Consistency
  # ============================================================

  @platform @android
  Scenario: Android uses Intent system
    Given I am using the Android app
    When I tap on any actionable contact field
    Then an Intent with ACTION_VIEW should be created
    And the appropriate URI scheme should be used

  @platform @ios
  Scenario: iOS uses openURL environment
    Given I am using the iOS app
    When I tap on any actionable contact field
    Then openURL should be called with the appropriate URL
    And the system should handle app routing

  @platform @desktop
  Scenario: Desktop uses Tauri opener plugin
    Given I am using the desktop app
    When I tap on any actionable contact field
    Then the Tauri opener plugin should be invoked
    And the system default application should open

  @platform @cli
  Scenario: CLI uses open crate
    Given I am using the CLI
    And I view Bob's contact details
    When I select to open a contact field
    Then the open crate should launch the system handler

  @platform @tui
  Scenario: TUI offers to open in external app
    Given I am using the TUI
    And I view Bob's contact details
    When I press Enter on a contact field
    Then I should see options to open or copy
    And selecting open should use the system handler

  # ============================================================
  # Accessibility
  # ============================================================

  @a11y
  Scenario: Actionable fields are announced as buttons
    Given Bob has multiple contact fields
    When a screen reader focuses on an email field
    Then it should announce "bob@company.com, email, button"
    And it should indicate the action "double tap to compose email"

  @a11y
  Scenario: Action menu is accessible
    Given Bob has a phone field
    When I activate the context menu via accessibility action
    Then the action menu should be announced
    And each option should be focusable and labeled

  @a11y @keyboard
  Scenario: Keyboard navigation on desktop
    Given I am using the desktop app with keyboard
    And I am viewing Bob's contact details
    When I press Tab to navigate to a phone field
    And I press Enter
    Then the dialer should open
    And focus should return to the app when closed

  # ============================================================
  # Visual Feedback
  # ============================================================

  @ui
  Scenario: Actionable fields show tap affordance
    Given Bob has multiple contact fields
    When I view Bob's contact details
    Then actionable fields should have a subtle indicator
    And they should show hover/press feedback on interaction

  @ui
  Scenario: Brief loading indicator when opening external app
    Given Bob has an email field
    When I tap on the email
    Then a brief loading indicator should appear
    And it should dismiss when the external app opens

  @ui
  Scenario: Confirmation toast after copy to clipboard
    Given Bob has a phone field
    When I long-press and select "Copy to Clipboard"
    Then I should see a toast "Copied to clipboard"
    And the toast should auto-dismiss after 2 seconds

  # ============================================================
  # Security
  # ============================================================

  @security
  Scenario: URLs are validated before opening
    Given Bob has a website field with value "javascript:alert(1)"
    When I tap on the website
    Then the URL should be rejected as invalid
    And the browser should not open
    And I should see "Invalid URL"

  @security
  Scenario: Only safe URI schemes are allowed
    Given Bob has a custom field with value "file:///etc/passwd"
    When I tap on the field
    Then the file: scheme should be blocked
    And I should see "This action is not supported"

  @security
  Scenario: Allowed URI schemes whitelist
    Then the following URI schemes should be allowed:
      | scheme  | purpose              |
      | tel     | Phone calls          |
      | mailto  | Email composition    |
      | sms     | Text messages        |
      | https   | Secure websites      |
      | http    | Websites             |
      | geo     | Map coordinates      |
    And all other schemes should be blocked by default
