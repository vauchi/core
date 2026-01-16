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
import uniffi.webbook_mobile.MobileContactCard

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
}

private data class Tuple4<A, B, C, D>(val a: A, val b: B, val c: C, val d: D)
