# UDL File Deprecated

The `vauchi_mobile.udl.deprecated` file is no longer used.

## Why?

As of 2026-01-21, all UniFFI types are defined using Rust proc-macros:
- `#[uniffi::export]` for functions and methods
- `#[derive(uniffi::Record)]` for structs
- `#[derive(uniffi::Enum)]` for enums
- `#[derive(uniffi::Object)]` for objects
- `#[uniffi::export(callback_interface)]` for callback traits

The old UDL file only contained a subset of types and caused bindings drift.
See `docs/planning/done/POSTMORTEM-ANDROID-BINDINGS-DRIFT.md` for details.

## Binding Generation

Bindings are now generated using library mode which extracts metadata from the
compiled library:

```bash
RUSTFLAGS="-Cstrip=none" cargo build -p vauchi-mobile --release
cargo run --bin uniffi-bindgen -- generate \
    --library target/release/libvauchi_mobile.so \
    --language kotlin \
    --out-dir ../android/app/src/main/kotlin/
```

The `--library` flag extracts all proc-macro-defined types automatically.

## Safe to Delete

The `.deprecated` file can be safely deleted. It's kept only as historical
reference.
