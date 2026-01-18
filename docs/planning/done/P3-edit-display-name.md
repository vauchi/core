# Plan: Edit Display Name

**Priority**: HIGH
**Effort**: 2-3 days
**Impact**: Core feature missing in ALL frontends

## Problem Statement

Users cannot edit their display name after initial setup. This is a basic identity management feature missing across all 5 frontends.

## Success Criteria

- [x] Core API supports `update_display_name()`
- [ ] Mobile bindings expose the method (deferred)
- [x] Desktop has edit capability in Settings
- [ ] CLI/TUI/Android/iOS frontends (deferred)
- [ ] Changes propagate to contacts via sync (requires SyncController)

## Implementation

### Task 1: Core API

**File**: `vauchi-core/src/api.rs`

```rust
impl Vauchi {
    /// Update the user's display name
    pub fn update_display_name(&mut self, new_name: &str) -> VauchiResult<()> {
        let name = new_name.trim();
        if name.is_empty() {
            return Err(VauchiError::ValidationError("Display name cannot be empty".into()));
        }
        if name.len() > 100 {
            return Err(VauchiError::ValidationError("Display name too long".into()));
        }

        // Update identity
        self.identity.set_display_name(name);

        // Update card name field
        self.card.set_name(name);

        // Save changes
        self.storage.save_identity(&self.identity)?;
        self.storage.save_card(&self.card)?;

        // Queue card update for sync
        self.sync_manager.queue_card_update(&self.card)?;

        Ok(())
    }
}
```

**Test**: `vauchi-core/tests/api_tests.rs`
```rust
#[test]
fn test_update_display_name() {
    let mut vauchi = create_test_vauchi();
    vauchi.update_display_name("New Name").unwrap();
    assert_eq!(vauchi.get_display_name(), "New Name");
}
```

### Task 2: Mobile Bindings

**File**: `vauchi-mobile/src/lib.rs`

```rust
impl VauchiMobile {
    pub fn update_display_name(&self, new_name: String) -> Result<(), MobileError> {
        let mut vauchi = self.inner.lock().map_err(|_| MobileError::LockError)?;
        vauchi.update_display_name(&new_name)?;
        Ok(())
    }
}
```

**File**: `vauchi-mobile/src/vauchi_mobile.udl`
```
interface VauchiMobile {
    // ... existing
    [Throws=MobileError]
    void update_display_name(string new_name);
};
```

### Task 3: CLI Implementation

**File**: `vauchi-cli/src/commands/init.rs`

Add `name edit` subcommand:
```rust
#[derive(Subcommand)]
enum NameCommands {
    /// Edit your display name
    Edit {
        /// New display name
        name: String,
    },
}

// In execute_name_edit:
fn execute_name_edit(name: &str) -> Result<()> {
    let mut vauchi = load_vauchi()?;
    vauchi.update_display_name(name)?;
    println!("Display name updated to: {}", name);
    Ok(())
}
```

### Task 4: TUI Implementation

**File**: `vauchi-tui/src/ui/home.rs`

Add edit name dialog triggered by `e` on the name field:
```rust
// Add to AppState
edit_name_dialog: Option<EditNameDialog>,

// In render
if let Some(dialog) = &app.edit_name_dialog {
    render_edit_name_dialog(f, area, dialog);
}
```

**File**: `vauchi-tui/src/handlers/input.rs`
```rust
KeyCode::Char('e') if on_name_field => {
    app.edit_name_dialog = Some(EditNameDialog::new(app.display_name.clone()));
}
```

### Task 5: Desktop Implementation

**File**: `vauchi-desktop/src-tauri/src/identity.rs`
```rust
#[tauri::command]
async fn update_display_name(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut vauchi = state.vauchi.lock().await;
    vauchi.update_display_name(&name).map_err(|e| e.to_string())
}
```

**File**: `vauchi-desktop/ui/src/pages/Settings.tsx`
```tsx
const [editingName, setEditingName] = createSignal(false);
const [newName, setNewName] = createSignal('');

const handleUpdateName = async () => {
  await invoke('update_display_name', { name: newName() });
  setEditingName(false);
  refetchIdentity();
};

// In UI
{editingName() ? (
  <input value={newName()} onInput={e => setNewName(e.target.value)} />
  <button onClick={handleUpdateName}>Save</button>
) : (
  <span>{displayName()}</span>
  <button onClick={() => { setNewName(displayName()); setEditingName(true); }}>Edit</button>
)}
```

### Task 6: Android Implementation

**File**: `vauchi-android/.../ui/MainViewModel.kt`
```kotlin
fun updateDisplayName(newName: String) {
    viewModelScope.launch {
        try {
            withContext(Dispatchers.IO) {
                repository.updateDisplayName(newName)
            }
            loadUserData() // Refresh UI
            showMessage("Name updated")
        } catch (e: Exception) {
            showMessage("Failed to update name: ${e.message}")
        }
    }
}
```

**File**: `vauchi-android/.../MainActivity.kt` (Home screen)
Add edit button next to display name with dialog.

### Task 7: iOS Implementation

**File**: `vauchi-ios/Vauchi/ViewModels/VauchiViewModel.swift`
```swift
func updateDisplayName(_ newName: String) async {
    do {
        try await repository.updateDisplayName(newName)
        await loadIdentity()
        showSuccess(title: "Updated", message: "Display name changed")
    } catch {
        showError(title: "Error", message: error.localizedDescription)
    }
}
```

**File**: `vauchi-ios/Vauchi/Views/SettingsView.swift`
Make display name editable with tap-to-edit pattern.

## Verification

1. Unit test in core
2. Integration test: change name, sync, verify contacts see new name
3. Manual test in each frontend

## Files Modified

| Crate/App | Files |
|-----------|-------|
| vauchi-core | `src/api.rs`, `tests/api_tests.rs` |
| vauchi-mobile | `src/lib.rs`, `src/vauchi_mobile.udl` |
| vauchi-cli | `src/commands/init.rs` or new `name.rs` |
| vauchi-tui | `src/ui/home.rs`, `src/handlers/input.rs` |
| vauchi-desktop | `src-tauri/src/identity.rs`, `ui/src/pages/Settings.tsx` |
| vauchi-android | `MainViewModel.kt`, `MainActivity.kt` |
| vauchi-ios | `VauchiViewModel.swift`, `SettingsView.swift` |
