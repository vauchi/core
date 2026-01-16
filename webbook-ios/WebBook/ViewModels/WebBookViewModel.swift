// WebBookViewModel.swift
// Main state management for WebBook iOS app

import Foundation
import SwiftUI

// Note: Import the generated UniFFI bindings
// import webbook_mobile

/// Contact field for display
struct FieldInfo: Identifiable {
    let id: String
    let fieldType: String
    let label: String
    let value: String
}

/// Contact card for display
struct CardInfo {
    let displayName: String
    let fields: [FieldInfo]
}

/// Contact for display
struct ContactInfo: Identifiable {
    let id: String
    let displayName: String
    let verified: Bool
}

/// Identity information
struct IdentityInfo {
    let displayName: String
    let publicId: String
}

@MainActor
class WebBookViewModel: ObservableObject {
    // MARK: - Published State

    @Published var isLoading = true
    @Published var hasIdentity = false
    @Published var identity: IdentityInfo?
    @Published var card: CardInfo?
    @Published var contacts: [ContactInfo] = []
    @Published var errorMessage: String?

    // MARK: - Private Properties

    // private var webbook: WebBookMobile?

    // MARK: - Initialization

    init() {
        // Initialize WebBook mobile bindings
        // webbook = try? WebBookMobile(dataDir: getDataDirectory())
    }

    // MARK: - State Management

    func loadState() {
        isLoading = true
        errorMessage = nil

        Task {
            do {
                // Check if identity exists
                // hasIdentity = try webbook?.hasIdentity() ?? false
                hasIdentity = false // Placeholder

                if hasIdentity {
                    await loadIdentity()
                    await loadCard()
                    await loadContacts()
                }
            } catch {
                errorMessage = error.localizedDescription
            }

            isLoading = false
        }
    }

    // MARK: - Identity

    func createIdentity(name: String) async throws {
        // try webbook?.createIdentity(displayName: name)
        // Placeholder implementation
        hasIdentity = true
        identity = IdentityInfo(displayName: name, publicId: "placeholder-id")
        card = CardInfo(displayName: name, fields: [])
    }

    private func loadIdentity() async {
        // if let info = try? webbook?.getIdentity() {
        //     identity = IdentityInfo(displayName: info.displayName, publicId: info.publicId)
        // }
        identity = IdentityInfo(displayName: "User", publicId: "placeholder")
    }

    // MARK: - Card

    func loadCard() async {
        // if let cardData = try? webbook?.getCard() {
        //     card = CardInfo(
        //         displayName: cardData.displayName,
        //         fields: cardData.fields.map { FieldInfo(...) }
        //     )
        // }
        card = CardInfo(displayName: identity?.displayName ?? "User", fields: [])
    }

    func addField(type: String, label: String, value: String) async throws {
        // try webbook?.addField(fieldType: type, label: label, value: value)
        // Placeholder
        var fields = card?.fields ?? []
        fields.append(FieldInfo(id: UUID().uuidString, fieldType: type, label: label, value: value))
        card = CardInfo(displayName: card?.displayName ?? "User", fields: fields)
    }

    func removeField(id: String) async throws {
        // try webbook?.removeField(fieldId: id)
        var fields = card?.fields ?? []
        fields.removeAll { $0.id == id }
        card = CardInfo(displayName: card?.displayName ?? "User", fields: fields)
    }

    // MARK: - Contacts

    func loadContacts() async {
        // if let contactsData = try? webbook?.listContacts() {
        //     contacts = contactsData.map { ContactInfo(...) }
        // }
        contacts = []
    }

    func removeContact(id: String) async throws {
        // try webbook?.removeContact(contactId: id)
        contacts.removeAll { $0.id == id }
    }

    // MARK: - Exchange

    func generateQRData() throws -> String {
        // return try webbook?.generateExchangeQr() ?? ""
        return "wb://placeholder?name=\(identity?.displayName ?? "User")"
    }

    func completeExchange(data: String) async throws {
        // try webbook?.completeExchange(data: data)
    }

    // MARK: - Helpers

    private func getDataDirectory() -> String {
        let paths = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
        let appSupport = paths[0].appendingPathComponent("WebBook")
        try? FileManager.default.createDirectory(at: appSupport, withIntermediateDirectories: true)
        return appSupport.path
    }
}
