# Code Structure

## Module Organization

```
vauchi-cli/
├── src/
│   ├── main.rs              # CLI entry point, argument parsing
│   ├── config.rs            # CLI configuration
│   ├── display.rs           # Terminal output formatting
│   ├── protocol.rs          # Wire protocol for relay communication
│   └── commands/
│       ├── mod.rs           # Command module exports
│       ├── init.rs          # Identity creation
│       ├── card.rs          # Contact card management
│       ├── contacts.rs      # Contact list operations
│       ├── exchange.rs      # QR code exchange flow
│       ├── sync.rs          # Relay synchronization
│       └── backup.rs        # Export/import identity
└── Cargo.toml               # Crate configuration
```

## Components

### `main.rs` - CLI Entry Point

Argument parsing with clap and command dispatch.

| Item | Purpose |
|------|---------|
| `Cli` | Top-level argument struct |
| `Commands` | Enum of all subcommands |
| `main` | Parse args, route to handlers |

### `config.rs` - Configuration

CLI settings and data paths.

| Item | Purpose |
|------|---------|
| `CliConfig` | Data dir, relay URL |
| `get_data_dir` | Resolve data directory path |
| `get_relay_url` | Get relay server URL |

### `display.rs` - Terminal Output

Formatted output for cards, contacts, and status.

| Function | Purpose |
|----------|---------|
| `print_card` | Display contact card nicely |
| `print_contact_list` | Format contact list |
| `print_exchange_qr` | Render QR code in terminal |
| `print_success`/`print_error` | Status messages |

### `protocol.rs` - Wire Protocol

Relay message serialization.

| Item | Purpose |
|------|---------|
| `encode_message` | Serialize for transmission |
| `decode_message` | Parse incoming messages |

### Commands

#### `init.rs` - Identity Creation

| Function | Purpose |
|----------|---------|
| `run` | Create new identity with display name |

#### `card.rs` - Card Management

| Function | Purpose |
|----------|---------|
| `show` | Display own contact card |
| `add` | Add field to card |
| `edit` | Update field value |
| `remove` | Delete field from card |

#### `contacts.rs` - Contact Operations

| Function | Purpose |
|----------|---------|
| `list` | Show all contacts |
| `show` | Display single contact |
| `search` | Find contacts by name |
| `verify` | Mark contact as verified |
| `remove` | Delete contact |

#### `exchange.rs` - Contact Exchange

| Function | Purpose |
|----------|---------|
| `start` | Generate and display QR code |
| `complete` | Process scanned QR, initiate exchange |

#### `sync.rs` - Relay Synchronization

| Function | Purpose |
|----------|---------|
| `run` | Connect to relay, send/receive messages |

#### `backup.rs` - Identity Backup

| Function | Purpose |
|----------|---------|
| `export` | Save encrypted identity to file |
| `import` | Restore identity from backup |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `vauchi-core` | Core library |
| `clap` | Argument parsing |
| `tokio` | Async runtime |
| `tokio-tungstenite` | WebSocket client |
| `qrcode` | QR code generation |
| `dialoguer` | Interactive prompts |
| `colored` | Terminal colors |
