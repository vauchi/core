// WebBookRepository.swift
// Repository layer wrapping UniFFI bindings for WebBook iOS

import Foundation

/// Repository error types
enum WebBookRepositoryError: LocalizedError {
    case notInitialized
    case alreadyInitialized
    case identityNotFound
    case contactNotFound(String)
    case invalidQrCode
    case exchangeFailed(String)
    case syncFailed(String)
    case storageError(String)
    case cryptoError(String)
    case networkError(String)
    case invalidInput(String)
    case internalError(String)

    var errorDescription: String? {
        switch self {
        case .notInitialized:
            return "Library not initialized"
        case .alreadyInitialized:
            return "Already initialized"
        case .identityNotFound:
            return "Identity not found"
        case .contactNotFound(let id):
            return "Contact not found: \(id)"
        case .invalidQrCode:
            return "Invalid QR code"
        case .exchangeFailed(let msg):
            return "Exchange failed: \(msg)"
        case .syncFailed(let msg):
            return "Sync failed: \(msg)"
        case .storageError(let msg):
            return "Storage error: \(msg)"
        case .cryptoError(let msg):
            return "Crypto error: \(msg)"
        case .networkError(let msg):
            return "Network error: \(msg)"
        case .invalidInput(let msg):
            return "Invalid input: \(msg)"
        case .internalError(let msg):
            return "Internal error: \(msg)"
        }
    }

    /// Convert from MobileError to WebBookRepositoryError
    static func from(_ error: MobileError) -> WebBookRepositoryError {
        switch error {
        case .NotInitialized:
            return .notInitialized
        case .AlreadyInitialized:
            return .alreadyInitialized
        case .IdentityNotFound:
            return .identityNotFound
        case .ContactNotFound(let id):
            return .contactNotFound(id)
        case .InvalidQrCode:
            return .invalidQrCode
        case .ExchangeFailed(let msg):
            return .exchangeFailed(msg)
        case .SyncFailed(let msg):
            return .syncFailed(msg)
        case .StorageError(let msg):
            return .storageError(msg)
        case .CryptoError(let msg):
            return .cryptoError(msg)
        case .NetworkError(let msg):
            return .networkError(msg)
        case .InvalidInput(let msg):
            return .invalidInput(msg)
        case .SerializationError(let msg):
            return .internalError("Serialization: \(msg)")
        case .Internal(let msg):
            return .internalError(msg)
        }
    }
}

/// Sync status enum
enum WebBookSyncStatus {
    case idle
    case syncing
    case error
}

/// Sync result
struct WebBookSyncResult {
    let contactsAdded: UInt32
    let cardsUpdated: UInt32
    let updatesSent: UInt32
}

/// Field type enum matching Rust MobileFieldType
enum WebBookFieldType: String, CaseIterable {
    case email = "email"
    case phone = "phone"
    case website = "website"
    case address = "address"
    case social = "social"
    case custom = "custom"

    var displayName: String {
        switch self {
        case .email: return "Email"
        case .phone: return "Phone"
        case .website: return "Website"
        case .address: return "Address"
        case .social: return "Social"
        case .custom: return "Custom"
        }
    }

    /// Convert to MobileFieldType
    var toMobile: MobileFieldType {
        switch self {
        case .email: return .email
        case .phone: return .phone
        case .website: return .website
        case .address: return .address
        case .social: return .social
        case .custom: return .custom
        }
    }

    /// Convert from MobileFieldType
    static func from(_ mobile: MobileFieldType) -> WebBookFieldType {
        switch mobile {
        case .email: return .email
        case .phone: return .phone
        case .website: return .website
        case .address: return .address
        case .social: return .social
        case .custom: return .custom
        }
    }
}

/// Contact field
struct WebBookContactField: Identifiable {
    let id: String
    let fieldType: WebBookFieldType
    let label: String
    let value: String
}

/// Contact card
struct WebBookContactCard {
    let displayName: String
    let fields: [WebBookContactField]
}

/// Contact
struct WebBookContact: Identifiable {
    let id: String
    let displayName: String
    let isVerified: Bool
    let card: WebBookContactCard
    let addedAt: UInt64
}

/// Exchange data for QR code generation
struct WebBookExchangeData {
    let qrData: String
    let publicId: String
    let expiresAt: UInt64

    var isExpired: Bool {
        UInt64(Date().timeIntervalSince1970) > expiresAt
    }

    var timeRemaining: TimeInterval {
        let now = Date().timeIntervalSince1970
        return max(0, Double(expiresAt) - now)
    }
}

/// Exchange result
struct WebBookExchangeResult {
    let contactId: String
    let contactName: String
    let success: Bool
    let errorMessage: String?
}

/// Social network info
struct WebBookSocialNetwork: Identifiable {
    let id: String
    let displayName: String
    let urlTemplate: String
}

/// Repository class wrapping WebBookMobile UniFFI bindings
class WebBookRepository {
    // MARK: - Properties

    private let webbook: WebBookMobile
    private let dataDir: String
    private let relayUrl: String
    private static let storageKeyLength = 32  // 256-bit key

    // MARK: - Initialization

    /// Initialize repository with data directory and relay URL
    /// Uses iOS Keychain for secure storage key management
    init(dataDir: String? = nil, relayUrl: String = "wss://relay.webbook.app") throws {
        let dir = dataDir ?? WebBookRepository.defaultDataDir()
        self.dataDir = dir
        self.relayUrl = relayUrl

        // Create data directory if needed
        try FileManager.default.createDirectory(
            atPath: dir,
            withIntermediateDirectories: true,
            attributes: nil
        )

        // Get storage key from Keychain (or migrate/generate)
        let storageKeyBytes = try WebBookRepository.getOrCreateStorageKey(dataDir: dir)

        // Initialize WebBookMobile with secure key from Keychain
        do {
            self.webbook = try WebBookMobile.newWithSecureKey(
                dataDir: dir,
                relayUrl: relayUrl,
                storageKeyBytes: storageKeyBytes
            )
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Default data directory in Application Support
    static func defaultDataDir() -> String {
        let paths = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let appSupport = paths[0].appendingPathComponent("WebBook")
        return appSupport.path
    }

    // MARK: - Secure Key Management

    /// Get or create storage key from Keychain
    /// Handles migration from legacy file-based key storage
    private static func getOrCreateStorageKey(dataDir: String) throws -> [UInt8] {
        let keychain = KeychainService.shared
        let legacyKeyPath = (dataDir as NSString).appendingPathComponent("storage.key")

        // Try to load from Keychain first
        if let keyData = try? keychain.loadStorageKey() {
            if keyData.count == storageKeyLength {
                return Array(keyData)
            }
        }

        // Check for legacy file-based key (migration scenario)
        if FileManager.default.fileExists(atPath: legacyKeyPath) {
            // Load legacy key
            let legacyKeyData = try Data(contentsOf: URL(fileURLWithPath: legacyKeyPath))
            if legacyKeyData.count == storageKeyLength {
                // Migrate to Keychain
                try keychain.saveStorageKey(legacyKeyData)

                // Securely delete old file
                try FileManager.default.removeItem(atPath: legacyKeyPath)

                return Array(legacyKeyData)
            }
        }

        // Generate new key and store in Keychain
        let newKeyBytes = WebBookMobile.generateStorageKey()
        let newKeyData = Data(newKeyBytes)
        try keychain.saveStorageKey(newKeyData)

        return newKeyBytes
    }

    /// Export current storage key (for backup purposes only)
    /// WARNING: Handle the returned data with extreme care
    func exportStorageKey() -> [UInt8] {
        return webbook.exportStorageKey()
    }

    // MARK: - Type Conversion Helpers

    private func convertField(_ field: MobileContactField) -> WebBookContactField {
        WebBookContactField(
            id: field.id,
            fieldType: WebBookFieldType.from(field.fieldType),
            label: field.label,
            value: field.value
        )
    }

    private func convertCard(_ card: MobileContactCard) -> WebBookContactCard {
        WebBookContactCard(
            displayName: card.displayName,
            fields: card.fields.map(convertField)
        )
    }

    private func convertContact(_ contact: MobileContact) -> WebBookContact {
        WebBookContact(
            id: contact.id,
            displayName: contact.displayName,
            isVerified: contact.isVerified,
            card: convertCard(contact.card),
            addedAt: contact.addedAt
        )
    }

    // MARK: - Identity Operations

    /// Check if identity exists
    func hasIdentity() -> Bool {
        return webbook.hasIdentity()
    }

    /// Create new identity with display name
    func createIdentity(displayName: String) throws {
        do {
            try webbook.createIdentity(displayName: displayName)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Get public ID
    func getPublicId() throws -> String {
        do {
            return try webbook.getPublicId()
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Get display name
    func getDisplayName() throws -> String {
        do {
            return try webbook.getDisplayName()
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Card Operations

    /// Get own contact card
    func getOwnCard() throws -> WebBookContactCard {
        do {
            let card = try webbook.getOwnCard()
            return convertCard(card)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Add field to own card
    func addField(type: WebBookFieldType, label: String, value: String) throws {
        do {
            try webbook.addField(fieldType: type.toMobile, label: label, value: value)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Update field value
    func updateField(label: String, newValue: String) throws {
        do {
            try webbook.updateField(label: label, newValue: newValue)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Remove field by label
    func removeField(label: String) throws -> Bool {
        do {
            return try webbook.removeField(label: label)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Set display name
    func setDisplayName(_ name: String) throws {
        do {
            try webbook.setDisplayName(name: name)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Contact Operations

    /// List all contacts
    func listContacts() throws -> [WebBookContact] {
        do {
            return try webbook.listContacts().map(convertContact)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Get contact by ID
    func getContact(id: String) throws -> WebBookContact? {
        do {
            return try webbook.getContact(id: id).map(convertContact)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Search contacts
    func searchContacts(query: String) throws -> [WebBookContact] {
        do {
            return try webbook.searchContacts(query: query).map(convertContact)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Get contact count
    func contactCount() throws -> UInt32 {
        do {
            return try webbook.contactCount()
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Remove contact
    func removeContact(id: String) throws -> Bool {
        do {
            return try webbook.removeContact(id: id)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Verify contact fingerprint
    func verifyContact(id: String) throws {
        do {
            try webbook.verifyContact(id: id)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Visibility Operations

    /// Hide field from contact
    func hideFieldFromContact(contactId: String, fieldLabel: String) throws {
        do {
            try webbook.hideFieldFromContact(contactId: contactId, fieldLabel: fieldLabel)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Show field to contact
    func showFieldToContact(contactId: String, fieldLabel: String) throws {
        do {
            try webbook.showFieldToContact(contactId: contactId, fieldLabel: fieldLabel)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Check if field is visible to contact
    func isFieldVisibleToContact(contactId: String, fieldLabel: String) throws -> Bool {
        do {
            return try webbook.isFieldVisibleToContact(contactId: contactId, fieldLabel: fieldLabel)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Exchange Operations

    /// Generate QR data for exchange
    func generateExchangeQr() throws -> WebBookExchangeData {
        do {
            let data = try webbook.generateExchangeQr()
            return WebBookExchangeData(
                qrData: data.qrData,
                publicId: data.publicId,
                expiresAt: data.expiresAt
            )
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Complete exchange with scanned QR data
    func completeExchange(qrData: String) throws -> WebBookExchangeResult {
        do {
            let result = try webbook.completeExchange(qrData: qrData)
            return WebBookExchangeResult(
                contactId: result.contactId,
                contactName: result.contactName,
                success: result.success,
                errorMessage: result.errorMessage
            )
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Sync Operations

    /// Sync with relay server
    func sync() throws -> WebBookSyncResult {
        do {
            let result = try webbook.sync()
            return WebBookSyncResult(
                contactsAdded: result.contactsAdded,
                cardsUpdated: result.cardsUpdated,
                updatesSent: result.updatesSent
            )
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Get sync status
    func getSyncStatus() -> WebBookSyncStatus {
        switch webbook.getSyncStatus() {
        case .idle: return .idle
        case .syncing: return .syncing
        case .error: return .error
        }
    }

    /// Get pending update count
    func pendingUpdateCount() throws -> UInt32 {
        do {
            return try webbook.pendingUpdateCount()
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Backup Operations

    /// Export encrypted backup
    func exportBackup(password: String) throws -> String {
        do {
            return try webbook.exportBackup(password: password)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    /// Import backup
    func importBackup(data: String, password: String) throws {
        do {
            try webbook.importBackup(backupData: data, password: password)
        } catch let error as MobileError {
            throw WebBookRepositoryError.from(error)
        }
    }

    // MARK: - Social Networks

    /// List available social networks
    func listSocialNetworks() -> [WebBookSocialNetwork] {
        return webbook.listSocialNetworks().map { sn in
            WebBookSocialNetwork(
                id: sn.id,
                displayName: sn.displayName,
                urlTemplate: sn.urlTemplate
            )
        }
    }

    /// Search social networks
    func searchSocialNetworks(query: String) -> [WebBookSocialNetwork] {
        return webbook.searchSocialNetworks(query: query).map { sn in
            WebBookSocialNetwork(
                id: sn.id,
                displayName: sn.displayName,
                urlTemplate: sn.urlTemplate
            )
        }
    }

    /// Get profile URL for social network
    func getProfileUrl(networkId: String, username: String) -> String? {
        return webbook.getProfileUrl(networkId: networkId, username: username)
    }
}
