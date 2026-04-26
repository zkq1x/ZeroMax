import SwiftUI
import CoreImage.CIFilterBuiltins

struct QRLoginView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var qrImage: NSImage?
    @State private var status: QRStatus = .loading
    @State private var pollTask: Task<Void, Never>?

    enum QRStatus: Equatable {
        case loading
        case waiting
        case scanned
        case expired
        case error(String)
    }

    var body: some View {
        VStack(spacing: 20) {
            Text("Login with QR Code")
                .font(.title2.bold())

            Text("Open MAX on your phone and scan this code")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            // QR image
            Group {
                if let qrImage {
                    Image(nsImage: qrImage)
                        .interpolation(.none)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 200, height: 200)
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                } else {
                    ProgressView()
                        .frame(width: 200, height: 200)
                }
            }

            // Status
            switch status {
            case .loading:
                Text("Loading QR code...")
                    .foregroundStyle(.secondary)
            case .waiting:
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Waiting for scan...")
                }
                .foregroundStyle(.secondary)
            case .scanned:
                Label("Scanned! Logging in...", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
            case .expired:
                VStack(spacing: 8) {
                    Text("QR code expired")
                        .foregroundStyle(.red)
                    Button("Refresh") {
                        Task { await requestQR() }
                    }
                    .buttonStyle(.borderedProminent)
                }
            case .error(let msg):
                Text(msg)
                    .foregroundStyle(.red)
            }

            Button("Cancel") {
                pollTask?.cancel()
                dismiss()
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
        .padding(32)
        .frame(width: 320)
        .task {
            await requestQR()
        }
        .onDisappear {
            pollTask?.cancel()
        }
    }

    // MARK: - Logic

    private func requestQR() async {
        status = .loading
        qrImage = nil

        // Initialize and connect client if not already done.
        if MaxService.shared.client == nil {
            do {
                // Use WEB device type for QR login, dummy phone (not used for QR).
                try MaxService.shared.initialize(phone: "+70000000000", workDir: MaxService.workDir, deviceType: "WEB")
            } catch {
                status = .error("Failed to initialize: \(error.localizedDescription)")
                return
            }
        }

        guard let client = MaxService.shared.client else {
            status = .error("Client not initialized")
            return
        }

        // Connect + handshake only (no sync — we don't have a token yet).
        if !client.isConnected() {
            do {
                try await Task.detached {
                    try client.connectForAuth()
                }.value
            } catch {
                status = .error("Connection failed: \(error.localizedDescription)")
                return
            }
        }

        do {
            let qrData = try await Task.detached {
                try client.requestQr()
            }.value

            // Generate QR image from the link.
            qrImage = generateQRImage(from: qrData.qrLink)
            status = .waiting

            // Start polling.
            pollTask?.cancel()
            pollTask = Task {
                await pollQRStatus(
                    trackId: qrData.trackId,
                    intervalMs: qrData.pollingIntervalMs,
                    expiresAt: qrData.expiresAtMs
                )
            }
        } catch {
            status = .error(error.localizedDescription)
        }
    }

    private func pollQRStatus(trackId: String, intervalMs: UInt64, expiresAt: Int64) async {
        guard let client = MaxService.shared.client else { return }
        let interval = max(intervalMs, 1000)

        while !Task.isCancelled {
            // Check expiration.
            let nowMs = Int64(Date().timeIntervalSince1970 * 1000)
            if nowMs >= expiresAt {
                await MainActor.run { status = .expired }
                return
            }

            do {
                print("[QR] Polling trackId=\(trackId)...")
                let ready = try await Task.detached {
                    try client.pollQrStatus(trackId: trackId)
                }.value
                print("[QR] Poll result: \(ready)")

                if ready {
                    await MainActor.run { status = .scanned }

                    print("[QR] Completing QR login...")
                    let token = try await Task.detached {
                        try client.completeQrLogin(trackId: trackId)
                    }.value
                    print("[QR] Got token: \(token.prefix(20))...")

                    print("[QR] Setting token + sync...")
                    try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
                        DispatchQueue.global(qos: .userInitiated).async {
                            do {
                                try client.setToken(token: token)
                                try client.syncAfterLogin()
                                try? client.resolveDialogUsers()
                                cont.resume()
                            } catch {
                                cont.resume(throwing: error)
                            }
                        }
                    }

                    print("[QR] Loading data...")
                    await MainActor.run {
                        MaxService.shared.me = client.getMe()
                        MaxService.shared.chatItems = client.getChatList()
                        MaxService.shared.isAuthenticated = true

                        // Start event bridge.
                        let bridge = EventBridge(service: MaxService.shared)
                        client.setEventListener(listener: bridge)
                        client.startEventLoop()
                        client.startBackgroundReconnect()
                    }
                    print("[QR] Login complete!")
                    return
                }
            } catch {
                print("[QR] ERROR: \(error)")
                if !Task.isCancelled {
                    await MainActor.run { status = .error(error.localizedDescription) }
                }
                return
            }

            try? await Task.sleep(nanoseconds: interval * 1_000_000)
        }
    }

    // MARK: - QR Generation

    private func generateQRImage(from string: String) -> NSImage? {
        let context = CIContext()
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"

        guard let ciImage = filter.outputImage else { return nil }

        // Scale up for crisp rendering.
        let scale = 10.0
        let scaled = ciImage.transformed(by: CGAffineTransform(scaleX: scale, y: scale))

        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return NSImage(cgImage: cgImage, size: NSSize(width: scaled.extent.width, height: scaled.extent.height))
    }
}
