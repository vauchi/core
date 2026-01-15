# WebBook CLI

Command-line interface for WebBook - privacy-focused contact card exchange.

## Installation

```bash
cargo build -p webbook-cli --release
```

The binary will be at `target/release/webbook`.

## Usage

### Initialize Identity

Create a new identity with your display name:

```bash
webbook init "Your Name"
```

This creates your cryptographic identity and stores it locally.

### Manage Your Contact Card

```bash
# Show your contact card
webbook card show

# Add fields to your card
webbook card add email work "alice@company.com"
webbook card add phone mobile "+1-555-123-4567"
webbook card add social twitter "@alice"
webbook card add website personal "https://alice.dev"

# Edit a field
webbook card edit work "alice@newcompany.com"

# Remove a field
webbook card remove work
```

### Exchange Contacts

Exchange contacts with someone in person using QR codes:

```bash
# Generate your exchange QR code
webbook exchange start

# Complete exchange with someone else's QR data
webbook exchange complete "wb://..."
```

### Manage Contacts

```bash
# List all contacts
webbook contacts list

# Show contact details
webbook contacts show "contact-id"

# Search contacts by name
webbook contacts search "alice"

# Verify a contact's fingerprint
webbook contacts verify "contact-id"

# Remove a contact
webbook contacts remove "contact-id"
```

### Sync with Relay

Synchronize with the relay server to receive pending messages:

```bash
webbook sync
```

### Backup and Restore

```bash
# Export identity backup (encrypted)
webbook export backup.webbook

# Import from backup
webbook import backup.webbook
```

## Global Options

```bash
# Specify custom data directory (default: ~/.local/share/webbook)
webbook --data-dir /path/to/data <command>

# Specify relay server (default: ws://localhost:8080)
webbook --relay ws://relay.example.com:8080 <command>
```

## End-to-End Exchange Flow

1. **Alice** generates a QR code: `webbook exchange start`
2. **Bob** scans and completes: `webbook exchange complete "wb://..."`
   - Bob adds Alice as a contact (initially shows as "New Contact")
   - Bob sends his name to Alice via the relay
3. **Alice** syncs: `webbook sync`
   - Receives Bob's exchange request
   - Adds Bob as a contact with his display name
   - Sends her name back to Bob (bidirectional exchange)
4. **Bob** syncs: `webbook sync`
   - Receives Alice's response
   - Updates contact from "New Contact" to "Alice"
5. Both now see each other's actual display names

## Architecture

The CLI uses `webbook-core` for all cryptographic operations and data management. Communication with the relay server uses WebSocket with a simple JSON protocol.

```
webbook-cli/
├── src/
│   ├── main.rs          # CLI entry point and command routing
│   ├── commands/        # Command implementations
│   │   ├── init.rs      # Identity creation
│   │   ├── card.rs      # Contact card management
│   │   ├── contacts.rs  # Contact list management
│   │   ├── exchange.rs  # QR exchange flow
│   │   ├── sync.rs      # Relay synchronization
│   │   └── backup.rs    # Export/import
│   ├── config.rs        # CLI configuration
│   ├── display.rs       # Terminal output formatting
│   └── protocol.rs      # Wire protocol for relay
```

## License

MIT
