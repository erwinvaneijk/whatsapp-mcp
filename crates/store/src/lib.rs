//! `messages.db`: the queries the MCP tools need, with the same SQL as the
//! Python server (`whatsapp-mcp-server/whatsapp.py`), so results match row
//! for row. The bridge writes the database; this crate only reads it.
//!
//! SQLite is bundled, so full-text search (FTS5) is always available. The
//! bridge's FTS5 index is only used when it is kept up to date (see
//! [`Db::fts_ready`]).

mod names;
mod time;
mod write;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params_from_iter};
use serde::Serialize;

pub use names::{Names, NamesSource, names_source, user_part};
pub use time::{BadMoment, Timestamp, moment_for_sql};
pub use write::{
    ContactNames, MediaInfo, NewMessage, StoredSender, Writer, ensure_private_dir,
    ensure_private_file, go_time, migrate,
};

pub type Error = rusqlite::Error;
pub type Result<T> = std::result::Result<T, Error>;

/// One message, as the tools return it. Field order is the Python
/// dataclass's, which is the order of the JSON text.
#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub timestamp: Timestamp,
    pub sender: Option<String>,
    pub content: Option<String>,
    pub is_from_me: bool,
    pub chat_jid: String,
    pub id: String,
    pub chat_name: Option<String>,
    pub media_type: Option<String>,
    /// Set when this message is a reply.
    pub quoted_message_id: Option<String>,
}

/// One chat. Field order is the Python dataclass's.
#[derive(Debug, Clone, Serialize)]
pub struct Chat {
    pub jid: String,
    pub name: Option<String>,
    pub last_message_time: Option<Timestamp>,
    pub last_message: Option<String>,
    pub last_sender: Option<String>,
    pub last_is_from_me: Option<bool>,
}

/// One contact. Field order is the Python dataclass's.
#[derive(Debug, Clone, Serialize)]
pub struct Contact {
    pub phone_number: String,
    pub name: Option<String>,
    pub jid: String,
}

/// A message with the messages around it in its chat.
#[derive(Debug, Clone, Serialize)]
pub struct MessageContext {
    pub message: Message,
    pub before: Vec<Message>,
    pub after: Vec<Message>,
}

/// The bridge's store folder.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    pub fn messages_db(&self) -> PathBuf {
        self.dir.join("messages.db")
    }

    pub fn whatsapp_db(&self) -> PathBuf {
        self.dir.join("whatsapp.db")
    }

    /// Where contact names come from.
    pub fn names_source(&self) -> NamesSource {
        names_source(&self.messages_db(), &self.whatsapp_db())
    }

    /// Opens `messages.db` read-only, with the contact names loaded and
    /// `display_name()` and `jid_matches()` available in SQL.
    pub fn open(&self) -> Result<Db> {
        let names = Arc::new(Names::load(&self.messages_db(), &self.whatsapp_db()));
        Db::open(&self.messages_db(), names)
    }
}

/// Filters for [`Db::list_messages`].
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    /// UTC text from [`moment_for_sql`].
    pub after: Option<String>,
    pub before: Option<String>,
    pub sender_phone_number: Option<String>,
    pub chat_jid: Option<String>,
    pub query: Option<String>,
    pub limit: i64,
    pub page: i64,
}

/// The columns every message query selects, in this order.
const MESSAGE_COLUMNS: &str = "messages.timestamp, messages.sender, display_name(chats.jid, chats.name), messages.content, messages.is_from_me, chats.jid, messages.id, messages.media_type";

/// An open `messages.db`.
pub struct Db {
    conn: Connection,
    names: Arc<Names>,
}

impl Db {
    pub fn open(path: &Path, names: Arc<Names>) -> Result<Db> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
        let n = Arc::clone(&names);
        conn.create_scalar_function("display_name", 2, flags, move |ctx| {
            let jid: Option<String> = ctx.get(0)?;
            let stored: Option<String> = ctx.get(1)?;
            Ok(n.display_name(jid.as_deref(), stored.as_deref()))
        })?;
        let n = Arc::clone(&names);
        conn.create_scalar_function("jid_matches", 2, flags, move |ctx| {
            let jid: Option<String> = ctx.get(0)?;
            let phone: Option<String> = ctx.get(1)?;
            Ok(match (jid, phone) {
                (Some(jid), Some(phone)) => n.jid_matches(&jid, &phone),
                _ => false,
            })
        })?;
        Ok(Db { conn, names })
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    /// SQL for the quoted message ID, or NULL when the bridge has not added
    /// the `quoted_message_id` column yet (it does so on start).
    fn quoted_column(&self, table: &str) -> Result<String> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'quoted_message_id'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(if exists {
            format!("{table}.quoted_message_id")
        } else {
            "NULL".to_string()
        })
    }

    /// Whether the FTS5 index exists and is kept up to date. The bridge
    /// creates `messages_fts` and its triggers only when built with FTS5, and
    /// drops the triggers when run without it; then the index is stale.
    pub fn fts_ready(&self) -> Result<bool> {
        let trigger = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='messages_ai'",
                [],
                |_| Ok(()),
            )
            .optional()?;
        if trigger.is_none() {
            return Ok(false);
        }
        Ok(self
            .conn
            .execute_batch("SELECT 1 FROM messages_fts LIMIT 0")
            .is_ok())
    }

    /// Messages matching the filter, newest first.
    pub fn list_messages(&self, filter: &MessageFilter) -> Result<Vec<Message>> {
        let mut sql = vec![format!(
            "SELECT {MESSAGE_COLUMNS}, {} FROM messages",
            self.quoted_column("messages")?
        )];
        sql.push("JOIN chats ON messages.chat_jid = chats.jid".into());
        let mut wheres: Vec<&str> = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(after) = &filter.after {
            wheres.push("julianday(messages.timestamp) > julianday(?)");
            params.push(after.clone().into());
        }
        if let Some(before) = &filter.before {
            wheres.push("julianday(messages.timestamp) < julianday(?)");
            params.push(before.clone().into());
        }
        if let Some(sender) = &filter.sender_phone_number {
            wheres.push("messages.sender = ?");
            params.push(sender.clone().into());
        }
        if let Some(chat) = &filter.chat_jid {
            wheres.push("messages.chat_jid = ?");
            params.push(chat.clone().into());
        }
        if let Some(query) = &filter.query {
            if self.fts_ready()? {
                sql.push("JOIN messages_fts ON messages_fts.rowid = messages.rowid".into());
                wheres.push("messages_fts MATCH ?");
                // Quoted, so FTS5 reads it as a phrase, not query syntax;
                // the trailing * makes the last word a prefix.
                params.push(format!("\"{}\"*", query.replace('"', "\"\"")).into());
            } else {
                wheres.push("LOWER(messages.content) LIKE LOWER(?)");
                params.push(format!("%{query}%").into());
            }
        }
        if !wheres.is_empty() {
            sql.push(format!("WHERE {}", wheres.join(" AND ")));
        }
        sql.push("ORDER BY julianday(messages.timestamp) DESC".into());
        sql.push("LIMIT ? OFFSET ?".into());
        params.push(filter.limit.into());
        params.push(filter.page.saturating_mul(filter.limit).into());

        let mut statement = self.conn.prepare(&sql.join(" "))?;
        let rows = statement.query_map(params_from_iter(params), message_from_row)?;
        rows.collect::<Result<Vec<_>>>()?.into_iter().collect()
    }

    /// A message and up to `before` and `after` messages around it in its
    /// chat; `None` when there is no message with this ID.
    pub fn message_context(
        &self,
        message_id: &str,
        before: i64,
        after: i64,
    ) -> Result<Option<MessageContext>> {
        let quoted = self.quoted_column("messages")?;
        let target = self
            .conn
            .query_row(
                &format!(
                    "SELECT {MESSAGE_COLUMNS}, {quoted}, messages.timestamp, messages.chat_jid FROM messages JOIN chats ON messages.chat_jid = chats.jid WHERE messages.id = ?"
                ),
                [message_id],
                |row| {
                    Ok((
                        message_from_row(row)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                    ))
                },
            )
            .optional()?;
        let Some((message, timestamp, chat_jid)) = target else {
            return Ok(None);
        };
        let message = message?;
        let around = |comparison: &str, order: &str, limit: i64| -> Result<Vec<Message>> {
            let mut statement = self.conn.prepare(&format!(
                "SELECT {MESSAGE_COLUMNS}, {quoted} FROM messages JOIN chats ON messages.chat_jid = chats.jid WHERE messages.chat_jid = ? AND julianday(messages.timestamp) {comparison} julianday(?) ORDER BY julianday(messages.timestamp) {order} LIMIT ?"
            ))?;
            let rows = statement.query_map(
                rusqlite::params![chat_jid, timestamp, limit],
                message_from_row,
            )?;
            rows.collect::<Result<Vec<_>>>()?.into_iter().collect()
        };
        Ok(Some(MessageContext {
            before: around("<", "DESC", before)?,
            after: around(">", "ASC", after)?,
            message,
        }))
    }

    /// Chats matching `query` (in the name or JID), by last activity or name.
    pub fn list_chats(
        &self,
        query: Option<&str>,
        limit: i64,
        page: i64,
        include_last_message: bool,
        sort_by: &str,
    ) -> Result<Vec<Chat>> {
        let last_cols = if include_last_message {
            "messages.content as last_message, messages.sender as last_sender, messages.is_from_me as last_is_from_me"
        } else {
            "NULL as last_message, NULL as last_sender, NULL as last_is_from_me"
        };
        let mut sql = vec![format!(
            "SELECT chats.jid, display_name(chats.jid, chats.name), chats.last_message_time, {last_cols} FROM chats"
        )];
        if include_last_message {
            // Join the latest message by rowid, not by timestamp equality
            // (upstream #282), which dropped or duplicated chats.
            sql.push(
                "LEFT JOIN messages ON messages.rowid = (SELECT rowid FROM messages WHERE chat_jid = chats.jid ORDER BY julianday(timestamp) DESC LIMIT 1)".into(),
            );
        }
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(query) = query {
            sql.push(
                "WHERE (LOWER(display_name(chats.jid, chats.name)) LIKE LOWER(?) OR chats.jid LIKE ?)"
                    .into(),
            );
            params.push(format!("%{query}%").into());
            params.push(format!("%{query}%").into());
        }
        sql.push(format!(
            "ORDER BY {}",
            if sort_by == "last_active" {
                "julianday(chats.last_message_time) DESC"
            } else {
                "display_name(chats.jid, chats.name)"
            }
        ));
        sql.push("LIMIT ? OFFSET ?".into());
        params.push(limit.into());
        params.push(page.saturating_mul(limit).into());
        let mut statement = self.conn.prepare(&sql.join(" "))?;
        let rows = statement.query_map(params_from_iter(params), chat_from_row)?;
        rows.collect::<Result<Vec<_>>>()?.into_iter().collect()
    }

    /// Contacts (not groups) whose name, JID or mapped number matches.
    pub fn search_contacts(&self, query: &str) -> Result<Vec<Contact>> {
        let pattern = format!("%{query}%");
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT jid, display_name(jid, name) AS resolved_name FROM chats WHERE (LOWER(display_name(jid, name)) LIKE LOWER(?) OR LOWER(jid) LIKE LOWER(?) OR jid_matches(jid, ?)) AND jid NOT LIKE '%@g.us' ORDER BY resolved_name, jid LIMIT 50",
        )?;
        let rows = statement.query_map(rusqlite::params![pattern, pattern, query], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        rows.map(|row| {
            let (jid, name) = row?;
            Ok(Contact {
                phone_number: self.names.phone_number(&jid),
                name,
                jid,
            })
        })
        .collect()
    }

    /// The chats with this contact: their own chat, and chats they wrote in.
    pub fn contact_chats(&self, jid: &str, limit: i64, page: i64) -> Result<Vec<Chat>> {
        let mut statement = self.conn.prepare(
            "SELECT c.jid, display_name(c.jid, c.name), c.last_message_time, m.content as last_message, m.sender as last_sender, m.is_from_me as last_is_from_me FROM chats c LEFT JOIN messages m ON m.rowid = (SELECT rowid FROM messages WHERE chat_jid = c.jid ORDER BY julianday(timestamp) DESC LIMIT 1) WHERE c.jid = ? OR c.jid IN (SELECT chat_jid FROM messages WHERE sender = ?) ORDER BY julianday(c.last_message_time) DESC LIMIT ? OFFSET ?",
        )?;
        let rows = statement.query_map(
            rusqlite::params![jid, jid, limit, page.saturating_mul(limit)],
            chat_from_row,
        )?;
        rows.collect::<Result<Vec<_>>>()?.into_iter().collect()
    }

    /// The most recent message from or with this contact.
    pub fn last_interaction(&self, jid: &str) -> Result<Option<Message>> {
        let quoted = self.quoted_column("m")?;
        let message = self
            .conn
            .query_row(
                &format!(
                    "SELECT m.timestamp, m.sender, display_name(c.jid, c.name), m.content, m.is_from_me, c.jid, m.id, m.media_type, {quoted} FROM messages m JOIN chats c ON m.chat_jid = c.jid WHERE m.sender = ? OR c.jid = ? ORDER BY julianday(m.timestamp) DESC LIMIT 1"
                ),
                [jid, jid],
                message_from_row,
            )
            .optional()?;
        message.transpose()
    }

    /// One chat by JID.
    pub fn chat(&self, chat_jid: &str, include_last_message: bool) -> Result<Option<Chat>> {
        let mut sql = format!(
            "SELECT c.jid, display_name(c.jid, c.name), c.last_message_time, {} FROM chats c",
            if include_last_message {
                "m.content as last_message, m.sender as last_sender, m.is_from_me as last_is_from_me"
            } else {
                "NULL as last_message, NULL as last_sender, NULL as last_is_from_me"
            }
        );
        if include_last_message {
            sql.push_str(" LEFT JOIN messages m ON m.rowid = (SELECT rowid FROM messages WHERE chat_jid = c.jid ORDER BY julianday(timestamp) DESC LIMIT 1)");
        }
        sql.push_str(" WHERE c.jid = ?");
        let chat = self
            .conn
            .query_row(&sql, [chat_jid], chat_from_row)
            .optional()?;
        chat.transpose()
    }

    /// The direct chat (not a group) whose JID contains this phone number.
    pub fn direct_chat_by_contact(&self, phone: &str) -> Result<Option<Chat>> {
        let chat = self
            .conn
            .query_row(
                "SELECT c.jid, display_name(c.jid, c.name), c.last_message_time, m.content as last_message, m.sender as last_sender, m.is_from_me as last_is_from_me FROM chats c LEFT JOIN messages m ON m.rowid = (SELECT rowid FROM messages WHERE chat_jid = c.jid ORDER BY julianday(timestamp) DESC LIMIT 1) WHERE jid_matches(c.jid, ?) AND c.jid NOT LIKE '%@g.us' LIMIT 1",
                [phone],
                chat_from_row,
            )
            .optional()?;
        chat.transpose()
    }

    /// The name to show for a message's sender: the chat stored under that
    /// JID (or containing its number), resolved like a chat name, else the
    /// JID itself. A database error gives the JID too, as in Python.
    pub fn sender_name(&self, sender: &str) -> String {
        let stored = self.stored_sender_name(sender).unwrap_or_else(|e| {
            log::warn!("Database error while getting sender name: {e}");
            None
        });
        self.names
            .display_name(Some(sender), stored.as_deref())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| sender.to_string())
    }

    fn stored_sender_name(&self, sender: &str) -> Result<Option<String>> {
        let exact = self
            .conn
            .query_row(
                "SELECT name FROM chats WHERE jid = ? LIMIT 1",
                [sender],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        let row = match exact {
            Some(name) => Some(name),
            None => self
                .conn
                .query_row(
                    "SELECT name FROM chats WHERE jid LIKE ? LIMIT 1",
                    [format!("%{}%", user_part(sender))],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?,
        };
        Ok(row.flatten().filter(|name| !name.is_empty()))
    }
}

/// A message row; the inner error is a timestamp that cannot be read.
fn message_from_row(row: &Row<'_>) -> Result<Result<Message>> {
    let timestamp: String = row.get(0)?;
    let Some(timestamp) = Timestamp::parse_stored(&timestamp) else {
        return Ok(Err(bad_timestamp(&timestamp)));
    };
    Ok(Ok(Message {
        timestamp,
        sender: row.get(1)?,
        chat_name: row.get(2)?,
        content: row.get(3)?,
        is_from_me: row.get::<_, Option<bool>>(4)?.unwrap_or(false),
        chat_jid: row.get(5)?,
        id: row.get(6)?,
        media_type: row.get(7)?,
        quoted_message_id: row.get(8)?,
    }))
}

/// A chat row; the inner error is a timestamp that cannot be read.
fn chat_from_row(row: &Row<'_>) -> Result<Result<Chat>> {
    let time: Option<String> = row.get(2)?;
    let last_message_time = match time.as_deref().filter(|t| !t.is_empty()) {
        None => None,
        Some(text) => match Timestamp::parse_stored(text) {
            Some(t) => Some(t),
            None => return Ok(Err(bad_timestamp(text))),
        },
    };
    Ok(Ok(Chat {
        jid: row.get(0)?,
        name: row.get(1)?,
        last_message_time,
        last_message: row.get(3)?,
        last_sender: row.get(4)?,
        last_is_from_me: row.get(5)?,
    }))
}

fn bad_timestamp(text: &str) -> Error {
    Error::InvalidColumnType(
        0,
        format!("timestamp {text:?} is not an ISO-8601 time with offset"),
        rusqlite::types::Type::Text,
    )
}
