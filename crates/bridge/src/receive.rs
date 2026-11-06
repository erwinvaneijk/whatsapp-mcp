//! What the bridge stores for what arrives: live messages, history sync
//! conversations, chat names and LID mappings. Ported from the Go bridge's
//! `handleMessage`, `handleHistorySync` and `GetChatName`, with the same
//! values (sender rules, times in local time, the order of name sources).
//! Plain functions over the store, so they are tested without a connection;
//! `run` feeds them from whatsapp-rust's events.

use chrono::{DateTime, FixedOffset, Local, TimeZone, Utc};
use whatsapp_mcp_store::{NewMessage, Result as StoreResult, Writer};
use whatsapp_rust::prelude::wa;

use crate::convert;
use crate::jid::{GROUP_SERVER, Jid, LID_SERVER, USER_SERVER};

/// A time in local time, as the Go bridge stores it.
pub fn local(moment: DateTime<Utc>) -> DateTime<FixedOffset> {
    moment.with_timezone(&Local).fixed_offset()
}

/// A Unix time in seconds, in local time; `None` for 0 (no time), as the Go
/// bridge skips messages without one.
pub fn unix_local(seconds: u64) -> Option<DateTime<FixedOffset>> {
    if seconds == 0 {
        return None;
    }
    let seconds = i64::try_from(seconds).ok()?;
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|moment| moment.fixed_offset())
}

/// A message's text, quoted ID and media, as stored. `now` names media
/// files that have no name of their own.
fn message_row(msg: Option<&wa::Message>, now: DateTime<Local>) -> NewMessage {
    let Some(msg) = msg else {
        return NewMessage::default();
    };
    let media = convert::media(msg, now);
    NewMessage {
        content: convert::text_content(msg),
        quoted_message_id: convert::quoted_message_id(msg),
        media_type: media.media_type,
        filename: media.filename,
        url: media.url,
        media_key: media.media_key,
        file_sha256: media.file_sha256,
        file_enc_sha256: media.file_enc_sha256,
        file_length: media.file_length,
        ..NewMessage::default()
    }
}

/// A live message as stored: the sender is the user part of the sender's
/// JID, as in the Go bridge. Text and media may both be empty; such a
/// message is not stored (see [`NewMessage`] and `Writer::store_message`).
pub fn live_message(
    id: &str,
    chat_jid: &str,
    sender_user: &str,
    timestamp: DateTime<Utc>,
    is_from_me: bool,
    msg: &wa::Message,
    now: DateTime<Local>,
) -> NewMessage {
    NewMessage {
        id: id.to_string(),
        chat_jid: chat_jid.to_string(),
        sender: sender_user.to_string(),
        timestamp: Some(local(timestamp)),
        is_from_me,
        ..message_row(Some(msg), now)
    }
}

/// A history sync conversation, ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryChat {
    /// The time of the conversation's first (newest) message, for the chat.
    pub latest: DateTime<FixedOffset>,
    /// The messages with text or media; others are left out.
    pub messages: Vec<NewMessage>,
}

/// The messages of a history sync conversation, with the Go bridge's rules:
/// nothing when the conversation has no messages, or its first message has
/// no time; messages without text and media, or without a time, are left
/// out. The sender of a message from someone else is the key's participant
/// (a full JID, in groups) or else the chat's user; of a message from me, my
/// own number (`own_user`).
pub fn history_conversation(
    conversation: &wa::Conversation,
    chat: &Jid,
    own_user: &str,
    now: DateTime<Local>,
) -> Option<HistoryChat> {
    let first = conversation.messages.first()?;
    let latest = first
        .message
        .as_option()
        .and_then(|web| unix_local(web.message_timestamp.unwrap_or_default()))?;
    let chat_jid = conversation.id.as_str();
    let messages = conversation
        .messages
        .iter()
        .filter_map(|item| {
            let web = item.message.as_option()?;
            let row = message_row(web.message.as_option(), now);
            if row.content.is_empty() && row.media_type.is_empty() {
                return None;
            }
            let key = web.key.as_option();
            let is_from_me = key.and_then(|k| k.from_me).unwrap_or(false);
            let participant = key
                .and_then(|k| k.participant.as_deref())
                .filter(|p| !p.is_empty());
            let sender = match (is_from_me, participant) {
                (false, Some(participant)) => participant.to_string(),
                (true, _) => own_user.to_string(),
                (false, None) => chat.user.clone(),
            };
            let timestamp = unix_local(web.message_timestamp.unwrap_or_default())?;
            Some(NewMessage {
                id: key.and_then(|k| k.id.clone()).unwrap_or_default(),
                chat_jid: chat_jid.to_string(),
                sender,
                timestamp: Some(timestamp),
                is_from_me,
                ..row
            })
        })
        .collect();
    Some(HistoryChat { latest, messages })
}

/// A chat's name, or that a group's name must be asked from WhatsApp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatName {
    Known(String),
    /// A group without a stored name or a name in the history sync: ask
    /// WhatsApp for its subject, else use [`group_fallback`].
    AskGroup,
}

/// The Go bridge's `GetChatName`: the stored name when the chat has one;
/// for a group the name from the history sync conversation; for a contact
/// the name saved in the phone (also under the contact's other ID, LID or
/// phone number), else the sender, else the number.
pub fn chat_name(
    writer: &Writer,
    chat: &Jid,
    chat_jid: &str,
    conversation: Option<&wa::Conversation>,
    sender: &str,
) -> StoreResult<ChatName> {
    if let Some(name) = writer.chat_name(chat_jid)? {
        return Ok(ChatName::Known(name));
    }
    if chat.server == GROUP_SERVER {
        let from_history = conversation.and_then(|c| {
            [&c.display_name, &c.name]
                .into_iter()
                .find_map(|name| name.as_deref().filter(|n| !n.is_empty()))
        });
        return Ok(match from_history {
            Some(name) => ChatName::Known(name.to_string()),
            None => ChatName::AskGroup,
        });
    }
    if let Some(name) = contact_name(writer, chat)? {
        return Ok(ChatName::Known(name));
    }
    Ok(ChatName::Known(if sender.is_empty() {
        chat.user.clone()
    } else {
        sender.to_string()
    }))
}

fn contact_name(writer: &Writer, chat: &Jid) -> StoreResult<Option<String>> {
    let chat = chat.to_non_ad();
    if let Some(name) = writer.contact_full_name(&chat.to_string())? {
        return Ok(Some(name));
    }
    match counterpart(writer, &chat)? {
        Some(other) => writer.contact_full_name(&other.to_string()),
        None => Ok(None),
    }
}

/// The other ID of a contact, from `lid_map`: the phone number for a LID,
/// the LID for a phone number.
pub fn counterpart(writer: &Writer, jid: &Jid) -> StoreResult<Option<Jid>> {
    let (is_lid, other_server) = match jid.server.as_str() {
        LID_SERVER => (true, USER_SERVER),
        USER_SERVER => (false, LID_SERVER),
        _ => return Ok(None),
    };
    Ok(writer
        .lid_counterpart(&jid.user, is_lid)?
        .map(|user| Jid::new(user, other_server)))
}

/// The name of a group whose subject is unknown.
pub fn group_fallback(chat: &Jid) -> String {
    format!("Group {}", chat.user)
}

/// The LID and phone number (user parts) when one JID is a LID and the
/// other a phone number, in either order.
pub fn lid_pair(a: &Jid, b: &Jid) -> Option<(String, String)> {
    match (a.server.as_str(), b.server.as_str()) {
        (LID_SERVER, USER_SERVER) if !a.user.is_empty() && !b.user.is_empty() => {
            Some((a.user.clone(), b.user.clone()))
        }
        (USER_SERVER, LID_SERVER) => lid_pair(b, a),
        _ => None,
    }
}

/// [`lid_pair`] for two JIDs as text; `None` when either does not parse.
pub fn lid_pair_text(a: &str, b: &str) -> Option<(String, String)> {
    lid_pair(&Jid::parse(a).ok()?, &Jid::parse(b).ok()?)
}
