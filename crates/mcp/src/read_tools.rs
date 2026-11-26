//! The read tools: they read `messages.db` (and contact names from
//! `whatsapp.db`) and never contact the bridge. Errors say what was wrong.

use std::collections::HashSet;

use whatsapp_mcp_store::{
    Chat, ChatActivity, Contact, Db, MEDIA_TYPES, MediaFilter, MediaMessage, Message,
    MessageContext, MessageFilter, PooledDb, Store, moment_for_sql,
};

use crate::args::{Args, BadArgument, check_non_negative};
use crate::output::{Output, ToolError};
use crate::untrusted::{mark_untrusted, one_line};

impl From<BadArgument> for ToolError {
    fn from(e: BadArgument) -> Self {
        ToolError(e.0)
    }
}

/// Runs a read tool. `None` when `name` is not one.
pub fn call(store: &Store, name: &str, args: &Args<'_>) -> Option<Result<Output, ToolError>> {
    Some(match name {
        "search_contacts" => search_contacts(store, args),
        "list_messages" => list_messages(store, args),
        "list_chats" => list_chats(store, args),
        "get_chat" => get_chat(store, args),
        "get_direct_chat_by_contact" => get_direct_chat_by_contact(store, args),
        "get_contact_chats" => get_contact_chats(store, args),
        "get_last_interaction" => get_last_interaction(store, args),
        "get_message_context" => get_message_context(store, args),
        "list_media" => list_media(store, args),
        "list_recent_activity" => list_recent_activity(store, args),
        _ => return None,
    })
}

/// Opens `messages.db`. When that fails, the error says where it was
/// looking, so a missing store or a wrong `store_dir` is not mistaken for
/// "no results" (issue #69).
fn open(store: &Store) -> Result<PooledDb, ToolError> {
    store.open().map_err(|e| {
        let path = store.messages_db().display().to_string();
        let mut reason = e.to_string();
        // SQLite's message usually names the file already.
        if !reason.contains(&path) {
            reason = format!("{path}: {reason}");
        }
        log::warn!("Cannot open the message database: {reason}");
        ToolError(format!(
            "Cannot open the message database: {reason}. Check that the bridge has run and that store_dir points to its store folder."
        ))
    })
}

/// A query's result, or a tool error with the reason, so a broken database
/// is not mistaken for "no results" (issue #69).
fn checked<T>(result: whatsapp_mcp_store::Result<T>) -> Result<T, ToolError> {
    result.map_err(|e| ToolError(database_error(&e)))
}

fn search_contacts(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let query = args.string("query")?;
    let contacts = checked(open(store)?.search_contacts(&query))?;
    Output::list(contacts.into_iter().map(mark_contact))
}

fn list_messages(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let after = args.opt_string("after")?;
    let before = args.opt_string("before")?;
    let sender_phone_number = args.opt_string("sender_phone_number")?;
    let chat_jid = args.opt_string("chat_jid")?;
    let query = args.opt_string("query")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    let include_context = args.bool_or("include_context", true)?;
    let context_before = args.int_or("context_before", 1)?;
    let context_after = args.int_or("context_after", 1)?;
    check_non_negative(&[
        ("limit", limit),
        ("page", page),
        ("context_before", context_before),
        ("context_after", context_after),
    ])?;
    // Empty text filters count as not given.
    let filter = MessageFilter {
        after: non_empty(after)
            .map(|v| moment_for_sql(&v, "after"))
            .transpose()
            .map_err(|e| ToolError(e.to_string()))?,
        before: non_empty(before)
            .map(|v| moment_for_sql(&v, "before"))
            .transpose()
            .map_err(|e| ToolError(e.to_string()))?,
        sender_phone_number: non_empty(sender_phone_number),
        chat_jid: non_empty(chat_jid),
        query: non_empty(query),
        limit,
        page,
    };
    let db = open(store)?;
    let messages = checked(db.list_messages(&filter))?;
    let shown = if include_context && !messages.is_empty() {
        let mut windows = Vec::new();
        for message in &messages {
            let context = checked(db.message_context(&message.id, context_before, context_after))?
                .ok_or_else(|| not_found(&message.id))?;
            // `before` comes newest first; a window reads oldest first.
            let mut window: Vec<Message> = context.before.into_iter().rev().collect();
            window.push(context.message);
            window.extend(context.after);
            windows.push(window);
        }
        merge_windows(windows)
    } else {
        messages
    };
    Output::text(format_messages(&db, &shown, true))
}

fn list_chats(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let query = args.opt_string("query")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    let include_last_message = args.bool_or("include_last_message", true)?;
    let sort_by = args.string_or("sort_by", "last_active")?;
    check_non_negative(&[("limit", limit), ("page", page)])?;
    let query = non_empty(query);
    let chats = checked(open(store)?.list_chats(
        query.as_deref(),
        limit,
        page,
        include_last_message,
        &sort_by,
    ))?;
    Output::list(chats.into_iter().map(mark_chat))
}

fn get_chat(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let chat_jid = args.string("chat_jid")?;
    let include_last_message = args.bool_or("include_last_message", true)?;
    let chat = checked(open(store)?.chat(&chat_jid, include_last_message))?;
    Output::optional(chat.map(mark_chat))
}

fn get_direct_chat_by_contact(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let phone = args.string("sender_phone_number")?;
    let chat = checked(open(store)?.direct_chat_by_contact(&phone))?;
    Output::optional(chat.map(mark_chat))
}

fn get_contact_chats(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let jid = args.string("jid")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    check_non_negative(&[("limit", limit), ("page", page)])?;
    let chats = checked(open(store)?.contact_chats(&jid, limit, page))?;
    Output::list(chats.into_iter().map(mark_chat))
}

fn get_last_interaction(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let jid = args.string("jid")?;
    let db = open(store)?;
    let text =
        checked(db.last_interaction(&jid))?.map(|message| format_message(&db, &message, true));
    Output::optional(text)
}

fn get_message_context(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let message_id = args.string("message_id")?;
    let before = args.int_or("before", 5)?;
    let after = args.int_or("after", 5)?;
    check_non_negative(&[("before", before), ("after", after)])?;
    let context = checked(open(store)?.message_context(&message_id, before, after))?
        .ok_or_else(|| not_found(&message_id))?;
    Output::object(&mark_context(context))
}

fn list_media(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let chat_jid = args.opt_string("chat_jid")?;
    let media_type = args.opt_string("media_type")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    check_non_negative(&[("limit", limit), ("page", page)])?;
    let media_type = non_empty(media_type.map(|t| t.trim().to_lowercase()));
    if let Some(given) = &media_type
        && !MEDIA_TYPES.contains(&given.as_str())
    {
        return Err(ToolError(format!(
            "'media_type' must be one of {}, got '{given}'",
            MEDIA_TYPES.join(", ")
        )));
    }
    let filter = MediaFilter {
        chat_jid: non_empty(chat_jid),
        media_type,
        limit,
        page,
    };
    let db = open(store)?;
    let media = checked(db.list_media(&filter))?;
    if media.is_empty() {
        return Output::text("No media to display.".to_string());
    }
    Output::text(media.iter().map(|m| format_media(&db, m)).collect())
}

fn list_recent_activity(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let after = args.string("after")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    check_non_negative(&[("limit", limit), ("page", page)])?;
    let after = moment_for_sql(&after, "after").map_err(|e| ToolError(e.to_string()))?;
    let chats = checked(open(store)?.recent_activity(&after, limit, page))?;
    Output::list(chats.into_iter().map(mark_activity))
}

/// Context windows, one per match, as the messages to show: windows in the
/// same chat that share a message become one, so no message is shown twice
/// (issue #68). Each merged window is in time order; the windows keep the
/// order of their first match.
fn merge_windows(windows: Vec<Vec<Message>>) -> Vec<Message> {
    let key = |m: &Message| (m.chat_jid.clone(), m.id.clone());
    let mut merged: Vec<Vec<Message>> = Vec::new();
    for window in windows {
        let ids: HashSet<_> = window.iter().map(key).collect();
        // Every earlier window this one overlaps joins the first of them.
        let overlapping: Vec<usize> = (0..merged.len())
            .filter(|&i| merged[i].iter().any(|m| ids.contains(&key(m))))
            .collect();
        let Some((&first, rest)) = overlapping.split_first() else {
            merged.push(window);
            continue;
        };
        for &i in rest.iter().rev() {
            let other = merged.remove(i);
            merged[first].extend(other);
        }
        merged[first].extend(window);
        let target = &mut merged[first];
        let mut seen = HashSet::new();
        target.retain(|m| seen.insert(key(m)));
        // Stable: messages with the same time keep their order.
        target.sort_by_key(|m| m.timestamp.0);
    }
    merged.into_iter().flatten().collect()
}

fn not_found(message_id: &str) -> ToolError {
    ToolError(format!("Message with ID {message_id} not found"))
}

fn database_error(e: &whatsapp_mcp_store::Error) -> String {
    log::warn!("Database error: {e}");
    format!("Database error: {e}")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn mark_chat(chat: Chat) -> Chat {
    Chat {
        name: chat.name.as_deref().map(one_line),
        last_message: chat.last_message.as_deref().map(mark_untrusted),
        ..chat
    }
}

fn mark_activity(activity: ChatActivity) -> ChatActivity {
    ChatActivity {
        name: activity.name.as_deref().map(one_line),
        last_message: activity.last_message.as_deref().map(mark_untrusted),
        ..activity
    }
}

fn mark_message(message: Message) -> Message {
    Message {
        content: message.content.as_deref().map(mark_untrusted),
        chat_name: message.chat_name.as_deref().map(one_line),
        ..message
    }
}

fn mark_context(context: MessageContext) -> MessageContext {
    MessageContext {
        message: mark_message(context.message),
        before: context.before.into_iter().map(mark_message).collect(),
        after: context.after.into_iter().map(mark_message).collect(),
    }
}

fn mark_contact(contact: Contact) -> Contact {
    Contact {
        name: contact.name.as_deref().map(one_line),
        ..contact
    }
}

/// One message as a line of text, with its ID, so it can be used with
/// reply_to_message_id, react_to_message and get_message_context.
pub fn format_message(db: &Db, message: &Message, show_chat_info: bool) -> String {
    let time = message.timestamp.display();
    let mut out = match message.chat_name.as_deref().filter(|n| !n.is_empty()) {
        Some(chat) if show_chat_info => format!("[{time}] Chat: {} ", one_line(chat)),
        _ => format!("[{time}] "),
    };
    let prefix = match message.media_type.as_deref().filter(|m| !m.is_empty()) {
        Some(media) => format!(
            "[{media} - Message ID: {} - Chat JID: {}] ",
            message.id, message.chat_jid
        ),
        None => format!("[Message ID: {}] ", message.id),
    };
    let reply = match message
        .quoted_message_id
        .as_deref()
        .filter(|q| !q.is_empty())
    {
        Some(quoted) => format!(" (reply to {quoted})"),
        None => String::new(),
    };
    let sender = if message.is_from_me {
        "Me".to_string()
    } else {
        one_line(&db.sender_name(message.sender.as_deref().unwrap_or("")))
    };
    out.push_str(&format!(
        "From: {sender}{reply}: {prefix}{}\n",
        mark_untrusted(message.content.as_deref().unwrap_or(""))
    ));
    out
}

/// A message with media as a line of text, like [`format_message`], with
/// its file name. The name can come from the sender, so it is marked too.
fn format_media(db: &Db, media: &MediaMessage) -> String {
    let mut line = format_message(db, &media.message, true);
    if let Some(filename) = media.filename.as_deref().filter(|f| !f.is_empty()) {
        line.truncate(line.trim_end_matches('\n').len());
        line.push_str(&format!(" File: {}\n", mark_untrusted(filename)));
    }
    line
}

fn format_messages(db: &Db, messages: &[Message], show_chat_info: bool) -> String {
    if messages.is_empty() {
        return "No messages to display.".to_string();
    }
    messages
        .iter()
        .map(|m| format_message(db, m, show_chat_info))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_mcp_store::Timestamp;

    fn m(chat: &str, id: &str, minute: u32) -> Message {
        Message {
            timestamp: Timestamp::parse_stored(&format!("2026-11-02 19:{minute:02}:00+01:00"))
                .unwrap(),
            sender: None,
            content: None,
            is_from_me: false,
            chat_jid: chat.into(),
            id: id.into(),
            chat_name: None,
            media_type: None,
            quoted_message_id: None,
        }
    }

    fn ids(messages: &[Message]) -> Vec<&str> {
        messages.iter().map(|m| m.id.as_str()).collect()
    }

    /// Issue #69: a database that opens but cannot be read is an error in
    /// every read tool, not an empty result.
    #[test]
    fn database_errors_are_tool_errors() {
        let dir = tempfile::tempdir().unwrap();
        // An empty file is a valid SQLite database without any tables.
        std::fs::write(dir.path().join("messages.db"), b"").unwrap();
        let store = Store::new(dir.path());
        let args: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{"query":"x","chat_jid":"1@s.whatsapp.net","sender_phone_number":"1","jid":"1@s.whatsapp.net","message_id":"M1"}"#,
        )
        .unwrap();
        // Its `after` is a moment; get_message_context's is a number.
        let mut activity_args = args.clone();
        activity_args.insert("after".into(), "2026-11-01".into());
        for tool in [
            "search_contacts",
            "list_messages",
            "list_chats",
            "get_chat",
            "get_direct_chat_by_contact",
            "get_contact_chats",
            "get_last_interaction",
            "get_message_context",
            "list_media",
            "list_recent_activity",
        ] {
            let args = if tool == "list_recent_activity" {
                &activity_args
            } else {
                &args
            };
            let result = call(&store, tool, &Args::new(Some(args))).unwrap();
            let Err(ToolError(message)) = result else {
                panic!("{tool}: not an error");
            };
            assert!(
                message.starts_with("Database error: no such table"),
                "{tool}: {message}"
            );
        }
    }

    #[test]
    fn overlapping_windows_merge_in_time_order() {
        let a = |id: &str, minute| m("a@g.us", id, minute);
        let shown = merge_windows(vec![
            vec![a("7", 7), a("8", 8), a("9", 9)],
            vec![a("2", 2), a("3", 3), a("4", 4)],
            vec![a("6", 6), a("7", 7), a("8", 8)],
        ]);
        assert_eq!(ids(&shown), ["6", "7", "8", "9", "2", "3", "4"]);
    }

    #[test]
    fn a_window_can_join_two_earlier_ones() {
        let a = |id: &str, minute| m("a@g.us", id, minute);
        let shown = merge_windows(vec![
            vec![a("5", 5), a("6", 6)],
            vec![a("1", 1), a("2", 2)],
            vec![a("2", 2), a("3", 3), a("4", 4), a("5", 5)],
        ]);
        assert_eq!(ids(&shown), ["1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn the_same_id_in_another_chat_is_another_message() {
        let shown = merge_windows(vec![
            vec![m("a@g.us", "1", 1), m("a@g.us", "2", 2)],
            vec![m("b@g.us", "2", 2), m("b@g.us", "3", 3)],
        ]);
        assert_eq!(ids(&shown), ["1", "2", "2", "3"]);
        assert_eq!(shown[2].chat_jid, "b@g.us");
    }
}
