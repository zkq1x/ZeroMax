import SwiftUI

struct ChatRowView: View {
    let chat: FfiChatItem

    var body: some View {
        HStack(spacing: 12) {
            // Avatar
            AvatarView(
                url: chat.avatarUrl,
                initial: avatarInitial,
                color: avatarColor,
                size: 40
            )

            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    if chat.chatType == .channel {
                        Image(systemName: "megaphone.fill")
                            .font(.system(size: 10))
                            .foregroundStyle(.secondary)
                    }

                    Text(chat.title)
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)

                    Spacer()

                    Text(formatTime(chat.lastMessageTime))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }

                Text(chat.lastMessageText.isEmpty ? " " : chat.lastMessageText)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 4)
    }

    private var avatarInitial: String {
        String(chat.title.prefix(1)).uppercased()
    }

    private var avatarColor: Color {
        let colors: [Color] = [.blue, .green, .orange, .purple, .pink, .teal, .indigo]
        let index = abs(chat.id.hashValue) % colors.count
        return colors[index]
    }

    private func formatTime(_ timestamp: Int64) -> String {
        guard timestamp > 0 else { return "" }
        let date = Date(timeIntervalSince1970: Double(timestamp) / 1000)
        let calendar = Calendar.current

        if calendar.isDateInToday(date) {
            let formatter = DateFormatter()
            formatter.dateFormat = "HH:mm"
            return formatter.string(from: date)
        } else {
            let formatter = DateFormatter()
            formatter.dateFormat = "dd.MM"
            return formatter.string(from: date)
        }
    }
}

/// Reusable avatar view — shows image from URL or fallback letter.
struct AvatarView: View {
    let url: String?
    let initial: String
    let color: Color
    let size: CGFloat

    var body: some View {
        if let urlStr = url, !urlStr.isEmpty, let imageUrl = URL(string: urlStr) {
            AsyncImage(url: imageUrl) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                        .frame(width: size, height: size)
                        .clipShape(Circle())
                case .failure:
                    fallbackAvatar
                case .empty:
                    fallbackAvatar
                        .overlay(ProgressView().controlSize(.mini))
                @unknown default:
                    fallbackAvatar
                }
            }
        } else {
            fallbackAvatar
        }
    }

    private var fallbackAvatar: some View {
        ZStack {
            Circle()
                .fill(color)
                .frame(width: size, height: size)
            Text(initial)
                .font(.system(size: size * 0.4, weight: .semibold))
                .foregroundStyle(.white)
        }
    }
}
