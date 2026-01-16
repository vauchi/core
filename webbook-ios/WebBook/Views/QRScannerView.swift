// QRScannerView.swift
// Camera-based QR code scanning

import SwiftUI
import AVFoundation

struct QRScannerView: View {
    @EnvironmentObject var viewModel: WebBookViewModel
    @Environment(\.dismiss) var dismiss
    @State private var scannedCode: String?
    @State private var isProcessing = false
    @State private var errorMessage: String?

    var body: some View {
        NavigationView {
            ZStack {
                // Camera view
                CameraPreview(scannedCode: $scannedCode)
                    .ignoresSafeArea()

                // Overlay
                VStack {
                    Spacer()

                    // Scan frame
                    RoundedRectangle(cornerRadius: 16)
                        .stroke(Color.white, lineWidth: 3)
                        .frame(width: 250, height: 250)
                        .background(Color.clear)

                    Spacer()

                    // Status
                    VStack(spacing: 12) {
                        if let error = errorMessage {
                            Text(error)
                                .foregroundColor(.red)
                                .padding()
                                .background(Color.black.opacity(0.7))
                                .cornerRadius(8)
                        } else if isProcessing {
                            HStack {
                                ProgressView()
                                    .progressViewStyle(CircularProgressViewStyle(tint: .white))
                                Text("Processing...")
                                    .foregroundColor(.white)
                            }
                            .padding()
                            .background(Color.black.opacity(0.7))
                            .cornerRadius(8)
                        } else {
                            Text("Point camera at a WebBook QR code")
                                .foregroundColor(.white)
                                .padding()
                                .background(Color.black.opacity(0.7))
                                .cornerRadius(8)
                        }
                    }
                    .padding(.bottom, 50)
                }
            }
            .navigationTitle("Scan QR Code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .foregroundColor(.white)
                }
            }
            .onChange(of: scannedCode) { newValue in
                if let code = newValue {
                    processScannedCode(code)
                }
            }
        }
    }

    private func processScannedCode(_ code: String) {
        guard !isProcessing else { return }

        isProcessing = true
        errorMessage = nil

        Task {
            do {
                try await viewModel.completeExchange(data: code)
                dismiss()
            } catch {
                errorMessage = error.localizedDescription
                scannedCode = nil
            }
            isProcessing = false
        }
    }
}

// Camera preview using AVFoundation
struct CameraPreview: UIViewRepresentable {
    @Binding var scannedCode: String?

    func makeUIView(context: Context) -> UIView {
        let view = CameraView()
        view.delegate = context.coordinator
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(scannedCode: $scannedCode)
    }

    class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        @Binding var scannedCode: String?

        init(scannedCode: Binding<String?>) {
            _scannedCode = scannedCode
        }

        func metadataOutput(_ output: AVCaptureMetadataOutput,
                           didOutput metadataObjects: [AVMetadataObject],
                           from connection: AVCaptureConnection) {
            guard let metadataObject = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
                  let code = metadataObject.stringValue,
                  code.hasPrefix("wb://") else {
                return
            }

            DispatchQueue.main.async {
                self.scannedCode = code
            }
        }
    }
}

class CameraView: UIView {
    weak var delegate: AVCaptureMetadataOutputObjectsDelegate?

    private var captureSession: AVCaptureSession?
    private var previewLayer: AVCaptureVideoPreviewLayer?

    override func layoutSubviews() {
        super.layoutSubviews()
        previewLayer?.frame = bounds

        if captureSession == nil {
            setupCamera()
        }
    }

    private func setupCamera() {
        let session = AVCaptureSession()

        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device) else {
            return
        }

        if session.canAddInput(input) {
            session.addInput(input)
        }

        let output = AVCaptureMetadataOutput()
        if session.canAddOutput(output) {
            session.addOutput(output)
            output.setMetadataObjectsDelegate(delegate, queue: DispatchQueue.main)
            output.metadataObjectTypes = [.qr]
        }

        let preview = AVCaptureVideoPreviewLayer(session: session)
        preview.videoGravity = .resizeAspectFill
        preview.frame = bounds
        layer.addSublayer(preview)

        self.captureSession = session
        self.previewLayer = preview

        DispatchQueue.global(qos: .userInitiated).async {
            session.startRunning()
        }
    }

    deinit {
        captureSession?.stopRunning()
    }
}

#Preview {
    QRScannerView()
        .environmentObject(WebBookViewModel())
}
