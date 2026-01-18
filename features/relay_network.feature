@relay @infrastructure
Feature: Relay Network
  As a Vauchi user or contributor
  I want a decentralized relay network for message delivery
  So that updates can be delivered even when direct P2P connection fails

  Background:
    Given the relay network is operational
    And there are volunteer-run relay nodes available

  # Relay Usage

  @usage
  Scenario: Automatic fallback to relay
    Given I am trying to send an update to Bob
    And direct P2P connection to Bob fails
    When the system detects connection failure
    Then it should automatically try relay nodes
    And the update should be delivered via relay
    And I should see "Delivered via relay"

  @usage
  Scenario: Direct connection preferred
    Given Bob and I can establish direct P2P connection
    When I send an update to Bob
    Then the update should go directly to Bob
    And relay nodes should not be used
    And latency should be minimal

  @usage
  Scenario: Relay stores messages for offline contacts
    Given Carol is offline
    When I send an update to Carol
    Then the relay should store the encrypted update
    And the update should be delivered when Carol comes online
    And the relay should delete the blob after delivery

  @usage
  Scenario: Relay blob expiration
    Given I sent an update to Dave via relay
    And Dave has been offline for 7 days
    Then the relay should expire the update blob
    And my client should be notified of failed delivery
    And I should be prompted to retry later

  # Privacy in Relay

  @privacy
  Scenario: Relay only sees encrypted blobs
    Given I am sending data through a relay
    Then the relay should only receive encrypted blobs
    And the relay should have no decryption keys
    And the relay should learn nothing about content

  @privacy
  Scenario: Relay cannot identify users
    Given I am using a relay
    Then the relay should not require user accounts
    And the relay should not log user identities
    And routing should use anonymous identifiers

  @privacy
  Scenario: Relay cannot correlate sender and recipient
    Given I send an update via relay
    Then the relay should not know I am the sender
    And the relay should not know who the recipient is
    And metadata should be minimized

  @privacy
  Scenario: Tor support for relay access
    Given I want maximum privacy
    When I enable Tor mode
    Then relay connections should go through Tor
    And relay nodes should offer .onion addresses
    And my IP should be hidden from relays

  # Running a Relay Node

  @contribute
  Scenario: Deploy relay node with Docker
    Given I want to contribute a relay node
    When I run the Docker image
    Then a relay node should start
    And it should join the relay network
    And it should begin accepting connections

  @contribute
  Scenario: Relay node configuration
    Given I am setting up a relay node
    When I configure the node
    Then I should be able to set bandwidth limits
    And I should be able to set storage limits
    And I should be able to set geographic restrictions

  @contribute
  Scenario: Relay node monitoring
    Given I am running a relay node
    When I view the monitoring dashboard
    Then I should see bandwidth usage
    And I should see number of blobs stored
    And I should see uptime statistics
    And I should NOT see any user data

  @contribute
  Scenario: Relay node health check
    Given a relay node is running
    Then it should respond to health check probes
    And unhealthy nodes should be removed from network
    And the network should route around failed nodes

  # Contribution Model

  @contribution
  Scenario: Prompt to contribute after using relays
    Given I have used relay bandwidth significantly
    When the app detects high relay usage
    Then I should see a non-intrusive suggestion to contribute
    And I should be able to dismiss the suggestion
    And dismissing should not affect functionality

  @contribution
  Scenario: View relay contribution options
    When I view the contribution screen
    Then I should see options to run a relay node
    And I should see option to donate to relay operators
    And I should see current network health
    And contribution should always be voluntary

  @contribution
  Scenario: No special privileges for contributors
    Given I am running a relay node
    Then I should not get priority routing
    And I should not get extra storage
    And all users should be treated equally

  # Relay Network Health

  @health
  Scenario: Multiple relay nodes for redundancy
    Given the relay network has 100 nodes
    When 10 nodes go offline
    Then messages should still be deliverable
    And remaining nodes should handle increased load
    And network should remain functional

  @health
  Scenario: Geographic distribution of relays
    Given users are distributed globally
    Then relay nodes should be distributed globally
    And users should connect to nearby relays
    And latency should be minimized

  @health
  Scenario: Relay node discovery via DHT
    Given I need to find a relay node
    When I query the DHT
    Then I should discover available relay nodes
    And nodes should be ranked by latency/availability
    And I should connect to the best available

  # Abuse Prevention

  @abuse
  Scenario: Rate limiting on relay nodes
    Given a user is sending excessive traffic
    Then the relay should rate limit them
    And legitimate traffic should not be affected
    And the rate limit should be per-anonymous-identifier

  @abuse
  Scenario: Storage limits per blob
    Given a user tries to store a very large blob
    Then the relay should reject oversized blobs
    And the maximum blob size should be enforced
    And storage should be fairly distributed

  @abuse
  Scenario: Automatic cleanup of stale blobs
    Given blobs have been stored on a relay
    When 7 days pass without retrieval
    Then stale blobs should be automatically deleted
    And storage should be reclaimed
    And the sender should be notified

  @abuse
  Scenario: Sybil attack resistance
    Given an attacker tries to flood the network
    Then proof-of-work may be required
    And rate limits should prevent flooding
    And the network should remain functional

  # Relay Protocol

  @protocol
  Scenario: Relay protocol versioning
    Given relays support protocol version 1.0
    When version 1.1 is released
    Then relays should support both versions
    And clients should negotiate version
    And upgrade path should be smooth

  @protocol
  Scenario: Relay node authentication
    Given I am connecting to a relay
    Then the relay should authenticate with a certificate
    And I should verify the relay's identity
    And connection should be encrypted (TLS)

  @protocol
  Scenario: Relay gossip protocol
    Given relay nodes are in the network
    Then they should gossip routing information
    And network state should converge
    And new nodes should discover the network

  # Fallback Behavior

  @fallback
  Scenario: No relays available
    Given all relay nodes are unreachable
    When I try to send an update
    Then the update should be queued locally
    And I should be notified "Updates will send when network available"
    And retry should occur periodically

  @fallback
  Scenario: Partial relay network failure
    Given half the relay nodes are down
    When I send an update
    Then remaining relays should handle the load
    And delivery should succeed with higher latency
    And user experience should remain acceptable

  # User Preferences

  @preferences
  Scenario: Opt out of relay usage
    Given I want direct P2P only
    When I disable relay usage
    Then my updates should only go via direct connection
    And I should be warned about potential delivery failures
    And the setting should be respected

  @preferences
  Scenario: Prefer specific relay nodes
    Given I have trusted relay nodes
    When I configure preferred relays
    Then those relays should be used first
    And fallback to other relays should occur if needed

  @preferences
  Scenario: Block specific relay nodes
    Given I don't trust certain relay nodes
    When I block those relays
    Then my traffic should avoid those nodes
    And delivery should use alternative routes

  # Relay Federation - Message Offloading

  @federation
  Scenario: Relay detects high storage usage
    Given a relay node has storage limits configured
    When stored blobs exceed 80% of capacity
    Then the relay should enter "offload mode"
    And it should attempt to redistribute oldest blobs to peer relays
    And new incoming blobs should still be accepted

  @federation
  Scenario: Relay offloads messages to peer relays
    Given relay A is at 85% storage capacity
    And relay A knows about peer relays B and C
    When relay A initiates offloading
    Then it should transfer some blobs to relay B or C
    And transferred blobs should be removed from relay A
    And relay A should record where blobs were sent

  @federation
  Scenario: Peer relay discovery
    Given a relay node starts up
    When it queries the relay registry
    Then it should discover other relay nodes
    And it should establish peer connections
    And it should exchange capacity information periodically

  @federation
  Scenario: Relay registry for peer discovery
    Given multiple relay nodes exist
    Then there should be a registry of known relays
    And the registry should include relay capacity status
    And relays should self-register on startup
    And the registry should remove unresponsive relays

  @federation
  Scenario: Client queries multiple relays for messages
    Given my messages may be on relay A or B
    When I sync for pending messages
    Then the client should query all known relays
    And messages should be collected from all sources
    And duplicate messages should be deduplicated

  @federation
  Scenario: Relay includes forwarding hints
    Given relay A has offloaded blob X to relay B
    When the recipient queries relay A for messages
    Then relay A should respond with "blob X is at relay B"
    And the client should fetch blob X from relay B
    And the hint should be temporary (TTL-based)

  @federation
  Scenario: Offloaded messages preserve TTL
    Given a blob has 30 days remaining TTL
    When it is offloaded to another relay
    Then the new relay should honor the original expiration
    And the blob should not get extra lifetime from transfer
    And TTL should be included in transfer metadata

  @federation
  Scenario: Relay refuses offload when at capacity
    Given relay B is at 95% capacity
    When relay A tries to offload blobs to relay B
    Then relay B should refuse the transfer
    And relay A should try other peers
    And relay A should mark relay B as "full" temporarily

  @federation
  Scenario: Graceful handling of relay shutdown
    Given relay A is shutting down for maintenance
    When it initiates graceful shutdown
    Then it should offload all stored blobs to peers
    And it should deregister from the relay registry
    And clients should be directed to other relays

  @federation
  Scenario: Load balancing across relay network
    Given the relay network has varying capacity
    When a new blob needs to be stored
    Then it should be routed to a relay with available capacity
    And heavily loaded relays should receive less traffic
    And the network should self-balance over time

  # Federation Security

  @federation @security
  Scenario: Only authorized relays can join federation
    Given the relay federation uses mutual TLS
    When an unknown relay tries to join
    Then it should be rejected unless it has valid credentials
    And federation traffic should be encrypted
    And relay identity should be verified

  @federation @security
  Scenario: Blob integrity preserved during transfer
    Given a blob is being transferred between relays
    Then the blob should be transferred with its signature
    And the receiving relay should verify integrity
    And corrupted blobs should be rejected

  @federation @security
  Scenario: Relay cannot read offloaded blobs
    Given relay A offloads encrypted blobs to relay B
    Then relay B receives the same encrypted data
    And neither relay can decrypt the content
    And the E2E encryption is preserved
