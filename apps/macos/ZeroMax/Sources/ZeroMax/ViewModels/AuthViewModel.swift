import Foundation
import SwiftUI

/// Manages the login flow state machine.
@MainActor
final class AuthViewModel: ObservableObject {
    enum AuthState: Equatable {
        case phoneInput
        case codeInput(tempToken: String)
        case twoFactor(trackId: String, hint: String?)
        case loading(message: String)
        case error(String)
    }

    @Published var state: AuthState = .phoneInput
    @Published var phone = "+7"
    @Published var code = ""
    @Published var password = ""

    private var maxService: MaxService { MaxService.shared }

    // MARK: - Work directory

    private var workDir: String { MaxService.workDir }

    // MARK: - Actions

    func requestCode() async {
        guard !phone.isEmpty else { return }
        state = .loading(message: "Requesting code...")

        do {
            print("[AUTH] Initializing client (DESKTOP)...")
            try maxService.initialize(phone: phone, workDir: workDir, deviceType: "DESKTOP")
            guard let client = maxService.client else { return }

            print("[AUTH] Connecting for auth...")
            try await Task.detached {
                try client.connectForAuth()
            }.value
            print("[AUTH] Connected. Requesting code...")

            let phone = self.phone
            let tempToken = try await Task.detached {
                try client.requestCode(phone: phone, language: "ru")
            }.value
            print("[AUTH] Got token: \(tempToken.prefix(20))...")

            state = .codeInput(tempToken: tempToken)
            code = ""
        } catch {
            print("[AUTH] ERROR: \(error)")
            state = .error(error.localizedDescription)
        }
    }

    func verifyCode() async {
        guard case .codeInput(let tempToken) = state else { return }
        guard code.count == 6 else {
            state = .error("Code must be 6 digits")
            return
        }

        state = .loading(message: "Verifying code...")

        do {
            guard let client = maxService.client else { return }

            let code = self.code
            let result = try await Task.detached {
                try client.verifyCode(code: code, tempToken: tempToken)
            }.value

            switch result {
            case .loggedIn(let token):
                try await finishLogin(token: token)
            case .twoFactorRequired(let trackId, let hint):
                state = .twoFactor(trackId: trackId, hint: hint)
                password = ""
            }
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    func submit2FA() async {
        guard case .twoFactor(let trackId, _) = state else { return }
        guard !password.isEmpty else { return }

        state = .loading(message: "Checking password...")

        do {
            guard let client = maxService.client else { return }

            let password = self.password
            let token = try await Task.detached {
                try client.check2faPassword(trackId: trackId, password: password)
            }.value

            if let token {
                try await finishLogin(token: token)
            } else {
                state = .twoFactor(trackId: trackId, hint: "Wrong password, try again")
            }
        } catch {
            state = .error(error.localizedDescription)
        }
    }

    func dismissError() {
        state = .phoneInput
    }

    // MARK: - Private

    private func finishLogin(token: String) async throws {
        state = .loading(message: "Syncing...")

        guard let client = maxService.client else { return }

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

        // Update UI state.
        await MainActor.run {
            maxService.me = client.getMe()
            maxService.chatItems = client.getChatList()
            maxService.isAuthenticated = true

            let bridge = EventBridge(service: maxService)
            client.setEventListener(listener: bridge)
            client.startEventLoop()
            client.startBackgroundReconnect()
        }
    }
}
