-- Copied byte for byte from whatsapp-bridge/main.go (NewMessageStore), so the
-- schema text in sqlite_master is the same as the Go bridge's. Do not reformat.

		CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
			content,
			content='messages',
			content_rowid='rowid',
			tokenize='unicode61 remove_diacritics 2'
		);
		CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages
			WHEN new.content IS NOT NULL AND new.content != ''
		BEGIN
			INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
		END;
		CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages
			WHEN old.content IS NOT NULL AND old.content != ''
		BEGIN
			INSERT INTO messages_fts(messages_fts, rowid, content)
				VALUES('delete', old.rowid, old.content);
		END;
		CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages
		BEGIN
			INSERT INTO messages_fts(messages_fts, rowid, content)
				SELECT 'delete', old.rowid, old.content
				WHERE old.content IS NOT NULL AND old.content != '';
			INSERT INTO messages_fts(rowid, content)
				SELECT new.rowid, new.content
				WHERE new.content IS NOT NULL AND new.content != '';
		END;
	