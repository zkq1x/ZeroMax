import SwiftUI

struct MessageBubbleView: View {
    let message: FfiMessage

    var body: some View {
        HStack {
            if message.isOutgoing { Spacer(minLength: 60) }

            VStack(alignment: message.isOutgoing ? .trailing : .leading, spacing: 2) {
                // Sender name (group chats).
                if !message.isOutgoing && !message.senderName.isEmpty {
                    Text(message.senderName)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(Color.accentColor.opacity(0.8))
                }

                // Message bubble.
                VStack(alignment: .leading, spacing: 4) {
                    // Text content.
                    if !message.text.isEmpty {
                        Text(message.text)
                            .font(.system(size: 13))
                            .textSelection(.enabled)
                    }

                    // Status + time row.
                    HStack(spacing: 4) {
                        Text(formatTime(message.time))
                            .font(.system(size: 10))
                            .foregroundStyle(.tertiary)

                        if let status = message.status {
                            Text(statusLabel(status))
                                .font(.system(size: 10))
                                .foregroundStyle(.tertiary)
                                .italic()
                        }
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(bubbleBackground)
                .clipShape(RoundedRectangle(cornerRadius: 16))
            }

            if !message.isOutgoing { Spacer(minLength: 60) }
        }
    }

    private var bubbleBackground: some ShapeStyle {
        message.isOutgoing
            ? AnyShapeStyle(Color.accentColor.opacity(0.15))
            : AnyShapeStyle(Color.secondary.opacity(0.1))
    }

    private func formatTime(_ timestamp: Int64) -> String {
        let date = Date(timeIntervalSince1970: Double(timestamp) / 1000)
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm"
        return formatter.string(from: date)
    }

    private func statusLabel(_ status: String) -> String {
        switch status.lowercased() {
        case "edited": return "edited"
        case "removed": return "deleted"
        default: return status
        }
    }
}
