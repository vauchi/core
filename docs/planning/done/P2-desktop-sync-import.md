# Plan: Desktop App Sync & Import

**Priority**: HIGH
**Effort**: 1-2 days
**Impact**: Feature completeness

## Problem Statement

Desktop app is missing critical functionality that exists in other frontends:
1. **No sync UI** - Cannot trigger sync or see sync status
2. **No import backup** - Backend command exists but no UI
3. **No field removal** - Can add fields but not delete them

## Success Criteria

- [ ] Sync button in Settings with loading state (deferred - requires SyncController infrastructure)
- [ ] Sync status indicator (idle/syncing/success/error) (deferred - requires SyncController infrastructure)
- [x] Import backup section with dialog UI
- [x] Delete button on card fields in Home page

## Implementation

### Task 1: Add Sync Command

**File**: `vauchi-desktop/src-tauri/src/lib.rs`

```rust
#[tauri::command]
async fn sync(state: State<'_, AppState>) -> Result<SyncResult, String> {
    let vauchi = state.vauchi.lock().await;
    vauchi.sync().await.map_err(|e| e.to_string())
}

// Add to invoke_handler
.invoke_handler(tauri::generate_handler![
    // ... existing commands
    sync,
])
```

### Task 2: Sync UI in Settings

**File**: `vauchi-desktop/ui/src/pages/Settings.tsx`

```tsx
// Add state
const [syncState, setSyncState] = createSignal<'idle' | 'syncing' | 'success' | 'error'>('idle');
const [lastSync, setLastSync] = createSignal<Date | null>(null);

// Add sync function
const handleSync = async () => {
  setSyncState('syncing');
  try {
    await invoke('sync');
    setSyncState('success');
    setLastSync(new Date());
  } catch (e) {
    setSyncState('error');
  }
};

// Add UI in Sync section
<div class="setting-item">
  <span>Relay Sync</span>
  <button onClick={handleSync} disabled={syncState() === 'syncing'}>
    {syncState() === 'syncing' ? 'Syncing...' : 'Sync Now'}
  </button>
  <span class="sync-status">{syncState()}</span>
</div>
```

### Task 3: Import Backup UI

**File**: `vauchi-desktop/ui/src/pages/Settings.tsx`

```tsx
// Add import state
const [showImport, setShowImport] = createSignal(false);
const [importData, setImportData] = createSignal('');
const [importPassword, setImportPassword] = createSignal('');

// Add import function
const handleImport = async () => {
  try {
    await invoke('import_backup', {
      backupData: importData(),
      password: importPassword()
    });
    setShowImport(false);
    // Refresh identity info
    loadIdentity();
  } catch (e) {
    setError(e.toString());
  }
};

// Add UI in Backup section
<button onClick={() => setShowImport(true)}>Import Backup</button>

{showImport() && (
  <div class="import-dialog">
    <textarea
      placeholder="Paste backup data..."
      value={importData()}
      onInput={e => setImportData(e.target.value)}
    />
    <input
      type="password"
      placeholder="Backup password"
      value={importPassword()}
      onInput={e => setImportPassword(e.target.value)}
    />
    <button onClick={handleImport}>Import</button>
    <button onClick={() => setShowImport(false)}>Cancel</button>
  </div>
)}
```

### Task 4: Field Deletion in Home

**File**: `vauchi-desktop/ui/src/pages/Home.tsx`

```tsx
// Add delete handler
const handleDeleteField = async (fieldId: string) => {
  if (!confirm('Delete this field?')) return;
  try {
    await invoke('remove_field', { fieldId });
    refetch(); // Refresh card
  } catch (e) {
    setError(e.toString());
  }
};

// Add delete button to field row
<For each={card()?.fields}>
  {(field) => (
    <div class="field-row">
      <span class="field-icon">{getIcon(field.type)}</span>
      <span class="field-label">{field.label}</span>
      <span class="field-value">{field.value}</span>
      <button class="delete-btn" onClick={() => handleDeleteField(field.id)}>
        ×
      </button>
    </div>
  )}
</For>
```

## Verification

```bash
# Build and test
cd vauchi-desktop
npm run build  # Build UI
cargo tauri dev  # Run app

# Manual testing:
# 1. Click Sync in Settings, verify status changes
# 2. Export backup, then import it (should restore)
# 3. Add a field, then delete it
```

## Files Modified

- `vauchi-desktop/src-tauri/src/lib.rs` (add sync command)
- `vauchi-desktop/ui/src/pages/Settings.tsx` (sync + import UI)
- `vauchi-desktop/ui/src/pages/Home.tsx` (field deletion)
