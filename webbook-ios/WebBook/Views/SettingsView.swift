// SettingsView.swift
// Settings and backup view

import SwiftUI

struct SettingsView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @State private var showExportSheet = false
    @State private var showImportSheet = false

    var body: some View {
        NavigationView {
            List {
                // Identity section
                Section("Identity") {
                    HStack {
                        Text("Display Name")
                        Spacer()
                        Text(viewModel.identity?.displayName ?? "Unknown")
                            .foregroundColor(.secondary)
                    }

                    HStack {
                        Text("Public ID")
                        Spacer()
                        Text(viewModel.identity?.publicId ?? "Unknown")
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .fontDesign(.monospaced)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }

                // Backup section
                Section("Backup") {
                    Button(action: { showExportSheet = true }) {
                        Label("Export Backup", systemImage: "square.and.arrow.up")
                    }

                    Button(action: { showImportSheet = true }) {
                        Label("Import Backup", systemImage: "square.and.arrow.down")
                    }
                }

                // Sync section
                Section("Sync") {
                    HStack {
                        Text("Relay Server")
                        Spacer()
                        Text("relay.webbook.app")
                            .foregroundColor(.secondary)
                    }

                    Button(action: {}) {
                        Label("Sync Now", systemImage: "arrow.triangle.2.circlepath")
                    }
                }

                // About section
                Section("About") {
                    HStack {
                        Text("Version")
                        Spacer()
                        Text("0.1.0")
                            .foregroundColor(.secondary)
                    }

                    Link(destination: URL(string: "https://github.com/webbook")!) {
                        Label("GitHub", systemImage: "link")
                    }

                    HStack {
                        Text("WebBook")
                        Spacer()
                        Text("Privacy-focused contact card exchange")
                            .foregroundColor(.secondary)
                            .font(.caption)
                    }
                }

                // Security section
                Section("Security") {
                    NavigationLink(destination: LinkedDevicesView()) {
                        Label("Linked Devices", systemImage: "laptopcomputer.and.iphone")
                    }
                }
            }
            .navigationTitle("Settings")
            .sheet(isPresented: $showExportSheet) {
                ExportBackupSheet()
            }
            .sheet(isPresented: $showImportSheet) {
                ImportBackupSheet()
            }
        }
    }
}

struct LinkedDevicesView: View {
    var body: some View {
        List {
            Section {
                HStack {
                    Image(systemName: "iphone")
                        .foregroundColor(.cyan)
                    VStack(alignment: .leading) {
                        Text("This Device")
                            .font(.body)
                        Text("iPhone - Current")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
            } header: {
                Text("Devices")
            } footer: {
                Text("Manage devices that have access to your identity.")
            }

            Section {
                Button(action: {}) {
                    Label("Link New Device", systemImage: "plus.circle")
                }
            }
        }
        .navigationTitle("Linked Devices")
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct ExportBackupSheet: View {
    @Environment(\.dismiss) var dismiss
    @State private var password = ""
    @State private var confirmPassword = ""
    @State private var isExporting = false

    var body: some View {
        NavigationView {
            Form {
                Section {
                    SecureField("Password", text: $password)
                    SecureField("Confirm Password", text: $confirmPassword)
                } header: {
                    Text("Encrypt Backup")
                } footer: {
                    Text("Your backup will be encrypted with this password. Don't forget it!")
                }

                Section {
                    Button(action: exportBackup) {
                        HStack {
                            Spacer()
                            if isExporting {
                                ProgressView()
                            } else {
                                Text("Export")
                            }
                            Spacer()
                        }
                    }
                    .disabled(password.isEmpty || password != confirmPassword || isExporting)
                }
            }
            .navigationTitle("Export Backup")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }

    private func exportBackup() {
        isExporting = true
        // Export logic would go here
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            dismiss()
        }
    }
}

struct ImportBackupSheet: View {
    @Environment(\.dismiss) var dismiss

    var body: some View {
        NavigationView {
            VStack(spacing: 20) {
                Image(systemName: "doc.badge.arrow.up")
                    .font(.system(size: 60))
                    .foregroundColor(.cyan)

                Text("Import Backup")
                    .font(.title)

                Text("Select a backup file to restore your identity")
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)

                Button(action: {}) {
                    Label("Choose File", systemImage: "folder")
                        .frame(maxWidth: .infinity)
                        .padding()
                        .background(Color.cyan)
                        .foregroundColor(.white)
                        .cornerRadius(10)
                }
                .padding(.horizontal)

                Spacer()
            }
            .padding()
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}

#Preview {
    SettingsView()
        .environmentObject(WebBookViewModel())
}
