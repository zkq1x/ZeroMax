import AppKit
import Combine
import Foundation

/// Singleton wrapping ZeroMaxClient from the Rust FFI layer.
@MainActor
final class MaxService: ObservableObject {
    static let shared = MaxService()

    @Published var isAuthenticated = false
    @Published var isLoading = false
    @Published var me: FfiMe?
    @Published var chatItems: [FfiChatItem] = []

    /// Combine subjects fed by EventBridge.
    let newMessageSubject = PassthroughSubject<FfiMessage, Never>()
    let messageEditedSubject = PassthroughSubject<FfiMessage, Never>()
    let messageDeletedSubject = PassthroughSubject<FfiMessage, Never>()
    let chatUpdateSubject = PassthroughSubject<FfiChatItem, Never>()
    let typingSubject = PassthroughSubject<(chatId: Int64, userId: Int64), Never>()

    private(set) var client: ZeroMaxClient?
    private var eventBridge: EventBridge?

    /// Local cache of user_id → display name to avoid repeated FFI calls.
    private var userNameCache: [Int64: String] = [:]

    /// Consistent working directory for session storage.
    static var workDir: String {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!.appendingPathComponent("ZeroMax", isDirectory: true)
        try? FileManager.default.createDirectory(at: appSupport, withIntermediateDirectories: true)
        return appSupport.path
    }

    private init() {}

    /// Initialize the client.
    func initialize(phone: String, workDir: String? = nil, token: String? = nil, deviceType: String = "DESKTOP") throws {
        let config = FfiClientConfig(
            phone: phone,
            workDir: workDir ?? Self.workDir,
            token: token,
            deviceType: deviceType
        )
        client = try ZeroMaxClient.newClient(config: config)
    }

    /// Try to restore session from saved token on app launch.
    /// Returns true if auto-login succeeded.
    func tryAutoLogin() async -> Bool {
        isLoading = true
        defer { isLoading = false }

        do {
            // Initialize with a dummy phone — the real phone is in the DB.
            // Token will be loaded from SQLite by the Rust core.
            try initialize(phone: "+70000000000", deviceType: "DESKTOP")
            guard let client else { return false }

            // Try connect — will handshake + sync with saved token.
            try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        try client.connect()
                        // Load all chats (paginated).
                        try? client.loadAllChats()
                        // Resolve dialog participant names (after all chats loaded).
                        try? client.resolveDialogUsers()
                        cont.resume()
                    } catch {
                        cont.resume(throwing: error)
                    }
                }
            }

            // If we got here, token was valid.
            me = client.getMe()
            chatItems = client.getChatList()
            isAuthenticated = true

            startEventBridge()
            print("[AUTO-LOGIN] Success! \(chatItems.count) chats loaded")

            // Background preload messages for all chats.
            preloadMessages()

            return true
        } catch {
            print("[AUTO-LOGIN] No saved session or token expired: \(error)")
            client = nil
            return false
        }
    }

    /// Connect, sync, and start listening for events.
    func connect() async throws {
        guard let client else { return }

        try await Task.detached {
            try client.connect()
        }.value

        me = client.getMe()
        chatItems = client.getChatList()
        isAuthenticated = true

        startEventBridge()
    }

    private func startEventBridge() {
        guard let client else { return }
        let bridge = EventBridge(service: self)
        self.eventBridge = bridge
        client.setEventListener(listener: bridge)
        client.startEventLoop()
        client.startBackgroundReconnect()
    }

    /// Background preload: fetch last 50 messages for each chat and cache them.
    func preloadMessages() {
        guard let client else { return }
        let chatIds = chatItems.map { $0.id }
        DispatchQueue.global(qos: .utility).async {
            for chatId in chatIds {
                do {
                    let messages = try client.fetchHistory(chatId: chatId, fromTime: nil, count: 50)
                    if !messages.isEmpty {
                        MessageCache.shared.saveMessages(messages)
                    }
                } catch {
                    // Silently skip — preload is best-effort.
                }
            }
            NSLog("[PRELOAD] Cached messages for %d chats", chatIds.count)
        }
    }

    /// Load the chat list from cached sync data.
    func refreshChatList() {
        guard let client else { return }
        chatItems = client.getChatList()
    }

    /// Resolve a user ID to a display name, with caching.
    func resolveUserName(_ userId: Int64) -> String {
        if let cached = userNameCache[userId] {
            return cached
        }
        guard let client else { return "" }
        if let user = try? client.getUser(userId: userId) {
            userNameCache[userId] = user.displayName
            return user.displayName
        }
        return ""
    }

    /// Logout and reset state.
    func logout() async {
        if let client {
            try? client.serverLogout()
        }
        client = nil
        eventBridge = nil
        me = nil
        chatItems = []
        userNameCache = [:]
        isAuthenticated = false

        // Delete the session database so auto-login doesn't use old token.
        let dbPath = Self.workDir + "/session.db"
        try? FileManager.default.removeItem(atPath: dbPath)
    }
}

// MARK: - EventBridge

final class EventBridge: EventListener {
    private weak var service: MaxService?

    init(service: MaxService) {
        self.service = service
    }

    func onNewMessage(message: FfiMessage) {
        // Save to cache immediately (background thread safe).
        MessageCache.shared.saveMessages([message])
        Task { @MainActor in
            service?.newMessageSubject.send(message)
        }
    }

    func onMessageEdited(message: FfiMessage) {
        MessageCache.shared.saveMessages([message])
        Task { @MainActor in
            service?.messageEditedSubject.send(message)
        }
    }

    func onMessageDeleted(message: FfiMessage) {
        Task { @MainActor in
            service?.messageDeletedSubject.send(message)
        }
    }

    func onChatUpdated(chat: FfiChatItem) {
        Task { @MainActor in
            service?.chatUpdateSubject.send(chat)
            service?.refreshChatList()
        }
    }

    func onTyping(chatId: Int64, userId: Int64) {
        Task { @MainActor in
            service?.typingSubject.send((chatId: chatId, userId: userId))
        }
    }
}
