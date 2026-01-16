package com.webbook.data

import android.content.Context
import uniffi.webbook_mobile.MobileContactCard
import uniffi.webbook_mobile.MobileExchangeData
import uniffi.webbook_mobile.MobileFieldType
import uniffi.webbook_mobile.WebBookMobile

class WebBookRepository(context: Context) {
    private val webbook: WebBookMobile

    init {
        val dataDir = context.filesDir.absolutePath
        webbook = WebBookMobile(dataDir, "ws://localhost:8080")
    }

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
}
