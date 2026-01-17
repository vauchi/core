# Plan: Contact Search (TUI + Desktop)

**Priority**: MEDIUM
**Effort**: 1 day
**Impact**: UX improvement for managing contacts

## Problem Statement

TUI and Desktop apps have no way to search/filter contacts. With many contacts, finding a specific person requires scrolling through the entire list.

## Success Criteria

- [ ] TUI has search input that filters contacts in real-time
- [ ] Desktop has search bar in Contacts page
- [ ] Search matches on name and field values (like Android/iOS)

## Implementation

### Task 1: TUI Contact Search

**File**: `webbook-tui/src/ui/contacts.rs`

```rust
// Add search state to contacts view
pub struct ContactsView {
    contacts: Vec<Contact>,
    filtered_contacts: Vec<Contact>,
    selected_index: usize,
    search_query: String,
    search_mode: bool,
}

impl ContactsView {
    fn filter_contacts(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_contacts = self.contacts.clone();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_contacts = self.contacts.iter()
                .filter(|c| {
                    c.name.to_lowercase().contains(&query) ||
                    c.fields.iter().any(|f| f.value.to_lowercase().contains(&query))
                })
                .cloned()
                .collect();
        }
        self.selected_index = 0;
    }
}

// In render function, add search bar
fn render(f: &mut Frame, area: Rect, view: &ContactsView) {
    // Search bar at top
    let search_block = Block::default()
        .borders(Borders::ALL)
        .title("Search (/)");
    let search_input = Paragraph::new(view.search_query.as_str())
        .block(search_block);
    f.render_widget(search_input, search_area);

    // Contact list (use filtered_contacts)
    for contact in &view.filtered_contacts {
        // ... render contact row
    }
}
```

**File**: `webbook-tui/src/handlers/input.rs`

```rust
// Add search mode handling
KeyCode::Char('/') => {
    app.contacts_view.search_mode = true;
}

// In search mode, capture characters
if app.contacts_view.search_mode {
    match key.code {
        KeyCode::Char(c) => {
            app.contacts_view.search_query.push(c);
            app.contacts_view.filter_contacts();
        }
        KeyCode::Backspace => {
            app.contacts_view.search_query.pop();
            app.contacts_view.filter_contacts();
        }
        KeyCode::Esc | KeyCode::Enter => {
            app.contacts_view.search_mode = false;
        }
        _ => {}
    }
}
```

### Task 2: Desktop Contact Search

**File**: `webbook-desktop/ui/src/pages/Contacts.tsx`

```tsx
// Add search state
const [searchQuery, setSearchQuery] = createSignal('');

// Filter contacts
const filteredContacts = createMemo(() => {
  const query = searchQuery().toLowerCase();
  if (!query) return contacts() || [];

  return (contacts() || []).filter(c =>
    c.name.toLowerCase().includes(query) ||
    c.fields?.some(f => f.value.toLowerCase().includes(query))
  );
});

// Add search bar UI
<div class="search-bar">
  <input
    type="text"
    placeholder="Search contacts..."
    value={searchQuery()}
    onInput={(e) => setSearchQuery(e.target.value)}
  />
  {searchQuery() && (
    <button class="clear-search" onClick={() => setSearchQuery('')}>×</button>
  )}
</div>

// Use filteredContacts in list
<For each={filteredContacts()}>
  {(contact) => <ContactRow contact={contact} />}
</For>
```

**File**: `webbook-desktop/ui/src/pages/Contacts.css` (or styled-components)

```css
.search-bar {
  display: flex;
  padding: 0.5rem;
  border-bottom: 1px solid var(--border-color);
}

.search-bar input {
  flex: 1;
  padding: 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 4px;
}

.clear-search {
  background: none;
  border: none;
  cursor: pointer;
  padding: 0 0.5rem;
}
```

## Verification

```bash
# TUI
cargo run -p webbook-tui
# Press 'c' for contacts, then '/' to search
# Type a name, verify filtering works

# Desktop
cd webbook-desktop
npm run dev
# Navigate to Contacts, use search bar
```

## Files Modified

| App | Files |
|-----|-------|
| TUI | `src/ui/contacts.rs`, `src/handlers/input.rs` |
| Desktop | `ui/src/pages/Contacts.tsx` |
