# Contact Actions - Open in External App

**Phase**: 2 | **Complexity**: Medium | **Feature File**: `features/contact_actions.feature`

## Goal
When users tap on contact info (phone, email, website, social, address), open it in the appropriate system app using standard URI schemes (`tel:`, `mailto:`, `https:`, etc.).

## Supported Platforms
- Android (Kotlin/Compose)
- iOS (Swift/SwiftUI)
- Desktop (Tauri 2 + Solid.js)
- CLI (Rust)
- TUI (Rust/Ratatui)

## Implementation Phases

### Phase 1: Core URI Builder (vauchi-core)

**Goal**: Shared logic for generating URIs from contact fields.

**Files to Create/Modify**:
- `vauchi-core/src/contact_card/uri.rs` - URI generation logic
- `vauchi-core/src/contact_card/mod.rs` - Export new module

**Functionality**:
```rust
pub enum ContactAction {
    Call(String),        // tel:+1234567890
    SendSms(String),     // sms:+1234567890
    SendEmail(String),   // mailto:user@example.com
    OpenUrl(String),     // https://example.com
    OpenMap(String),     // Platform-specific map query
    CopyToClipboard,     // Fallback action
}

pub fn field_to_action(field: &ContactField) -> ContactAction;
pub fn field_to_uri(field: &ContactField) -> Option<String>;
pub fn detect_field_type(value: &str) -> Option<FieldType>; // Heuristic for Custom fields
```

**Security**:
- Whitelist allowed schemes: `tel`, `mailto`, `sms`, `https`, `http`, `geo`
- Block dangerous schemes: `file`, `javascript`, `data`, etc.
- Validate/sanitize values before URI encoding

**Tests** (TDD - write first):
- `vauchi-core/tests/uri_builder_tests.rs`
- Test each field type → URI mapping
- Test heuristic detection for custom fields
- Test security: blocked schemes, XSS prevention

---

### Phase 2: Desktop (Tauri)

**Goal**: Implement contact actions using Tauri opener plugin.

**Dependencies**:
```toml
# src-tauri/Cargo.toml
tauri-plugin-opener = "2.2"
```

```json
// package.json
"@tauri-apps/plugin-opener": "^2.2.0"
```

**Files to Create/Modify**:
- `vauchi-desktop/src-tauri/src/commands/actions.rs` - Tauri command
- `vauchi-desktop/src-tauri/src/lib.rs` - Register plugin + command
- `vauchi-desktop/src/components/ContactField.tsx` - Click handler
- `vauchi-desktop/src/lib/actions.ts` - TypeScript bindings

**Tauri Command**:
```rust
#[tauri::command]
fn open_contact_action(field_type: String, value: String) -> Result<(), String>;
```

**Frontend**:
```typescript
import { open } from '@tauri-apps/plugin-opener';
// or via Tauri command for security validation
```

---

### Phase 3: Android

**Goal**: Implement using Android Intent system.

**Files to Create/Modify**:
- `vauchi-android/app/src/main/java/com/example/vauchi/util/ContactActions.kt`
- `vauchi-android/app/src/main/java/com/example/vauchi/ui/contacts/ContactDetailScreen.kt`

**Implementation**:
```kotlin
fun openContactField(context: Context, fieldType: FieldType, value: String) {
    val intent = when (fieldType) {
        FieldType.Phone -> Intent(Intent.ACTION_DIAL, Uri.parse("tel:$value"))
        FieldType.Email -> Intent(Intent.ACTION_SENDTO, Uri.parse("mailto:$value"))
        FieldType.Website -> Intent(Intent.ACTION_VIEW, Uri.parse(value))
        FieldType.Address -> Intent(Intent.ACTION_VIEW, Uri.parse("geo:0,0?q=${Uri.encode(value)}"))
        FieldType.Social -> Intent(Intent.ACTION_VIEW, Uri.parse(profileUrl))
        else -> null
    }
    intent?.let {
        if (it.resolveActivity(context.packageManager) != null) {
            context.startActivity(it)
        } else {
            // Fallback: copy to clipboard, show toast
        }
    }
}
```

**UI**: Add `clickable` modifier to field rows in Compose.

---

### Phase 4: iOS

**Goal**: Implement using SwiftUI openURL.

**Files to Create/Modify**:
- `vauchi-ios/Vauchi/Utilities/ContactActions.swift`
- `vauchi-ios/Vauchi/Views/ContactDetailView.swift`

**Implementation**:
```swift
@Environment(\.openURL) var openURL

func openField(_ field: ContactField) {
    guard let url = urlForField(field) else { return }
    openURL(url)
}

func urlForField(_ field: ContactField) -> URL? {
    switch field.fieldType {
    case .phone: return URL(string: "tel:\(field.value)")
    case .email: return URL(string: "mailto:\(field.value)")
    case .website: return URL(string: field.value)
    case .address: return URL(string: "maps://?q=\(field.value.addingPercentEncoding(...))")
    // ...
    }
}
```

**UI**: Use `Button` or `Link` view for tappable fields.

---

### Phase 5: CLI/TUI

**Goal**: Implement using Rust `open` crate.

**Dependencies**:
```toml
# vauchi-cli/Cargo.toml and vauchi-tui/Cargo.toml
open = "5"
```

**Files to Create/Modify**:
- `vauchi-cli/src/actions.rs`
- `vauchi-tui/src/actions.rs`

**Implementation**:
```rust
use open;

pub fn open_contact_field(field: &ContactField) -> Result<(), Error> {
    if let Some(uri) = field_to_uri(field) {
        open::that(&uri)?;
    }
    Ok(())
}
```

**CLI UX**: Add `--open` flag to `contacts show` command.
**TUI UX**: Press `Enter` or `o` on focused field to open.

---

## Test Strategy (TDD)

### Unit Tests (vauchi-core)
1. URI generation for each field type
2. Phone number normalization
3. URL validation and sanitization
4. Heuristic detection accuracy
5. Security: blocked schemes

### Integration Tests (per platform)
1. Mock/stub system handlers
2. Verify correct Intent/URL/command generated
3. Test fallback when no handler available

### E2E Tests
1. Android: Espresso test with intent verification
2. iOS: XCTest with URL scheme verification
3. Desktop: Playwright/WebDriver test

---

## Acceptance Criteria

- [ ] Tapping phone opens dialer with number pre-filled
- [ ] Tapping email opens mail client with To: field set
- [ ] Tapping website opens browser
- [ ] Tapping social opens profile (app or web fallback)
- [ ] Tapping address opens maps app
- [ ] Long-press shows action menu (call/SMS/copy)
- [ ] Graceful fallback when no handler available
- [ ] Security: Only whitelisted URI schemes allowed
- [ ] Accessibility: Screen reader announces actions
- [ ] Works on all 5 platforms

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Platform fragmentation | Core URI logic in vauchi-core, thin platform wrappers |
| No handler installed | Always offer copy-to-clipboard fallback |
| URI injection attacks | Strict whitelist, value sanitization |
| Social URL templates change | Fetch from registry, cache locally |

---

## Dependencies

- `open` crate (Rust) - CLI/TUI
- `tauri-plugin-opener` 2.2+ (Desktop)
- Android Intent APIs (Android)
- SwiftUI openURL (iOS)

## Estimated Effort

| Phase | Scope |
|-------|-------|
| Phase 1 (Core) | URI builder + tests |
| Phase 2 (Desktop) | Tauri integration |
| Phase 3 (Android) | Intent wiring + UI |
| Phase 4 (iOS) | openURL wiring + UI |
| Phase 5 (CLI/TUI) | open crate + UI |
