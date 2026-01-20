# CLAUDE.md - vauchi-desktop

Tauri desktop application with SolidJS frontend.

## Rules

- Tauri backend in Rust, frontend in SolidJS/TypeScript
- Depends on `vauchi-core`
- Follow Tauri security best practices

## Commands

```bash
cargo tauri dev                             # Development mode
cargo tauri build                           # Production build
cargo test -p vauchi-desktop                # Run Rust tests
npm test                                    # Run frontend tests (in src-ui/)
```

## Structure

- `src/` - Tauri Rust backend
- `src-ui/` - SolidJS frontend (if present)
