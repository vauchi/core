// ContactDetailView.swift
// Contact detail view with visibility controls

import SwiftUI

struct ContactDetailView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    let contact: ContactInfo
    @State private var showRemoveAlert = false

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                // Header
                VStack(spacing: 12) {
                    ZStack {
                        Circle()
                            .fill(Color.cyan)
                            .frame(width: 80, height: 80)

                        Text(String(contact.displayName.prefix(1)).uppercased())
                            .font(.largeTitle)
                            .foregroundColor(.white)
                    }

                    Text(contact.displayName)
                        .font(.title)
                        .fontWeight(.bold)

                    HStack(spacing: 4) {
                        if contact.verified {
                            Image(systemName: "checkmark.seal.fill")
                                .foregroundColor(.green)
                        }
                        Text(contact.verified ? "Verified" : "Not verified")
                            .foregroundColor(.secondary)
                    }

                    Text("ID: \(String(contact.id.prefix(16)))...")
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .fontDesign(.monospaced)
                }
                .padding()

                // Contact info section
                VStack(alignment: .leading, spacing: 12) {
                    Text("Contact Info")
                        .font(.headline)
                        .padding(.horizontal)

                    VStack(spacing: 8) {
                        Text("No visible fields")
                            .foregroundColor(.secondary)
                            .padding()
                            .frame(maxWidth: .infinity)
                            .background(Color(.systemGray6))
                            .cornerRadius(10)
                    }
                    .padding(.horizontal)
                }

                // Visibility section
                VStack(alignment: .leading, spacing: 12) {
                    Text("Visibility")
                        .font(.headline)
                        .padding(.horizontal)

                    Text("Control which of your fields this contact can see.")
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .padding(.horizontal)

                    // Visibility controls would go here
                    Text("All fields visible")
                        .foregroundColor(.secondary)
                        .padding()
                        .frame(maxWidth: .infinity)
                        .background(Color(.systemGray6))
                        .cornerRadius(10)
                        .padding(.horizontal)
                }

                Spacer(minLength: 40)

                // Remove button
                Button(role: .destructive) {
                    showRemoveAlert = true
                } label: {
                    Label("Remove Contact", systemImage: "trash")
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Color(.systemGray6))
                        .cornerRadius(10)
                }
                .padding(.horizontal)
            }
            .padding(.vertical)
        }
        .navigationBarTitleDisplayMode(.inline)
        .alert("Remove Contact", isPresented: $showRemoveAlert) {
            Button("Cancel", role: .cancel) { }
            Button("Remove", role: .destructive) {
                Task {
                    try? await viewModel.removeContact(id: contact.id)
                }
            }
        } message: {
            Text("Are you sure you want to remove \(contact.displayName)?")
        }
    }
}

#Preview {
    NavigationView {
        ContactDetailView(contact: ContactInfo(id: "test", displayName: "Alice", verified: true))
            .environmentObject(WebBookViewModel())
    }
}
