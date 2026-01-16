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

    fun addField(fieldType: MobileFieldType, label: String, value: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    repository.addField(fieldType, label, value)
                }
                loadUserData()
            } catch (e: Exception) {
                // Silently fail or show error
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
            } catch (e: Exception) {
                // Silently fail or show error
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
            } catch (e: Exception) {
                // Silently fail or show error
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
            } catch (e: Exception) {
                // Silently fail
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
            } catch (e: Exception) {
                // Silently fail
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
}

private data class Tuple4<A, B, C, D>(val a: A, val b: B, val c: C, val d: D)
