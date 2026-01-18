package com.vauchi.data

import android.content.Context
import android.content.SharedPreferences
import android.util.Base64
import uniffi.vauchi_mobile.MobileContactCard
import uniffi.vauchi_mobile.MobileExchangeData
import uniffi.vauchi_mobile.MobileFieldType
import uniffi.vauchi_mobile.MobileSyncResult
import uniffi.vauchi_mobile.VauchiMobile
import java.io.File

/**
 * Repository class wrapping VauchiMobile UniFFI bindings.
 * Uses Android KeyStore for secure storage key management.
 */
class VauchiRepository(context: Context) {
    private val vauchi: VauchiMobile
    private val prefs: SharedPreferences
    private val keyStoreHelper = KeyStoreHelper()

    companion object {
        private const val PREFS_NAME = "vauchi_settings"
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
        vauchi = VauchiMobile.newWithSecureKey(dataDir, relayUrl, storageKeyBytes.toList().map { it.toUByte() })
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
    fun exportStorageKey(): ByteArray = vauchi.exportStorageKey().map { it.toByte() }.toByteArray()

    fun getRelayUrl(): String = prefs.getString(KEY_RELAY_URL, DEFAULT_RELAY_URL) ?: DEFAULT_RELAY_URL

    fun setRelayUrl(url: String) {
        prefs.edit().putString(KEY_RELAY_URL, url).apply()
    }

    fun sync(): MobileSyncResult = vauchi.sync()

    fun hasIdentity(): Boolean = vauchi.hasIdentity()

    fun createIdentity(displayName: String) {
        vauchi.createIdentity(displayName)
    }

    fun getDisplayName(): String = vauchi.getDisplayName()

    fun getPublicId(): String = vauchi.getPublicId()

    fun getOwnCard(): MobileContactCard = vauchi.getOwnCard()

    fun addField(fieldType: MobileFieldType, label: String, value: String) {
        vauchi.addField(fieldType, label, value)
    }

    fun updateField(label: String, newValue: String) {
        vauchi.updateField(label, newValue)
    }

    fun removeField(label: String): Boolean = vauchi.removeField(label)

    fun generateExchangeQr(): MobileExchangeData = vauchi.generateExchangeQr()

    fun completeExchange(qrData: String) = vauchi.completeExchange(qrData)

    fun contactCount(): UInt = vauchi.contactCount()

    fun listContacts() = vauchi.listContacts()

    fun getContact(id: String) = vauchi.getContact(id)

    fun removeContact(id: String) = vauchi.removeContact(id)

    // Visibility operations
    fun hideFieldFromContact(contactId: String, fieldLabel: String) {
        vauchi.hideFieldFromContact(contactId, fieldLabel)
    }

    fun showFieldToContact(contactId: String, fieldLabel: String) {
        vauchi.showFieldToContact(contactId, fieldLabel)
    }

    fun isFieldVisibleToContact(contactId: String, fieldLabel: String): Boolean {
        return vauchi.isFieldVisibleToContact(contactId, fieldLabel)
    }

    // Backup operations
    fun exportBackup(password: String): String = vauchi.exportBackup(password)

    fun importBackup(backupData: String, password: String) {
        vauchi.importBackup(backupData, password)
    }

    fun checkPasswordStrength(password: String) = uniffi.vauchi_mobile.checkPasswordStrength(password)

    // Social network operations
    fun listSocialNetworks() = vauchi.listSocialNetworks()

    fun searchSocialNetworks(query: String) = vauchi.searchSocialNetworks(query)

    fun getProfileUrl(networkId: String, username: String): String? =
        vauchi.getProfileUrl(networkId, username)

    // Verification operations
    fun verifyContact(id: String) = vauchi.verifyContact(id)

    fun getPublicKey(): String = vauchi.getPublicKey()

    // Recovery operations
    fun createRecoveryClaim(oldPkHex: String) = vauchi.createRecoveryClaim(oldPkHex)

    fun parseRecoveryClaim(claimB64: String) = vauchi.parseRecoveryClaim(claimB64)

    fun createRecoveryVoucher(claimB64: String) = vauchi.createRecoveryVoucher(claimB64)

    fun addRecoveryVoucher(voucherB64: String) = vauchi.addRecoveryVoucher(voucherB64)

    fun getRecoveryStatus() = vauchi.getRecoveryStatus()

    fun getRecoveryProof(): String? = vauchi.getRecoveryProof()

    fun verifyRecoveryProof(proofB64: String) = vauchi.verifyRecoveryProof(proofB64)
}
