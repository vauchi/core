package com.vauchi

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Warning
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import com.vauchi.ui.ExchangeScreen
import com.vauchi.ui.ContactsScreen
import com.vauchi.ui.ContactDetailScreen
import com.vauchi.ui.QrScannerScreen
import com.vauchi.ui.SettingsScreen
import com.vauchi.ui.DevicesScreen
import com.vauchi.ui.RecoveryScreen
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import kotlinx.coroutines.launch
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.vauchi.ui.MainViewModel
import com.vauchi.ui.PasswordStrengthResult
import com.vauchi.ui.SyncState
import com.vauchi.ui.UiState
import com.vauchi.ui.theme.VauchiTheme
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import uniffi.vauchi_mobile.MobileContactCard
import uniffi.vauchi_mobile.MobileFieldType

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            VauchiTheme {
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
    Home, Exchange, Contacts, ContactDetail, QrScanner, Settings, Devices, Recovery
}

@Composable
fun MainScreen(viewModel: MainViewModel = viewModel()) {
    val uiState by viewModel.uiState.collectAsState()
    val snackbarMessage by viewModel.snackbarMessage.collectAsState()
    val syncState by viewModel.syncState.collectAsState()
    val isOnline by viewModel.isOnline.collectAsState()
    val lastSyncTime by viewModel.lastSyncTime.collectAsState()
    var currentScreen by remember { mutableStateOf(Screen.Home) }
    var selectedContactId by remember { mutableStateOf<String?>(null) }
    val coroutineScope = rememberCoroutineScope()
    val snackbarHostState = remember { SnackbarHostState() }
    val lifecycleOwner = LocalLifecycleOwner.current

    // Auto-sync when app comes to foreground
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME && uiState is UiState.Ready) {
                viewModel.sync()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    // Show snackbar when message changes
    LaunchedEffect(snackbarMessage) {
        snackbarMessage?.let {
            snackbarHostState.showSnackbar(it)
            viewModel.clearSnackbar()
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
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
                    onContacts = { currentScreen = Screen.Contacts },
                    onSettings = { currentScreen = Screen.Settings },
                    socialNetworks = viewModel.listSocialNetworks(),
                    onGetProfileUrl = viewModel::getProfileUrl,
                    syncState = syncState,
                    isOnline = isOnline,
                    lastSyncTime = lastSyncTime,
                    onSync = { viewModel.sync() }
                )
                is UiState.Error -> ErrorScreen(
                    message = state.message,
                    onRetry = { viewModel.refresh() }
                )
            }
        }
        Screen.Exchange -> {
            ExchangeScreen(
                onBack = { currentScreen = Screen.Home },
                onGenerateQr = { viewModel.generateExchangeQr() },
                onScanQr = { currentScreen = Screen.QrScanner }
            )
        }
        Screen.QrScanner -> {
            QrScannerScreen(
                onBack = { currentScreen = Screen.Exchange },
                onQrScanned = { qrData ->
                    coroutineScope.launch {
                        viewModel.completeExchange(qrData)
                        currentScreen = Screen.Home
                    }
                }
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
                },
                syncState = syncState,
                onSync = { viewModel.sync() }
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
                    },
                    onVerifyContact = { viewModel.verifyContact(it) },
                    onGetOwnPublicKey = { viewModel.getOwnPublicKey() }
                )
            }
        }
        Screen.Settings -> {
            val state = uiState
            if (state is UiState.Ready) {
                SettingsScreen(
                    displayName = state.displayName,
                    onBack = { currentScreen = Screen.Home },
                    onExportBackup = { password -> viewModel.exportBackup(password) },
                    onImportBackup = { data, password -> viewModel.importBackup(data, password) },
                    relayUrl = viewModel.getRelayUrl(),
                    onRelayUrlChange = { viewModel.setRelayUrl(it) },
                    syncState = syncState,
                    onSync = { viewModel.sync() },
                    onDevices = { currentScreen = Screen.Devices },
                    onRecovery = { currentScreen = Screen.Recovery },
                    onCheckPasswordStrength = { viewModel.checkPasswordStrength(it) }
                )
            }
        }
        Screen.Devices -> {
            val state = uiState
            if (state is UiState.Ready) {
                DevicesScreen(
                    displayName = state.displayName,
                    publicId = state.publicId,
                    onBack = { currentScreen = Screen.Settings },
                    onGenerateLink = { null } // TODO: implement device linking
                )
            }
        }
        Screen.Recovery -> {
            RecoveryScreen(
                viewModel = viewModel,
                onBack = { currentScreen = Screen.Settings }
            )
        }
        }

        // Snackbar for feedback messages
        SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter)
        )
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
            text = "Welcome to Vauchi",
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
    onContacts: () -> Unit,
    onSettings: () -> Unit,
    socialNetworks: List<uniffi.vauchi_mobile.MobileSocialNetwork> = emptyList(),
    onGetProfileUrl: (String, String) -> String? = { _, _ -> null },
    syncState: SyncState = SyncState.Idle,
    isOnline: Boolean = true,
    lastSyncTime: Instant? = null,
    onSync: () -> Unit = {}
) {
    var showAddDialog by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Vauchi") },
                actions = {
                    // Sync status indicator
                    SyncStatusChip(
                        syncState = syncState,
                        isOnline = isOnline,
                        lastSyncTime = lastSyncTime,
                        onSync = onSync
                    )
                    IconButton(onClick = onSettings) {
                        Icon(Icons.Default.Settings, contentDescription = "Settings")
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showAddDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add field")
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            // Offline banner
            if (!isOnline) {
                OfflineBanner()
            }

            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
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
                    val context = LocalContext.current
                    val isSocialField = field.fieldType == MobileFieldType.SOCIAL
                    val profileUrl = if (isSocialField) {
                        onGetProfileUrl(field.label, field.value)
                    } else null

                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .then(
                                if (profileUrl != null) {
                                    Modifier.clickable {
                                        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(profileUrl))
                                        context.startActivity(intent)
                                    }
                                } else Modifier
                            ),
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
                                    text = if (isSocialField) "@${field.value}" else field.value,
                                    style = MaterialTheme.typography.bodyLarge,
                                    color = if (profileUrl != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
                                )
                            }
                            if (profileUrl != null) {
                                Icon(
                                    Icons.Default.Share,
                                    contentDescription = "Open profile",
                                    tint = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier.size(20.dp)
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
    }

    if (showAddDialog) {
        AddFieldDialog(
            onDismiss = { showAddDialog = false },
            onAdd = { type, label, value ->
                onAddField(type, label, value)
                showAddDialog = false
            },
            socialNetworks = socialNetworks
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddFieldDialog(
    onDismiss: () -> Unit,
    onAdd: (MobileFieldType, String, String) -> Unit,
    socialNetworks: List<uniffi.vauchi_mobile.MobileSocialNetwork> = emptyList()
) {
    var selectedType by remember { mutableStateOf(MobileFieldType.EMAIL) }
    var label by remember { mutableStateOf("") }
    var value by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }
    var socialExpanded by remember { mutableStateOf(false) }
    var selectedNetwork by remember { mutableStateOf<uniffi.vauchi_mobile.MobileSocialNetwork?>(null) }
    var socialSearch by remember { mutableStateOf("") }

    val fieldTypes = listOf(
        MobileFieldType.EMAIL to "Email",
        MobileFieldType.PHONE to "Phone",
        MobileFieldType.WEBSITE to "Website",
        MobileFieldType.ADDRESS to "Address",
        MobileFieldType.SOCIAL to "Social",
        MobileFieldType.CUSTOM to "Custom"
    )

    // Filter social networks by search
    val filteredNetworks = remember(socialSearch, socialNetworks) {
        if (socialSearch.isBlank()) {
            socialNetworks.take(10)
        } else {
            socialNetworks.filter {
                it.displayName.contains(socialSearch, ignoreCase = true)
            }.take(10)
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Field") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                // Field type dropdown
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
                                    if (type != MobileFieldType.SOCIAL && label.isEmpty()) {
                                        label = name
                                    }
                                    if (type == MobileFieldType.SOCIAL) {
                                        label = ""
                                        selectedNetwork = null
                                    }
                                    expanded = false
                                }
                            )
                        }
                    }
                }

                // Social network picker (only shown for SOCIAL type)
                if (selectedType == MobileFieldType.SOCIAL) {
                    ExposedDropdownMenuBox(
                        expanded = socialExpanded,
                        onExpandedChange = { socialExpanded = !socialExpanded }
                    ) {
                        OutlinedTextField(
                            value = selectedNetwork?.displayName ?: socialSearch,
                            onValueChange = {
                                socialSearch = it
                                selectedNetwork = null
                                socialExpanded = true
                            },
                            label = { Text("Social Network") },
                            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = socialExpanded) },
                            modifier = Modifier
                                .menuAnchor()
                                .fillMaxWidth()
                        )
                        ExposedDropdownMenu(
                            expanded = socialExpanded,
                            onDismissRequest = { socialExpanded = false }
                        ) {
                            filteredNetworks.forEach { network ->
                                DropdownMenuItem(
                                    text = { Text(network.displayName) },
                                    onClick = {
                                        selectedNetwork = network
                                        label = network.displayName
                                        socialSearch = network.displayName
                                        socialExpanded = false
                                    }
                                )
                            }
                            if (filteredNetworks.isEmpty()) {
                                DropdownMenuItem(
                                    text = { Text("No networks found") },
                                    onClick = { },
                                    enabled = false
                                )
                            }
                        }
                    }

                    OutlinedTextField(
                        value = value,
                        onValueChange = { value = it },
                        label = { Text("Username") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                } else {
                    // Regular label and value fields
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
fun ErrorScreen(
    message: String,
    onRetry: () -> Unit = {}
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        contentAlignment = Alignment.Center
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(
                Icons.Default.Warning,
                contentDescription = null,
                modifier = Modifier.size(64.dp),
                tint = MaterialTheme.colorScheme.error
            )
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = "Something went wrong",
                style = MaterialTheme.typography.headlineMedium,
                color = MaterialTheme.colorScheme.error
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = message,
                style = MaterialTheme.typography.bodyLarge,
                textAlign = TextAlign.Center,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Spacer(modifier = Modifier.height(24.dp))
            Button(onClick = onRetry) {
                Icon(Icons.Default.Refresh, contentDescription = null)
                Spacer(modifier = Modifier.width(8.dp))
                Text("Retry")
            }
        }
    }
}

@Composable
fun OfflineBanner() {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.errorContainer)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(
            Icons.Default.Warning,
            contentDescription = null,
            modifier = Modifier.size(16.dp),
            tint = MaterialTheme.colorScheme.onErrorContainer
        )
        Spacer(modifier = Modifier.width(8.dp))
        Text(
            text = "You're offline",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onErrorContainer
        )
    }
}

@Composable
fun SyncStatusChip(
    syncState: SyncState,
    isOnline: Boolean,
    lastSyncTime: Instant?,
    onSync: () -> Unit
) {
    val (text, color) = when {
        !isOnline -> "Offline" to MaterialTheme.colorScheme.outline
        syncState is SyncState.Syncing -> "Syncing..." to MaterialTheme.colorScheme.primary
        syncState is SyncState.Error -> "Sync failed" to MaterialTheme.colorScheme.error
        syncState is SyncState.Success || lastSyncTime != null -> {
            val timeText = lastSyncTime?.let {
                val formatter = DateTimeFormatter.ofPattern("HH:mm")
                    .withZone(ZoneId.systemDefault())
                formatter.format(it)
            } ?: ""
            "Synced $timeText" to MaterialTheme.colorScheme.primary
        }
        else -> "Tap to sync" to MaterialTheme.colorScheme.outline
    }

    TextButton(
        onClick = { if (isOnline && syncState !is SyncState.Syncing) onSync() },
        enabled = isOnline && syncState !is SyncState.Syncing
    ) {
        if (syncState is SyncState.Syncing) {
            CircularProgressIndicator(
                modifier = Modifier.size(16.dp),
                strokeWidth = 2.dp
            )
            Spacer(modifier = Modifier.width(4.dp))
        }
        Text(
            text = text,
            style = MaterialTheme.typography.labelMedium,
            color = color
        )
    }
}
