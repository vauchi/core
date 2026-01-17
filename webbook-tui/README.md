# WebBook TUI

Terminal user interface for privacy-focused contact card exchange.

## Features

- **Contact Card Management**: Create and edit your personal contact card
- **QR Exchange**: Display QR codes in terminal for contact exchange
- **Contacts Browser**: Navigate and manage contacts with keyboard
- **Selective Visibility**: Control field visibility per contact
- **Encrypted Backup**: Export/import with password-protected encryption

## Tech Stack

- Rust + Ratatui (terminal UI framework)
- Crossterm (cross-platform terminal handling)
- Direct integration with `webbook-core`

## Quick Start

```bash
# Run directly
cargo run -p webbook-tui

# Or build and run
cargo build -p webbook-tui --release
./target/release/webbook-tui
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `e` | Exchange (QR) |
| `c` | Contacts |
| `s` | Settings |
| `d` | Devices |
| `b` | Backup |
| `n` | Sync now |
| `a` | Add field |
| `x` | Delete |
| `?` | Help |
| `q` | Quit |

## Project Structure

```
webbook-tui/src/
├── main.rs          # Entry point, event loop
├── app.rs           # Application state
├── backend.rs       # WebBook core integration
├── ui/              # Screen renderers (12 screens)
└── handlers/        # Keyboard event handlers
```

## License

MIT
