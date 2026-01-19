package com.vauchi.ui

import android.content.Context
import com.vauchi.util.ClipboardUtils
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    displayName: String,
    onBack: () -> Unit,
    onExportBackup: suspend (String) -> String?,
    onImportBackup: suspend (String, String) -> Boolean,
    onUpdateDisplayName: suspend (String) -> Boolean = { true },
    relayUrl: String = "",
    onRelayUrlChange: (String) -> Unit = {},
    syncState: SyncState = SyncState.Idle,
    onSync: () -> Unit = {},
    onDevices: () -> Unit = {},
    onRecovery: () -> Unit = {},
    onCheckPasswordStrength: (String) -> PasswordStrengthResult = { PasswordStrengthResult() }
) {
    var showExportDialog by remember { mutableStateOf(false) }
    var showImportDialog by remember { mutableStateOf(false) }
    var showEditNameDialog by remember { mutableStateOf(false) }
    var snackbarMessage by remember { mutableStateOf<String?>(null) }
    val snackbarHostState = remember { SnackbarHostState() }
    var editableRelayUrl by remember(relayUrl) { mutableStateOf(relayUrl) }

    LaunchedEffect(snackbarMessage) {
        snackbarMessage?.let {
            snackbarHostState.showSnackbar(it)
            snackbarMessage = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        },
        snackbarHost = { SnackbarHost(snackbarHostState) }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(24.dp)
        ) {
            // Account Section
            Text(
                text = "Account",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                ),
                onClick = { showEditNameDialog = true }
            ) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column {
                        Text(
                            text = "Display Name",
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Text(
                            text = displayName,
                            style = MaterialTheme.typography.bodyLarge
                        )
                    }
                    Text(
                        text = "Edit",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }

            HorizontalDivider()

            // Sync Section
            Text(
                text = "Sync",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                )
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    OutlinedTextField(
                        value = editableRelayUrl,
                        onValueChange = { editableRelayUrl = it },
                        label = { Text("Relay URL") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )

                    if (editableRelayUrl != relayUrl) {
                        Button(
                            onClick = { onRelayUrlChange(editableRelayUrl) },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text("Save Relay URL")
                        }
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(
                            text = when (syncState) {
                                is SyncState.Idle -> "Ready to sync"
                                is SyncState.Syncing -> "Syncing..."
                                is SyncState.Success -> "Sync complete"
                                is SyncState.Error -> "Sync failed"
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = when (syncState) {
                                is SyncState.Error -> MaterialTheme.colorScheme.error
                                else -> MaterialTheme.colorScheme.onSurfaceVariant
                            }
                        )
                        Button(
                            onClick = onSync,
                            enabled = syncState !is SyncState.Syncing
                        ) {
                            if (syncState is SyncState.Syncing) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 2.dp
                                )
                            } else {
                                Text("Sync Now")
                            }
                        }
                    }
                }
            }

            HorizontalDivider()

            // Backup Section
            Text(
                text = "Backup & Restore",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            Text(
                text = "Back up your identity to restore it on another device or after reinstalling.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                Button(
                    onClick = { showExportDialog = true },
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Export Backup")
                }
                OutlinedButton(
                    onClick = { showImportDialog = true },
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Import Backup")
                }
            }

            HorizontalDivider()

            // Devices & Recovery Section
            Text(
                text = "Devices & Recovery",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                OutlinedButton(
                    onClick = onDevices,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Devices")
                }
                OutlinedButton(
                    onClick = onRecovery,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Recovery")
                }
            }

            HorizontalDivider()

            // About Section
            Text(
                text = "About",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary
            )

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                )
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Text(
                        text = "Vauchi",
                        style = MaterialTheme.typography.titleMedium
                    )
                    Text(
                        text = "Privacy-focused contact exchange",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Text(
                        text = "Version 0.1.0",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }
    }

    if (showExportDialog) {
        ExportBackupDialog(
            onDismiss = { showExportDialog = false },
            onExport = onExportBackup,
            onResult = { success, message ->
                snackbarMessage = message
                if (success) showExportDialog = false
            },
            onCheckPasswordStrength = onCheckPasswordStrength
        )
    }

    if (showImportDialog) {
        ImportBackupDialog(
            onDismiss = { showImportDialog = false },
            onImport = onImportBackup,
            onResult = { success, message ->
                snackbarMessage = message
                if (success) showImportDialog = false
            }
        )
    }

    if (showEditNameDialog) {
        EditDisplayNameDialog(
            currentName = displayName,
            onDismiss = { showEditNameDialog = false },
            onUpdateName = onUpdateDisplayName,
            onResult = { success, message ->
                snackbarMessage = message
                if (success) showEditNameDialog = false
            }
        )
    }
}

@Composable
fun ExportBackupDialog(
    onDismiss: () -> Unit,
    onExport: suspend (String) -> String?,
    onResult: (Boolean, String) -> Unit,
    onCheckPasswordStrength: (String) -> PasswordStrengthResult = { PasswordStrengthResult() }
) {
    var password by remember { mutableStateOf("") }
    var confirmPassword by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    var backupCode by remember { mutableStateOf<String?>(null) }
    var passwordStrength by remember { mutableStateOf(PasswordStrengthResult()) }
    val context = LocalContext.current
    val coroutineScope = rememberCoroutineScope()

    // Check password strength as user types
    LaunchedEffect(password) {
        if (password.isNotEmpty()) {
            passwordStrength = onCheckPasswordStrength(password)
        } else {
            passwordStrength = PasswordStrengthResult()
        }
    }

    AlertDialog(
        onDismissRequest = { if (!isLoading) onDismiss() },
        title = { Text(if (backupCode == null) "Export Backup" else "Backup Code") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                if (backupCode == null) {
                    Text(
                        text = "Create a password to encrypt your backup. You'll need this password to restore.",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    OutlinedTextField(
                        value = password,
                        onValueChange = { password = it },
                        label = { Text("Password") },
                        visualTransformation = PasswordVisualTransformation(),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        enabled = !isLoading
                    )

                    // Password strength indicator
                    if (password.isNotEmpty()) {
                        PasswordStrengthIndicator(strength = passwordStrength)
                    }

                    OutlinedTextField(
                        value = confirmPassword,
                        onValueChange = { confirmPassword = it },
                        label = { Text("Confirm Password") },
                        visualTransformation = PasswordVisualTransformation(),
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                        enabled = !isLoading
                    )
                    if (password.isNotEmpty() && confirmPassword.isNotEmpty() && password != confirmPassword) {
                        Text(
                            text = "Passwords don't match",
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall
                        )
                    }
                } else {
                    Text(
                        text = "Your backup code has been copied to clipboard. Store it safely!",
                        style = MaterialTheme.typography.bodyMedium
                    )
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant
                        )
                    ) {
                        Text(
                            text = backupCode!!.take(50) + "...",
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(12.dp)
                        )
                    }
                }
            }
        },
        confirmButton = {
            if (backupCode == null) {
                TextButton(
                    onClick = {
                        if (passwordStrength.isAcceptable && password == confirmPassword) {
                            isLoading = true
                            coroutineScope.launch {
                                val result = onExport(password)
                                if (result != null) {
                                    // Copy to clipboard with auto-clear after 30 seconds
                                    ClipboardUtils.copyWithAutoClear(context, coroutineScope, result, "Vauchi Backup")
                                    backupCode = result
                                    onResult(false, "Backup copied to clipboard (auto-clears in 30s)")
                                } else {
                                    onResult(false, "Failed to create backup")
                                }
                                isLoading = false
                            }
                        }
                    },
                    enabled = passwordStrength.isAcceptable && password == confirmPassword && !isLoading
                ) {
                    if (isLoading) {
                        CircularProgressIndicator(modifier = Modifier.size(16.dp))
                    } else {
                        Text("Export")
                    }
                }
            } else {
                TextButton(onClick = { onResult(true, "Backup exported successfully") }) {
                    Text("Done")
                }
            }
        },
        dismissButton = {
            if (backupCode == null) {
                TextButton(onClick = onDismiss, enabled = !isLoading) {
                    Text("Cancel")
                }
            }
        }
    )
}

@Composable
fun ImportBackupDialog(
    onDismiss: () -> Unit,
    onImport: suspend (String, String) -> Boolean,
    onResult: (Boolean, String) -> Unit
) {
    var backupData by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    val coroutineScope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = { if (!isLoading) onDismiss() },
        title = { Text("Import Backup") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Text(
                    text = "Paste your backup code and enter the password you used when creating the backup.",
                    style = MaterialTheme.typography.bodyMedium
                )
                OutlinedTextField(
                    value = backupData,
                    onValueChange = { backupData = it },
                    label = { Text("Backup Code") },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 3,
                    enabled = !isLoading
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("Password") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !isLoading
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (backupData.isNotBlank() && password.isNotBlank()) {
                        isLoading = true
                        coroutineScope.launch {
                            val success = onImport(backupData.trim(), password)
                            if (success) {
                                onResult(true, "Backup restored successfully")
                            } else {
                                onResult(false, "Failed to restore backup. Check your password.")
                            }
                            isLoading = false
                        }
                    }
                },
                enabled = backupData.isNotBlank() && password.isNotBlank() && !isLoading
            ) {
                if (isLoading) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp))
                } else {
                    Text("Import")
                }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !isLoading) {
                Text("Cancel")
            }
        }
    )
}

@Composable
fun EditDisplayNameDialog(
    currentName: String,
    onDismiss: () -> Unit,
    onUpdateName: suspend (String) -> Boolean,
    onResult: (Boolean, String) -> Unit
) {
    var newName by remember { mutableStateOf(currentName) }
    var isLoading by remember { mutableStateOf(false) }
    val coroutineScope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = { if (!isLoading) onDismiss() },
        title = { Text("Edit Display Name") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Text(
                    text = "Enter your new display name. This is how contacts will see you.",
                    style = MaterialTheme.typography.bodyMedium
                )
                OutlinedTextField(
                    value = newName,
                    onValueChange = { newName = it },
                    label = { Text("Display Name") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !isLoading
                )
                if (newName.isBlank()) {
                    Text(
                        text = "Name cannot be empty",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (newName.isNotBlank() && newName != currentName) {
                        isLoading = true
                        coroutineScope.launch {
                            val success = onUpdateName(newName.trim())
                            if (success) {
                                onResult(true, "Display name updated")
                            } else {
                                onResult(false, "Failed to update display name")
                            }
                            isLoading = false
                        }
                    } else if (newName == currentName) {
                        onDismiss()
                    }
                },
                enabled = newName.isNotBlank() && !isLoading
            ) {
                if (isLoading) {
                    CircularProgressIndicator(modifier = Modifier.size(16.dp))
                } else {
                    Text("Save")
                }
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !isLoading) {
                Text("Cancel")
            }
        }
    )
}

// Password strength types and UI

enum class PasswordStrengthLevel {
    TooWeak,
    Fair,
    Strong,
    VeryStrong
}

data class PasswordStrengthResult(
    val level: PasswordStrengthLevel = PasswordStrengthLevel.TooWeak,
    val description: String = "",
    val feedback: String = "",
    val isAcceptable: Boolean = false
)

@Composable
fun PasswordStrengthIndicator(strength: PasswordStrengthResult) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(4.dp)
    ) {
        // Strength bar
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            val segmentCount = 4
            val filledSegments = when (strength.level) {
                PasswordStrengthLevel.TooWeak -> 1
                PasswordStrengthLevel.Fair -> 2
                PasswordStrengthLevel.Strong -> 3
                PasswordStrengthLevel.VeryStrong -> 4
            }
            val color = when (strength.level) {
                PasswordStrengthLevel.TooWeak -> MaterialTheme.colorScheme.error
                PasswordStrengthLevel.Fair -> MaterialTheme.colorScheme.tertiary
                PasswordStrengthLevel.Strong -> MaterialTheme.colorScheme.primary
                PasswordStrengthLevel.VeryStrong -> MaterialTheme.colorScheme.primary
            }

            repeat(segmentCount) { index ->
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .height(4.dp)
                        .padding(horizontal = 1.dp)
                ) {
                    Surface(
                        modifier = Modifier.fillMaxSize(),
                        color = if (index < filledSegments) color else color.copy(alpha = 0.2f),
                        shape = MaterialTheme.shapes.small
                    ) {}
                }
            }
        }

        // Strength description
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            Text(
                text = strength.description,
                style = MaterialTheme.typography.labelSmall,
                color = when (strength.level) {
                    PasswordStrengthLevel.TooWeak -> MaterialTheme.colorScheme.error
                    PasswordStrengthLevel.Fair -> MaterialTheme.colorScheme.tertiary
                    else -> MaterialTheme.colorScheme.primary
                }
            )
            if (strength.isAcceptable) {
                Text(
                    text = "OK",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary
                )
            }
        }

        // Feedback for weak passwords
        if (strength.feedback.isNotEmpty()) {
            Text(
                text = strength.feedback,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}
