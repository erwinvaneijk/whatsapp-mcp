-- Copied byte for byte from whatsapp-bridge/main.go (NewMessageStore), so the
-- schema text in sqlite_master is the same as the Go bridge's. Do not reformat.

		CREATE TABLE IF NOT EXISTS chats (
			jid TEXT PRIMARY KEY,
			name TEXT,
			last_message_time TIMESTAMP
		);
		
		CREATE TABLE IF NOT EXISTS messages (
			id TEXT,
			chat_jid TEXT,
			sender TEXT,
			content TEXT,
			timestamp TIMESTAMP,
			is_from_me BOOLEAN,
			media_type TEXT,
			filename TEXT,
			url TEXT,
			media_key BLOB,
			file_sha256 BLOB,
			file_enc_sha256 BLOB,
			file_length INTEGER,
			quoted_message_id TEXT,
			PRIMARY KEY (id, chat_jid),
			FOREIGN KEY (chat_jid) REFERENCES chats(jid)
		);

		-- #278: without these indexes every list_messages / list_chats /
		-- get_message_context / get_last_interaction call is a full-table
		-- scan. Latency then scales linearly with history size. The three
		-- messages indexes cover the hot filter+sort predicates; the chats
		-- index covers the last-active ORDER BY in list_chats. All are
		-- IF NOT EXISTS so an existing store gets them for free on first
		-- start after the upgrade.
		--
		-- #146: times are stored with their UTC offset (+01:00, +02:00), which
		-- do not compare or sort correctly as text, so the MCP server compares
		-- and sorts on julianday(...). These indexes are on that expression;
		-- they replace the earlier ones on the raw text, which nothing uses
		-- any more.
		CREATE INDEX IF NOT EXISTS idx_messages_chat_moment
			ON messages(chat_jid, julianday(timestamp));
		CREATE INDEX IF NOT EXISTS idx_messages_sender_moment
			ON messages(sender, julianday(timestamp));
		CREATE INDEX IF NOT EXISTS idx_messages_moment
			ON messages(julianday(timestamp));
		CREATE INDEX IF NOT EXISTS idx_chats_last_message_moment
			ON chats(julianday(last_message_time));
		DROP INDEX IF EXISTS idx_messages_chat_time;
		DROP INDEX IF EXISTS idx_messages_sender_time;
		DROP INDEX IF EXISTS idx_messages_timestamp;
		DROP INDEX IF EXISTS idx_chats_last_message;
	