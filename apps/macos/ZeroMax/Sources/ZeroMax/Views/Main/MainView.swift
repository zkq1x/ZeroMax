import SwiftUI

struct MainView: View {
    @EnvironmentObject var maxService: MaxService
    @StateObject private var chatListVM = ChatListViewModel()
    @State private var selectedChatId: Int64? = nil
    @State private var conversationVM: ConversationViewModel? = nil

    var body: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 400)
        } detail: {
            detail
        }
        .onChange(of: selectedChatId) { newId in
            guard let chatId = newId, conversationVM?.chatId != chatId else {
                if newId == nil { conversationVM = nil }
                return
            }
            let title = chatListVM.chats.first(where: { $0.id == chatId })?.title ?? "Chat"
            conversationVM = ConversationViewModel(chatId: chatId, chatTitle: title)
        }
    }

    // MARK: - Sidebar

    private var sidebar: some View {
        VStack(spacing: 0) {
            // Header
            HStack {
                Text("Chats")
                    .font(.headline)
                Spacer()
                Menu {
                    if let me = maxService.me {
                        Text(me.displayName)
                        Text(me.phone)
                        Divider()
                    }
                    Button("Logout", role: .destructive) {
                        Task { await maxService.logout() }
                    }
                } label: {
                    Image(systemName: "person.circle")
                }
                .menuStyle(.borderlessButton)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            // Chat list with tap selection (reliable on all macOS versions).
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(chatListVM.filteredChats, id: \.id) { chat in
                        ChatRowView(chat: chat)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 2)
                            .background(
                                RoundedRectangle(cornerRadius: 8)
                                    .fill(selectedChatId == chat.id
                                          ? Color.accentColor.opacity(0.15)
                                          : Color.clear)
                            )
                            .contentShape(Rectangle())
                            .onTapGesture {
                                selectedChatId = chat.id
                            }
                    }
                }
                .padding(.vertical, 4)
            }

            Divider()

            // Search
            HStack {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search", text: $chatListVM.searchText)
                    .textFieldStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
    }

    // MARK: - Detail

    @ViewBuilder
    private var detail: some View {
        if let vm = conversationVM {
            ConversationView(viewModel: vm)
                .id(vm.chatId)
        } else {
            VStack(spacing: 12) {
                Image(systemName: "bubble.left.and.bubble.right")
                    .font(.system(size: 48))
                    .foregroundStyle(.tertiary)
                Text("Select a chat")
                    .font(.title2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}
