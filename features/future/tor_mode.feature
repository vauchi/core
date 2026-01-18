@privacy @tor @network @opt-in
Feature: Tor Mode
  As a privacy-conscious Vauchi user
  I want to route my relay connections through Tor
  So that my IP address is hidden from relay operators and network observers

  Note: This is an OPT-IN feature. Tor mode is disabled by default.
  Users must explicitly enable it in Privacy settings.

  Background:
    Given I have an existing identity
    And I have contacts who may send me updates

  # Opt-in Default State

  @opt-in @default
  Scenario: Tor mode is disabled by default
    Given I have just installed the app
    When I check Privacy settings
    Then Tor mode should be OFF
    And connections should use direct networking
    And no Tor components should be loaded

  @opt-in @default
  Scenario: Tor mode not mentioned in basic onboarding
    Given I am going through initial app setup
    Then Tor mode should not be part of basic setup
    And it should only be available in advanced Privacy settings
    And new users should not be confused by Tor options

  # Enabling Tor Mode

  @settings
  Scenario: Enable Tor mode from settings
    Given Tor mode is disabled
    When I navigate to Privacy settings
    And I enable "Tor mode"
    Then Tor mode should be activated
    And I should see a confirmation "Connections will be routed through Tor"
    And the Tor circuit should be established

  @settings
  Scenario: Disable Tor mode
    Given Tor mode is enabled
    When I disable "Tor mode"
    Then Tor mode should be deactivated
    And connections should use direct networking
    And I should see a warning about reduced privacy

  @settings
  Scenario: Tor mode persists across app restarts
    Given Tor mode is enabled
    When I close and reopen the app
    Then Tor mode should still be enabled
    And Tor circuit should be re-established automatically

  # Connection Behavior

  @connection
  Scenario: Relay connections use Tor when enabled
    Given Tor mode is enabled
    And a Tor circuit is established
    When I sync with the relay server
    Then the connection should go through Tor
    And the relay should see a Tor exit node IP, not my real IP

  @connection
  Scenario: Connect to relay .onion address
    Given Tor mode is enabled
    And the relay provides a .onion address
    When I sync with the relay
    Then I should connect to the .onion address
    And the connection should stay within the Tor network
    And no exit node should be used

  @connection
  Scenario: Fallback to clearnet relay if .onion unavailable
    Given Tor mode is enabled
    And the relay's .onion address is unreachable
    When I sync with the relay
    Then I should connect via Tor to the clearnet address
    And I should see a notice "Using Tor exit node (no .onion available)"

  @connection
  Scenario: Connection fails gracefully without Tor
    Given Tor mode is enabled
    And Tor network is unreachable
    When I try to sync
    Then the sync should fail
    And I should see "Cannot connect to Tor network"
    And no connection should be made without Tor

  # Circuit Management

  @circuit
  Scenario: Establish new Tor circuit
    Given Tor mode is enabled
    When I request a new circuit
    Then a new Tor circuit should be established
    And I should get a new exit node IP
    And existing connections should be migrated

  @circuit
  Scenario: Automatic circuit rotation
    Given Tor mode is enabled
    And a circuit has been active for 10 minutes
    Then a new circuit should be established automatically
    And the old circuit should be closed gracefully

  @circuit
  Scenario: View current circuit info
    Given Tor mode is enabled
    And a circuit is established
    When I view Tor status
    Then I should see the number of hops
    And I should see approximate latency
    And I should NOT see exit node IP (privacy)

  # Performance

  @performance
  Scenario: User warned about Tor latency
    Given Tor mode is disabled
    When I try to enable Tor mode
    Then I should see a notice "Tor adds latency to connections"
    And I should be asked to confirm

  @performance
  Scenario: Sync works despite Tor latency
    Given Tor mode is enabled
    When I sync with the relay
    Then sync should complete successfully
    And timeout should be extended for Tor latency

  @performance
  Scenario: Background sync respects Tor mode
    Given Tor mode is enabled
    And background sync is configured
    When background sync triggers
    Then it should use Tor for the connection
    And battery usage may be higher due to Tor

  # Privacy Guarantees

  @privacy-guarantee
  Scenario: Real IP never leaked when Tor enabled
    Given Tor mode is enabled
    When any network operation occurs
    Then my real IP should never be sent to any server
    And DNS queries should go through Tor
    And WebRTC should be disabled

  @privacy-guarantee
  Scenario: Tor mode does not affect E2E encryption
    Given Tor mode is enabled
    When I receive an update from a contact
    Then the update should still be E2E encrypted
    And Tor provides transport privacy, not content privacy
    And decryption should work normally

  @privacy-guarantee
  Scenario: Local operations work without Tor
    Given Tor mode is enabled
    But Tor network is unavailable
    When I view my contacts locally
    Then local data should be accessible
    And only sync operations should be blocked

  # Bootstrap and Onboarding

  @onboarding
  Scenario: Tor mode option shown during setup
    Given I am setting up a new identity
    When I reach the privacy options step
    Then I should see an option to enable Tor mode
    And the option should explain benefits and tradeoffs
    And I should be able to skip and configure later

  @bootstrap
  Scenario: Tor bootstrap progress shown
    Given Tor mode is enabled
    And Tor is not yet connected
    When the app starts
    Then I should see Tor bootstrap progress
    And I should see percentage complete
    And I should be able to use local features while bootstrapping

  @bootstrap
  Scenario: Tor bootstrap failure handling
    Given Tor mode is enabled
    And Tor cannot connect (network blocked)
    When bootstrap times out
    Then I should see "Tor connection failed"
    And I should be offered options:
      | option              | description                    |
      | Retry               | Try connecting again           |
      | Use bridges         | Configure Tor bridges          |
      | Disable Tor         | Fall back to direct connection |

  # Bridges (for censored networks)

  @bridges
  Scenario: Configure Tor bridges
    Given Tor mode is enabled
    And direct Tor access is blocked
    When I configure bridge addresses
    Then bridges should be used for Tor connection
    And I should be able to connect despite blocking

  @bridges
  Scenario: Request bridges from BridgeDB
    Given Tor mode is enabled
    And I don't have bridge addresses
    When I request bridges
    Then the app should help me get bridges from BridgeDB
    And bridges should be saved for future use

  @bridges
  Scenario: Pluggable transports support
    Given Tor is blocked in my region
    When I configure obfs4 bridges
    Then traffic should be obfuscated
    And it should look like regular HTTPS traffic

  # Status Indicators

  @status
  Scenario: Tor status indicator in app
    Given Tor mode is enabled
    Then I should see a Tor status indicator
    And the indicator should show:
      | state        | indicator |
      | Connecting   | Yellow    |
      | Connected    | Green     |
      | Disconnected | Red       |

  @status
  Scenario: Tor status in sync screen
    Given Tor mode is enabled
    When I view the sync status
    Then I should see "Syncing via Tor"
    And I should see current circuit age

  # Edge Cases

  @edge-cases
  Scenario: Tor mode with no network
    Given Tor mode is enabled
    And device has no network connectivity
    When I try to sync
    Then I should see "No network connection"
    And Tor should not be blamed for the failure

  @edge-cases
  Scenario: Switch networks while Tor active
    Given Tor mode is enabled
    And I am connected via WiFi
    When I switch to mobile data
    Then Tor circuit should be re-established
    And sync should resume automatically

  @edge-cases
  Scenario: Tor mode and battery saver
    Given Tor mode is enabled
    And device enters battery saver mode
    Then Tor connections should be maintained
    But circuit rotation frequency may decrease
    And background sync may be delayed
