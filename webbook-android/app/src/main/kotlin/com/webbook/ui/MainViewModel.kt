package com.webbook.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.webbook.data.WebBookRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.webbook_mobile.MobileContact
import uniffi.webbook_mobile.MobileContactCard
import uniffi.webbook_mobile.MobileExchangeData
import uniffi.webbook_mobile.MobileExchangeResult
import uniffi.webbook_mobile.MobileFieldType
import uniffi.webbook_mobile.MobileSocialNetwork
import uniffi.webbook_mobile.MobileSyncResult

sealed class SyncState {
    object Idle : SyncState()
    object Syncing : SyncState()
    data class Success(val result: MobileSyncResult) : SyncState()
    data class Error(val message: String) : SyncState()
}

sealed class UiState {
    object Loading : UiState()
    object Setup : UiState()
    data class Ready(
        val displayName: String,
        val publicId: String,
        val card: MobileContactCard,
        val contactCount: UInt
    ) : UiState()
    data class Error(val message: String) : UiState()
}

class MainViewModel(application: Application) : AndroidViewModel(application) {
    private val repository: WebBookRepository by lazy {
        WebBookRepository(application)
    }

    private val _uiState = MutableStateFlow<UiState>(UiState.Loading)
    val uiState: StateFlow<UiState> = _uiState.asStateFlow()

    // Snackbar message channel for user feedback
    private val _snackbarMessage = MutableStateFlow<String?>(null)
    val snackbarMessage: StateFlow<String?> = _snackbarMessage.asStateFlow()

    // Sync state
    private val _syncState = MutableStateFlow<SyncState>(SyncState.Idle)
    val syncState: StateFlow<SyncState> = _syncState.asStateFlow()

    fun clearSnackbar() {
        _snackbarMessage.value = null
    }

    private fun showMessage(message: String) {
        _snackbarMessage.value = message
    }

    fun clearSyncState() {
        _syncState.value = SyncState.Idle
    }

    init {
        checkIdentity()
    }

    private fun checkIdentity() {
        viewModelScope.launch {
            try {
                val hasIdentity = withContext(Dispatchers.IO) {
                    repository.hasIdentity()
                }
                if (hasIdentity) {
                    loadUserData()
                } else {
                    _uiState.value = UiState.Setup
                }
            } catch (e: Exception) {
                _uiState.value = UiState.Error(e.message ?: "Unknown error")
            }
        }
    }

    fun createIdentity(displayName: String) {
        viewModelScope.launch {
            try {
                _uiState.value = UiState.Loading
                withContext(Dispatchers.IO) {
                    repository.createIdentity(displayName)
                }
                loadUserData()
            } catch (e: Exception) {
                _uiState.value = UiState.Error(e.message ?: "Failed to create identity")
            }
        }
    }

    private suspend fun loadUserData() {
        try {
            val (displayName, publicId, card, contactCount) = withContext(Dispatchers.IO) {
                Tuple4(
                    repository.getDisplayName(),
                    repository.getPublicId(),
                    repository.getOwnCard(),
                    repository.contactCount()
                )
            }
            _uiState.value = UiState.Ready(displayName, publicId, card, contactCount)
        } catch (e: Exception) {
            _uiState.value = UiState.Error(e.message ?: "Failed to load user data")
        }
    }

    fun refresh() {
        viewModelScope.launch {
            loadUserData()
        }
    }

    fun sync() {
        viewModelScope.launch {
            _syncState.value = SyncState.Syncing
            try {
                val result = withContext(Dispatchers.IO) {
                    repository.sync()
                }
                _syncState.value = SyncState.Success(result)
                loadUserData()
                val msg = buildString {
                    append("Sync complete")
                    if (result.contactsAdded > 0u) append(" - ${result.contactsAdded} new contacts")
                    if (result.cardsUpdated > 0u) append(" - ${result.cardsUpdated} cards updated")
                }
                showMessage(msg)
            } catch (e: Exception) {
                _syncState.value = SyncState.Error(e.message ?: "Sync failed")
                showMessage("Sync failed: ${e.message}")
            }
        }
    }

    fun getRelayUrl(): String = repository.getRelayUrl()

    fun setRelayUrl(url: String) {
        repository.setRelayUrl(url)
        showMessage("Relay URL updated (restart app to apply)")
    }

    fun addField(fieldType: MobileFieldType, label: String, value: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.addField(fieldType, label, value)
                }
                loadUserData()
                showMessage("Field added")
            } catch (e: Exception) {
                showMessage("Failed to add field: ${e.message}")
            }
        }
    }

    fun updateField(label: String, newValue: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.updateField(label, newValue)
                }
                loadUserData()
                showMessage("Field updated")
            } catch (e: Exception) {
                showMessage("Failed to update field: ${e.message}")
            }
        }
    }

    fun removeField(label: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.removeField(label)
                }
                loadUserData()
                showMessage("Field removed")
            } catch (e: Exception) {
                showMessage("Failed to remove field: ${e.message}")
            }
        }
    }

    suspend fun generateExchangeQr(): MobileExchangeData? {
        return try {
            withContext(Dispatchers.IO) {
                repository.generateExchangeQr()
            }
        } catch (e: Exception) {
            null
        }
    }

    suspend fun completeExchange(qrData: String): MobileExchangeResult? {
        return try {
            val result = withContext(Dispatchers.IO) {
                repository.completeExchange(qrData)
            }
            loadUserData()
            result
        } catch (e: Exception) {
            null
        }
    }

    suspend fun listContacts(): List<MobileContact> {
        return try {
            withContext(Dispatchers.IO) {
                repository.listContacts()
            }
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun removeContact(id: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.removeContact(id)
                }
                loadUserData()
                showMessage("Contact removed")
            } catch (e: Exception) {
                showMessage("Failed to remove contact: ${e.message}")
            }
        }
    }

    suspend fun getContact(id: String): MobileContact? {
        return try {
            withContext(Dispatchers.IO) {
                repository.getContact(id)
            }
        } catch (e: Exception) {
            null
        }
    }

    suspend fun getOwnCard(): MobileContactCard? {
        return try {
            withContext(Dispatchers.IO) {
                repository.getOwnCard()
            }
        } catch (e: Exception) {
            null
        }
    }

    fun setFieldVisibility(contactId: String, fieldLabel: String, visible: Boolean) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    if (visible) {
                        repository.showFieldToContact(contactId, fieldLabel)
                    } else {
                        repository.hideFieldFromContact(contactId, fieldLabel)
                    }
                }
                showMessage(if (visible) "Field shown to contact" else "Field hidden from contact")
            } catch (e: Exception) {
                showMessage("Failed to update visibility: ${e.message}")
            }
        }
    }

    suspend fun isFieldVisibleToContact(contactId: String, fieldLabel: String): Boolean {
        return try {
            withContext(Dispatchers.IO) {
                repository.isFieldVisibleToContact(contactId, fieldLabel)
            }
        } catch (e: Exception) {
            true // Default to visible on error
        }
    }

    suspend fun exportBackup(password: String): String? {
        return try {
            withContext(Dispatchers.IO) {
                repository.exportBackup(password)
            }
        } catch (e: Exception) {
            null
        }
    }

    suspend fun importBackup(backupData: String, password: String): Boolean {
        return try {
            withContext(Dispatchers.IO) {
                repository.importBackup(backupData, password)
            }
            loadUserData()
            true
        } catch (e: Exception) {
            false
        }
    }

    // Social network operations
    fun listSocialNetworks(): List<MobileSocialNetwork> {
        return try {
            repository.listSocialNetworks()
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun searchSocialNetworks(query: String): List<MobileSocialNetwork> {
        return try {
            repository.searchSocialNetworks(query)
        } catch (e: Exception) {
            emptyList()
        }
    }

    fun getProfileUrl(networkId: String, username: String): String? {
        return try {
            repository.getProfileUrl(networkId, username)
        } catch (e: Exception) {
            null
        }
    }
}

private data class Tuple4<A, B, C, D>(val a: A, val b: B, val c: C, val d: D)
