//! What the bridge stores for live messages and history sync, with the Go
//! bridge's rules (handleMessage, handleHistorySync, GetChatName).

use chrono::{DateTime, Local, TimeZone, Utc};
use whatsapp_mcp_bridge::Jid;
use whatsapp_mcp_bridge::receive::{
    ChatName, chat_name, counterpart, group_fallback, history_conversation, lid_pair,
    lid_pair_text, live_message, local, unix_local,
};
use whatsapp_mcp_store::{ContactNames, Writer};
use whatsapp_rust::prelude::wa;
use whatsapp_rust::prelude::wa::message::ImageMessage;

macro_rules! make {
    ($t:ty { $($f:ident : $v:expr),* $(,)? }) => {{
        let mut value = <$t>::default();
        $( value.$f = $v; )*
        value
    }};
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 11, 4, 22, 0, 52).unwrap()
}

fn text(t: &str) -> wa::Message {
    make!(wa::Message { conversation: s(t) })
}

/// A history message: ID, time, from me, participant, content.
fn history_item(
    id: &str,
    seconds: u64,
    from_me: Option<bool>,
    participant: Option<&str>,
    message: Option<wa::Message>,
) -> wa::HistorySyncMsg {
    let key = make!(wa::MessageKey {
        id: s(id),
        from_me: from_me,
        participant: participant.map(str::to_string),
    });
    let mut web = make!(wa::WebMessageInfo {
        key: key.into(),
        message_timestamp: Some(seconds),
    });
    if let Some(message) = message {
        web.message = message.into();
    }
    make!(wa::HistorySyncMsg {
        message: web.into()
    })
}

fn conversation(id: &str, messages: Vec<wa::HistorySyncMsg>) -> wa::Conversation {
    make!(wa::Conversation {
        id: id.to_string(),
        messages: messages,
    })
}

#[test]
fn times_are_local_and_zero_is_no_time() {
    assert_eq!(unix_local(0), None);
    let t = unix_local(1_793_829_652).unwrap();
    assert_eq!(t.timestamp(), 1_793_829_652);
    assert_eq!(
        t.offset(),
        Local.timestamp_opt(1_793_829_652, 0).unwrap().offset()
    );
    let utc = Utc.timestamp_opt(1_793_829_652, 0).unwrap();
    assert_eq!(local(utc), t);
}

#[test]
fn live_message_row() {
    let ts = Utc.timestamp_opt(1_793_829_652, 0).unwrap();
    let row = live_message(
        "3EB0A1",
        "31600000001@s.whatsapp.net",
        "31600000001",
        ts,
        false,
        &text("hello"),
        now(),
    );
    assert_eq!(row.id, "3EB0A1");
    assert_eq!(row.chat_jid, "31600000001@s.whatsapp.net");
    assert_eq!(row.sender, "31600000001");
    assert_eq!(row.content, "hello");
    assert_eq!(row.timestamp, Some(local(ts)));
    assert!(!row.is_from_me);
    assert!(row.media_type.is_empty());

    let image = make!(wa::Message {
        image_message: make!(ImageMessage {
            caption: s("look"),
            url: s("https://mmg.whatsapp.net/x"),
            file_length: Some(12),
        })
        .into(),
    });
    let row = live_message("id", "c@g.us", "1", ts, true, &image, now());
    assert_eq!(
        (
            row.content.as_str(),
            row.media_type.as_str(),
            row.file_length
        ),
        ("look", "image", 12)
    );
    assert_eq!(row.filename, "image_20261104_220052.jpg");
    assert!(row.is_from_me);
}

#[test]
fn history_sender_rules() {
    let chat = Jid::parse("120363000000000001@g.us").unwrap();
    let conv = conversation(
        "120363000000000001@g.us",
        vec![
            history_item(
                "a",
                1_793_829_652,
                Some(false),
                Some("31600000002@s.whatsapp.net"),
                Some(text("from a member")),
            ),
            history_item("b", 1_793_829_600, Some(true), None, Some(text("mine"))),
            history_item(
                "c",
                1_793_829_500,
                Some(false),
                None,
                Some(text("no participant")),
            ),
            history_item("d", 1_793_829_400, None, Some(""), Some(text("no from_me"))),
        ],
    );
    let chat_history = history_conversation(&conv, &chat, "31600000009", now()).unwrap();
    assert_eq!(chat_history.latest, unix_local(1_793_829_652).unwrap());
    let senders: Vec<(&str, &str, bool)> = chat_history
        .messages
        .iter()
        .map(|m| (m.id.as_str(), m.sender.as_str(), m.is_from_me))
        .collect();
    assert_eq!(
        senders,
        vec![
            // A participant is stored as the full JID, as the Go bridge does.
            ("a", "31600000002@s.whatsapp.net", false),
            ("b", "31600000009", true),
            ("c", "120363000000000001", false),
            ("d", "120363000000000001", false),
        ]
    );
    assert!(
        chat_history
            .messages
            .iter()
            .all(|m| m.chat_jid == "120363000000000001@g.us")
    );
}

#[test]
fn history_leaves_out_what_go_leaves_out() {
    let chat = Jid::parse("31600000001@s.whatsapp.net").unwrap();
    // No messages: nothing, not even the chat.
    assert!(
        history_conversation(
            &conversation("31600000001@s.whatsapp.net", vec![]),
            &chat,
            "",
            now()
        )
        .is_none()
    );
    // The first message has no time: the whole conversation is skipped.
    let conv = conversation(
        "31600000001@s.whatsapp.net",
        vec![
            history_item("a", 0, Some(false), None, Some(text("x"))),
            history_item("b", 1_793_829_652, Some(false), None, Some(text("y"))),
        ],
    );
    assert!(history_conversation(&conv, &chat, "", now()).is_none());
    // Later messages without a time, without content or without a message
    // are left out; the chat is kept.
    let conv = conversation(
        "31600000001@s.whatsapp.net",
        vec![
            history_item("a", 1_793_829_652, Some(false), None, Some(text("kept"))),
            history_item("b", 0, Some(false), None, Some(text("no time"))),
            history_item(
                "c",
                1_793_829_600,
                Some(false),
                None,
                Some(wa::Message::default()),
            ),
            history_item("d", 1_793_829_500, Some(false), None, None),
        ],
    );
    let kept = history_conversation(&conv, &chat, "", now()).unwrap();
    let ids: Vec<&str> = kept.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["a"]);
}

#[test]
fn chat_names_follow_go_order() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    let group = Jid::parse("120363000000000001@g.us").unwrap();
    let contact = Jid::parse("31600000001@s.whatsapp.net").unwrap();
    let lid = Jid::parse("123456789012345@lid").unwrap();

    // A group: the history sync name (display name first), else ask.
    let mut conv = conversation("120363000000000001@g.us", vec![]);
    assert_eq!(
        chat_name(&writer, &group, "120363000000000001@g.us", Some(&conv), "").unwrap(),
        ChatName::AskGroup
    );
    conv.name = s("Book club");
    assert_eq!(
        chat_name(&writer, &group, "120363000000000001@g.us", Some(&conv), "").unwrap(),
        ChatName::Known("Book club".into())
    );
    conv.display_name = s("Book club 2026");
    assert_eq!(
        chat_name(&writer, &group, "120363000000000001@g.us", Some(&conv), "").unwrap(),
        ChatName::Known("Book club 2026".into())
    );
    assert_eq!(group_fallback(&group), "Group 120363000000000001");

    // A contact: the sender, else the number, until a saved name is known.
    assert_eq!(
        chat_name(&writer, &contact, "31600000001@s.whatsapp.net", None, "").unwrap(),
        ChatName::Known("31600000001".into())
    );
    assert_eq!(
        chat_name(
            &writer,
            &lid,
            "123456789012345@lid",
            None,
            "123456789012345"
        )
        .unwrap(),
        ChatName::Known("123456789012345".into())
    );
    writer
        .store_contact(
            "31600000001@s.whatsapp.net",
            &ContactNames {
                full_name: "Maria Jansen".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        chat_name(&writer, &contact, "31600000001@s.whatsapp.net", None, "x").unwrap(),
        ChatName::Known("Maria Jansen".into())
    );
    // A LID chat finds the name saved under the phone number.
    writer
        .store_lid_mapping("123456789012345", "31600000001")
        .unwrap();
    assert_eq!(
        chat_name(&writer, &lid, "123456789012345@lid", None, "x").unwrap(),
        ChatName::Known("Maria Jansen".into())
    );

    // A stored name wins over everything.
    writer
        .store_chat(
            "120363000000000001@g.us",
            "Stored",
            unix_local(1_793_829_652).unwrap(),
        )
        .unwrap();
    assert_eq!(
        chat_name(&writer, &group, "120363000000000001@g.us", Some(&conv), "").unwrap(),
        ChatName::Known("Stored".into())
    );
}

#[test]
fn counterparts_and_lid_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    writer
        .store_lid_mapping("123456789012345", "31600000001")
        .unwrap();
    let pn = Jid::parse("31600000001@s.whatsapp.net").unwrap();
    let lid = Jid::parse("123456789012345@lid").unwrap();
    assert_eq!(counterpart(&writer, &pn).unwrap(), Some(lid.clone()));
    assert_eq!(counterpart(&writer, &lid).unwrap(), Some(pn.clone()));
    let group = Jid::parse("120363000000000001@g.us").unwrap();
    assert_eq!(counterpart(&writer, &group).unwrap(), None);

    let expected = Some(("123456789012345".to_string(), "31600000001".to_string()));
    assert_eq!(lid_pair(&lid, &pn), expected);
    assert_eq!(lid_pair(&pn, &lid), expected);
    assert_eq!(lid_pair(&pn, &pn), None);
    assert_eq!(lid_pair(&lid, &group), None);
    assert_eq!(
        lid_pair_text("31600000001:3@s.whatsapp.net", "123456789012345@lid"),
        expected
    );
    assert_eq!(lid_pair_text("not@a@jid", "123456789012345@lid"), None);
}
