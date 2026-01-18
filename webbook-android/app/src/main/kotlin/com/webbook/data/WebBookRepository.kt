package com.webbook.data

import android.content.Context
import android.content.SharedPreferences
import android.util.Base64
import uniffi.webbook_mobile.MobileContactCard
import uniffi.webbook_mobile.MobileExchangeData
import uniffi.webbook_mobile.MobileFieldType
import uniffi.webbook_mobile.MobileSyncResult
import uniffi.webbook_mobile.WebBookMobile
import java.io.File

/**
 * Repository class wrapping WebBookMobile UniFFI bindings.
 * Uses Android KeyStore for secure storage key management.
 */
class WebBookRepository(context: Context) {
    private val webbook: WebBookMobile
    private val prefs: SharedPreferences
    private val keyStoreHelper = KeyStoreHelper()

    companion object {
        private const val PREFS_NAME = "webbook_settings"
        private const val KEY_RELAY_URL = "relay_url"
        private const val KEY_ENCRYPTED_STORAGE_KEY = "encrypted_storage_key"
        private const val DEFAULT_RELAY_URL = "ws://localhost:8080"
        private const val LEGACY_KEY_FILENAME = "storage.key"
    }

    init {
        val dataDir = context.filesDir.absolutePath
        prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val relayUrl = prefs.getString(KEY_RELAY_URL, DEFAULT_RELAY_URL) ?: DEFAULT_RELAY_URL

        // Get or create storage key using Android KeyStore
        val storageKeyBytes = getOrCreateStorageKey(dataDir)

        // Initialize with secure key from KeyStore
        webbook = WebBookMobile.newWithSecureKey(dataDir, relayUrl, storageKeyBytes.toList().map { it.toUByte() })
    }

    /**
     * Get or create storage key from Android KeyStore.
     * Handles migration from legacy file-based key storage.
     */
    private fun getOrCreateStorageKey(dataDir: String): ByteArray {
        // Try to load encrypted key from preferences
        val encryptedKeyBase64 = prefs.getString(KEY_ENCRYPTED_STORAGE_KEY, null)
        if (encryptedKeyBase64 != null) {
            try {
                val encryptedKey = Base64.decode(encryptedKeyBase64, Base64.DEFAULT)
                return keyStoreHelper.decryptStorageKey(encryptedKey)
            } catch (e: Exception) {
                // Key decryption failed, might need to regenerate
                // Clear the invalid key
                prefs.edit().remove(KEY_ENCRYPTED_STORAGE_KEY).apply()
            }
        }

        // Check for legacy file-based key (migration scenario)
        val legacyKeyFile = File(dataDir, LEGACY_KEY_FILENAME)
        if (legacyKeyFile.exists()) {
            try {
                val legacyKey = legacyKeyFile.readBytes()
                if (legacyKey.size == 32) {
                    // Encrypt and save to preferences
                    val encryptedKey = keyStoreHelper.encryptStorageKey(legacyKey)
                    val encryptedBase64 = Base64.encodeToString(encryptedKey, Base64.DEFAULT)
                    prefs.edit().putString(KEY_ENCRYPTED_STORAGE_KEY, encryptedBase64).apply()

                    // Securely delete legacy file
                    legacyKeyFile.delete()

                    return legacyKey
                }
            } catch (e: Exception) {
                // Failed to migrate, generate new key
            }
        }

        // Generate new key, encrypt with KeyStore, and save
        val encryptedKey = keyStoreHelper.generateEncryptedStorageKey()
        val encryptedBase64 = Base64.encodeToString(encryptedKey, Base64.DEFAULT)
        prefs.edit().putString(KEY_ENCRYPTED_STORAGE_KEY, encryptedBase64).apply()

        // Decrypt to get the actual storage key bytes
        return keyStoreHelper.decryptStorageKey(encryptedKey)
    }

    /**
     * Export current storage key (for backup purposes only).
     * WARNING: Handle the returned data with extreme care.
     */
    fun exportStorageKey(): ByteArray = webbook.exportStorageKey().map { it.toByte() }.toByteArray()

    fun getRelayUrl(): String = prefs.getString(KEY_RELAY_URL, DEFAULT_RELAY_URL) ?: DEFAULT_RELAY_URL

    fun setRelayUrl(url: String) {
        prefs.edit().putString(KEY_RELAY_URL, url).apply()
    }

    fun sync(): MobileSyncResult = webbook.sync()

    fun hasIdentity(): Boolean = webbook.hasIdentity()

    fun createIdentity(displayName: String) {
        webbook.createIdentity(displayName)
    }

    fun getDisplayName(): String = webbook.getDisplayName()

    fun getPublicId(): String = webbook.getPublicId()

    fun getOwnCard(): MobileContactCard = webbook.getOwnCard()

    fun addField(fieldType: MobileFieldType, label: String, value: String) {
        webbook.addField(fieldType, label, value)
    }

    fun updateField(label: String, newValue: String) {
        webbook.updateField(label, newValue)
    }

    fun removeField(label: String): Boolean = webbook.removeField(label)

    fun generateExchangeQr(): MobileExchangeData = webbook.generateExchangeQr()

    fun completeExchange(qrData: String) = webbook.completeExchange(qrData)

    fun contactCount(): UInt = webbook.contactCount()

    fun listContacts() = webbook.listContacts()

    fun getContact(id: String) = webbook.getContact(id)

    fun removeContact(id: String) = webbook.removeContact(id)

    // Visibility operations
    fun hideFieldFromContact(contactId: String, fieldLabel: String) {
        webbook.hideFieldFromContact(contactId, fieldLabel)
    }

    fun showFieldToContact(contactId: String, fieldLabel: String) {
        webbook.showFieldToContact(contactId, fieldLabel)
    }

    fun isFieldVisibleToContact(contactId: String, fieldLabel: String): Boolean {
        return webbook.isFieldVisibleToContact(contactId, fieldLabel)
    }

    // Backup operations
    fun exportBackup(password: String): String = webbook.exportBackup(password)

    fun importBackup(backupData: String, password: String) {
        webbook.importBackup(backupData, password)
    }

    fun checkPasswordStrength(password: String) = uniffi.webbook_mobile.checkPasswordStrength(password)

    // Social network operations
    fun listSocialNetworks() = webbook.listSocialNetworks()

    fun searchSocialNetworks(query: String) = webbook.searchSocialNetworks(query)

    fun getProfileUrl(networkId: String, username: String): String? =
        webbook.getProfileUrl(networkId, username)

    // Verification operations
    fun verifyContact(id: String) = webbook.verifyContact(id)

    fun getPublicKey(): String = webbook.getPublicKey()

    // Recovery operations
    fun createRecoveryClaim(oldPkHex: String) = webbook.createRecoveryClaim(oldPkHex)

    fun parseRecoveryClaim(claimB64: String) = webbook.parseRecoveryClaim(claimB64)

    fun createRecoveryVoucher(claimB64: String) = webbook.createRecoveryVoucher(claimB64)

    fun addRecoveryVoucher(voucherB64: String) = webbook.addRecoveryVoucher(voucherB64)

    fun getRecoveryStatus() = webbook.getRecoveryStatus()

    fun getRecoveryProof(): String? = webbook.getRecoveryProof()

    fun verifyRecoveryProof(proofB64: String) = webbook.verifyRecoveryProof(proofB64)
}
