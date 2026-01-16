// ExchangeView.swift
// QR code display for contact exchange

import SwiftUI
import CoreImage.CIFilterBuiltins

struct ExchangeView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @State private var showScanner = false
    @State private var qrData: String = ""

    var body: some View {
        NavigationView {
            ScrollView {
                VStack(spacing: 30) {
                    // QR Code section
                    VStack(spacing: 16) {
                        Text("Your QR Code")
                            .font(.headline)

                        Text("Have someone scan this to add you as a contact")
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .multilineTextAlignment(.center)

                        if let qrImage = generateQRCode(from: qrData) {
                            Image(uiImage: qrImage)
                                .interpolation(.none)
                                .resizable()
                                .scaledToFit()
                                .frame(width: 200, height: 200)
                                .padding()
                                .background(Color.white)
                                .cornerRadius(12)
                        } else {
                            ProgressView()
                                .frame(width: 200, height: 200)
                        }
                    }
                    .padding()
                    .frame(maxWidth: .infinity)
                    .background(Color(.systemGray6))
                    .cornerRadius(12)
                    .padding(.horizontal)

                    // Scan section
                    VStack(spacing: 16) {
                        Text("Scan a Code")
                            .font(.headline)

                        Text("Scan someone else's QR code to add them")
                            .font(.caption)
                            .foregroundColor(.secondary)

                        Button(action: { showScanner = true }) {
                            Label("Open Camera", systemImage: "camera")
                                .frame(maxWidth: .infinity)
                                .padding()
                                .background(Color.cyan)
                                .foregroundColor(.white)
                                .cornerRadius(10)
                        }
                    }
                    .padding()
                    .frame(maxWidth: .infinity)
                    .background(Color(.systemGray6))
                    .cornerRadius(12)
                    .padding(.horizontal)
                }
                .padding(.vertical)
            }
            .navigationTitle("Exchange")
            .onAppear { loadQRData() }
            .sheet(isPresented: $showScanner) {
                QRScannerView()
            }
        }
    }

    private func loadQRData() {
        do {
            qrData = try viewModel.generateQRData()
        } catch {
            qrData = "Error generating QR code"
        }
    }

    private func generateQRCode(from string: String) -> UIImage? {
        let context = CIContext()
        let filter = CIFilter.qrCodeGenerator()

        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"

        guard let outputImage = filter.outputImage else { return nil }

        // Scale up for better quality
        let transform = CGAffineTransform(scaleX: 10, y: 10)
        let scaledImage = outputImage.transformed(by: transform)

        guard let cgImage = context.createCGImage(scaledImage, from: scaledImage.extent) else {
            return nil
        }

        return UIImage(cgImage: cgImage)
    }
}

#Preview {
    ExchangeView()
        .environmentObject(WebBookViewModel())
}
