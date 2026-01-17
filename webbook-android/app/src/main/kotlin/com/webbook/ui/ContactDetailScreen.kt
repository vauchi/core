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
    onIsFieldVisible: suspend (String, String) -> Boolean
) {
    var contact by remember { mutableStateOf<MobileContact?>(null) }
    var ownCard by remember { mutableStateOf<MobileContactCard?>(null) }
    var fieldVisibility by remember { mutableStateOf<Map<String, Boolean>>(emptyMap()) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(contactId) {
        contact = onGetContact(contactId)
        ownCard = onGetOwnCard()

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
