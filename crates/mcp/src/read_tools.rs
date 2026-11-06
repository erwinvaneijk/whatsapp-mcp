//! The read tools: they read `messages.db` (and contact names from
//! `whatsapp.db`) and never contact the bridge. Same queries, formatting and
//! error handling as the Python server, except that errors now say what was
//! wrong (see `docs/rust-1.0/interface.md`, section 5).

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

/// A database error in a tool that, like its Python original, answers with
/// an empty result instead of failing.
fn log_db_error(e: &whatsapp_mcp_store::Error) {
    eprintln!("Database error: {e}");
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
    // Empty text filters count as not given, as in Python (`if after:`).
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
        let mut shown = Vec::new();
        for message in &messages {
            let context = match db.message_context(&message.id, context_before, context_after) {
                Ok(Some(context)) => context,
                Ok(None) => return Err(not_found(&message.id)),
                Err(e) => return Output::text(database_error(&e)),
            };
            shown.extend(context.before);
            shown.push(context.message);
            shown.extend(context.after);
        }
        shown
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

fn not_found(message_id: &str) -> ToolError {
    ToolError(format!("Message with ID {message_id} not found"))
}

fn database_error(e: &whatsapp_mcp_store::Error) -> String {
    eprintln!("Database error: {e}");
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
