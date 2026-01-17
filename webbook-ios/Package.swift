// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "WebBook",
    platforms: [
        .macOS(.v13),
        .iOS(.v16)
    ],
    products: [
        .library(name: "WebBook", targets: ["WebBook"])
    ],
    targets: [
        .target(
            name: "WebBook",
            path: "WebBook",
            exclude: ["Info.plist"],
            sources: [
                "ContentView.swift",
                "WebBookApp.swift",
                "Services/WebBookRepository.swift",
                "Services/KeychainService.swift",
                "Services/SettingsService.swift",
                "Services/NetworkMonitor.swift",
                "Services/BackgroundSyncService.swift",
                "Services/ContactActions.swift",
                "ViewModels/WebBookViewModel.swift",
                "Views/ContactsView.swift",
                "Views/ContactDetailView.swift",
                "Views/ExchangeView.swift",
                "Views/HomeView.swift",
                "Views/QRScannerView.swift",
                "Views/SettingsView.swift",
                "Views/SetupView.swift",
                "Generated/webbook_mobile.swift"
            ]
        ),
        .testTarget(
            name: "WebBookTests",
            dependencies: ["WebBook"],
            path: "WebBookTests"
        )
    ]
)
