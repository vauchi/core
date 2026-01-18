# Vauchi CLI

Command-line interface for Vauchi - privacy-focused contact card exchange.

## Installation

```bash
cargo build -p vauchi-cli --release
```

The binary will be at `target/release/vauchi`.

## Usage

### Initialize Identity

Create a new identity with your display name:

```bash
vauchi init "Your Name"
```

This creates your cryptographic identity and stores it locally.

### Manage Your Contact Card

```bash
# Show your contact card
vauchi card show

# Add fields to your card
vauchi card add email work "alice@company.com"
vauchi card add phone mobile "+1-555-123-4567"
vauchi card add social twitter "@alice"
vauchi card add website personal "https://alice.dev"

# Edit a field
vauchi card edit work "alice@newcompany.com"

# Remove a field
vauchi card remove work
```

### Exchange Contacts

Exchange contacts with someone in person using QR codes:

```bash
# Generate your exchange QR code
vauchi exchange start

# Complete exchange with someone else's QR data
vauchi exchange complete "wb://..."
```

### Manage Contacts

```bash
# List all contacts
vauchi contacts list

# Show contact details
vauchi contacts show "contact-id"

# Search contacts by name
vauchi contacts search "alice"

# Verify a contact's fingerprint
vauchi contacts verify "contact-id"

# Remove a contact
vauchi contacts remove "contact-id"
```

### Sync with Relay

Synchronize with the relay server to receive pending messages:

```bash
vauchi sync
```

### Backup and Restore

```bash
# Export identity backup (encrypted)
vauchi export backup.vauchi

# Import from backup
vauchi import backup.vauchi
```

## Global Options

```bash
# Specify custom data directory (default: ~/.local/share/vauchi)
vauchi --data-dir /path/to/data <command>

# Specify relay server (default: ws://localhost:8080)
vauchi --relay ws://relay.example.com:8080 <command>
```

## End-to-End Exchange Flow

1. **Alice** generates a QR code: `vauchi exchange start`
2. **Bob** scans and completes: `vauchi exchange complete "wb://..."`
   - Bob adds Alice as a contact (initially shows as "New Contact")
   - Bob sends his name to Alice via the relay
3. **Alice** syncs: `vauchi sync`
   - Receives Bob's exchange request
   - Adds Bob as a contact with his display name
   - Sends her name back to Bob (bidirectional exchange)
4. **Bob** syncs: `vauchi sync`
   - Receives Alice's response
   - Updates contact from "New Contact" to "Alice"
5. Both now see each other's actual display names

## Architecture

The CLI uses `vauchi-core` for all cryptographic operations and data management. Communication with the relay server uses WebSocket with a simple JSON protocol.

```
vauchi-cli/
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
