import SwiftUI

struct ChatListView: View {
    @ObservedObject var viewModel: ChatListViewModel
    @Binding var selectedChatId: Int64?

    var body: some View {
        List(selection: $selectedChatId) {
            ForEach(viewModel.filteredChats, id: \.id) { chat in
                ChatRowView(chat: chat)
                    .tag(chat.id)
            }
        }
        .listStyle(.sidebar)
        .searchable(text: $viewModel.searchText, prompt: "Search chats")
        .navigationTitle("Chats")
    }
}
