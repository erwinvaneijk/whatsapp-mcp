//! `messages.db`: the queries the MCP tools need. The parity harness checks
//! their results against its golden files. The bridge writes the database;
//! this crate only reads it.
//!
//! SQLite is bundled, so full-text search (FTS5) is always available. The
//! bridge's FTS5 index is only used when it is kept up to date (see
//! [`Db::fts_ready`]).

mod cache;
mod names;
mod time;
mod write;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use cache::{Cache, SharedNames};

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params_from_iter};
use serde::Serialize;

pub use cache::{NAMES_MAX_AGE, PooledDb};
pub use names::{Names, NamesSource, names_source, user_part};
pub use time::{BadMoment, Timestamp, moment_for_sql};
pub use write::{
    ContactNames, MediaInfo, NewMessage, StoredSender, Writer, ensure_private_dir,
    ensure_private_file, migrate, stored_time,
};

pub type Error = rusqlite::Error;
pub type Result<T> = std::result::Result<T, Error>;

/// One message, as the tools return it. Field order is the order of the
/// JSON output, which the parity golden files check.
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

/// One chat. Field order is the order of the JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct Chat {
    pub jid: String,
    pub name: Option<String>,
    pub last_message_time: Option<Timestamp>,
    pub last_message: Option<String>,
    pub last_sender: Option<String>,
    pub last_is_from_me: Option<bool>,
}

/// A message with media, and the name of its file.
#[derive(Debug, Clone)]
pub struct MediaMessage {
    pub message: Message,
    /// A document's own name; for other media one the bridge made up.
    pub filename: Option<String>,
}

/// A chat with messages after some moment, and how many. Field order is
/// the order of the JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct ChatActivity {
    pub jid: String,
    pub name: Option<String>,
    /// Messages after the moment.
    pub message_count: i64,
    /// Of those, the ones not sent by me.
    pub from_others_count: i64,
    pub last_message_time: Option<Timestamp>,
    pub last_message: Option<String>,
    pub last_sender: Option<String>,
    pub last_is_from_me: Option<bool>,
}

/// One contact. Field order is the order of the JSON output.
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

/// The bridge's store folder. Clones share open connections and the
/// contact names, so `serve` keeps them between tool calls.
#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    cache: Arc<Cache>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("dir", &self.dir).finish()
    }
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store {
            dir: dir.into(),
            cache: Arc::default(),
        }
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

    /// `messages.db`, read-only, with the contact names loaded and
    /// `display_name()` and `jid_matches()` available in SQL. Connections
    /// and names are kept for the next call; see [`cache`](crate::cache).
    pub fn open(&self) -> Result<PooledDb> {
        self.cache
            .checkout(&self.messages_db(), &self.whatsapp_db())
    }

    /// Makes the next [`Store::open`] load the names again, as if they
    /// were older than [`NAMES_MAX_AGE`] (tests).
    #[doc(hidden)]
    pub fn expire_names(&self) {
        self.cache.expire_names();
    }
}

/// A row's place in a list's order, so the next page can start right after
/// it (issue #86). Paging by offset shifts when messages arrive between
/// calls; paging after a key does not. Every list orders by a time or name
/// and then by a tie-breaker, so the order is total.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    /// Messages, newest first: `julianday(timestamp)`, then the rowid.
    Message { time: f64, rowid: i64 },
    /// Chats by activity, latest first: the `julianday` of the last
    /// activity (-1 when there is none), then the JID.
    ChatTime { time: f64, jid: String },
    /// Chats by name: the display name (empty when there is none), then
    /// the JID.
    ChatName { name: String, jid: String },
}

/// One page of a list, and the key of its last row when there is more.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<Key>,
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
    /// Start after this message instead of at `page`.
    pub cursor: Option<Key>,
}

/// The kinds of media the bridge stores in `messages.media_type`.
pub const MEDIA_TYPES: [&str; 4] = ["image", "video", "audio", "document"];

/// Filters for [`Db::list_media`].
#[derive(Debug, Clone, Default)]
pub struct MediaFilter {
    pub chat_jid: Option<String>,
    /// One of [`MEDIA_TYPES`]; `None` for any media.
    pub media_type: Option<String>,
    pub limit: i64,
    pub page: i64,
    /// Start after this message instead of at `page`.
    pub cursor: Option<Key>,
}

/// The columns every message query selects, in this order.
const MESSAGE_COLUMNS: &str = "messages.timestamp, messages.sender, display_name(chats.jid, chats.name), messages.content, messages.is_from_me, chats.jid, messages.id, messages.media_type";

/// An open `messages.db`.
pub struct Db {
    conn: Connection,
    names: SharedNames,
    /// The schema checks, with the `schema_version` they were made at.
    schema: Cell<Option<(i64, Schema)>>,
}

/// What the queries need to know about the schema, which the bridge can
/// change while `serve` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Schema {
    /// `messages.quoted_message_id` exists (the bridge adds it on start).
    quoted_message_id: bool,
    /// The FTS5 index exists and is kept up to date.
    fts: bool,
}

impl Db {
    /// Opens `messages.db` read-only with these names.
    pub fn open(path: &Path, names: Arc<Names>) -> Result<Db> {
        Db::open_shared(path, Arc::new(RwLock::new(names)))
    }

    /// Opens `messages.db` read-only; the SQL functions read the names
    /// from `names` on each call, so they can be replaced while it is open.
    pub(crate) fn open_shared(path: &Path, names: SharedNames) -> Result<Db> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
        let n = Arc::clone(&names);
        conn.create_scalar_function("display_name", 2, flags, move |ctx| {
            let jid: Option<String> = ctx.get(0)?;
            let stored: Option<String> = ctx.get(1)?;
            Ok(read(&n).display_name(jid.as_deref(), stored.as_deref()))
        })?;
        let n = Arc::clone(&names);
        conn.create_scalar_function("jid_matches", 2, flags, move |ctx| {
            let jid: Option<String> = ctx.get(0)?;
            let phone: Option<String> = ctx.get(1)?;
            Ok(match (jid, phone) {
                (Some(jid), Some(phone)) => read(&n).jid_matches(&jid, &phone),
                _ => false,
            })
        })?;
        Ok(Db {
            conn,
            names,
            schema: Cell::new(None),
        })
    }

    /// The contact names in use now.
    pub fn names(&self) -> Arc<Names> {
        read(&self.names)
    }

    /// The schema checks, made again only when the schema has changed
    /// since the last time: SQLite raises `schema_version` on every change,
    /// so one cheap pragma replaces the checks on each query (issue #72).
    fn schema(&self) -> Result<Schema> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA schema_version", [], |row| row.get(0))?;
        if let Some((seen, schema)) = self.schema.get()
            && seen == version
        {
            return Ok(schema);
        }
        let schema = Schema {
            quoted_message_id: self.has_quoted_message_id()?,
            fts: self.check_fts()?,
        };
        self.schema.set(Some((version, schema)));
        Ok(schema)
    }

    fn has_quoted_message_id(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'quoted_message_id'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    fn check_fts(&self) -> Result<bool> {
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

    /// SQL for the quoted message ID, or NULL when the bridge has not added
    /// the `quoted_message_id` column yet (it does so on start).
    fn quoted_column(&self, table: &str) -> Result<String> {
        Ok(if self.schema()?.quoted_message_id {
            format!("{table}.quoted_message_id")
        } else {
            "NULL".to_string()
        })
    }

    /// Whether the FTS5 index exists and is kept up to date. The bridge
    /// creates `messages_fts` and its triggers only when built with FTS5, and
    /// drops the triggers when run without it; then the index is stale.
    pub fn fts_ready(&self) -> Result<bool> {
        Ok(self.schema()?.fts)
    }

    /// Messages matching the filter, newest first.
    pub fn list_messages(&self, filter: &MessageFilter) -> Result<Page<Message>> {
        let mut sql = vec![format!(
            "SELECT {MESSAGE_COLUMNS}, {}, {MESSAGE_KEY} FROM messages",
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
                wheres.push("LOWER(messages.content) LIKE LOWER(?) ESCAPE '\\'");
                params.push(contains_pattern(query).into());
            }
        }
        let after = filter.cursor.as_ref().map(message_after).transpose()?;
        if let Some((condition, values)) = &after {
            wheres.push(condition);
            params.extend(values.iter().cloned());
        }
        if !wheres.is_empty() {
            sql.push(format!("WHERE {}", wheres.join(" AND ")));
        }
        sql.push(format!("ORDER BY {MESSAGE_ORDER}"));
        sql.push("LIMIT ? OFFSET ?".into());
        push_paging(&mut params, filter.limit, filter.page, after.is_some());

        let mut statement = self.conn.prepare(&sql.join(" "))?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            Ok(message_from_row(row)?.map(|m| (m, message_key(row, 9))))
        })?;
        page_of(rows, filter.limit)
    }

    /// Messages with media, newest first.
    pub fn list_media(&self, filter: &MediaFilter) -> Result<Page<MediaMessage>> {
        let mut sql = format!(
            "SELECT {MESSAGE_COLUMNS}, {}, messages.filename, {MESSAGE_KEY} FROM messages JOIN chats ON messages.chat_jid = chats.jid WHERE COALESCE(messages.media_type, '') != ''",
            self.quoted_column("messages")?
        );
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(chat) = &filter.chat_jid {
            sql.push_str(" AND messages.chat_jid = ?");
            params.push(chat.clone().into());
        }
        if let Some(media_type) = &filter.media_type {
            sql.push_str(" AND messages.media_type = ?");
            params.push(media_type.clone().into());
        }
        let after = filter.cursor.as_ref().map(message_after).transpose()?;
        if let Some((condition, values)) = &after {
            sql.push_str(&format!(" AND {condition}"));
            params.extend(values.iter().cloned());
        }
        sql.push_str(&format!(" ORDER BY {MESSAGE_ORDER} LIMIT ? OFFSET ?"));
        push_paging(&mut params, filter.limit, filter.page, after.is_some());
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            let filename: Option<String> = row.get(9)?;
            Ok(message_from_row(row)?
                .map(|message| (MediaMessage { message, filename }, message_key(row, 10))))
        })?;
        page_of(rows, filter.limit)
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
        cursor: Option<&Key>,
        include_last_message: bool,
        sort_by: &str,
    ) -> Result<Page<Chat>> {
        let by_time = sort_by == "last_active";
        let sort = if by_time {
            "COALESCE(julianday(chats.last_message_time), -1)"
        } else {
            "COALESCE(display_name(chats.jid, chats.name), '')"
        };
        let last_cols = if include_last_message {
            "messages.content as last_message, messages.sender as last_sender, messages.is_from_me as last_is_from_me"
        } else {
            "NULL as last_message, NULL as last_sender, NULL as last_is_from_me"
        };
        let mut sql = vec![format!(
            "SELECT chats.jid, display_name(chats.jid, chats.name), chats.last_message_time, {last_cols}, {sort} FROM chats"
        )];
        if include_last_message {
            // Join the latest message by rowid, not by timestamp equality
            // (upstream #282), which dropped or duplicated chats.
            sql.push(
                "LEFT JOIN messages ON messages.rowid = (SELECT rowid FROM messages WHERE chat_jid = chats.jid ORDER BY julianday(timestamp) DESC LIMIT 1)".into(),
            );
        }
        let mut params: Vec<rusqlite::types::Value> = Vec::new();
        let mut wheres: Vec<String> = Vec::new();
        if let Some(query) = query {
            wheres.push(
                "(LOWER(display_name(chats.jid, chats.name)) LIKE LOWER(?) ESCAPE '\\' OR chats.jid LIKE ? ESCAPE '\\')"
                    .into(),
            );
            params.push(contains_pattern(query).into());
            params.push(contains_pattern(query).into());
        }
        if let Some(key) = cursor {
            let (condition, values) = chat_after(key, sort, "chats.jid", by_time)?;
            wheres.push(condition);
            params.extend(values);
        }
        if !wheres.is_empty() {
            sql.push(format!("WHERE {}", wheres.join(" AND ")));
        }
        let direction = if by_time { "DESC" } else { "ASC" };
        sql.push(format!("ORDER BY {sort} {direction}, chats.jid"));
        sql.push("LIMIT ? OFFSET ?".into());
        push_paging(&mut params, limit, page, cursor.is_some());
        let mut statement = self.conn.prepare(&sql.join(" "))?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            Ok(chat_from_row(row)?.map(|chat| {
                let key = chat_key(row, 0, 6, by_time);
                (chat, key)
            }))
        })?;
        page_of(rows, limit)
    }

    /// Chats with messages after `after` (UTC text from [`moment_for_sql`]),
    /// most recently active first, with how many.
    pub fn recent_activity(
        &self,
        after: &str,
        limit: i64,
        page: i64,
        cursor: Option<&Key>,
    ) -> Result<Page<ChatActivity>> {
        let mut params: Vec<rusqlite::types::Value> = vec![after.to_string().into()];
        let condition = match cursor {
            Some(key) => {
                let (condition, values) = chat_after(key, "a.latest", "c.jid", true)?;
                params.extend(values);
                format!("WHERE {condition} ")
            }
            None => String::new(),
        };
        push_paging(&mut params, limit, page, cursor.is_some());
        let mut statement = self.conn.prepare(&format!(
            "SELECT c.jid, display_name(c.jid, c.name), c.last_message_time, m.content, m.sender, m.is_from_me, a.message_count, a.from_others_count, a.latest FROM (SELECT chat_jid, COUNT(*) AS message_count, SUM(CASE WHEN is_from_me THEN 0 ELSE 1 END) AS from_others_count, MAX(julianday(timestamp)) AS latest FROM messages WHERE julianday(timestamp) > julianday(?) GROUP BY chat_jid) a JOIN chats c ON c.jid = a.chat_jid LEFT JOIN messages m ON m.rowid = (SELECT rowid FROM messages WHERE chat_jid = c.jid ORDER BY julianday(timestamp) DESC LIMIT 1) {condition}ORDER BY a.latest DESC, c.jid LIMIT ? OFFSET ?",
        ))?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            let message_count: i64 = row.get(6)?;
            let from_others_count: i64 = row.get(7)?;
            let key = chat_key(row, 0, 8, true);
            Ok(chat_from_row(row)?.map(|chat| {
                let activity = ChatActivity {
                    jid: chat.jid,
                    name: chat.name,
                    message_count,
                    from_others_count,
                    last_message_time: chat.last_message_time,
                    last_message: chat.last_message,
                    last_sender: chat.last_sender,
                    last_is_from_me: chat.last_is_from_me,
                };
                (activity, key)
            }))
        })?;
        page_of(rows, limit)
    }

    /// Contacts (not groups) whose name, JID or mapped number matches.
    pub fn search_contacts(&self, query: &str) -> Result<Vec<Contact>> {
        let pattern = contains_pattern(query);
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT jid, display_name(jid, name) AS resolved_name FROM chats WHERE (LOWER(display_name(jid, name)) LIKE LOWER(?) ESCAPE '\\' OR LOWER(jid) LIKE LOWER(?) ESCAPE '\\' OR jid_matches(jid, ?)) AND jid NOT LIKE '%@g.us' ORDER BY resolved_name, jid LIMIT 50",
        )?;
        let names = self.names();
        let rows = statement.query_map(rusqlite::params![pattern, pattern, query], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        rows.map(|row| {
            let (jid, name) = row?;
            Ok(Contact {
                phone_number: names.phone_number(&jid),
                name,
                jid,
            })
        })
        .collect()
    }

    /// The chats with this contact: their own chat, and chats they wrote in.
    pub fn contact_chats(
        &self,
        jid: &str,
        limit: i64,
        page: i64,
        cursor: Option<&Key>,
    ) -> Result<Page<Chat>> {
        const SORT: &str = "COALESCE(julianday(c.last_message_time), -1)";
        let mut params: Vec<rusqlite::types::Value> =
            vec![jid.to_string().into(), jid.to_string().into()];
        let condition = match cursor {
            Some(key) => {
                let (condition, values) = chat_after(key, SORT, "c.jid", true)?;
                params.extend(values);
                format!(" AND {condition}")
            }
            None => String::new(),
        };
        push_paging(&mut params, limit, page, cursor.is_some());
        let mut statement = self.conn.prepare(&format!(
            "SELECT c.jid, display_name(c.jid, c.name), c.last_message_time, m.content as last_message, m.sender as last_sender, m.is_from_me as last_is_from_me, {SORT} FROM chats c LEFT JOIN messages m ON m.rowid = (SELECT rowid FROM messages WHERE chat_jid = c.jid ORDER BY julianday(timestamp) DESC LIMIT 1) WHERE (c.jid = ? OR c.jid IN (SELECT chat_jid FROM messages WHERE sender = ?)){condition} ORDER BY {SORT} DESC, c.jid LIMIT ? OFFSET ?",
        ))?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            Ok(chat_from_row(row)?.map(|chat| {
                let key = chat_key(row, 0, 6, true);
                (chat, key)
            }))
        })?;
        page_of(rows, limit)
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
    /// JID itself. A database error gives the JID too.
    pub fn sender_name(&self, sender: &str) -> String {
        let stored = self.stored_sender_name(sender).unwrap_or_else(|e| {
            log::warn!("Database error while getting sender name: {e}");
            None
        });
        self.names()
            .display_name(Some(sender), stored.as_deref())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| sender.to_string())
    }

    /// The stored name of the sender's own chat. Only exact JIDs are tried:
    /// the sender as stored, then its user as a phone number and as a LID.
    /// A pattern match would also find a group JID that contains the number
    /// (`<creator>-<time>@g.us`) or a longer number (issue #66).
    fn stored_sender_name(&self, sender: &str) -> Result<Option<String>> {
        let user = bare_user(sender);
        let row = self
            .conn
            .query_row(
                "SELECT name FROM chats WHERE jid IN (?1, ?2, ?3) ORDER BY CASE jid WHEN ?1 THEN 0 WHEN ?2 THEN 1 ELSE 2 END LIMIT 1",
                [
                    sender.to_string(),
                    format!("{user}@s.whatsapp.net"),
                    format!("{user}@lid"),
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        Ok(row.flatten().filter(|name| !name.is_empty()))
    }
}

/// A `LIKE` pattern (with `ESCAPE '\'`) for text that contains `text`
/// literally: its `%`, `_` and `\` are escaped, so they are not wildcards
/// (issue #67).
fn contains_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// The names now in `shared`.
fn read(shared: &SharedNames) -> Arc<Names> {
    Arc::clone(&shared.read().unwrap_or_else(PoisonError::into_inner))
}

/// The user of a JID or a stored sender, without agent or device:
/// `31600000001` for `31600000001:5@s.whatsapp.net`.
fn bare_user(sender: &str) -> &str {
    let user = user_part(sender);
    user.split([':', '.']).next().unwrap_or(user)
}

/// A message row; the inner error is a timestamp that cannot be read.
/// The order of every message list, and the columns of its [`Key`].
const MESSAGE_ORDER: &str = "julianday(messages.timestamp) DESC, messages.rowid DESC";
const MESSAGE_KEY: &str = "julianday(messages.timestamp), messages.rowid";

/// The condition for messages after `key` in [`MESSAGE_ORDER`].
fn message_after(key: &Key) -> Result<(&'static str, Vec<rusqlite::types::Value>)> {
    let Key::Message { time, rowid } = key else {
        return Err(Error::InvalidQuery);
    };
    Ok((
        "(julianday(messages.timestamp) < ? OR (julianday(messages.timestamp) = ? AND messages.rowid < ?))",
        vec![(*time).into(), (*time).into(), (*rowid).into()],
    ))
}

/// The condition for chats after `key`, ordered by `sort` (descending for
/// a time, ascending for a name) and then by `jid`.
fn chat_after(
    key: &Key,
    sort: &str,
    jid: &str,
    by_time: bool,
) -> Result<(String, Vec<rusqlite::types::Value>)> {
    let (value, tie): (rusqlite::types::Value, &str) = match (key, by_time) {
        (Key::ChatTime { time, jid }, true) => ((*time).into(), jid),
        (Key::ChatName { name, jid }, false) => (name.clone().into(), jid),
        _ => return Err(Error::InvalidQuery),
    };
    let beyond = if by_time { "<" } else { ">" };
    Ok((
        format!("({sort} {beyond} ? OR ({sort} = ? AND {jid} > ?))"),
        vec![value.clone(), value, tie.to_string().into()],
    ))
}

/// `LIMIT ? OFFSET ?` for a page: one row more than asked, to tell whether
/// there is a next page. After a cursor the offset is 0.
fn push_paging(params: &mut Vec<rusqlite::types::Value>, limit: i64, page: i64, cursor: bool) {
    params.push(limit.saturating_add(1).into());
    let offset = if cursor {
        0
    } else {
        page.saturating_mul(limit)
    };
    params.push(offset.into());
}

/// The rows of a query from [`push_paging`] as a page of at most `limit`.
fn page_of<T>(
    rows: impl Iterator<Item = Result<Result<(T, Result<Key>)>>>,
    limit: i64,
) -> Result<Page<T>> {
    let mut items = Vec::new();
    let mut keys = Vec::new();
    for row in rows {
        let (item, key) = row??;
        items.push(item);
        keys.push(key?);
    }
    let limit = usize::try_from(limit).unwrap_or(0);
    let next = if items.len() > limit {
        items.truncate(limit);
        keys.truncate(limit);
        keys.pop()
    } else {
        None
    };
    Ok(Page { items, next })
}

/// The [`Key::Message`] in the columns at `at` and `at + 1`.
fn message_key(row: &Row<'_>, at: usize) -> Result<Key> {
    Ok(Key::Message {
        time: row.get(at)?,
        rowid: row.get(at + 1)?,
    })
}

/// The chat [`Key`] from the JID at `jid` and the sort value at `sort`.
fn chat_key(row: &Row<'_>, jid: usize, sort: usize, by_time: bool) -> Result<Key> {
    let jid: String = row.get(jid)?;
    Ok(if by_time {
        Key::ChatTime {
            time: row.get(sort)?,
            jid,
        }
    } else {
        Key::ChatName {
            name: row.get(sort)?,
            jid,
        }
    })
}

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
