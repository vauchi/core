package com.vauchi.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import com.vauchi.util.ClipboardUtils

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecoveryScreen(
    viewModel: MainViewModel,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var selectedTab by remember { mutableStateOf(0) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Recovery") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // Tab selection
            TabRow(selectedTabIndex = selectedTab) {
                Tab(
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 },
                    text = { Text("Recover") }
                )
                Tab(
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 },
                    text = { Text("Help Others") }
                )
            }

            when (selectedTab) {
                0 -> RecoverIdentityContent(viewModel = viewModel, context = context, scope = scope)
                1 -> HelpOthersContent(viewModel = viewModel, context = context, scope = scope)
            }
        }
    }
}

@Composable
fun RecoverIdentityContent(
    viewModel: MainViewModel,
    context: Context,
    scope: kotlinx.coroutines.CoroutineScope
) {
    var oldPublicKey by remember { mutableStateOf("") }
    var showClaimDialog by remember { mutableStateOf(false) }
    var isCreatingClaim by remember { mutableStateOf(false) }
    var generatedClaimData by remember { mutableStateOf<String?>(null) }

    fun copyToClipboard(text: String, label: String) {
        // Auto-clear after 30 seconds for sensitive recovery data
        ClipboardUtils.copyWithAutoClear(context, scope, text, label)
    }

    Column(
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Info card
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.primaryContainer
            )
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Icon(
                    Icons.Default.Lock,
                    contentDescription = null,
                    modifier = Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.onPrimaryContainer
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Lost Your Device?",
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "You can recover your contact relationships through social vouching.",
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onPrimaryContainer
                )
            }
        }

        // Recovery Settings
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.tertiaryContainer
            )
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "Recovery Settings",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onTertiaryContainer
                )
                Spacer(modifier = Modifier.height(8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(
                        text = "Required vouchers:",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                    Text(
                        text = "3",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(
                        text = "Claim expiry:",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                    Text(
                        text = "7 days",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onTertiaryContainer
                    )
                }
            }
        }

        // Steps
        Text(
            text = "How Recovery Works",
            style = MaterialTheme.typography.titleMedium
        )

        RecoveryStep(
            number = 1,
            title = "Create New Identity",
            description = "First, create a new identity on your new device."
        )

        RecoveryStep(
            number = 2,
            title = "Generate Recovery Claim",
            description = "Create a claim using your OLD public key from your lost identity."
        )

        RecoveryStep(
            number = 3,
            title = "Collect Vouchers",
            description = "Meet with 3+ trusted contacts in person. Have them vouch for your recovery."
        )

        RecoveryStep(
            number = 4,
            title = "Share Recovery Proof",
            description = "Once you have enough vouchers, share your recovery proof with all contacts."
        )

        Spacer(modifier = Modifier.height(8.dp))

        // Create Claim Button
        Button(
            onClick = { showClaimDialog = true },
            modifier = Modifier.fillMaxWidth()
        ) {
            Text("Start Recovery Process")
        }
    }

    if (showClaimDialog) {
        AlertDialog(
            onDismissRequest = {
                if (!isCreatingClaim) {
                    showClaimDialog = false
                    generatedClaimData = null
                }
            },
            title = { Text(if (generatedClaimData != null) "Recovery Claim Created" else "Create Recovery Claim") },
            text = {
                Column {
                    if (generatedClaimData != null) {
                        Text(
                            text = "Share this claim with your trusted contacts:",
                            style = MaterialTheme.typography.bodyMedium
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Card(
                            modifier = Modifier.fillMaxWidth(),
                            colors = CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.surfaceVariant
                            )
                        ) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text(
                                    text = generatedClaimData!!.take(60) + "...",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                        OutlinedButton(
                            onClick = {
                                copyToClipboard(generatedClaimData!!, "Recovery Claim")
                            },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Icon(Icons.Default.ContentCopy, contentDescription = null)
                            Spacer(modifier = Modifier.width(8.dp))
                            Text("Copy Claim Data")
                        }
                    } else {
                        Text(
                            text = "Enter your OLD public key (from backup or previous device):",
                            style = MaterialTheme.typography.bodyMedium
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        OutlinedTextField(
                            value = oldPublicKey,
                            onValueChange = { oldPublicKey = it },
                            label = { Text("Old Public Key (hex)") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = false,
                            minLines = 2,
                            enabled = !isCreatingClaim
                        )
                    }
                }
            },
            confirmButton = {
                if (generatedClaimData != null) {
                    TextButton(
                        onClick = {
                            showClaimDialog = false
                            generatedClaimData = null
                            oldPublicKey = ""
                        }
                    ) {
                        Text("Done")
                    }
                } else {
                    TextButton(
                        onClick = {
                            scope.launch {
                                isCreatingClaim = true
                                val claim = viewModel.createRecoveryClaim(oldPublicKey.trim())
                                if (claim != null) {
                                    generatedClaimData = claim.claimData
                                }
                                isCreatingClaim = false
                            }
                        },
                        enabled = oldPublicKey.length >= 64 && !isCreatingClaim
                    ) {
                        if (isCreatingClaim) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(16.dp),
                                strokeWidth = 2.dp
                            )
                        } else {
                            Text("Create Claim")
                        }
                    }
                }
            },
            dismissButton = {
                if (generatedClaimData == null) {
                    TextButton(
                        onClick = { showClaimDialog = false },
                        enabled = !isCreatingClaim
                    ) {
                        Text("Cancel")
                    }
                }
            }
        )
    }
}

@Composable
fun HelpOthersContent(
    viewModel: MainViewModel,
    context: Context,
    scope: kotlinx.coroutines.CoroutineScope
) {
    var claimData by remember { mutableStateOf("") }
    var showVouchDialog by remember { mutableStateOf(false) }
    var isCreatingVoucher by remember { mutableStateOf(false) }
    var isParsing by remember { mutableStateOf(false) }
    var parsedClaimInfo by remember { mutableStateOf<uniffi.vauchi_mobile.MobileRecoveryClaim?>(null) }
    var generatedVoucherData by remember { mutableStateOf<String?>(null) }

    fun copyToClipboard(text: String, label: String) {
        // Auto-clear after 30 seconds for sensitive recovery data
        ClipboardUtils.copyWithAutoClear(context, scope, text, label)
    }

    Column(
        verticalArrangement = Arrangement.spacedBy(16.dp)
    ) {
        // Info card
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer
            )
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Icon(
                    Icons.Default.CheckCircle,
                    contentDescription = null,
                    modifier = Modifier.size(48.dp),
                    tint = MaterialTheme.colorScheme.onSecondaryContainer
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Help a Contact Recover",
                    style = MaterialTheme.typography.titleMedium,
                    textAlign = TextAlign.Center
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "If a contact lost their device, you can vouch for their identity.",
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    color = MaterialTheme.colorScheme.onSecondaryContainer
                )
            }
        }

        // Warning
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.errorContainer
            )
        ) {
            Row(
                modifier = Modifier.padding(16.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Icon(
                    Icons.Default.Warning,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onErrorContainer
                )
                Spacer(modifier = Modifier.width(12.dp))
                Text(
                    text = "Only vouch for someone you can verify IN PERSON. This prevents identity theft.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onErrorContainer
                )
            }
        }

        // Steps
        Text(
            text = "How to Vouch",
            style = MaterialTheme.typography.titleMedium
        )

        RecoveryStep(
            number = 1,
            title = "Verify Identity",
            description = "Meet your contact in person. Verify they are who they claim to be."
        )

        RecoveryStep(
            number = 2,
            title = "Get Their Claim",
            description = "They will share their claim data with you."
        )

        RecoveryStep(
            number = 3,
            title = "Create Voucher",
            description = "Sign a voucher confirming their identity."
        )

        RecoveryStep(
            number = 4,
            title = "Share Voucher",
            description = "Give them the voucher data to add to their recovery proof."
        )

        Spacer(modifier = Modifier.height(8.dp))

        // Vouch Button
        Button(
            onClick = { showVouchDialog = true },
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.secondary
            )
        ) {
            Text("Vouch for Someone")
        }
    }

    if (showVouchDialog) {
        AlertDialog(
            onDismissRequest = {
                if (!isCreatingVoucher && !isParsing) {
                    showVouchDialog = false
                    claimData = ""
                    parsedClaimInfo = null
                    generatedVoucherData = null
                }
            },
            title = {
                Text(
                    when {
                        generatedVoucherData != null -> "Voucher Created"
                        parsedClaimInfo != null -> "Confirm Voucher"
                        else -> "Vouch for Recovery"
                    }
                )
            },
            text = {
                Column {
                    when {
                        generatedVoucherData != null -> {
                            Text(
                                text = "Give this voucher to your contact:",
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            Card(
                                modifier = Modifier.fillMaxWidth(),
                                colors = CardDefaults.cardColors(
                                    containerColor = MaterialTheme.colorScheme.surfaceVariant
                                )
                            ) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    Text(
                                        text = generatedVoucherData!!.take(60) + "...",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                            }
                            Spacer(modifier = Modifier.height(8.dp))
                            OutlinedButton(
                                onClick = {
                                    copyToClipboard(generatedVoucherData!!, "Recovery Voucher")
                                },
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Icon(Icons.Default.ContentCopy, contentDescription = null)
                                Spacer(modifier = Modifier.width(8.dp))
                                Text("Copy Voucher Data")
                            }
                        }
                        parsedClaimInfo != null -> {
                            Text(
                                text = "Claim Details:",
                                style = MaterialTheme.typography.titleSmall
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                text = "Old ID: ${parsedClaimInfo!!.oldPublicKey.take(16)}...",
                                style = MaterialTheme.typography.bodySmall
                            )
                            Text(
                                text = "New ID: ${parsedClaimInfo!!.newPublicKey.take(16)}...",
                                style = MaterialTheme.typography.bodySmall
                            )
                            if (parsedClaimInfo!!.isExpired) {
                                Spacer(modifier = Modifier.height(8.dp))
                                Text(
                                    text = "This claim has EXPIRED!",
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.error
                                )
                            } else {
                                Spacer(modifier = Modifier.height(8.dp))
                                Text(
                                    text = "Verify this person's identity IN PERSON before vouching!",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.error
                                )
                            }
                        }
                        else -> {
                            Text(
                                text = "Paste the recovery claim data from your contact:",
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            OutlinedTextField(
                                value = claimData,
                                onValueChange = { claimData = it },
                                label = { Text("Claim Data (base64)") },
                                modifier = Modifier.fillMaxWidth(),
                                singleLine = false,
                                minLines = 3,
                                enabled = !isParsing
                            )
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(
                                text = "Verify this person's identity IN PERSON before vouching!",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.error
                            )
                        }
                    }
                }
            },
            confirmButton = {
                when {
                    generatedVoucherData != null -> {
                        TextButton(
                            onClick = {
                                showVouchDialog = false
                                claimData = ""
                                parsedClaimInfo = null
                                generatedVoucherData = null
                            }
                        ) {
                            Text("Done")
                        }
                    }
                    parsedClaimInfo != null -> {
                        TextButton(
                            onClick = {
                                scope.launch {
                                    isCreatingVoucher = true
                                    val voucher = viewModel.createRecoveryVoucher(claimData.trim())
                                    if (voucher != null) {
                                        generatedVoucherData = voucher.voucherData
                                    }
                                    isCreatingVoucher = false
                                }
                            },
                            enabled = !parsedClaimInfo!!.isExpired && !isCreatingVoucher
                        ) {
                            if (isCreatingVoucher) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 2.dp
                                )
                            } else {
                                Text("Create Voucher")
                            }
                        }
                    }
                    else -> {
                        TextButton(
                            onClick = {
                                scope.launch {
                                    isParsing = true
                                    val claim = viewModel.parseRecoveryClaim(claimData.trim())
                                    if (claim != null) {
                                        parsedClaimInfo = claim
                                    }
                                    isParsing = false
                                }
                            },
                            enabled = claimData.length >= 20 && !isParsing
                        ) {
                            if (isParsing) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 2.dp
                                )
                            } else {
                                Text("Verify Claim")
                            }
                        }
                    }
                }
            },
            dismissButton = {
                if (generatedVoucherData == null) {
                    TextButton(
                        onClick = {
                            if (parsedClaimInfo != null) {
                                parsedClaimInfo = null
                            } else {
                                showVouchDialog = false
                                claimData = ""
                            }
                        },
                        enabled = !isCreatingVoucher && !isParsing
                    ) {
                        Text(if (parsedClaimInfo != null) "Back" else "Cancel")
                    }
                }
            }
        )
    }
}

@Composable
fun RecoveryStep(
    number: Int,
    title: String,
    description: String
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.Top
    ) {
        Surface(
            shape = MaterialTheme.shapes.small,
            color = MaterialTheme.colorScheme.primaryContainer,
            modifier = Modifier.size(32.dp)
        ) {
            Box(contentAlignment = Alignment.Center) {
                Text(
                    text = number.toString(),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer
                )
            }
        }
        Spacer(modifier = Modifier.width(12.dp))
        Column {
            Text(
                text = title,
                style = MaterialTheme.typography.titleSmall
            )
            Text(
                text = description,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}
