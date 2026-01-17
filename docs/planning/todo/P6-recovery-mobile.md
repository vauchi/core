# Plan: Recovery Workflow for Mobile

**Priority**: HIGH
**Effort**: 1 week
**Impact**: Account recovery (critical safety feature)

## Problem Statement

Recovery workflow is fully implemented in CLI but mobile apps only have informational UI:
- Android: Shows instructions but delegates to CLI
- iOS: No recovery views at all
- Desktop: Can create claims/vouchers but no full workflow

## Success Criteria

- [x] Mobile apps can create recovery claims (bindings done)
- [x] Mobile apps can vouch for contacts (bindings done)
- [x] Mobile apps can add vouchers to claims (bindings done)
- [x] Mobile apps show recovery progress (bindings done)
- [ ] iOS has complete recovery UI (NOT STARTED)
- [ ] Android uses new bindings in UI (PARTIAL - informational only)

## Current State

### CLI (Reference Implementation)

| Command | Status | Description |
|---------|--------|-------------|
| `recovery claim` | ✅ | Create claim from old public key |
| `recovery vouch` | ✅ | Vouch for someone's recovery |
| `recovery add-voucher` | ✅ | Add voucher to proof |
| `recovery status` | ✅ | Show current progress |
| `recovery proof` | ✅ | Export completed proof |
| `recovery verify` | ✅ | Verify someone's proof |

### Mobile Bindings

**File**: `webbook-mobile/src/lib.rs`

Currently exposed (verified in Desktop):
- `create_recovery_claim(old_pk_hex)` ✅
- `create_recovery_voucher(claim_b64)` ✅
- `parse_recovery_claim(claim_b64)` ✅

Missing:
- `add_voucher_to_claim(voucher_b64)` ❌
- `get_recovery_status()` ❌
- `get_recovery_proof()` ❌
- `verify_recovery_proof(proof_b64)` ❌

## Implementation

### Task 1: Complete Mobile Bindings

**File**: `webbook-mobile/src/lib.rs`

```rust
impl WebBookMobile {
    /// Add a voucher to the pending recovery claim
    pub fn add_voucher(&self, voucher_b64: String) -> Result<RecoveryProgress, MobileError> {
        let mut webbook = self.inner.lock().map_err(|_| MobileError::LockError)?;

        let voucher = RecoveryVoucher::from_base64(&voucher_b64)
            .map_err(|e| MobileError::InvalidData(e.to_string()))?;

        webbook.recovery_manager().add_voucher(voucher)?;

        let status = webbook.recovery_manager().get_status()?;
        Ok(RecoveryProgress {
            vouchers_collected: status.vouchers.len() as u32,
            vouchers_needed: status.threshold,
            is_complete: status.vouchers.len() >= status.threshold as usize,
        })
    }

    /// Get current recovery status
    pub fn get_recovery_status(&self) -> Result<Option<RecoveryStatus>, MobileError> {
        let webbook = self.inner.lock().map_err(|_| MobileError::LockError)?;

        match webbook.recovery_manager().get_status() {
            Ok(status) => Ok(Some(RecoveryStatus {
                old_public_key: hex::encode(&status.old_pk),
                vouchers_collected: status.vouchers.len() as u32,
                vouchers_needed: status.threshold,
                expires_at: status.expires_at,
            })),
            Err(_) => Ok(None), // No pending recovery
        }
    }

    /// Get completed recovery proof
    pub fn get_recovery_proof(&self) -> Result<Option<String>, MobileError> {
        let webbook = self.inner.lock().map_err(|_| MobileError::LockError)?;

        let status = webbook.recovery_manager().get_status()?;
        if status.vouchers.len() >= status.threshold as usize {
            let proof = webbook.recovery_manager().create_proof()?;
            Ok(Some(proof.to_base64()))
        } else {
            Ok(None)
        }
    }

    /// Verify a recovery proof from a contact
    pub fn verify_recovery_proof(&self, proof_b64: String) -> Result<RecoveryVerification, MobileError> {
        let webbook = self.inner.lock().map_err(|_| MobileError::LockError)?;

        let proof = RecoveryProof::from_base64(&proof_b64)
            .map_err(|e| MobileError::InvalidData(e.to_string()))?;

        let result = webbook.recovery_manager().verify_proof(&proof)?;

        Ok(RecoveryVerification {
            old_public_key: hex::encode(&proof.old_pk),
            new_public_key: hex::encode(&proof.new_pk),
            voucher_count: proof.vouchers.len() as u32,
            known_vouchers: result.known_voucher_count as u32,
            confidence: match result.confidence {
                Confidence::High => "high".into(),
                Confidence::Medium => "medium".into(),
                Confidence::Low => "low".into(),
            },
            recommendation: result.recommendation,
        })
    }
}
```

**File**: `webbook-mobile/src/webbook_mobile.udl`

```
dictionary RecoveryProgress {
    u32 vouchers_collected;
    u32 vouchers_needed;
    boolean is_complete;
};

dictionary RecoveryStatus {
    string old_public_key;
    u32 vouchers_collected;
    u32 vouchers_needed;
    u64 expires_at;
};

dictionary RecoveryVerification {
    string old_public_key;
    string new_public_key;
    u32 voucher_count;
    u32 known_vouchers;
    string confidence;
    string recommendation;
};

interface WebBookMobile {
    // ... existing

    [Throws=MobileError]
    RecoveryProgress add_voucher(string voucher_b64);

    [Throws=MobileError]
    RecoveryStatus? get_recovery_status();

    [Throws=MobileError]
    string? get_recovery_proof();

    [Throws=MobileError]
    RecoveryVerification verify_recovery_proof(string proof_b64);
};
```

### Task 2: Android Recovery UI

**File**: `webbook-android/.../ui/RecoveryScreen.kt`

Replace informational UI with functional workflow:

```kotlin
@Composable
fun RecoveryScreen(viewModel: MainViewModel) {
    var selectedTab by remember { mutableIntStateOf(0) }
    var recoveryStatus by remember { mutableStateOf<RecoveryStatus?>(null) }

    LaunchedEffect(Unit) {
        recoveryStatus = viewModel.getRecoveryStatus()
    }

    TabRow(selectedTabIndex = selectedTab) {
        Tab(selected = selectedTab == 0, onClick = { selectedTab = 0 }) {
            Text("Recover My Identity")
        }
        Tab(selected = selectedTab == 1, onClick = { selectedTab = 1 }) {
            Text("Help Others")
        }
    }

    when (selectedTab) {
        0 -> RecoverIdentityTab(viewModel, recoveryStatus)
        1 -> VouchForOthersTab(viewModel)
    }
}

@Composable
fun RecoverIdentityTab(viewModel: MainViewModel, status: RecoveryStatus?) {
    if (status != null) {
        // Show progress
        Card {
            Text("Recovery in Progress")
            Text("${status.vouchersCollected}/${status.vouchersNeeded} vouchers")
            LinearProgressIndicator(progress = status.vouchersCollected.toFloat() / status.vouchersNeeded)

            OutlinedTextField(
                value = voucherInput,
                onValueChange = { voucherInput = it },
                label = { Text("Paste voucher from contact") }
            )

            Button(onClick = {
                viewModel.addVoucher(voucherInput)
            }) {
                Text("Add Voucher")
            }

            if (status.vouchersCollected >= status.vouchersNeeded) {
                Button(onClick = {
                    viewModel.getRecoveryProof()
                }) {
                    Text("Get Recovery Proof")
                }
            }
        }
    } else {
        // Show create claim UI
        OutlinedTextField(
            value = oldPkInput,
            onValueChange = { oldPkInput = it },
            label = { Text("Old Public Key (hex)") }
        )

        Button(onClick = {
            viewModel.createRecoveryClaim(oldPkInput)
        }) {
            Text("Create Recovery Claim")
        }
    }
}
```

### Task 3: iOS Recovery UI

**File**: `webbook-ios/WebBook/Views/RecoveryView.swift`

Create new view (currently missing):

```swift
struct RecoveryView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @State private var selectedTab = 0
    @State private var recoveryStatus: RecoveryStatus?

    var body: some View {
        NavigationView {
            VStack {
                Picker("", selection: $selectedTab) {
                    Text("Recover").tag(0)
                    Text("Help Others").tag(1)
                }
                .pickerStyle(.segmented)

                if selectedTab == 0 {
                    RecoverIdentityView(status: recoveryStatus)
                } else {
                    VouchForOthersView()
                }
            }
            .navigationTitle("Recovery")
            .onAppear {
                Task {
                    recoveryStatus = try? await viewModel.getRecoveryStatus()
                }
            }
        }
    }
}

struct RecoverIdentityView: View {
    let status: RecoveryStatus?
    @State private var oldPk = ""
    @State private var voucherInput = ""
    @EnvironmentObject var viewModel: WebBookViewModel

    var body: some View {
        if let status = status {
            // Progress view
            VStack {
                Text("Recovery in Progress")
                    .font(.headline)

                ProgressView(value: Double(status.vouchersCollected),
                            total: Double(status.vouchersNeeded))

                Text("\(status.vouchersCollected)/\(status.vouchersNeeded) vouchers")

                TextField("Paste voucher", text: $voucherInput)
                    .textFieldStyle(.roundedBorder)

                Button("Add Voucher") {
                    Task {
                        try? await viewModel.addVoucher(voucherInput)
                        voucherInput = ""
                    }
                }

                if status.vouchersCollected >= status.vouchersNeeded {
                    Button("Get Proof") {
                        Task {
                            if let proof = try? await viewModel.getRecoveryProof() {
                                UIPasteboard.general.string = proof
                            }
                        }
                    }
                    .buttonStyle(.borderedProminent)
                }
            }
        } else {
            // Create claim view
            VStack {
                Text("Enter your old identity's public key")
                TextField("Old Public Key (hex)", text: $oldPk)
                    .textFieldStyle(.roundedBorder)

                Button("Create Recovery Claim") {
                    Task {
                        try? await viewModel.createRecoveryClaim(oldPk)
                    }
                }
            }
        }
    }
}
```

**Add to ContentView.swift navigation**:
```swift
// Add Recovery tab
TabView {
    // ... existing tabs
    RecoveryView()
        .tabItem {
            Label("Recovery", systemImage: "arrow.counterclockwise")
        }
}
```

### Task 4: ViewModel Updates

**Android** (`MainViewModel.kt`):
```kotlin
fun getRecoveryStatus(): RecoveryStatus? = runBlocking {
    withContext(Dispatchers.IO) {
        repository.getRecoveryStatus()
    }
}

fun addVoucher(voucher: String) {
    viewModelScope.launch {
        try {
            val progress = withContext(Dispatchers.IO) {
                repository.addVoucher(voucher)
            }
            showMessage("Voucher added (${progress.vouchersCollected}/${progress.vouchersNeeded})")
        } catch (e: Exception) {
            showMessage("Failed: ${e.message}")
        }
    }
}
```

**iOS** (`WebBookViewModel.swift`):
```swift
func getRecoveryStatus() async throws -> RecoveryStatus? {
    try await repository.getRecoveryStatus()
}

func addVoucher(_ voucher: String) async throws -> RecoveryProgress {
    try await repository.addVoucher(voucher)
}
```

## Verification

1. Create identity on Device A
2. "Lose" Device A, create new identity on Device B
3. Create recovery claim on Device B with Device A's public key
4. Have 3+ contacts vouch (using their devices)
5. Add vouchers to claim on Device B
6. Generate proof, share with contacts
7. Contacts verify and accept new identity

## Files Modified

| Component | Files |
|-----------|-------|
| Mobile bindings | `webbook-mobile/src/lib.rs`, `src/webbook_mobile.udl` |
| Android | `RecoveryScreen.kt`, `MainViewModel.kt` |
| iOS | New `RecoveryView.swift`, `WebBookViewModel.swift`, `ContentView.swift` |
