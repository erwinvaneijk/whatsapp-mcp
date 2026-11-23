//! The bridge's handling of WhatsApp events, without a connection (issue
//! #82): real whatsapp-rust events go into `handle_event`, with a fake for
//! what it asks the client, and the test reads back what was stored.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Mutex;

use buffa::Message as _;
use chrono::{DateTime, TimeZone, Utc};
use whatsapp_mcp_store::Writer;
use whatsapp_rust::types::events::{
    BatchOrigin, Connected, ContactUpdate, Disconnected, Event, InboundMessage, LazyHistorySync,
    MessageBatch,
};
use whatsapp_rust::types::message::{MessageInfo, MessageSource};
use whatsapp_rust::wacore::net::DisconnectReason;

use super::*;

const MARIA: &str = "31600000001@s.whatsapp.net";
const TOM_LID: &str = "200000000000002@lid";
const CLUB: &str = "120363000000000001@g.us";

macro_rules! make {
    ($t:ty { $($f:ident : $v:expr),* $(,)? }) => {{
        let mut value = <$t>::default();
        $( value.$f = $v; )*
        value
    }};
}

/// A fake client: known LID mappings and group subjects, and which groups
/// were asked for.
#[derive(Default)]
struct Fake {
    /// JID -> (LID user, phone number).
    lids: HashMap<String, (String, String)>,
    subjects: HashMap<String, String>,
    own: String,
    asked: Mutex<Vec<String>>,
}

impl Lookups for Fake {
    async fn lid_pn(&self, jid: &Jid) -> Option<(String, String)> {
        self.lids.get(&jid.to_non_ad_string()).cloned()
    }

    async fn group_subject(&self, chat_jid: &str) -> Option<String> {
        self.asked.lock().unwrap().push(chat_jid.to_string());
        self.subjects.get(chat_jid).cloned()
    }

    fn own_user(&self) -> String {
        self.own.clone()
    }
}

fn fake() -> Fake {
    Fake {
        own: "31600000000".into(),
        ..Fake::default()
    }
}

fn shared() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    (dir, Shared::new(writer, false))
}

fn jid(text: &str) -> Jid {
    text.parse().unwrap()
}

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_793_829_652 + seconds, 0).unwrap()
}

fn text(t: &str) -> wa::Message {
    make!(wa::Message {
        conversation: Some(t.to_string())
    })
}

/// A live message event: one message in `chat` from `sender`.
fn live(
    id: &str,
    chat: &str,
    sender: &str,
    from_me: bool,
    push_name: &str,
    message: wa::Message,
) -> Event {
    live_with_alt(id, chat, sender, None, from_me, push_name, message)
}

fn live_with_alt(
    id: &str,
    chat: &str,
    sender: &str,
    sender_alt: Option<&str>,
    from_me: bool,
    push_name: &str,
    message: wa::Message,
) -> Event {
    let info = MessageInfo {
        source: MessageSource {
            chat: jid(chat),
            sender: jid(sender),
            is_from_me: from_me,
            is_group: chat.ends_with("@g.us"),
            sender_alt: sender_alt.map(jid),
            ..MessageSource::default()
        },
        id: id.into(),
        push_name: push_name.into(),
        timestamp: at(0),
        ..MessageInfo::default()
    };
    let inbound = InboundMessage::builder()
        .message(Arc::new(message))
        .info(Arc::new(info))
        .build();
    Event::Messages(
        MessageBatch::builder()
            .messages(Arc::from(vec![inbound]))
            .origin(BatchOrigin::Live)
            .build(),
    )
}

/// A history sync chunk as WhatsApp sends it: the protobuf, compressed.
fn history(sync: &wa::HistorySync, progress: Option<u32>) -> Event {
    let raw = sync.encode_to_vec();
    let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zlib.write_all(&raw).unwrap();
    let compressed = zlib.finish().unwrap();
    Event::HistorySync(Box::new(LazyHistorySync::new(
        compressed.into(),
        raw.len(),
        0,
        None,
        progress,
    )))
}

fn history_message(id: &str, seconds: u64, from_me: bool, content: &str) -> wa::HistorySyncMsg {
    let key = make!(wa::MessageKey {
        id: Some(id.to_string()),
        from_me: Some(from_me),
    });
    let web = make!(wa::WebMessageInfo {
        key: key.into(),
        message_timestamp: Some(seconds),
        message: text(content).into(),
    });
    make!(wa::HistorySyncMsg {
        message: web.into()
    })
}

async fn handle(shared: &Shared, fake: &Fake, event: Event) {
    handle_event(shared, fake, &event, false).await;
}

/// Rows of `messages`: (id, chat_jid, sender, content, is_from_me).
fn messages(shared: &Shared) -> Vec<(String, String, String, String, bool)> {
    let writer = shared.writer();
    let mut statement = writer
        .connection()
        .prepare("SELECT id, chat_jid, sender, content, is_from_me FROM messages ORDER BY id")
        .unwrap();
    statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn chat_name(shared: &Shared, chat: &str) -> Option<String> {
    shared.writer().chat_name(chat).unwrap()
}

fn lid_for(shared: &Shared, pn: &str) -> Option<String> {
    shared.writer().lid_counterpart(pn, false).unwrap()
}

fn push_name(shared: &Shared, jid: &str) -> Option<String> {
    shared
        .writer()
        .connection()
        .query_row("SELECT push_name FROM contacts WHERE jid = ?", [jid], |r| {
            r.get(0)
        })
        // No row: no push name.
        .ok()
        .flatten()
}

#[tokio::test]
async fn a_live_message_is_stored_with_its_chat_and_push_name() {
    let (_dir, shared) = shared();
    let fake = fake();
    handle(
        &shared,
        &fake,
        live("M1", MARIA, MARIA, false, "Maria", text("Hi!")),
    )
    .await;
    assert_eq!(
        messages(&shared),
        [(
            "M1".into(),
            MARIA.into(),
            "31600000001".into(),
            "Hi!".into(),
            false
        )]
    );
    // No saved name yet: the chat is stored under the number; serve shows
    // the push name over it when it reads.
    assert_eq!(chat_name(&shared, MARIA).as_deref(), Some("31600000001"));
    assert_eq!(push_name(&shared, MARIA).as_deref(), Some("Maria"));
}

#[tokio::test]
async fn an_own_message_keeps_no_push_name() {
    let (_dir, shared) = shared();
    let fake = fake();
    let own = "31600000000@s.whatsapp.net";
    handle(
        &shared,
        &fake,
        live("M2", MARIA, own, true, "Me", text("Hello")),
    )
    .await;
    let rows = messages(&shared);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].4, "from me");
    assert_eq!(push_name(&shared, own), None);
}

#[tokio::test]
async fn a_message_without_text_or_media_is_not_stored() {
    let (_dir, shared) = shared();
    let fake = fake();
    handle(
        &shared,
        &fake,
        live("M3", MARIA, MARIA, false, "", wa::Message::default()),
    )
    .await;
    assert!(messages(&shared).is_empty());
}

#[tokio::test]
async fn a_new_group_gets_its_subject_from_whatsapp() {
    let (_dir, shared) = shared();
    let mut fake = fake();
    fake.subjects.insert(CLUB.into(), "Book club".into());
    handle(
        &shared,
        &fake,
        live("G1", CLUB, MARIA, false, "Maria", text("Chapter 3?")),
    )
    .await;
    assert_eq!(chat_name(&shared, CLUB).as_deref(), Some("Book club"));
    assert_eq!(*fake.asked.lock().unwrap(), [CLUB]);
    // Known now: not asked again.
    handle(
        &shared,
        &fake,
        live("G2", CLUB, MARIA, false, "Maria", text("Yes")),
    )
    .await;
    assert_eq!(fake.asked.lock().unwrap().len(), 1);
    assert_eq!(messages(&shared).len(), 2);
}

#[tokio::test]
async fn a_group_without_a_subject_gets_a_name_anyway() {
    let (_dir, shared) = shared();
    let fake = fake();
    handle(
        &shared,
        &fake,
        live("G1", CLUB, MARIA, false, "", text("Hi")),
    )
    .await;
    assert_eq!(
        chat_name(&shared, CLUB).as_deref(),
        Some("Group 120363000000000001")
    );
}

#[tokio::test]
async fn lid_mappings_are_learned_from_messages() {
    let (_dir, shared) = shared();
    let mut fake = fake();
    // From the message's alternative sender ID.
    handle(
        &shared,
        &fake,
        live_with_alt(
            "M1",
            TOM_LID,
            TOM_LID,
            Some("31600000002@s.whatsapp.net"),
            false,
            "Tom",
            text("Hi"),
        ),
    )
    .await;
    assert_eq!(
        lid_for(&shared, "31600000002").as_deref(),
        Some("200000000000002")
    );
    // Else from the library's own map.
    fake.lids.insert(
        "200000000000003@lid".into(),
        ("200000000000003".into(), "31600000003".into()),
    );
    handle(
        &shared,
        &fake,
        live(
            "M2",
            "200000000000003@lid",
            "200000000000003@lid",
            false,
            "Noor",
            text("Hello"),
        ),
    )
    .await;
    assert_eq!(
        lid_for(&shared, "31600000003").as_deref(),
        Some("200000000000003")
    );
}

#[tokio::test]
async fn a_history_sync_stores_conversations_names_and_lid_mappings() {
    let (_dir, shared) = shared();
    let fake = fake();
    let conversation = make!(wa::Conversation {
        id: MARIA.to_string(),
        messages: vec![
            history_message("H1", 1_793_829_600, false, "Earlier"),
            history_message("H2", 1_793_829_652, true, "Later"),
        ],
    });
    let push = make!(wa::Pushname {
        id: Some(MARIA.to_string()),
        pushname: Some("Maria".to_string()),
    });
    let mapping = make!(wa::PhoneNumberToLIDMapping {
        pn_jid: Some("31600000002@s.whatsapp.net".to_string()),
        lid_jid: Some(TOM_LID.to_string()),
    });
    let sync = make!(wa::HistorySync {
        conversations: vec![conversation],
        pushnames: vec![push],
        phone_number_to_lid_mappings: vec![mapping],
    });
    handle(&shared, &fake, history(&sync, Some(40))).await;
    let rows = messages(&shared);
    assert_eq!(
        rows.iter()
            .map(|r| (r.0.as_str(), r.2.as_str(), r.4))
            .collect::<Vec<_>>(),
        [("H1", "31600000001", false), ("H2", "31600000000", true)]
    );
    assert_eq!(push_name(&shared, MARIA).as_deref(), Some("Maria"));
    assert_eq!(
        lid_for(&shared, "31600000002").as_deref(),
        Some("200000000000002")
    );
    assert!(
        !shared.history_done.load(Ordering::Relaxed),
        "40% is not done"
    );
    handle(
        &shared,
        &fake,
        history(&wa::HistorySync::default(), Some(100)),
    )
    .await;
    assert!(shared.history_done.load(Ordering::Relaxed));
}

#[tokio::test]
async fn a_history_chunk_that_cannot_be_decoded_stores_nothing() {
    let (_dir, shared) = shared();
    let fake = fake();
    let broken = Event::HistorySync(Box::new(LazyHistorySync::new(
        b"not zlib".to_vec().into(),
        100,
        0,
        None,
        Some(10),
    )));
    handle(&shared, &fake, broken).await;
    assert!(messages(&shared).is_empty());
}

#[tokio::test]
async fn contact_updates_store_names_and_lid_mappings() {
    let (_dir, shared) = shared();
    let fake = fake();
    let action = make!(wa::sync_action_value::ContactAction {
        full_name: Some("Maria Jansen".to_string()),
        first_name: Some("Maria".to_string()),
        lid_jid: Some("200000000000001@lid".to_string()),
    });
    let update = ContactUpdate::builder()
        .jid(jid(MARIA))
        .timestamp(at(0))
        .action(Box::new(action))
        .from_full_sync(false)
        .build();
    handle(&shared, &fake, Event::ContactUpdate(update)).await;
    assert_eq!(
        shared.writer().contact_full_name(MARIA).unwrap().as_deref(),
        Some("Maria Jansen")
    );
    assert_eq!(
        lid_for(&shared, "31600000001").as_deref(),
        Some("200000000000001")
    );
}

/// The library reconnects by itself; the bridge follows the connection
/// state, which `link` and sending depend on.
#[tokio::test]
async fn the_connection_state_follows_connects_and_disconnects() {
    let (_dir, shared) = shared();
    let fake = fake();
    assert!(!shared.is_connected());
    let connected = || Event::Connected(Connected::builder().build());
    let disconnected = || {
        Event::Disconnected(
            Disconnected::builder()
                .reason(DisconnectReason::StreamEnded)
                .build(),
        )
    };
    handle(&shared, &fake, connected()).await;
    assert!(shared.is_connected());
    handle(&shared, &fake, disconnected()).await;
    assert!(!shared.is_connected());
    // Reconnected.
    handle(&shared, &fake, connected()).await;
    assert!(shared.is_connected());
    // Messages that arrive while connected are stored as usual.
    handle(
        &shared,
        &fake,
        live("M1", MARIA, MARIA, false, "", text("Back")),
    )
    .await;
    assert_eq!(messages(&shared).len(), 1);
}

/// `link` stops once connected and the history sync is complete, or after
/// LINK_QUIET without history.
#[tokio::test]
async fn link_finishes_when_the_history_is_in_or_quiet() {
    let (_dir, shared) = shared();
    let fake = fake();
    assert!(!shared.link_finished());
    handle(
        &shared,
        &fake,
        Event::Connected(Connected::builder().build()),
    )
    .await;
    assert!(!shared.link_finished(), "just connected");
    handle(
        &shared,
        &fake,
        history(&wa::HistorySync::default(), Some(100)),
    )
    .await;
    assert!(shared.link_finished(), "history complete");

    let (_dir, quiet) = self::shared();
    handle(
        &quiet,
        &fake,
        Event::Connected(Connected::builder().build()),
    )
    .await;
    *quiet.last_activity.lock().unwrap() = Instant::now().checked_sub(LINK_QUIET);
    assert!(quiet.link_finished(), "nothing for LINK_QUIET");
}
