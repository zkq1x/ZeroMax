import Foundation
import SQLite3

/// SQLite cache for messages. Provides instant offline access to chat history.
final class MessageCache {
    static let shared = MessageCache()

    private var db: OpaquePointer?

    private init() {
        let path = MaxService.workDir + "/messages.db"
        if sqlite3_open(path, &db) != SQLITE_OK {
            NSLog("[MessageCache] Failed to open database at %@", path)
            db = nil
            return
        }
        createTable()
    }

    deinit {
        sqlite3_close(db)
    }

    // MARK: - Schema

    private func createTable() {
        let sql = """
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER NOT NULL,
                chat_id INTEGER NOT NULL,
                time INTEGER NOT NULL,
                text TEXT NOT NULL DEFAULT '',
                sender_id INTEGER NOT NULL DEFAULT 0,
                sender_name TEXT NOT NULL DEFAULT '',
                is_outgoing INTEGER NOT NULL DEFAULT 0,
                status TEXT,
                PRIMARY KEY (id, chat_id)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_chat_time ON messages(chat_id, time);
            """
        exec(sql)
    }

    // MARK: - Public API

    /// Load cached messages for a chat, sorted by time ascending.
    func loadMessages(chatId: Int64, limit: Int = 200) -> [FfiMessage] {
        guard let db else { return [] }
        let sql = "SELECT id, chat_id, time, text, sender_id, sender_name, is_outgoing, status FROM messages WHERE chat_id = ? ORDER BY time ASC LIMIT ?"

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, chatId)
        sqlite3_bind_int(stmt, 2, Int32(limit))

        var messages: [FfiMessage] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            let msg = FfiMessage(
                id: sqlite3_column_int64(stmt, 0),
                chatId: sqlite3_column_int64(stmt, 1),
                time: sqlite3_column_int64(stmt, 2),
                text: String(cString: sqlite3_column_text(stmt, 3)),
                senderId: sqlite3_column_int64(stmt, 4),
                senderName: String(cString: sqlite3_column_text(stmt, 5)),
                isOutgoing: sqlite3_column_int(stmt, 6) != 0,
                status: sqlite3_column_type(stmt, 7) != SQLITE_NULL
                    ? String(cString: sqlite3_column_text(stmt, 7))
                    : nil
            )
            messages.append(msg)
        }
        return messages
    }

    /// Save messages to cache (insert or replace).
    func saveMessages(_ messages: [FfiMessage]) {
        guard let db, !messages.isEmpty else { return }

        exec("BEGIN TRANSACTION")

        let sql = "INSERT OR REPLACE INTO messages (id, chat_id, time, text, sender_id, sender_name, is_outgoing, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else {
            exec("ROLLBACK")
            return
        }

        for msg in messages {
            sqlite3_bind_int64(stmt, 1, msg.id)
            sqlite3_bind_int64(stmt, 2, msg.chatId)
            sqlite3_bind_int64(stmt, 3, msg.time)
            sqlite3_bind_text(stmt, 4, (msg.text as NSString).utf8String, -1, nil)
            sqlite3_bind_int64(stmt, 5, msg.senderId)
            sqlite3_bind_text(stmt, 6, (msg.senderName as NSString).utf8String, -1, nil)
            sqlite3_bind_int(stmt, 7, msg.isOutgoing ? 1 : 0)
            if let status = msg.status {
                sqlite3_bind_text(stmt, 8, (status as NSString).utf8String, -1, nil)
            } else {
                sqlite3_bind_null(stmt, 8)
            }

            sqlite3_step(stmt)
            sqlite3_reset(stmt)
        }

        sqlite3_finalize(stmt)
        exec("COMMIT")
    }

    /// Get the latest message time for a chat (for incremental sync).
    func getLatestTime(chatId: Int64) -> Int64? {
        guard let db else { return nil }
        let sql = "SELECT MAX(time) FROM messages WHERE chat_id = ?"

        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int64(stmt, 1, chatId)

        if sqlite3_step(stmt) == SQLITE_ROW && sqlite3_column_type(stmt, 0) != SQLITE_NULL {
            return sqlite3_column_int64(stmt, 0)
        }
        return nil
    }

    /// Delete all cached messages for a chat.
    func deleteMessages(chatId: Int64) {
        exec("DELETE FROM messages WHERE chat_id = \(chatId)")
    }

    /// Delete all cached data.
    func clearAll() {
        exec("DELETE FROM messages")
    }

    // MARK: - Private

    private func exec(_ sql: String) {
        guard let db else { return }
        var err: UnsafeMutablePointer<CChar>?
        if sqlite3_exec(db, sql, nil, nil, &err) != SQLITE_OK {
            if let err {
                NSLog("[MessageCache] SQL error: %s", String(cString: err))
                sqlite3_free(err)
            }
        }
    }
}
