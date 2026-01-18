package com.webbook.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.webbook.util.ContactActions
import kotlinx.coroutines.launch
import uniffi.webbook_mobile.MobileContact
import uniffi.webbook_mobile.MobileContactCard
import uniffi.webbook_mobile.MobileContactField

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContactDetailScreen(
    contactId: String,
    onBack: () -> Unit,
    onGetContact: suspend (String) -> MobileContact?,
    onGetOwnCard: suspend () -> MobileContactCard?,
    onSetFieldVisibility: (String, String, Boolean) -> Unit,
    onIsFieldVisible: suspend (String, String) -> Boolean,
    onVerifyContact: suspend (String) -> Boolean,
    onGetOwnPublicKey: suspend () -> String?
) {
    var contact by remember { mutableStateOf<MobileContact?>(null) }
    var ownCard by remember { mutableStateOf<MobileContactCard?>(null) }
    var ownPublicKey by remember { mutableStateOf<String?>(null) }
    var fieldVisibility by remember { mutableStateOf<Map<String, Boolean>>(emptyMap()) }
    var isLoading by remember { mutableStateOf(true) }
    var showVerification by remember { mutableStateOf(false) }
    var isVerifying by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(contactId) {
        contact = onGetContact(contactId)
        ownCard = onGetOwnCard()
        ownPublicKey = onGetOwnPublicKey()

        // Load visibility for each of our fields
        ownCard?.let { card ->
            val visibilityMap = mutableMapOf<String, Boolean>()
            card.fields.forEach { field ->
                visibilityMap[field.label] = onIsFieldVisible(contactId, field.label)
            }
            fieldVisibility = visibilityMap
        }
        isLoading = false
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(contact?.displayName ?: "Contact") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        if (isLoading) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                contentAlignment = Alignment.Center
            ) {
                CircularProgressIndicator()
            }
        } else if (contact == null) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                contentAlignment = Alignment.Center
            ) {
                Text("Contact not found")
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                contentPadding = PaddingValues(vertical = 16.dp)
            ) {
                // Contact Info Section
                item {
                    Text(
                        text = "Their Info",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                }

                contact?.let { c ->
                    if (c.card.fields.isEmpty()) {
                        item {
                            Text(
                                text = "No contact info shared",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    } else {
                        items(c.card.fields) { field ->
                            ContactFieldItem(field = field)
                        }
                    }

                    // Verification status
                    item {
                        Spacer(modifier = Modifier.height(8.dp))
                        Card(
                            modifier = Modifier.fillMaxWidth(),
                            colors = CardDefaults.cardColors(
                                containerColor = if (c.isVerified)
                                    MaterialTheme.colorScheme.primaryContainer
                                else
                                    MaterialTheme.colorScheme.surfaceVariant
                            )
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
                                        text = if (c.isVerified) "Verified" else "Not Verified",
                                        style = MaterialTheme.typography.titleSmall,
                                        color = if (c.isVerified)
                                            MaterialTheme.colorScheme.onPrimaryContainer
                                        else
                                            MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                    Text(
                                        text = if (c.isVerified)
                                            "You have verified this contact's identity"
                                        else
                                            "Verify fingerprints in person",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = if (c.isVerified)
                                            MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.8f)
                                        else
                                            MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.8f)
                                    )
                                }
                                if (!c.isVerified) {
                                    Button(
                                        onClick = { showVerification = true }
                                    ) {
                                        Text("Verify")
                                    }
                                }
                            }
                        }
                    }
                }

                // Divider
                item {
                    Spacer(modifier = Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(modifier = Modifier.height(8.dp))
                }

                // Visibility Section
                item {
                    Text(
                        text = "What They Can See",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.primary
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = "Toggle which of your fields this contact can see",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }

                ownCard?.let { card ->
                    if (card.fields.isEmpty()) {
                        item {
                            Text(
                                text = "You have no fields to share",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    } else {
                        items(card.fields) { field ->
                            VisibilityToggleItem(
                                field = field,
                                isVisible = fieldVisibility[field.label] ?: true,
                                onToggle = { visible ->
                                    fieldVisibility = fieldVisibility + (field.label to visible)
                                    onSetFieldVisibility(contactId, field.label, visible)
                                }
                            )
                        }
                    }
                }
            }
        }
    }

    // Verification Dialog
    if (showVerification) {
        AlertDialog(
            onDismissRequest = { if (!isVerifying) showVerification = false },
            title = { Text("Verify ${contact?.displayName}") },
            text = {
                Column(
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Text(
                        text = "Compare these fingerprints with ${contact?.displayName} in person to verify their identity.",
                        style = MaterialTheme.typography.bodyMedium
                    )

                    // Their fingerprint
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant
                        )
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = "Their Fingerprint",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            Text(
                                text = contact?.publicKey?.chunked(4)?.joinToString(" ")?.uppercase() ?: "",
                                style = MaterialTheme.typography.bodySmall,
                                maxLines = 3
                            )
                        }
                    }

                    // Our fingerprint
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant
                        )
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text(
                                text = "Your Fingerprint",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                            Text(
                                text = ownPublicKey?.chunked(4)?.joinToString(" ")?.uppercase() ?: "",
                                style = MaterialTheme.typography.bodySmall,
                                maxLines = 3
                            )
                        }
                    }

                    Text(
                        text = "Only mark as verified if the fingerprints match!",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        scope.launch {
                            isVerifying = true
                            val success = onVerifyContact(contactId)
                            if (success) {
                                contact = onGetContact(contactId)
                                showVerification = false
                            }
                            isVerifying = false
                        }
                    },
                    enabled = !isVerifying
                ) {
                    if (isVerifying) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(16.dp),
                            strokeWidth = 2.dp
                        )
                    } else {
                        Text("Mark as Verified")
                    }
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { showVerification = false },
                    enabled = !isVerifying
                ) {
                    Text("Cancel")
                }
            }
        )
    }
}

@Composable
fun ContactFieldItem(field: MobileContactField) {
    val context = LocalContext.current

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { ContactActions.openField(context, field) },
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant
        )
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = field.label,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Text(
                    text = field.value,
                    style = MaterialTheme.typography.bodyLarge
                )
                Text(
                    text = ContactActions.getActionDescription(field.fieldType),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary
                )
            }
            Icon(
                imageVector = Icons.Filled.ChevronRight,
                contentDescription = "Open",
                tint = MaterialTheme.colorScheme.primary
            )
        }
    }
}

@Composable
fun VisibilityToggleItem(
    field: MobileContactField,
    isVisible: Boolean,
    onToggle: (Boolean) -> Unit
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = if (isVisible)
                MaterialTheme.colorScheme.surfaceVariant
            else
                MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)
        )
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = field.label,
                    style = MaterialTheme.typography.labelMedium,
                    color = if (isVisible)
                        MaterialTheme.colorScheme.onSurfaceVariant
                    else
                        MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
                )
                Text(
                    text = field.value,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (isVisible)
                        MaterialTheme.colorScheme.onSurface
                    else
                        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f)
                )
            }
            Switch(
                checked = isVisible,
                onCheckedChange = onToggle
            )
        }
    }
}
