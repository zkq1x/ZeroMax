import Combine
import Foundation

@MainActor
final class ConversationViewModel: ObservableObject {
    let chatId: Int64
    let chatTitle: String

    @Published var messages: [FfiMessage] = []
    @Published var inputText = ""
    @Published var isLoading = false
    @Published var isLoadingMore = false
    @Published var errorMessage: String?
    @Published var replyTo: FfiMessage?
    @Published var editingMessage: FfiMessage?
    @Published var typingText: String?

    private var hasMoreHistory = true
    private var cancellables = Set<AnyCancellable>()
    private var typingTimer: Timer?
    private var typingDisplayTimer: Timer?
    private let cache = MessageCache.shared
    private var maxService: MaxService { MaxService.shared }

    init(chatId: Int64, chatTitle: String) {
        self.chatId = chatId
        self.chatTitle = chatTitle

        // 1. Load from cache instantly.
        messages = cache.loadMessages(chatId: chatId)

        // New messages.
        maxService.newMessageSubject
            .filter { $0.chatId == chatId }
            .receive(on: RunLoop.main)
            .sink { [weak self] msg in
                guard let self else { return }
                if !self.messages.contains(where: { $0.id == msg.id }) {
                    self.messages.append(msg)
                    self.cache.saveMessages([msg])
                }
                self.markAsRead(msg.id)
                self.typingText = nil
            }
            .store(in: &cancellables)

        // Edits.
        maxService.messageEditedSubject
            .filter { $0.chatId == chatId }
            .receive(on: RunLoop.main)
            .sink { [weak self] edited in
                if let idx = self?.messages.firstIndex(where: { $0.id == edited.id }) {
                    self?.messages[idx] = edited
                    self?.cache.saveMessages([edited])
                }
            }
            .store(in: &cancellables)

        // Deletes.
        maxService.messageDeletedSubject
            .filter { $0.chatId == chatId }
            .receive(on: RunLoop.main)
            .sink { [weak self] deleted in
                self?.messages.removeAll { $0.id == deleted.id }
            }
            .store(in: &cancellables)

        // Typing.
        maxService.typingSubject
            .filter { $0.chatId == chatId }
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.showTyping()
            }
            .store(in: &cancellables)
    }

    // MARK: - History

    func loadHistory() async {
        // If cache had messages, don't show loading spinner.
        if messages.isEmpty {
            isLoading = true
        }
        defer { isLoading = false }

        do {
            guard let client = maxService.client else { return }
            let chatId = self.chatId
            let history: [FfiMessage] = try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        let result = try client.fetchHistory(chatId: chatId, fromTime: nil, count: 50)
                        continuation.resume(returning: result)
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }

            // Merge server messages with cached.
            let sorted = history.sorted { $0.time < $1.time }
            cache.saveMessages(sorted)

            // Reload from cache for consistent view.
            messages = cache.loadMessages(chatId: chatId)
            hasMoreHistory = history.count >= 50
        } catch {
            // If cache had messages, don't overwrite with error.
            if messages.isEmpty {
                errorMessage = error.localizedDescription
            }
        }
    }

    func loadMoreHistory() async {
        guard hasMoreHistory, !isLoadingMore else { return }
        guard let oldestTime = messages.first?.time else { return }

        isLoadingMore = true
        defer { isLoadingMore = false }

        do {
            guard let client = maxService.client else { return }
            let chatId = self.chatId
            let older: [FfiMessage] = try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        let result = try client.fetchHistory(chatId: chatId, fromTime: oldestTime, count: 50)
                        continuation.resume(returning: result)
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }
            hasMoreHistory = older.count >= 50
            cache.saveMessages(older)
            messages = cache.loadMessages(chatId: chatId)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: - Send / Edit

    func sendMessage() async {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        if let editing = editingMessage {
            inputText = ""
            editingMessage = nil
            do {
                guard let client = maxService.client else { return }
                let chatId = self.chatId
                let edited: FfiMessage = try await withCheckedThrowingContinuation { continuation in
                    DispatchQueue.global(qos: .userInitiated).async {
                        do {
                            let result = try client.editMessage(chatId: chatId, messageId: editing.id, text: text)
                            continuation.resume(returning: result)
                        } catch {
                            continuation.resume(throwing: error)
                        }
                    }
                }
                if let idx = messages.firstIndex(where: { $0.id == editing.id }) {
                    messages[idx] = edited
                }
                cache.saveMessages([edited])
            } catch {
                errorMessage = error.localizedDescription
            }
            return
        }

        let replyId = replyTo?.id
        inputText = ""
        replyTo = nil

        do {
            guard let client = maxService.client else { return }
            let chatId = self.chatId
            let sent: FfiMessage = try await withCheckedThrowingContinuation { continuation in
                DispatchQueue.global(qos: .userInitiated).async {
                    do {
                        let result = try client.sendMessage(chatId: chatId, text: text, replyTo: replyId)
                        continuation.resume(returning: result)
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }
            messages.append(sent)
            cache.saveMessages([sent])
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    // MARK: - Actions

    func deleteMessage(_ messageId: Int64) {
        guard let client = maxService.client else { return }
        messages.removeAll { $0.id == messageId }
        let chatId = self.chatId
        Task.detached {
            try? client.deleteMessage(chatId: chatId, messageIds: [messageId], forMe: false)
        }
    }

    func startEditing(_ message: FfiMessage) {
        editingMessage = message
        inputText = message.text
        replyTo = nil
    }

    func cancelEditing() {
        editingMessage = nil
        inputText = ""
    }

    func setReply(_ message: FfiMessage) {
        replyTo = message
        editingMessage = nil
    }

    func cancelReply() {
        replyTo = nil
    }

    func markAsRead(_ messageId: Int64) {
        guard let client = maxService.client else { return }
        let chatId = self.chatId
        Task.detached {
            try? client.readMessage(chatId: chatId, messageId: messageId)
        }
    }

    // MARK: - Reactions

    func toggleReaction(_ messageId: Int64, emoji: String) {
        guard let client = maxService.client else { return }
        let msgIdStr = String(messageId)
        let chatId = self.chatId
        Task.detached {
            _ = try? client.addReaction(chatId: chatId, messageId: msgIdStr, reaction: emoji)
        }
    }

    // MARK: - Typing

    func onInputChanged() {
        guard typingTimer == nil else { return }
        guard let client = maxService.client else { return }

        let chatId = self.chatId
        Task.detached {
            try? client.sendTyping(chatId: chatId)
        }

        typingTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: false) { [weak self] _ in
            Task { @MainActor in
                self?.typingTimer = nil
            }
        }
    }

    private func showTyping() {
        typingText = "typing..."

        typingDisplayTimer?.invalidate()
        typingDisplayTimer = Timer.scheduledTimer(withTimeInterval: 4.0, repeats: false) { [weak self] _ in
            Task { @MainActor in
                self?.typingText = nil
            }
        }
    }
}
