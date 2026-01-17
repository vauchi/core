package com.webbook.data

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Helper class for secure key management using Android KeyStore.
 *
 * The storage encryption key is generated and stored in the Android KeyStore,
 * which provides hardware-backed security on supported devices.
 */
class KeyStoreHelper {
    companion object {
        private const val KEYSTORE_ALIAS = "webbook_storage_key"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val KEY_SIZE_BITS = 256
        private const val GCM_TAG_LENGTH = 128
        private const val STORAGE_KEY_LENGTH = 32 // 256-bit key for AES

        // Prefix for encrypted data: 12-byte IV + ciphertext + 16-byte tag
        private const val GCM_IV_LENGTH = 12
    }

    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply {
        load(null)
    }

    /**
     * Get or generate the master key from Android KeyStore.
     * This key is used to encrypt/decrypt the storage key.
     */
    private fun getOrCreateMasterKey(): SecretKey {
        val existingKey = keyStore.getEntry(KEYSTORE_ALIAS, null) as? KeyStore.SecretKeyEntry
        if (existingKey != null) {
            return existingKey.secretKey
        }

        // Generate new key in KeyStore
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE
        )

        val keySpec = KeyGenParameterSpec.Builder(
            KEYSTORE_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(KEY_SIZE_BITS)
            .setUserAuthenticationRequired(false) // Allow access without biometric
            .build()

        keyGenerator.init(keySpec)
        return keyGenerator.generateKey()
    }

    /**
     * Generate a new random storage key and encrypt it with the master key.
     * Returns the encrypted storage key bytes (IV + ciphertext + tag).
     */
    fun generateEncryptedStorageKey(): ByteArray {
        val storageKey = uniffi.webbook_mobile.generateStorageKey().toByteArray()
        return encryptStorageKey(storageKey)
    }

    /**
     * Encrypt a storage key using the master key.
     */
    fun encryptStorageKey(storageKey: ByteArray): ByteArray {
        val masterKey = getOrCreateMasterKey()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, masterKey)

        val iv = cipher.iv
        val encrypted = cipher.doFinal(storageKey)

        // Return IV + encrypted data
        return iv + encrypted
    }

    /**
     * Decrypt a storage key using the master key.
     */
    fun decryptStorageKey(encryptedData: ByteArray): ByteArray {
        if (encryptedData.size < GCM_IV_LENGTH + STORAGE_KEY_LENGTH) {
            throw IllegalArgumentException("Invalid encrypted data length")
        }

        val masterKey = getOrCreateMasterKey()
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")

        val iv = encryptedData.sliceArray(0 until GCM_IV_LENGTH)
        val encrypted = encryptedData.sliceArray(GCM_IV_LENGTH until encryptedData.size)

        val spec = GCMParameterSpec(GCM_TAG_LENGTH, iv)
        cipher.init(Cipher.DECRYPT_MODE, masterKey, spec)

        return cipher.doFinal(encrypted)
    }

    /**
     * Check if a master key exists in the KeyStore.
     */
    fun hasMasterKey(): Boolean {
        return keyStore.containsAlias(KEYSTORE_ALIAS)
    }

    /**
     * Delete the master key from KeyStore (for testing/reset).
     */
    fun deleteMasterKey() {
        if (keyStore.containsAlias(KEYSTORE_ALIAS)) {
            keyStore.deleteEntry(KEYSTORE_ALIAS)
        }
    }
}

// Extension to convert List<UByte> to ByteArray
private fun List<UByte>.toByteArray(): ByteArray = ByteArray(size) { this[it].toByte() }
