//! Contact names and the LID-to-phone map, read from the bridge's whatsmeow
//! store (`whatsapp.db`). Same rules as `_load_contacts` and
//! `_build_resolvers` in the Python server.
//!
//! `messages.db` only has names for groups and a few contacts, so without
//! these a chat under a LID shows up as a bare number.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// Where the names are: table and column names.
struct Tables {
    contacts: &'static str,
    jid_column: &'static str,
    lid_map: &'static str,
}

/// The Rust bridge's tables in `messages.db`.
const OWN_TABLES: Tables = Tables {
    contacts: "contacts",
    jid_column: "jid",
    lid_map: "lid_map",
};

/// whatsmeow's tables in `whatsapp.db` (the Go bridge).
const WHATSMEOW_TABLES: Tables = Tables {
    contacts: "whatsmeow_contacts",
    jid_column: "their_jid",
    lid_map: "whatsmeow_lid_map",
};

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Where contact names come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamesSource {
    /// The Rust bridge's tables `contacts` and `lid_map` in `messages.db`.
    MessagesDb,
    /// whatsmeow's tables in the Go bridge's `whatsapp.db`.
    WhatsappDb,
    /// Neither: chats and senders show as numbers.
    None,
}

/// Where [`Names::load`] reads the names from, by the same rule.
pub fn names_source(messages_db: &Path, whatsapp_db: &Path) -> NamesSource {
    if Names::own_tables(messages_db).is_some() {
        NamesSource::MessagesDb
    } else if open_read_only(whatsapp_db).is_ok_and(|conn| {
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            [WHATSMEOW_TABLES.contacts],
            |row| row.get::<_, i64>(0),
        )
        .is_ok_and(|n| n == 1)
    }) {
        NamesSource::WhatsappDb
    } else {
        NamesSource::None
    }
}

/// Names and number mappings, keyed by the user part of a JID (before `@`).
#[derive(Debug, Default, Clone)]
pub struct Names {
    /// Saved contact names: full name, else first name.
    saved: HashMap<String, String>,
    /// Push or business names, for contacts without a saved name.
    fallback: HashMap<String, String>,
    lid_to_pn: HashMap<String, String>,
    pn_to_lid: HashMap<String, String>,
}

/// The user part of a JID: everything before the first `@`.
pub fn user_part(jid: &str) -> &str {
    jid.split('@').next().unwrap_or(jid)
}

impl Names {
    /// Reads the names. With the Rust bridge they are in `messages.db`
    /// (tables `contacts` and `lid_map`); with the Go bridge they are only in
    /// whatsmeow's `whatsapp.db`. Any problem (no file, no tables) gives no
    /// names, with a note on stderr, as in the Python server.
    pub fn load(messages_db: &Path, whatsapp_db: &Path) -> Names {
        let result = match Names::own_tables(messages_db) {
            Some(conn) => Names::from_tables(&conn, &OWN_TABLES),
            None => open_read_only(whatsapp_db)
                .and_then(|conn| Names::from_tables(&conn, &WHATSMEOW_TABLES)),
        };
        result.unwrap_or_else(|e| {
            eprintln!("Database error while loading contact names: {e}");
            Names::default()
        })
    }

    /// `messages.db`, when it has both of the Rust bridge's names tables.
    /// With only one (a migration cut short), whatsapp.db is used instead.
    fn own_tables(messages_db: &Path) -> Option<Connection> {
        let conn = open_read_only(messages_db).ok()?;
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('contacts', 'lid_map')",
                [],
                |row| row.get(0),
            )
            .ok()?;
        (tables == 2).then_some(conn)
    }

    fn from_tables(conn: &Connection, tables: &Tables) -> rusqlite::Result<Names> {
        let mut names = Names::default();
        let mut contacts = conn.prepare(&format!(
            "SELECT {}, first_name, full_name, push_name, business_name FROM {}",
            tables.jid_column, tables.contacts
        ))?;
        let rows = contacts.query_map([], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (jid, first, full, push, business) = row?;
            let user = user_part(jid.as_deref().unwrap_or("")).to_string();
            // The first non-empty value wins, like Python's `or`; the first
            // row for a user wins, like `setdefault`.
            if let Some(name) = non_empty(&full).or(non_empty(&first)) {
                names.saved.entry(user).or_insert_with(|| name.to_string());
            } else if let Some(name) = non_empty(&push).or(non_empty(&business)) {
                names
                    .fallback
                    .entry(user)
                    .or_insert_with(|| name.to_string());
            }
        }
        let mut map = conn.prepare(&format!("SELECT lid, pn FROM {}", tables.lid_map))?;
        let rows = map.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (lid, pn) = row?;
            names.lid_to_pn.insert(lid, pn);
        }
        names.pn_to_lid = names
            .lid_to_pn
            .iter()
            .map(|(lid, pn)| (pn.clone(), lid.clone()))
            .collect();
        Ok(names)
    }

    fn lookup<'a>(&'a self, names: &'a HashMap<String, String>, user: &str) -> Option<&'a str> {
        [
            Some(user),
            self.lid_to_pn.get(user).map(String::as_str).or(Some("")),
            self.pn_to_lid.get(user).map(String::as_str).or(Some("")),
        ]
        .into_iter()
        .flatten()
        .find_map(|key| non_empty_str(names.get(key).map(String::as_str)))
    }

    /// The name to show for a chat or sender: the saved contact name, else
    /// the stored chat name (unless it is just the number), else the push
    /// name, else the stored name as it is. Groups keep their stored name.
    pub fn display_name(&self, jid: Option<&str>, stored: Option<&str>) -> Option<String> {
        let Some(jid) = jid.filter(|j| !j.is_empty() && !j.ends_with("@g.us")) else {
            return stored.map(str::to_string);
        };
        let user = user_part(jid);
        let useful_stored = if stored == Some(user) { None } else { stored };
        self.lookup(&self.saved, user)
            .or(non_empty_str(useful_stored))
            .or(self.lookup(&self.fallback, user))
            .or(stored)
            .map(str::to_string)
    }

    /// Whether a chat JID contains this phone number: in the JID itself, or
    /// in the number or LID it maps to.
    pub fn jid_matches(&self, jid: &str, phone: &str) -> bool {
        let user = user_part(jid);
        [
            Some(jid),
            self.lid_to_pn.get(user).map(String::as_str),
            self.pn_to_lid.get(user).map(String::as_str),
        ]
        .into_iter()
        .flatten()
        .filter(|ident| !ident.is_empty())
        .any(|ident| ident.contains(phone))
    }

    /// The phone number for a chat JID: mapped from the LID when there is
    /// one, else the JID's own number.
    pub fn phone_number(&self, jid: &str) -> String {
        let user = user_part(jid);
        self.lid_to_pn
            .get(user)
            .cloned()
            .unwrap_or_else(|| user.to_string())
    }
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|v| !v.is_empty())
}

fn non_empty_str(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        let mut n = Names::default();
        n.saved.insert("31600000001".into(), "Maria Jansen".into());
        n.fallback.insert("31600000002".into(), "Tom B.".into());
        n.lid_to_pn
            .insert("200000000000002".into(), "31600000002".into());
        n.pn_to_lid
            .insert("31600000002".into(), "200000000000002".into());
        n
    }

    #[test]
    fn display_name_rules() {
        let n = names();
        let d = |jid: &str, stored: Option<&str>| n.display_name(Some(jid), stored);
        // Saved name wins over the stored chat name.
        assert_eq!(
            d("31600000001@s.whatsapp.net", Some("Mia")).as_deref(),
            Some("Maria Jansen")
        );
        // A stored name that is just the number is skipped for the push name,
        // found through the LID map.
        assert_eq!(
            d("200000000000002@lid", Some("200000000000002")).as_deref(),
            Some("Tom B.")
        );
        // A useful stored name wins over the push name.
        assert_eq!(
            d("31600000002@s.whatsapp.net", Some("Tommy")).as_deref(),
            Some("Tommy")
        );
        // Groups keep their stored name.
        assert_eq!(d("1@g.us", Some("Club")).as_deref(), Some("Club"));
        // Unknown: the stored name as it is, even if empty.
        assert_eq!(d("3169@s.whatsapp.net", Some("")).as_deref(), Some(""));
        assert_eq!(d("3169@s.whatsapp.net", None), None);
    }

    #[test]
    fn jid_matches_and_phone_number() {
        let n = names();
        assert!(n.jid_matches("200000000000002@lid", "3160000000"));
        assert!(n.jid_matches("31600000002@s.whatsapp.net", "2000000"));
        assert!(!n.jid_matches("31600000001@s.whatsapp.net", "999"));
        assert_eq!(n.phone_number("200000000000002@lid"), "31600000002");
        assert_eq!(n.phone_number("31600000001@s.whatsapp.net"), "31600000001");
    }
}
