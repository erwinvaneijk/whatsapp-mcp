//! Writing messages.db: the schema must be the Go bridge's, older databases
//! must migrate, and what is written must read back the same through the
//! MCP server's queries. Ported from whatsapp-bridge/fts_test.go,
//! moment_index_test.go, quoted_test.go and parity_schema_test.go.

use chrono::{DateTime, FixedOffset, TimeZone};
use rusqlite::Connection;
use whatsapp_mcp_store::{
    ContactNames, MediaInfo, MessageFilter, NewMessage, Store, StoredSender, Writer, go_time,
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
    let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
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
    assert_eq!(want.len(), 10, "statements in parity/fixture/schema.sql");
    assert!(
        got.iter().all(|s| !s.contains('\r')),
        "schema text has CRLF line endings; see .gitattributes"
    );
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
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&store), 0o700);
    assert_eq!(mode(&store.join("messages.db")), 0o600);
}

/// From fsperm_test.go: SQLite gives the WAL files the database file's mode,
/// so creating messages.db as 0600 first keeps them private too.
#[cfg(unix)]
#[test]
fn wal_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    writer
        .store_chat("c@s.whatsapp.net", "C", time("2026-11-04T22:00:52+01:00"))
        .unwrap();
    for name in ["messages.db", "messages.db-wal", "messages.db-shm"] {
        let path = dir.path().join(name);
        assert!(path.exists(), "{name} exists while the database is open");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{name}");
    }
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

#[test]
fn file_sizes_that_do_not_fit_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer.store_chat("c@s.whatsapp.net", "C", now).unwrap();
    let message = NewMessage {
        id: "big".into(),
        chat_jid: "c@s.whatsapp.net".into(),
        media_type: "document".into(),
        file_length: u64::MAX,
        timestamp: Some(now),
        ..Default::default()
    };
    assert!(writer.store_message(&message).is_err());
    // A damaged row with a negative size is an error, not a huge size.
    writer
        .connection()
        .execute(
            "INSERT INTO messages (id, chat_jid, media_type, file_length) VALUES ('neg', 'c@s.whatsapp.net', 'image', -1)",
            [],
        )
        .unwrap();
    assert!(writer.media_info("neg", "c@s.whatsapp.net").is_err());
}

#[test]
fn names_need_both_tables_in_messages_db() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    writer
        .store_contact(
            "31600000001@s.whatsapp.net",
            &ContactNames {
                full_name: "Maria Jansen".into(),
                ..Default::default()
            },
        )
        .unwrap();
    writer
        .connection()
        .execute_batch("DROP TABLE lid_map")
        .unwrap();
    drop(writer);
    // whatsapp.db (the Go bridge's) has a name of its own for the contact.
    let whatsmeow = Connection::open(dir.path().join("whatsapp.db")).unwrap();
    whatsmeow
        .execute_batch(
            "CREATE TABLE whatsmeow_contacts (our_jid TEXT, their_jid TEXT, first_name TEXT, full_name TEXT, push_name TEXT, business_name TEXT, redacted_phone TEXT);
             CREATE TABLE whatsmeow_lid_map (lid TEXT PRIMARY KEY, pn TEXT UNIQUE NOT NULL);
             INSERT INTO whatsmeow_contacts VALUES ('me', '31600000001@s.whatsapp.net', NULL, 'Maria from whatsapp.db', NULL, NULL, NULL);",
        )
        .unwrap();
    drop(whatsmeow);
    // Only contacts in messages.db, no lid_map: whatsapp.db is used instead,
    // rather than failing on the missing table and showing no names at all.
    let db = Store::new(dir.path()).open().unwrap();
    assert_eq!(db.sender_name("31600000001"), "Maria from whatsapp.db");
}

#[test]
fn lookups_for_chat_names_and_the_lid_map() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let chat = "31600000001@s.whatsapp.net";
    assert_eq!(writer.chat_name(chat).unwrap(), None);
    writer
        .store_chat(chat, "", time("2026-11-04T22:00:52+01:00"))
        .unwrap();
    assert_eq!(writer.chat_name(chat).unwrap(), None, "empty is no name");
    writer
        .store_chat(chat, "Maria", time("2026-11-04T22:00:53+01:00"))
        .unwrap();
    assert_eq!(writer.chat_name(chat).unwrap().as_deref(), Some("Maria"));

    assert_eq!(writer.contact_full_name(chat).unwrap(), None);
    writer
        .store_contact(
            chat,
            &ContactNames {
                push_name: "Mia".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        writer.contact_full_name(chat).unwrap(),
        None,
        "push name only"
    );
    writer
        .store_contact(
            chat,
            &ContactNames {
                full_name: "Maria Jansen".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        writer.contact_full_name(chat).unwrap().as_deref(),
        Some("Maria Jansen")
    );

    writer
        .store_lid_mapping("123456789012345", "31600000001")
        .unwrap();
    assert_eq!(
        writer
            .lid_counterpart("123456789012345", true)
            .unwrap()
            .as_deref(),
        Some("31600000001")
    );
    assert_eq!(
        writer
            .lid_counterpart("31600000001", false)
            .unwrap()
            .as_deref(),
        Some("123456789012345")
    );
    assert_eq!(writer.lid_counterpart("31600000002", false).unwrap(), None);
}

#[test]
fn message_sender_and_shared_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let chat = "120363000000000001@g.us";
    writer
        .store_chat(chat, "Book club", time("2026-11-04T22:00:52+01:00"))
        .unwrap();
    let photo = |id: &str, sender: &str, from_me: bool| NewMessage {
        id: id.into(),
        chat_jid: chat.into(),
        sender: sender.into(),
        timestamp: Some(time("2026-11-04T22:00:52+01:00")),
        is_from_me: from_me,
        media_type: "image".into(),
        filename: "image_20261104_220052.jpg".into(),
        ..Default::default()
    };
    writer
        .store_message(&photo("A", "123456789012345", false))
        .unwrap();
    assert_eq!(
        writer.message_sender("A", chat).unwrap(),
        Some(StoredSender {
            sender: Some("123456789012345".into()),
            is_from_me: Some(false)
        })
    );
    assert_eq!(writer.message_sender("A", "other@g.us").unwrap(), None);
    assert!(writer.other_filenames(chat, "A").unwrap().is_empty());
    writer
        .store_message(&photo("B", "31600000009", true))
        .unwrap();
    assert_eq!(
        writer.message_sender("B", chat).unwrap(),
        Some(StoredSender {
            sender: Some("31600000009".into()),
            is_from_me: Some(true)
        })
    );
    assert_eq!(
        writer.other_filenames(chat, "A").unwrap(),
        vec!["image_20261104_220052.jpg".to_string()]
    );

    // A damaged row: NULLs are reported as they are.
    writer
        .connection()
        .execute(
            "UPDATE messages SET sender = NULL, is_from_me = NULL WHERE id = 'A'",
            [],
        )
        .unwrap();
    assert_eq!(
        writer.message_sender("A", chat).unwrap(),
        Some(StoredSender {
            sender: None,
            is_from_me: None
        })
    );
}

#[test]
fn where_names_come_from() {
    use whatsapp_mcp_store::NamesSource;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    assert_eq!(store.names_source(), NamesSource::None, "nothing yet");

    let whatsmeow = Connection::open(dir.path().join("whatsapp.db")).unwrap();
    whatsmeow
        .execute_batch(
            "CREATE TABLE whatsmeow_contacts (our_jid TEXT, their_jid TEXT, first_name TEXT, full_name TEXT, push_name TEXT, business_name TEXT, redacted_phone TEXT);",
        )
        .unwrap();
    drop(whatsmeow);
    assert_eq!(store.names_source(), NamesSource::WhatsappDb, "Go bridge");

    drop(Writer::open(dir.path()).unwrap());
    assert_eq!(store.names_source(), NamesSource::MessagesDb, "Rust bridge");
}

/// From fsperm_test.go: an existing folder or file that others can read is
/// made private, and a file keeps its content.
#[cfg(unix)]
#[test]
fn existing_folders_and_files_are_made_private() {
    use std::os::unix::fs::PermissionsExt;
    use whatsapp_mcp_store::{ensure_private_dir, ensure_private_file};
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let set = |p: &std::path::Path, m: u32| {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    };
    let base = tempfile::tempdir().unwrap();

    let created = base.path().join("store").join("chat");
    ensure_private_dir(&created).unwrap();
    assert_eq!(mode(&created), 0o700);
    let existing = base.path().join("legacy");
    std::fs::create_dir(&existing).unwrap();
    set(&existing, 0o755);
    ensure_private_dir(&existing).unwrap();
    assert_eq!(mode(&existing), 0o700);

    let new_file = base.path().join("new.db");
    ensure_private_file(&new_file).unwrap();
    assert_eq!(mode(&new_file), 0o600);
    let old_file = base.path().join("old.db");
    std::fs::write(&old_file, "keep me").unwrap();
    set(&old_file, 0o644);
    ensure_private_file(&old_file).unwrap();
    assert_eq!(mode(&old_file), 0o600);
    assert_eq!(std::fs::read_to_string(&old_file).unwrap(), "keep me");

    // The real store in a world-readable folder, as an older setup left it.
    let store = base.path().join("open-store");
    std::fs::create_dir(&store).unwrap();
    set(&store, 0o755);
    let writer = Writer::open(&store).unwrap();
    writer
        .store_chat("c@s.whatsapp.net", "C", time("2026-11-04T22:00:52+01:00"))
        .unwrap();
    assert_eq!(mode(&store), 0o700);
    for name in ["messages.db", "messages.db-wal", "messages.db-shm"] {
        assert_eq!(mode(&store.join(name)), 0o600, "{name}");
    }
}
