//! Writing `messages.db`, as the bridge does: the schema and migrations the
//! Go bridge applies on start, and storing chats, messages and media details
//! with the same SQL and the same values. Times are stored as Go's database
//! driver writes them, so the Go bridge, the Rust bridge and the MCP server
//! can share one database.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Timelike};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

const TABLES_SQL: &str = include_str!("schema/tables.sql");
const FTS_SQL: &str = include_str!("schema/fts.sql");
const NAMES_SQL: &str = include_str!("schema/names.sql");

/// A message to store. Text fields are empty, not absent, when there is
/// nothing, as in the Go bridge.
#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub id: String,
    pub chat_jid: String,
    pub sender: String,
    pub content: String,
    pub timestamp: Option<DateTime<FixedOffset>>,
    pub is_from_me: bool,
    pub media_type: String,
    pub filename: String,
    pub url: String,
    pub media_key: Vec<u8>,
    pub file_sha256: Vec<u8>,
    pub file_enc_sha256: Vec<u8>,
    pub file_length: u64,
    /// The message this one replies to; empty when it is not a reply.
    pub quoted_message_id: String,
}

/// What is needed to download a message's media again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaInfo {
    pub media_type: String,
    pub filename: String,
    pub url: String,
    pub media_key: Vec<u8>,
    pub file_sha256: Vec<u8>,
    pub file_enc_sha256: Vec<u8>,
    pub file_length: u64,
}

/// A contact's names, as whatsmeow keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactNames {
    pub first_name: String,
    pub full_name: String,
    pub push_name: String,
    pub business_name: String,
}

/// `messages.db` opened for writing.
pub struct Writer {
    conn: Connection,
}

impl Writer {
    /// Opens (or creates) `messages.db` in the store folder and brings its
    /// schema up to date. The folder and the file are made readable by the
    /// owner only.
    pub fn open(store_dir: &Path) -> Result<Writer> {
        ensure_private_dir(store_dir).map_err(io_error)?;
        let path = store_dir.join("messages.db");
        ensure_private_file(&path).map_err(io_error)?;
        let conn = Connection::open(&path)?;
        Writer::from_connection(conn)
    }

    /// Like [`Writer::open`], for a connection opened elsewhere (tests).
    pub fn from_connection(conn: Connection) -> Result<Writer> {
        // As the Go bridge: foreign keys on, WAL, normal sync, 5 s busy wait.
        conn.pragma_update(None, "foreign_keys", "on")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Writer { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Stores a chat, or updates it: a new non-empty name replaces the old
    /// one, the time always.
    pub fn store_chat(
        &self,
        jid: &str,
        name: &str,
        last_message_time: DateTime<FixedOffset>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chats (jid, name, last_message_time) VALUES (?, ?, ?)
		 ON CONFLICT(jid) DO UPDATE SET
		   name = COALESCE(NULLIF(excluded.name, ''), chats.name),
		   last_message_time = excluded.last_message_time",
            params![jid, name, go_time(&last_message_time)],
        )?;
        Ok(())
    }

    /// Stores a message, replacing one with the same ID in the same chat. A
    /// message without text and without media is not stored.
    pub fn store_message(&self, m: &NewMessage) -> Result<()> {
        if m.content.is_empty() && m.media_type.is_empty() {
            return Ok(());
        }
        let quoted = (!m.quoted_message_id.is_empty()).then_some(m.quoted_message_id.as_str());
        self.conn.execute(
            "INSERT OR REPLACE INTO messages
		(id, chat_jid, sender, content, timestamp, is_from_me, media_type, filename, url, media_key, file_sha256, file_enc_sha256, file_length, quoted_message_id)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                m.id,
                m.chat_jid,
                m.sender,
                m.content,
                m.timestamp.as_ref().map(go_time),
                m.is_from_me,
                m.media_type,
                m.filename,
                m.url,
                m.media_key,
                m.file_sha256,
                m.file_enc_sha256,
                m.file_length as i64,
                quoted,
            ],
        )?;
        Ok(())
    }

    /// Updates a stored message's media details.
    pub fn store_media_info(&self, id: &str, chat_jid: &str, info: &MediaInfo) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET url = ?, media_key = ?, file_sha256 = ?, file_enc_sha256 = ?, file_length = ? WHERE id = ? AND chat_jid = ?",
            params![
                info.url,
                info.media_key,
                info.file_sha256,
                info.file_enc_sha256,
                info.file_length as i64,
                id,
                chat_jid,
            ],
        )?;
        Ok(())
    }

    /// A message's media details, or `None` when the message is not stored.
    pub fn media_info(&self, id: &str, chat_jid: &str) -> Result<Option<MediaInfo>> {
        self.conn
            .query_row(
                "SELECT media_type, filename, url, media_key, file_sha256, file_enc_sha256, file_length FROM messages WHERE id = ? AND chat_jid = ?",
                params![id, chat_jid],
                |row| {
                    Ok(MediaInfo {
                        media_type: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        filename: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        url: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        media_key: row.get::<_, Option<Vec<u8>>>(3)?.unwrap_or_default(),
                        file_sha256: row.get::<_, Option<Vec<u8>>>(4)?.unwrap_or_default(),
                        file_enc_sha256: row.get::<_, Option<Vec<u8>>>(5)?.unwrap_or_default(),
                        file_length: row.get::<_, Option<i64>>(6)?.unwrap_or_default() as u64,
                    })
                },
            )
            .optional()
    }

    /// Whether a chat with this JID is stored.
    pub fn chat_exists(&self, jid: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM chats WHERE jid = ?", [jid], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Stores a contact's names; empty names do not replace known ones.
    pub fn store_contact(&self, jid: &str, names: &ContactNames) -> Result<()> {
        self.conn.execute(
            "INSERT INTO contacts (jid, first_name, full_name, push_name, business_name) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(jid) DO UPDATE SET
               first_name = COALESCE(NULLIF(excluded.first_name, ''), contacts.first_name),
               full_name = COALESCE(NULLIF(excluded.full_name, ''), contacts.full_name),
               push_name = COALESCE(NULLIF(excluded.push_name, ''), contacts.push_name),
               business_name = COALESCE(NULLIF(excluded.business_name, ''), contacts.business_name)",
            params![
                jid,
                names.first_name,
                names.full_name,
                names.push_name,
                names.business_name
            ],
        )?;
        Ok(())
    }

    /// Stores that a LID belongs to a phone number. A number maps to one
    /// LID: a newer mapping for the same number replaces the older one.
    pub fn store_lid_mapping(&self, lid: &str, pn: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO lid_map (lid, pn) VALUES (?, ?)",
            params![lid, pn],
        )?;
        Ok(())
    }
}

/// Creates the tables, indexes and full-text index, and brings an older
/// database up to date. Safe to run on every start.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(TABLES_SQL)?;
    // Older databases predate quoted_message_id (upstream #325).
    let has_quoted = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'quoted_message_id'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_quoted {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN quoted_message_id TEXT")?;
    }
    // SQLite is bundled with FTS5, so the index is always created. Rebuilding
    // fills it with messages stored while a Go bridge without FTS5 ran.
    conn.execute_batch(FTS_SQL)?;
    conn.execute_batch("INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')")?;
    conn.execute_batch(NAMES_SQL)?;
    Ok(())
}

/// A time as Go's SQLite driver writes it (`2006-01-02 15:04:05.999999999-07:00`):
/// a fraction only when there is one, without trailing zeros.
pub fn go_time(moment: &DateTime<FixedOffset>) -> String {
    let mut text = moment.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = moment.nanosecond() % 1_000_000_000;
    if nanos != 0 {
        let fraction = format!("{nanos:09}");
        text.push('.');
        text.push_str(fraction.trim_end_matches('0'));
    }
    text.push_str(&moment.format("%:z").to_string());
    text
}

fn io_error(e: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

/// Creates the folder if needed and makes it readable by the owner only.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    set_mode(dir, 0o700)
}

/// Creates the file if needed and makes it readable by the owner only.
pub fn ensure_private_file(path: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    set_mode(path, 0o600)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}
