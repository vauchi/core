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

**File**: `webbook-core/src/api.rs`

```rust
impl WebBook {
    /// Update the user's display name
    pub fn update_display_name(&mut self, new_name: &str) -> WebBookResult<()> {
        let name = new_name.trim();
        if name.is_empty() {
            return Err(WebBookError::ValidationError("Display name cannot be empty".into()));
        }
        if name.len() > 100 {
            return Err(WebBookError::ValidationError("Display name too long".into()));
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

**Test**: `webbook-core/tests/api_tests.rs`
```rust
#[test]
fn test_update_display_name() {
    let mut webbook = create_test_webbook();
    webbook.update_display_name("New Name").unwrap();
    assert_eq!(webbook.get_display_name(), "New Name");
}
```

### Task 2: Mobile Bindings

**File**: `webbook-mobile/src/lib.rs`

```rust
impl WebBookMobile {
    pub fn update_display_name(&self, new_name: String) -> Result<(), MobileError> {
        let mut webbook = self.inner.lock().map_err(|_| MobileError::LockError)?;
        webbook.update_display_name(&new_name)?;
        Ok(())
    }
}
```

**File**: `webbook-mobile/src/webbook_mobile.udl`
```
interface WebBookMobile {
    // ... existing
    [Throws=MobileError]
    void update_display_name(string new_name);
};
```

### Task 3: CLI Implementation

**File**: `webbook-cli/src/commands/init.rs`

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
    let mut webbook = load_webbook()?;
    webbook.update_display_name(name)?;
    println!("Display name updated to: {}", name);
    Ok(())
}
```

### Task 4: TUI Implementation

**File**: `webbook-tui/src/ui/home.rs`

Add edit name dialog triggered by `e` on the name field:
```rust
// Add to AppState
edit_name_dialog: Option<EditNameDialog>,

// In render
if let Some(dialog) = &app.edit_name_dialog {
    render_edit_name_dialog(f, area, dialog);
}
```

**File**: `webbook-tui/src/handlers/input.rs`
```rust
KeyCode::Char('e') if on_name_field => {
    app.edit_name_dialog = Some(EditNameDialog::new(app.display_name.clone()));
}
```

### Task 5: Desktop Implementation

**File**: `webbook-desktop/src-tauri/src/identity.rs`
```rust
#[tauri::command]
async fn update_display_name(name: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut webbook = state.webbook.lock().await;
    webbook.update_display_name(&name).map_err(|e| e.to_string())
}
```

**File**: `webbook-desktop/ui/src/pages/Settings.tsx`
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

**File**: `webbook-android/.../ui/MainViewModel.kt`
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

**File**: `webbook-android/.../MainActivity.kt` (Home screen)
Add edit button next to display name with dialog.

### Task 7: iOS Implementation

**File**: `webbook-ios/WebBook/ViewModels/WebBookViewModel.swift`
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

**File**: `webbook-ios/WebBook/Views/SettingsView.swift`
Make display name editable with tap-to-edit pattern.

## Verification

1. Unit test in core
2. Integration test: change name, sync, verify contacts see new name
3. Manual test in each frontend

## Files Modified

| Crate/App | Files |
|-----------|-------|
| webbook-core | `src/api.rs`, `tests/api_tests.rs` |
| webbook-mobile | `src/lib.rs`, `src/webbook_mobile.udl` |
| webbook-cli | `src/commands/init.rs` or new `name.rs` |
| webbook-tui | `src/ui/home.rs`, `src/handlers/input.rs` |
| webbook-desktop | `src-tauri/src/identity.rs`, `ui/src/pages/Settings.tsx` |
| webbook-android | `MainViewModel.kt`, `MainActivity.kt` |
| webbook-ios | `WebBookViewModel.swift`, `SettingsView.swift` |
