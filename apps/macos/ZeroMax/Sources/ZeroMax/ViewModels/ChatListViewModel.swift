import Combine
import Foundation

@MainActor
final class ChatListViewModel: ObservableObject {
    @Published var chats: [FfiChatItem] = []
    @Published var searchText = ""

    private var cancellables = Set<AnyCancellable>()
    private var maxService: MaxService { MaxService.shared }

    var filteredChats: [FfiChatItem] {
        if searchText.isEmpty {
            return chats
        }
        return chats.filter { $0.title.localizedCaseInsensitiveContains(searchText) }
    }

    init() {
        // Sync initial data.
        chats = maxService.chatItems

        // Observe chat updates.
        maxService.$chatItems
            .receive(on: RunLoop.main)
            .assign(to: &$chats)
    }
}
