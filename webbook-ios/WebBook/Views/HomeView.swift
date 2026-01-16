// HomeView.swift
// Main card view

import SwiftUI

struct HomeView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @State private var showAddField = false

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Header
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Hello, \(viewModel.card?.displayName ?? "User")!")
                            .font(.largeTitle)
                            .fontWeight(.bold)

                        if let publicId = viewModel.identity?.publicId {
                            Text("ID: \(String(publicId.prefix(16)))...")
                                .font(.caption)
                                .foregroundColor(.secondary)
                                .fontDesign(.monospaced)
                        }
                    }
                    .padding(.horizontal)

                    // Card Section
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Your Card")
                                .font(.headline)
                            Spacer()
                            Button(action: { showAddField = true }) {
                                Image(systemName: "plus.circle")
                                    .foregroundColor(.cyan)
                            }
                        }

                        if let fields = viewModel.card?.fields, !fields.isEmpty {
                            ForEach(fields) { field in
                                FieldRow(field: field)
                            }
                        } else {
                            Text("No fields yet. Add your first field!")
                                .foregroundColor(.secondary)
                                .padding()
                                .frame(maxWidth: .infinity)
                                .background(Color(.systemGray6))
                                .cornerRadius(10)
                        }
                    }
                    .padding()
                    .background(Color(.systemGray6))
                    .cornerRadius(12)
                    .padding(.horizontal)
                }
                .padding(.vertical)
            }
            .navigationTitle("Home")
            .sheet(isPresented: $showAddField) {
                AddFieldSheet()
            }
        }
    }
}

struct FieldRow: View {
    let field: FieldInfo

    private func icon(for type: String) -> String {
        switch type.lowercased() {
        case "email": return "envelope"
        case "phone": return "phone"
        case "website": return "globe"
        case "address": return "house"
        case "social": return "at"
        default: return "note.text"
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon(for: field.fieldType))
                .foregroundColor(.cyan)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 2) {
                Text(field.label)
                    .font(.caption)
                    .foregroundColor(.secondary)
                Text(field.value)
                    .font(.body)
            }

            Spacer()
        }
        .padding()
        .background(Color(.systemBackground))
        .cornerRadius(8)
    }
}

struct AddFieldSheet: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @Environment(\.dismiss) var dismiss

    @State private var fieldType = "email"
    @State private var label = ""
    @State private var value = ""
    @State private var isLoading = false
    @State private var errorMessage: String?

    let fieldTypes = ["email", "phone", "website", "address", "social", "custom"]

    var body: some View {
        NavigationView {
            Form {
                Section {
                    Picker("Type", selection: $fieldType) {
                        ForEach(fieldTypes, id: \.self) { type in
                            Text(type.capitalized).tag(type)
                        }
                    }

                    TextField("Label", text: $label)
                        .autocapitalization(.words)

                    TextField("Value", text: $value)
                        .autocapitalization(.none)
                }

                if let error = errorMessage {
                    Section {
                        Text(error)
                            .foregroundColor(.red)
                    }
                }
            }
            .navigationTitle("Add Field")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Add") { addField() }
                        .disabled(label.isEmpty || value.isEmpty || isLoading)
                }
            }
        }
    }

    private func addField() {
        isLoading = true
        errorMessage = nil

        Task {
            do {
                try await viewModel.addField(type: fieldType, label: label, value: value)
                dismiss()
            } catch {
                errorMessage = error.localizedDescription
            }
            isLoading = false
        }
    }
}

#Preview {
    HomeView()
        .environmentObject(WebBookViewModel())
}
