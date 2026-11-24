//! The read queries the MCP tools use, on small databases written with
//! [`Writer`]. No names tables, so only stored chat names count.

use chrono::{DateTime, FixedOffset};
use whatsapp_mcp_store::{MessageFilter, NewMessage, Store, Writer};

fn time(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(text).unwrap()
}

/// A store with these chats (JID, name), and nothing else.
fn store_with_chats(chats: &[(&str, &str)]) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    for (jid, name) in chats {
        writer
            .store_chat(jid, name, time("2026-11-04T22:00:52+01:00"))
            .unwrap();
    }
    drop(writer);
    let store = Store::new(dir.path());
    (dir, store)
}

/// Issue #66: a sender's name comes from their own chat only, never from a
/// chat whose JID merely contains their number.
#[test]
fn sender_name_does_not_match_other_chats_by_substring() {
    let (_dir, store) = store_with_chats(&[
        // An old-style group JID: the creator's number, a dash, a time.
        ("31600000001-1600000000@g.us", "Book club"),
        // A longer number that contains the sender's.
        ("316000000012@s.whatsapp.net", "Someone else"),
        // A number inside a LID.
        ("9931600000001@lid", "LID stranger"),
    ]);
    let db = store.open().unwrap();
    assert_eq!(db.sender_name("31600000001"), "31600000001");
    assert_eq!(
        db.sender_name("31600000001@s.whatsapp.net"),
        "31600000001@s.whatsapp.net"
    );
}

#[test]
fn sender_name_finds_the_senders_own_chat() {
    let (_dir, store) = store_with_chats(&[
        ("31600000001-1600000000@g.us", "Book club"),
        ("31600000001@s.whatsapp.net", "Maria"),
        ("200000000000002@lid", "Tom"),
    ]);
    let db = store.open().unwrap();
    // Stored as the user part, as a full JID, and with a device suffix.
    assert_eq!(db.sender_name("31600000001"), "Maria");
    assert_eq!(db.sender_name("31600000001@s.whatsapp.net"), "Maria");
    assert_eq!(db.sender_name("31600000001:5@s.whatsapp.net"), "Maria");
    // A LID sender, as the user part or as the JID.
    assert_eq!(db.sender_name("200000000000002"), "Tom");
    assert_eq!(db.sender_name("200000000000002@lid"), "Tom");
}

/// Issue #67: `%`, `_` and `\` in a search are literal, not wildcards.
#[test]
fn chat_and_contact_searches_take_wildcards_literally() {
    let (_dir, store) = store_with_chats(&[
        ("31600000001@s.whatsapp.net", "Maria_x"),
        ("31600000002@s.whatsapp.net", "Mariaxx"),
        ("31600000003@s.whatsapp.net", "50% off"),
        ("31600000004@s.whatsapp.net", "500 off"),
        ("31600000005@s.whatsapp.net", "back\\slash"),
    ]);
    let db = store.open().unwrap();
    let chats = |query: &str| -> Vec<String> {
        db.list_chats(Some(query), 20, 0, false, "name")
            .unwrap()
            .into_iter()
            .filter_map(|c| c.name)
            .collect()
    };
    assert_eq!(chats("a_x"), ["Maria_x"]);
    assert_eq!(chats("0%"), ["50% off"]);
    assert_eq!(chats("k\\s"), ["back\\slash"]);
    assert_eq!(chats("_"), ["Maria_x"]);
    let contacts = |query: &str| -> Vec<String> {
        db.search_contacts(query)
            .unwrap()
            .into_iter()
            .filter_map(|c| c.name)
            .collect()
    };
    assert_eq!(contacts("a_x"), ["Maria_x"]);
    assert_eq!(contacts("0%"), ["50% off"]);
    // Plain text still matches anywhere, in any case.
    assert_eq!(contacts("MARIA"), ["Maria_x", "Mariaxx"]);
}

#[test]
fn message_search_without_fts_takes_wildcards_literally() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let now = time("2026-11-04T22:00:52+01:00");
    writer
        .store_chat("31600000001@s.whatsapp.net", "Maria", now)
        .unwrap();
    for (id, content) in [
        ("M1", "100% sure"),
        ("M2", "1000 euros"),
        ("M3", "a_b"),
        ("M4", "axb"),
    ] {
        writer
            .store_message(&NewMessage {
                id: id.into(),
                chat_jid: "31600000001@s.whatsapp.net".into(),
                sender: "31600000001".into(),
                content: content.into(),
                timestamp: Some(now),
                ..Default::default()
            })
            .unwrap();
    }
    // Without the insert trigger the FTS index counts as stale, so the
    // search falls back to LIKE.
    writer
        .connection()
        .execute_batch("DROP TRIGGER messages_ai")
        .unwrap();
    drop(writer);
    let db = Store::new(dir.path()).open().unwrap();
    assert!(!db.fts_ready().unwrap());
    let search = |query: &str| -> Vec<String> {
        db.list_messages(&MessageFilter {
            query: Some(query.into()),
            limit: 10,
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect()
    };
    assert_eq!(search("100%"), ["M1"]);
    assert_eq!(search("a_b"), ["M3"]);
}

/// Issue #72: the schema checks are cached, but an open `Db` still sees the
/// bridge change the schema, as `serve` keeps its connection open.
#[test]
fn schema_checks_follow_schema_changes() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let db = Store::new(dir.path()).open().unwrap();
    assert!(db.fts_ready().unwrap());
    assert!(db.fts_ready().unwrap(), "the cached answer");
    // A bridge without FTS5 drops the trigger: the index goes stale.
    writer
        .connection()
        .execute_batch("DROP TRIGGER messages_ai")
        .unwrap();
    assert!(!db.fts_ready().unwrap());
    // The next start adds it back.
    whatsapp_mcp_store::migrate(writer.connection()).unwrap();
    assert!(db.fts_ready().unwrap());
}
