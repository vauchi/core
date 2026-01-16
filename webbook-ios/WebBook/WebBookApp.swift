// WebBookApp.swift
// Main entry point for WebBook iOS app

import SwiftUI

@main
struct WebBookApp: App {
    @StateObject private var viewModel = WebBookViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(viewModel)
        }
    }
}
