//! The read tools: they read `messages.db` (and contact names from
//! `whatsapp.db`) and never contact the bridge. Errors say what was wrong.

use std::collections::HashSet;

use whatsapp_mcp_store::{
    Chat, Contact, Db, Message, MessageContext, MessageFilter, Store, moment_for_sql,
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
        _ => return None,
    })
}

/// A database error in a tool that answers with an empty result instead of
/// failing (issue #69).
fn log_db_error(e: &whatsapp_mcp_store::Error) {
    log::warn!("Database error: {e}");
}

fn search_contacts(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let query = args.string("query")?;
    let contacts = store
        .open()
        .and_then(|db| db.search_contacts(&query))
        .unwrap_or_else(|e| {
            log_db_error(&e);
            Vec::new()
        });
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
    let db = match store.open() {
        Ok(db) => db,
        Err(e) => return Output::text(database_error(&e)),
    };
    let messages = match db.list_messages(&filter) {
        Ok(messages) => messages,
        Err(e) => return Output::text(database_error(&e)),
    };
    let shown = if include_context && !messages.is_empty() {
        let mut windows = Vec::new();
        for message in &messages {
            let context = match db.message_context(&message.id, context_before, context_after) {
                Ok(Some(context)) => context,
                Ok(None) => return Err(not_found(&message.id)),
                Err(e) => return Output::text(database_error(&e)),
            };
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
    let chats = store
        .open()
        .and_then(|db| {
            db.list_chats(
                query.as_deref(),
                limit,
                page,
                include_last_message,
                &sort_by,
            )
        })
        .unwrap_or_else(|e| {
            log_db_error(&e);
            Vec::new()
        });
    Output::list(chats.into_iter().map(mark_chat))
}

fn get_chat(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let chat_jid = args.string("chat_jid")?;
    let include_last_message = args.bool_or("include_last_message", true)?;
    let chat = store
        .open()
        .and_then(|db| db.chat(&chat_jid, include_last_message))
        .unwrap_or_else(|e| {
            log_db_error(&e);
            None
        });
    Output::optional(chat.map(mark_chat))
}

fn get_direct_chat_by_contact(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let phone = args.string("sender_phone_number")?;
    let chat = store
        .open()
        .and_then(|db| db.direct_chat_by_contact(&phone))
        .unwrap_or_else(|e| {
            log_db_error(&e);
            None
        });
    Output::optional(chat.map(mark_chat))
}

fn get_contact_chats(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let jid = args.string("jid")?;
    let limit = args.int_or("limit", 20)?;
    let page = args.int_or("page", 0)?;
    check_non_negative(&[("limit", limit), ("page", page)])?;
    let chats = store
        .open()
        .and_then(|db| db.contact_chats(&jid, limit, page))
        .unwrap_or_else(|e| {
            log_db_error(&e);
            Vec::new()
        });
    Output::list(chats.into_iter().map(mark_chat))
}

fn get_last_interaction(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let jid = args.string("jid")?;
    let text = store
        .open()
        .and_then(|db| {
            Ok(db
                .last_interaction(&jid)?
                .map(|message| format_message(&db, &message, true)))
        })
        .unwrap_or_else(|e| {
            log_db_error(&e);
            None
        });
    Output::optional(text)
}

fn get_message_context(store: &Store, args: &Args<'_>) -> Result<Output, ToolError> {
    let message_id = args.string("message_id")?;
    let before = args.int_or("before", 5)?;
    let after = args.int_or("after", 5)?;
    check_non_negative(&[("before", before), ("after", after)])?;
    let context = store
        .open()
        .and_then(|db| db.message_context(&message_id, before, after))
        .map_err(|e| ToolError(database_error(&e)))?
        .ok_or_else(|| not_found(&message_id))?;
    Output::object(&mark_context(context))
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
