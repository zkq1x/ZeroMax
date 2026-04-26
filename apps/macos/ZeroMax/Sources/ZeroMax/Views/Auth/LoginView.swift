import SwiftUI

struct LoginView: View {
    @StateObject private var viewModel = AuthViewModel()
    @State private var showQRLogin = false

    var body: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(spacing: 24) {
                // Logo
                Text("ZeroMax")
                    .font(.system(size: 36, weight: .bold))
                    .foregroundStyle(.primary)

                Text("Private MAX messenger client")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                // Form
                VStack(spacing: 16) {
                    switch viewModel.state {
                    case .phoneInput:
                        phoneInputView

                    case .codeInput:
                        codeInputView

                    case .twoFactor(_, let hint):
                        twoFactorView(hint: hint)

                    case .loading(let message):
                        loadingView(message: message)

                    case .error(let message):
                        errorView(message: message)
                    }
                }
                .frame(width: 300)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.background)
    }

    // MARK: - Subviews

    private var phoneInputView: some View {
        VStack(spacing: 12) {
            Button("Login with QR Code") {
                showQRLogin = true
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)

            Text("Scan from your phone's MAX app")
                .font(.caption)
                .foregroundStyle(.secondary)

            Divider().padding(.vertical, 4)

            Text("Or login with phone number:")
                .font(.caption)
                .foregroundStyle(.tertiary)

            TextField("Phone number", text: $viewModel.phone)
                .textFieldStyle(.roundedBorder)
                .font(.title3)

            Button("Request Code") {
                Task { await viewModel.requestCode() }
            }
            .buttonStyle(.bordered)
            .controlSize(.regular)
            .disabled(viewModel.phone.count < 10)
        }
        .sheet(isPresented: $showQRLogin) {
            QRLoginView()
        }
    }

    private var codeInputView: some View {
        VStack(spacing: 12) {
            Text("Enter the code sent to \(viewModel.phone)")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            TextField("6-digit code", text: $viewModel.code)
                .textFieldStyle(.roundedBorder)
                .font(.title3)
                .multilineTextAlignment(.center)

            Button("Verify") {
                Task { await viewModel.verifyCode() }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(viewModel.code.count != 6)

            Button("Back") {
                viewModel.state = .phoneInput
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
    }

    private func twoFactorView(hint: String?) -> some View {
        VStack(spacing: 12) {
            Text("Two-factor authentication")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if let hint {
                Text("Hint: \(hint)")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }

            SecureField("Password", text: $viewModel.password)
                .textFieldStyle(.roundedBorder)
                .font(.title3)

            Button("Submit") {
                Task { await viewModel.submit2FA() }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(viewModel.password.isEmpty)
        }
    }

    private func loadingView(message: String) -> some View {
        VStack(spacing: 12) {
            ProgressView()
                .controlSize(.large)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private func errorView(message: String) -> some View {
        VStack(spacing: 12) {
            Text(message)
                .font(.subheadline)
                .foregroundStyle(.red)
                .multilineTextAlignment(.center)

            Button("Try Again") {
                viewModel.dismissError()
            }
            .buttonStyle(.borderedProminent)
        }
    }
}
