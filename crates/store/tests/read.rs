//! The read queries the MCP tools use, on small databases written with
//! [`Writer`]. No names tables, so only stored chat names count.

use chrono::{DateTime, FixedOffset};
use whatsapp_mcp_store::{Store, Writer};

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
