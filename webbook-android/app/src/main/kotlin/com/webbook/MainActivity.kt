package com.webbook

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Person
import com.webbook.ui.ExchangeScreen
import com.webbook.ui.ContactsScreen
import com.webbook.ui.ContactDetailScreen
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.webbook.ui.MainViewModel
import com.webbook.ui.UiState
import com.webbook.ui.theme.WebBookTheme
import uniffi.webbook_mobile.MobileContactCard
import uniffi.webbook_mobile.MobileFieldType

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            WebBookTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    MainScreen()
                }
            }
        }
    }
}

enum class Screen {
    Home, Exchange, Contacts, ContactDetail
}

@Composable
fun MainScreen(viewModel: MainViewModel = viewModel()) {
    val uiState by viewModel.uiState.collectAsState()
    var currentScreen by remember { mutableStateOf(Screen.Home) }
    var selectedContactId by remember { mutableStateOf<String?>(null) }

    when (currentScreen) {
        Screen.Home -> {
            when (val state = uiState) {
                is UiState.Loading -> LoadingScreen()
                is UiState.Setup -> SetupScreen(onCreateIdentity = viewModel::createIdentity)
                is UiState.Ready -> ReadyScreen(
                    displayName = state.displayName,
                    publicId = state.publicId,
                    card = state.card,
                    contactCount = state.contactCount,
                    onAddField = viewModel::addField,
                    onRemoveField = viewModel::removeField,
                    onExchange = { currentScreen = Screen.Exchange },
                    onContacts = { currentScreen = Screen.Contacts }
                )
                is UiState.Error -> ErrorScreen(message = state.message)
            }
        }
        Screen.Exchange -> {
            ExchangeScreen(
                onBack = { currentScreen = Screen.Home },
                onGenerateQr = { viewModel.generateExchangeQr() },
                onCompleteExchange = { qrData -> viewModel.completeExchange(qrData) }
            )
        }
        Screen.Contacts -> {
            ContactsScreen(
                onBack = { currentScreen = Screen.Home },
                onListContacts = { viewModel.listContacts() },
                onRemoveContact = { id -> viewModel.removeContact(id) },
                onContactClick = { id ->
                    selectedContactId = id
                    currentScreen = Screen.ContactDetail
                }
            )
        }
        Screen.ContactDetail -> {
            selectedContactId?.let { contactId ->
                ContactDetailScreen(
                    contactId = contactId,
                    onBack = { currentScreen = Screen.Contacts },
                    onGetContact = { viewModel.getContact(it) },
                    onGetOwnCard = { viewModel.getOwnCard() },
                    onSetFieldVisibility = { cId, label, visible ->
                        viewModel.setFieldVisibility(cId, label, visible)
                    },
                    onIsFieldVisible = { cId, label ->
                        viewModel.isFieldVisibleToContact(cId, label)
                    }
                )
            }
        }
    }
}

@Composable
fun LoadingScreen() {
    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            CircularProgressIndicator()
            Spacer(modifier = Modifier.height(16.dp))
            Text("Loading...")
        }
    }
}

@Composable
fun SetupScreen(onCreateIdentity: (String) -> Unit) {
    var name by remember { mutableStateOf("") }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = "Welcome to WebBook",
            style = MaterialTheme.typography.headlineLarge
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "Privacy-focused contact exchange",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(modifier = Modifier.height(48.dp))
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text("Your name") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth()
        )
        Spacer(modifier = Modifier.height(24.dp))
        Button(
            onClick = { onCreateIdentity(name) },
            enabled = name.isNotBlank(),
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("Create Identity")
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReadyScreen(
    displayName: String,
    publicId: String,
    card: MobileContactCard,
    contactCount: UInt,
    onAddField: (MobileFieldType, String, String) -> Unit,
    onRemoveField: (String) -> Unit,
    onExchange: () -> Unit,
    onContacts: () -> Unit
) {
    var showAddDialog by remember { mutableStateOf(false) }

    Scaffold(
        floatingActionButton = {
            FloatingActionButton(onClick = { showAddDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add field")
            }
        }
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            contentPadding = PaddingValues(vertical = 24.dp)
        ) {
            item {
                Text(
                    text = "Hello, $displayName!",
                    style = MaterialTheme.typography.headlineMedium
                )
            }

            item {
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(
                            text = "Your Card",
                            style = MaterialTheme.typography.titleMedium
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = "Public ID: ${publicId.take(16)}...",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                }
            }

            item {
                Text(
                    text = "Fields",
                    style = MaterialTheme.typography.titleMedium
                )
            }

            if (card.fields.isEmpty()) {
                item {
                    Text(
                        text = "No fields yet. Tap + to add contact info!",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            } else {
                items(card.fields) { field ->
                    Card(
                        modifier = Modifier.fillMaxWidth(),
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
                                    style = MaterialTheme.typography.labelMedium
                                )
                                Text(
                                    text = field.value,
                                    style = MaterialTheme.typography.bodyLarge
                                )
                            }
                            IconButton(onClick = { onRemoveField(field.label) }) {
                                Icon(
                                    Icons.Default.Delete,
                                    contentDescription = "Delete",
                                    tint = MaterialTheme.colorScheme.error
                                )
                            }
                        }
                    }
                }
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    text = "Contacts: $contactCount",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }

            item {
                Spacer(modifier = Modifier.height(24.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    Button(
                        onClick = onExchange,
                        modifier = Modifier.weight(1f)
                    ) {
                        Icon(Icons.Default.Share, contentDescription = null)
                        Spacer(modifier = Modifier.width(8.dp))
                        Text("Exchange")
                    }
                    OutlinedButton(
                        onClick = onContacts,
                        modifier = Modifier.weight(1f)
                    ) {
                        Icon(Icons.Default.Person, contentDescription = null)
                        Spacer(modifier = Modifier.width(8.dp))
                        Text("Contacts")
                    }
                }
            }
        }
    }

    if (showAddDialog) {
        AddFieldDialog(
            onDismiss = { showAddDialog = false },
            onAdd = { type, label, value ->
                onAddField(type, label, value)
                showAddDialog = false
            }
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddFieldDialog(
    onDismiss: () -> Unit,
    onAdd: (MobileFieldType, String, String) -> Unit
) {
    var selectedType by remember { mutableStateOf(MobileFieldType.EMAIL) }
    var label by remember { mutableStateOf("") }
    var value by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }

    val fieldTypes = listOf(
        MobileFieldType.EMAIL to "Email",
        MobileFieldType.PHONE to "Phone",
        MobileFieldType.WEBSITE to "Website",
        MobileFieldType.ADDRESS to "Address",
        MobileFieldType.SOCIAL to "Social",
        MobileFieldType.CUSTOM to "Custom"
    )

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Field") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                ExposedDropdownMenuBox(
                    expanded = expanded,
                    onExpandedChange = { expanded = !expanded }
                ) {
                    OutlinedTextField(
                        value = fieldTypes.find { it.first == selectedType }?.second ?: "",
                        onValueChange = {},
                        readOnly = true,
                        label = { Text("Type") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                        modifier = Modifier
                            .menuAnchor()
                            .fillMaxWidth()
                    )
                    ExposedDropdownMenu(
                        expanded = expanded,
                        onDismissRequest = { expanded = false }
                    ) {
                        fieldTypes.forEach { (type, name) ->
                            DropdownMenuItem(
                                text = { Text(name) },
                                onClick = {
                                    selectedType = type
                                    if (label.isEmpty()) {
                                        label = name
                                    }
                                    expanded = false
                                }
                            )
                        }
                    }
                }

                OutlinedTextField(
                    value = label,
                    onValueChange = { label = it },
                    label = { Text("Label") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )

                OutlinedTextField(
                    value = value,
                    onValueChange = { value = it },
                    label = { Text("Value") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onAdd(selectedType, label, value) },
                enabled = label.isNotBlank() && value.isNotBlank()
            ) {
                Text("Add")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        }
    )
}

@Composable
fun ErrorScreen(message: String) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        contentAlignment = Alignment.Center
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = "Error",
                style = MaterialTheme.typography.headlineMedium,
                color = MaterialTheme.colorScheme.error
            )
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = message,
                style = MaterialTheme.typography.bodyLarge,
                textAlign = TextAlign.Center
            )
        }
    }
}
