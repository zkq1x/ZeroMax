import SwiftUI

struct ConversationView: View {
    @ObservedObject var viewModel: ConversationViewModel

    var body: some View {
        VStack(spacing: 0) {
            // Messages
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 4) {
                        // Load more trigger at top.
                        if viewModel.isLoadingMore {
                            ProgressView()
                                .controlSize(.small)
                                .padding(8)
                        } else {
                            Color.clear
                                .frame(height: 1)
                                .onAppear {
                                    Task { await viewModel.loadMoreHistory() }
                                }
                        }

                        ForEach(viewModel.messages, id: \.id) { message in
                            MessageBubbleView(message: message)
                                .id(message.id)
                                .contextMenu {
                                    Button("Reply") {
                                        viewModel.setReply(message)
                                    }
                                    Button("Copy") {
                                        NSPasteboard.general.clearContents()
                                        NSPasteboard.general.setString(message.text, forType: .string)
                                    }

                                    // Quick reactions.
                                    Menu("React") {
                                        ForEach(["👍", "❤️", "😂", "😮", "😢", "🔥"], id: \.self) { emoji in
                                            Button(emoji) {
                                                viewModel.toggleReaction(message.id, emoji: emoji)
                                            }
                                        }
                                    }

                                    if message.isOutgoing {
                                        Divider()
                                        Button("Edit") {
                                            viewModel.startEditing(message)
                                        }
                                        Button("Delete", role: .destructive) {
                                            viewModel.deleteMessage(message.id)
                                        }
                                    }
                                }
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                }
                .onChange(of: viewModel.messages.count) { _ in
                    if let last = viewModel.messages.last {
                        withAnimation {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
            }

            // Typing indicator
            if let typing = viewModel.typingText {
                HStack {
                    Text(typing)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .italic()
                    Spacer()
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 2)
            }

            Divider()

            // Edit bar
            if let editing = viewModel.editingMessage {
                HStack {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.orange)
                        .frame(width: 3, height: 30)

                    VStack(alignment: .leading, spacing: 1) {
                        Text("Editing")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(.orange)
                        Text(editing.text)
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    Spacer()

                    Button {
                        viewModel.cancelEditing()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
                .background(Color.orange.opacity(0.05))
            }

            // Reply bar
            if let reply = viewModel.replyTo {
                HStack {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.accentColor)
                        .frame(width: 3, height: 30)

                    VStack(alignment: .leading, spacing: 1) {
                        Text("Reply")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(Color.accentColor)
                        Text(reply.text)
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    Spacer()

                    Button {
                        viewModel.cancelReply()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
                .background(Color.secondary.opacity(0.05))
            }

            // Input
            MessageInputView(
                text: $viewModel.inputText,
                onSend: { Task { await viewModel.sendMessage() } }
            )
            .onChange(of: viewModel.inputText) { _ in
                viewModel.onInputChanged()
            }
        }
        .navigationTitle(viewModel.chatTitle)
        .task {
            print("[CONV] Loading history for chatId=\(viewModel.chatId)...")
            await viewModel.loadHistory()
            print("[CONV] Loaded \(viewModel.messages.count) messages, error=\(viewModel.errorMessage ?? "nil")")
        }
    }
}
