//! Writing messages.db: the schema must be the Go bridge's, older databases
//! must migrate, and what is written must read back the same through the
//! MCP server's queries. Ported from whatsapp-bridge/fts_test.go,
//! moment_index_test.go, quoted_test.go and parity_schema_test.go.

use std::path::Path;

use chrono::{DateTime, FixedOffset, TimeZone};
use rusqlite::Connection;
use whatsapp_mcp_store::{
    ContactNames, MediaInfo, MessageFilter, NewMessage, Store, Writer, go_time,
};

fn time(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(text).unwrap()
}

/// sqlite_master's statements in creation order, as the Go schema test dumps
/// them (FTS5's own shadow tables and SQLite's indexes left out).
fn schema(conn: &Connection) -> Vec<String> {
    let mut statement = conn
        .prepare(
            "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'messages_fts_%' ORDER BY rowid",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|sql| format!("{};", sql.unwrap()))
        .collect()
}

/// The statements of parity/fixture/schema.sql: the Go bridge's schema.
fn go_schema() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixture/schema.sql"
    );
    let text = std::fs::read_to_string(path).unwrap();
    text.split("\n\n")
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("--"))
        .map(str::to_string)
        .collect()
}

#[test]
fn schema_is_the_go_bridges_plus_the_names_tables() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let got = schema(writer.connection());
    let want = go_schema();
    assert_eq!(
        got[..want.len()],
        want[..],
        "the Go bridge's schema, in order"
    );
    let extra: Vec<String> = got[want.len()..]
        .iter()
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .collect();
    assert_eq!(extra, ["CREATE TABLE contacts", "CREATE TABLE lid_map"]);
}

#[test]
fn opening_twice_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let first = schema(Writer::open(dir.path()).unwrap().connection());
    let second = schema(Writer::open(dir.path()).unwrap().connection());
    assert_eq!(first, second);
}

#[cfg(unix)]
#[test]
fn store_folder_and_database_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    Writer::open(&store).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&store), 0o700);
    assert_eq!(mode(&store.join("messages.db")), 0o600);
}

/// From moment_index_test.go: indexes on the raw time text are replaced by
/// indexes on julianday(...).
#[test]
fn old_time_indexes_are_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("messages.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
         CREATE TABLE messages (id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
            is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT, media_key BLOB,
            file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER, PRIMARY KEY (id, chat_jid));
         CREATE INDEX idx_messages_chat_time ON messages(chat_jid, timestamp DESC);
         CREATE INDEX idx_messages_sender_time ON messages(sender, timestamp DESC);
         CREATE INDEX idx_messages_timestamp ON messages(timestamp DESC);
         CREATE INDEX idx_chats_last_message ON chats(last_message_time DESC);",
    )
    .unwrap();
    drop(conn);
    let writer = Writer::open(dir.path()).unwrap();
    let mut statement = writer
        .connection()
        .prepare("SELECT name, sql FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%' ORDER BY name")
        .unwrap();
    let indexes: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let names: Vec<&str> = indexes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "idx_chats_last_message_moment",
            "idx_messages_chat_moment",
            "idx_messages_moment",
            "idx_messages_sender_moment"
        ]
    );
    for (name, sql) in &indexes {
        assert!(sql.contains("julianday("), "{name}: {sql}");
    }
}

/// From quoted_test.go: an older database gets quoted_message_id; replies
/// store the quoted ID, other messages NULL.
#[test]
fn quoted_message_id_migration_and_storage() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("messages.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
         CREATE TABLE messages (id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
            is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT, media_key BLOB,
            file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER, PRIMARY KEY (id, chat_jid),
            FOREIGN KEY (chat_jid) REFERENCES chats(jid));",
    )
    .unwrap();
    drop(conn);
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer.store_chat("c@s.whatsapp.net", "C", now).unwrap();
    for (id, quoted) in [("r1", "ORIG1"), ("p1", "")] {
        writer
            .store_message(&NewMessage {
                id: id.into(),
                chat_jid: "c@s.whatsapp.net".into(),
                sender: "1".into(),
                content: "text".into(),
                timestamp: Some(now),
                quoted_message_id: quoted.into(),
                ..Default::default()
            })
            .unwrap();
    }
    let quoted = |id: &str| -> Option<String> {
        writer
            .connection()
            .query_row(
                "SELECT quoted_message_id FROM messages WHERE id = ?",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(quoted("r1").as_deref(), Some("ORIG1"));
    assert_eq!(quoted("p1"), None);
}

/// From fts_test.go: FTS triggers left behind by a database without the
/// index must not break inserts; afterwards search finds new messages by
/// prefix, and old messages are indexed too.
#[test]
fn leftover_fts_trigger_and_backfill() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("messages.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE chats (jid TEXT PRIMARY KEY, name TEXT, last_message_time TIMESTAMP);
         CREATE TABLE messages (id TEXT, chat_jid TEXT, sender TEXT, content TEXT, timestamp TIMESTAMP,
            is_from_me BOOLEAN, media_type TEXT, filename TEXT, url TEXT, media_key BLOB,
            file_sha256 BLOB, file_enc_sha256 BLOB, file_length INTEGER, PRIMARY KEY (id, chat_jid));
         INSERT INTO messages (id, chat_jid, sender, content, timestamp) VALUES ('old', '123@s.whatsapp.net', '123', 'older meetup notes', '2026-11-01 10:00:00+01:00');",
    )
    .unwrap();
    drop(conn);
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer
        .store_chat("123@s.whatsapp.net", "Test", now)
        .unwrap();
    writer
        .store_message(&NewMessage {
            id: "m1".into(),
            chat_jid: "123@s.whatsapp.net".into(),
            sender: "123".into(),
            content: "hello meeting".into(),
            timestamp: Some(now),
            ..Default::default()
        })
        .unwrap();
    let count: i64 = writer
        .connection()
        .query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH '\"meet\"*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "the new message and the backfilled old one");
}

#[test]
fn messages_without_text_or_media_are_not_stored() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    writer
        .store_chat("c@s.whatsapp.net", "C", time("2026-11-04T22:00:52+01:00"))
        .unwrap();
    writer
        .store_message(&NewMessage {
            id: "empty".into(),
            chat_jid: "c@s.whatsapp.net".into(),
            ..Default::default()
        })
        .unwrap();
    let count: i64 = writer
        .connection()
        .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn chat_names_are_kept_unless_a_new_one_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let t1 = time("2026-11-04T10:00:00+01:00");
    let t2 = time("2026-11-04T11:00:00+01:00");
    writer.store_chat("c@s.whatsapp.net", "Club", t1).unwrap();
    writer.store_chat("c@s.whatsapp.net", "", t2).unwrap();
    let (name, last): (String, String) = writer
        .connection()
        .query_row("SELECT name, last_message_time FROM chats", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(name, "Club");
    assert_eq!(last, "2026-11-04 11:00:00+01:00");
    assert!(writer.chat_exists("c@s.whatsapp.net").unwrap());
    assert!(!writer.chat_exists("x@s.whatsapp.net").unwrap());
}

#[test]
fn times_are_written_as_go_writes_them() {
    let offset = |h: i32| FixedOffset::east_opt(h * 3600).unwrap();
    let at = |h: i32, nanos: u32| {
        offset(h)
            .with_ymd_and_hms(2026, 11, 4, 22, 0, 52)
            .unwrap()
            .checked_add_signed(chrono::Duration::nanoseconds(nanos.into()))
            .unwrap()
    };
    assert_eq!(go_time(&at(1, 0)), "2026-11-04 22:00:52+01:00");
    assert_eq!(go_time(&at(0, 0)), "2026-11-04 22:00:52+00:00");
    assert_eq!(go_time(&at(-5, 0)), "2026-11-04 22:00:52-05:00");
    assert_eq!(go_time(&at(1, 120_000_000)), "2026-11-04 22:00:52.12+01:00");
    assert_eq!(go_time(&at(1, 1)), "2026-11-04 22:00:52.000000001+01:00");
}

#[test]
fn media_info_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer.store_chat("c@s.whatsapp.net", "C", now).unwrap();
    writer
        .store_message(&NewMessage {
            id: "img".into(),
            chat_jid: "c@s.whatsapp.net".into(),
            sender: "1".into(),
            timestamp: Some(now),
            media_type: "image".into(),
            filename: "IMG.jpg".into(),
            ..Default::default()
        })
        .unwrap();
    let info = MediaInfo {
        media_type: "image".into(),
        filename: "IMG.jpg".into(),
        url: "https://mmg.whatsapp.net/x".into(),
        media_key: vec![1, 2, 3],
        file_sha256: vec![4],
        file_enc_sha256: vec![5],
        file_length: 1234,
    };
    writer
        .store_media_info("img", "c@s.whatsapp.net", &info)
        .unwrap();
    assert_eq!(
        writer.media_info("img", "c@s.whatsapp.net").unwrap(),
        Some(info)
    );
    assert_eq!(writer.media_info("none", "c@s.whatsapp.net").unwrap(), None);
}

/// What the bridge writes, the MCP server reads back: messages with their
/// stored time, and names from the bridge's own tables, not whatsapp.db.
#[test]
fn written_data_reads_back_through_the_mcp_queries() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer
        .store_chat("200000000000002@lid", "200000000000002", now)
        .unwrap();
    writer
        .store_message(&NewMessage {
            id: "M1".into(),
            chat_jid: "200000000000002@lid".into(),
            sender: "200000000000002".into(),
            content: "Hi from Tom".into(),
            timestamp: Some(now),
            ..Default::default()
        })
        .unwrap();
    writer
        .store_contact(
            "31600000002@s.whatsapp.net",
            &ContactNames {
                push_name: "Tom B.".into(),
                ..Default::default()
            },
        )
        .unwrap();
    writer
        .store_lid_mapping("200000000000002", "31600000002")
        .unwrap();
    drop(writer);

    // No whatsapp.db at all: the names must come from messages.db.
    let db = Store::new(dir.path()).open().unwrap();
    let messages = db
        .list_messages(&MessageFilter {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].timestamp.display(),
        "2026-11-04 22:00:52 +01:00"
    );
    assert_eq!(messages[0].chat_name.as_deref(), Some("Tom B."));
    assert_eq!(
        db.names().phone_number("200000000000002@lid"),
        "31600000002"
    );
    assert_eq!(db.sender_name("200000000000002"), "Tom B.");
}

#[test]
fn contact_names_keep_known_values() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let jid = "31600000001@s.whatsapp.net";
    writer
        .store_contact(
            jid,
            &ContactNames {
                full_name: "Maria Jansen".into(),
                push_name: "Mia".into(),
                ..Default::default()
            },
        )
        .unwrap();
    writer
        .store_contact(
            jid,
            &ContactNames {
                push_name: "Maria J.".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let (full, push): (String, String) = writer
        .connection()
        .query_row(
            "SELECT full_name, push_name FROM contacts WHERE jid = ?",
            [jid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((full.as_str(), push.as_str()), ("Maria Jansen", "Maria J."));
}

#[test]
fn database_is_in_wal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let mode: String = writer
        .connection()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}
