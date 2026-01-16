// ContactsView.swift
// Contact list view

import SwiftUI

struct ContactsView: View {
    @EnvironmentObject var viewModel: WebBookViewModel

    var body: some View {
        NavigationView {
            Group {
                if viewModel.contacts.isEmpty {
                    EmptyContactsView()
                } else {
                    List(viewModel.contacts) { contact in
                        NavigationLink(destination: ContactDetailView(contact: contact)) {
                            ContactRow(contact: contact)
                        }
                    }
                    .listStyle(.plain)
                }
            }
            .navigationTitle("Contacts")
            .onAppear {
                Task { await viewModel.loadContacts() }
            }
        }
    }
}

struct ContactRow: View {
    let contact: ContactInfo

    var body: some View {
        HStack(spacing: 12) {
            // Avatar
            ZStack {
                Circle()
                    .fill(Color.cyan)
                    .frame(width: 44, height: 44)

                Text(String(contact.displayName.prefix(1)).uppercased())
                    .font(.headline)
                    .foregroundColor(.white)
            }

            // Info
            VStack(alignment: .leading, spacing: 2) {
                Text(contact.displayName)
                    .font(.body)
                    .fontWeight(.medium)

                HStack(spacing: 4) {
                    if contact.verified {
                        Image(systemName: "checkmark.seal.fill")
                            .foregroundColor(.green)
                            .font(.caption)
                    }
                    Text(contact.verified ? "Verified" : "Not verified")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }

            Spacer()
        }
        .padding(.vertical, 4)
    }
}

struct EmptyContactsView: View {
    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "person.2.slash")
                .font(.system(size: 60))
                .foregroundColor(.secondary)

            Text("No contacts yet")
                .font(.title2)
                .fontWeight(.medium)

            Text("Exchange with someone to add them as a contact")
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
        }
    }
}

#Preview {
    ContactsView()
        .environmentObject(WebBookViewModel())
}
