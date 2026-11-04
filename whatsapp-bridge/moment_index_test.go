package main

import (
	"database/sql"
	"os"
	"sort"
	"strings"
	"testing"
)

// #146: a database from before this change has indexes on the raw time
// text. On start the bridge creates indexes on julianday(...) and removes
// the old ones.
func TestTimeIndexesMigrate(t *testing.T) {
	t.Chdir(t.TempDir())
	if err := os.Mkdir("store", 0o700); err != nil {
		t.Fatal(err)
	}
	old, err := sql.Open("sqlite3", "file:store/messages.db")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := old.Exec(`
		CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
		CREATE TABLE messages (id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
			is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT, media_key BLOB,
			file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER, PRIMARY KEY (id, chat_jid));
		CREATE INDEX idx_messages_chat_time ON messages(chat_jid, timestamp DESC);
		CREATE INDEX idx_messages_sender_time ON messages(sender, timestamp DESC);
		CREATE INDEX idx_messages_timestamp ON messages(timestamp DESC);
		CREATE INDEX idx_chats_last_message ON chats(last_message_time DESC);`); err != nil {
		t.Fatal(err)
	}
	old.Close()

	store, err := NewMessageStore()
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	rows, err := store.db.Query(`SELECT name, sql FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'`)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	var names []string
	for rows.Next() {
		var name, sqlText string
		rows.Scan(&name, &sqlText)
		names = append(names, name)
		if !strings.Contains(sqlText, "julianday(") {
			t.Errorf("index %s is not on julianday(...): %s", name, sqlText)
		}
	}
	sort.Strings(names)
	want := "idx_chats_last_message_moment idx_messages_chat_moment idx_messages_moment idx_messages_sender_moment"
	if got := strings.Join(names, " "); got != want {
		t.Errorf("indexes: %s\nwant:    %s", got, want)
	}
}
