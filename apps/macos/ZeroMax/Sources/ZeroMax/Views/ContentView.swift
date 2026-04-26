import SwiftUI

struct ContentView: View {
    @EnvironmentObject var maxService: MaxService

    var body: some View {
        Group {
            if maxService.isLoading {
                VStack(spacing: 16) {
                    ProgressView()
                        .controlSize(.large)
                    Text("Connecting...")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if maxService.isAuthenticated {
                MainView()
            } else {
                LoginView()
            }
        }
        .animation(.easeInOut, value: maxService.isAuthenticated)
        .task {
            // Try auto-login from saved session on launch.
            await maxService.tryAutoLogin()
        }
    }
}
