# Vauchi TUI

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
- Direct integration with `vauchi-core`

## Quick Start

```bash
# Run directly
cargo run -p vauchi-tui

# Or build and run
cargo build -p vauchi-tui --release
./target/release/vauchi-tui
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
vauchi-tui/src/
├── main.rs          # Entry point, event loop
├── app.rs           # Application state
├── backend.rs       # Vauchi core integration
├── ui/              # Screen renderers (12 screens)
└── handlers/        # Keyboard event handlers
```

## ⚠️ Mandatory Development Rules

**TDD**: Red→Green→Refactor. Test FIRST or delete code and restart.

**Structure**: `src/` = production code only. `tests/` = tests only. Siblings, not nested.

See [CLAUDE.md](../../CLAUDE.md) for additional mandatory rules.

## License

MIT
