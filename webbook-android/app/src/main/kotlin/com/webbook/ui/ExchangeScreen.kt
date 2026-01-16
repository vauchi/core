package com.webbook.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import uniffi.webbook_mobile.MobileExchangeData

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ExchangeScreen(
    onBack: () -> Unit,
    onGenerateQr: suspend () -> MobileExchangeData?,
    onScanQr: () -> Unit
) {
    var exchangeData by remember { mutableStateOf<MobileExchangeData?>(null) }
    var qrBitmap by remember { mutableStateOf<Bitmap?>(null) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(Unit) {
        exchangeData = onGenerateQr()
        exchangeData?.let { data ->
            qrBitmap = generateQrBitmap(data.qrData)
        }
        isLoading = false
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Exchange Contact") },
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
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp)
        ) {
            if (isLoading) {
                CircularProgressIndicator()
                Text("Generating QR code...")
            } else {
                Text(
                    text = "Show this QR code to add a contact",
                    style = MaterialTheme.typography.bodyLarge
                )

                qrBitmap?.let { bitmap ->
                    Card(
                        modifier = Modifier.size(280.dp),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)
                    ) {
                        Box(
                            modifier = Modifier.fillMaxSize(),
                            contentAlignment = Alignment.Center
                        ) {
                            Image(
                                bitmap = bitmap.asImageBitmap(),
                                contentDescription = "QR Code",
                                modifier = Modifier.size(260.dp)
                            )
                        }
                    }
                }

                exchangeData?.let { data ->
                    Text(
                        text = "Expires: ${formatExpiry(data.expiresAt)}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }

                Spacer(modifier = Modifier.weight(1f))

                Button(
                    onClick = onScanQr,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Scan Contact's QR Code")
                }
            }
        }
    }
}

@Composable
fun ScanQrDialog(
    onDismiss: () -> Unit,
    onScan: (String) -> Unit
) {
    var manualInput by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Scan QR Code") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                Text(
                    text = "Camera scanning coming soon. For now, paste the QR data:",
                    style = MaterialTheme.typography.bodyMedium
                )
                OutlinedTextField(
                    value = manualInput,
                    onValueChange = { manualInput = it },
                    label = { Text("QR Data (wb://...)") },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 3
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onScan(manualInput) },
                enabled = manualInput.isNotBlank()
            ) {
                Text("Add Contact")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Cancel")
            }
        }
    )
}

private fun generateQrBitmap(data: String): Bitmap {
    val writer = QRCodeWriter()
    val bitMatrix = writer.encode(data, BarcodeFormat.QR_CODE, 512, 512)
    val width = bitMatrix.width
    val height = bitMatrix.height
    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.RGB_565)
    for (x in 0 until width) {
        for (y in 0 until height) {
            bitmap.setPixel(x, y, if (bitMatrix[x, y]) 0xFF000000.toInt() else 0xFFFFFFFF.toInt())
        }
    }
    return bitmap
}

private fun formatExpiry(timestamp: ULong): String {
    val seconds = timestamp.toLong()
    val now = System.currentTimeMillis() / 1000
    val diff = seconds - now
    return when {
        diff < 60 -> "Less than a minute"
        diff < 3600 -> "${diff / 60} minutes"
        else -> "${diff / 3600} hours"
    }
}
