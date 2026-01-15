@sync @updates
Feature: Sync and Updates
  As a WebBook user
  I want my contact card changes to automatically sync to my contacts
  And receive updates when my contacts change their information
  So that contact information stays current without manual effort

  Background:
    Given I have an existing identity as "Alice"
    And I have contacts "Bob", "Carol", and "Dave" in my contact list
    And the sync service is running

  # Update Propagation

  @propagation
  Scenario: Update propagates to online contacts
    Given Bob is online and connected
    And Bob can see my phone field
    When I change my phone number from "555-1111" to "555-2222"
    Then Bob should receive the update within 30 seconds
    And Bob's view of my contact card should show "555-2222"

  @propagation
  Scenario: Update queued for offline contacts
    Given Carol is offline
    And Carol can see my email field
    When I change my email to "newemail@example.com"
    Then the update should be queued for Carol
    And my local sync state should show "pending for Carol"

  @propagation
  Scenario: Queued updates delivered when contact comes online
    Given there is a pending update for Carol
    When Carol comes online
    Then Carol should receive the queued update
    And my sync state should show Carol as "synced"

  @propagation
  Scenario: Multiple updates coalesced for offline contact
    Given Carol is offline
    When I make 5 changes to my contact card
    And Carol comes online
    Then Carol should receive one combined update
    And all 5 changes should be reflected

  # Receiving Updates

  @receive
  Scenario: Receive contact card update
    Given Bob has my contact card
    When Bob updates his phone number
    And I am online
    Then I should receive Bob's update
    And my view of Bob's contact should show the new phone number
    And I should see a notification "Bob updated their contact info"

  @receive
  Scenario: Receive update while offline
    Given I am offline
    And Bob updates his phone number
    When I come back online
    Then I should receive Bob's update
    And my contacts should be current

  @receive
  Scenario: Update only visible fields
    Given Bob has updated a field I cannot see
    When sync occurs
    Then I should not receive any update from Bob
    And my view of Bob should remain unchanged

  # P2P Sync Network

  @p2p
  Scenario: Direct P2P sync when both online
    Given Bob and I are both online
    And we can establish a direct connection
    When I update my contact card
    Then the update should be sent directly to Bob
    And no relay should be used

  @p2p
  Scenario: DHT-based discovery
    Given Bob's IP address has changed
    When I try to sync with Bob
    Then I should discover Bob's new address via DHT
    And sync should succeed

  @p2p
  Scenario: Relay fallback when direct connection fails
    Given Bob is behind a restrictive NAT
    And direct P2P connection fails
    When I update my contact card
    Then the update should be sent via relay node
    And Bob should receive the update
    And the update should remain end-to-end encrypted

  # Conflict Resolution

  @conflict
  Scenario: Last-write-wins for single field
    Given I have synced my contact card to Device A and Device B
    When I update my phone on Device A at time T1
    And I update my phone on Device B at time T2 (T2 > T1)
    Then the phone value from Device B should win
    And both devices should converge to the same value

  @conflict
  Scenario: Concurrent updates to different fields
    Given I have synced my contact card to Device A and Device B
    When I update my phone on Device A
    And I update my email on Device B simultaneously
    Then both updates should be preserved
    And both devices should have the new phone and email

  @conflict
  Scenario: CRDT merge for complex changes
    Given I have made offline changes on Device A
    And I have made different offline changes on Device B
    When both devices come online
    Then changes should be merged using CRDT rules
    And no data should be lost
    And both devices should converge to the same state

  # Sync Status

  @status
  Scenario: View sync status for all contacts
    When I open the sync status screen
    Then I should see a list of all contacts
    And each contact should show "synced" or "pending"
    And pending contacts should show number of queued updates

  @status
  Scenario: View detailed sync status for a contact
    Given I have pending updates for Carol
    When I view Carol's sync details
    Then I should see when the last successful sync was
    And I should see how many updates are pending
    And I should see the reason for pending status (offline/unreachable)

  @status
  Scenario: Manual sync trigger
    Given sync is paused or delayed
    When I tap "Sync Now"
    Then immediate sync attempt should be made
    And sync status should update

  # Sync Reliability

  @reliability
  Scenario: Retry failed sync with exponential backoff
    Given a sync attempt to Bob fails
    Then retry should be scheduled in 2 seconds
    And if that fails, retry in 4 seconds
    And if that fails, retry in 8 seconds
    And maximum retry interval should be 5 minutes

  @reliability
  Scenario: Sync survives app restart
    Given I have pending updates to send
    When I restart the application
    Then pending updates should be preserved
    And sync should resume automatically

  @reliability
  Scenario: Sync survives device reboot
    Given I have pending updates to send
    When my device reboots
    And I launch WebBook
    Then pending updates should still be queued
    And sync should resume

  # Bandwidth and Efficiency

  @efficiency
  Scenario: Only changed fields transmitted
    Given I have a contact card with 10 fields
    When I update only my phone number
    Then only the phone field delta should be transmitted
    And the full contact card should not be sent

  @efficiency
  Scenario: Merkle tree for efficient sync detection
    Given Bob and I have been synced
    When we reconnect after a period offline
    Then Merkle tree comparison should detect changes
    And only changed data should be transmitted

  @efficiency
  Scenario: Compression of sync payloads
    Given I am sending an update
    Then the payload should be compressed before encryption
    And the encrypted payload should be as small as possible

  # Sync Settings

  @settings
  Scenario: Configure sync over WiFi only
    Given I have enabled "WiFi only" sync
    When I am on cellular data
    Then sync should not run
    And updates should be queued
    And I should see "Sync paused - waiting for WiFi"

  @settings
  Scenario: Disable background sync
    Given I have disabled background sync
    When the app is in the background
    Then no sync should occur
    And sync should only run when app is open

  @settings
  Scenario: Configure sync frequency
    Given I have set sync frequency to "every hour"
    Then sync should run at most once per hour
    And manual sync should always work

  # Security in Sync

  @security
  Scenario: All sync traffic is encrypted
    Given I am syncing with Bob
    When updates are transmitted
    Then all payloads should be encrypted with our shared key
    And relay nodes should only see encrypted blobs

  @security
  Scenario: Verify update signatures
    Given I receive an update claiming to be from Bob
    Then I should verify the update signature with Bob's public key
    And unsigned updates should be rejected
    And updates signed by wrong key should be rejected

  @security
  Scenario: Reject replay attacks
    Given an attacker captures an old update from Bob
    When the attacker replays it to me
    Then I should detect the replay via timestamp/nonce
    And the replayed update should be rejected

  # Multi-device Sync

  @multi-device
  Scenario: Sync my own contact card across my devices
    Given I have Device A and Device B linked to my identity
    When I update my contact card on Device A
    Then Device B should receive the update
    And my contact card should be identical on both devices

  @multi-device
  Scenario: Contact updates reach all my devices
    Given I have Device A and Device B linked
    And Bob updates his contact card
    Then both Device A and Device B should receive Bob's update
    And my view of Bob should be consistent across devices

  @multi-device
  Scenario: New device receives full state
    Given I have an existing contact card and contacts on Device A
    When I link new Device C
    Then Device C should receive my full contact card
    And Device C should receive all my contacts
    And Device C should be fully synced

  # Edge Cases

  @edge-case
  Scenario: Handle contact deletion during sync
    Given Bob deletes me from his contacts
    When I try to sync an update to Bob
    Then Bob should not receive my updates
    And I should eventually be notified that Bob removed me

  @edge-case
  Scenario: Handle identity key change
    Given Bob has rotated his identity keys
    When I try to sync with Bob
    Then I should detect the key mismatch
    And I should be prompted to re-verify Bob
    And sync should pause until verified

  @edge-case
  Scenario: Large sync queue handling
    Given I have been offline for a long time
    And 100 contacts have sent updates
    When I come online
    Then updates should be processed in batches
    And UI should remain responsive
    And progress should be shown
