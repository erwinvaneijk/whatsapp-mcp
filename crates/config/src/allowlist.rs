//! `send_allowlist` / `WHATSAPP_SEND_ALLOWLIST`: the chats the bridge may
//! send messages and reactions to. Same rules as `whatsapp-bridge/allowlist.go`.

use std::collections::BTreeSet;
use std::fmt;

use crate::ConfigError;

/// The chats the bridge may send to, as JIDs without a device part
/// (`31612345678@s.whatsapp.net`, `123456789012345@lid`, `…@g.us`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendAllowlist {
    jids: BTreeSet<String>,
}

impl SendAllowlist {
    /// Reads the setting: phone numbers with country code (a leading `+`,
    /// spaces, dashes, dots and brackets are allowed) or JIDs on a user, LID
    /// or group server. Entries are separated by commas, semicolons, tabs or
    /// line breaks.
    ///
    /// Returns `None` (no limit) when there are no entries, and an error for
    /// any entry it cannot read, so a typo does not silently allow or block
    /// the wrong chats.
    pub fn parse(value: &str) -> Result<Option<Self>, ConfigError> {
        let mut jids = BTreeSet::new();
        for entry in value
            .split([',', ';', '\n', '\r', '\t'])
            .map(str::trim)
            .filter(|e| !e.is_empty())
        {
            let jid = parse_entry(entry).map_err(|reason| ConfigError::Allowlist {
                entry: entry.to_string(),
                reason: reason.to_string(),
            })?;
            jids.insert(jid);
        }
        Ok((!jids.is_empty()).then_some(Self { jids }))
    }

    /// The JIDs, sorted.
    pub fn jids(&self) -> impl Iterator<Item = &str> {
        self.jids.iter().map(String::as_str)
    }

    /// Whether this exact JID (without device part) is listed. Matching a
    /// phone number to its LID and back is up to the bridge.
    pub fn contains(&self, jid: &str) -> bool {
        self.jids.contains(jid)
    }

    pub fn len(&self) -> usize {
        self.jids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jids.is_empty()
    }
}

impl fmt::Display for SendAllowlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list: Vec<&str> = self.jids().collect();
        f.write_str(&list.join(", "))
    }
}

const USER_SERVER: &str = "s.whatsapp.net";
const LID_SERVER: &str = "lid";
const GROUP_SERVER: &str = "g.us";

fn parse_entry(entry: &str) -> Result<String, &'static str> {
    if entry.contains('@') {
        return parse_jid(entry);
    }
    let digits: String = entry
        .chars()
        .filter(|c| !matches!(c, '+' | ' ' | '-' | '.' | '(' | ')'))
        .collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("not a phone number or JID");
    }
    Ok(format!("{digits}@{USER_SERVER}"))
}

/// A JID as whatsmeow's `ParseJID` reads it (`user[.agent][:device]@server`),
/// returned without agent and device.
fn parse_jid(entry: &str) -> Result<String, &'static str> {
    let mut parts = entry.split('@');
    let (Some(user), Some(server), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("not a phone number or JID");
    };
    let user = if let Some((user, rest)) = user.split_once('.') {
        // user.agent or user.agent:device
        let (agent, device) = match rest.split_once(':') {
            Some((agent, device)) => (agent, Some(device)),
            None => (rest, None),
        };
        if !is_number(agent) || device.is_some_and(|d| !is_number(d)) {
            return Err("invalid JID");
        }
        user
    } else if let Some((user, device)) = user.split_once(':') {
        if !is_number(device) {
            return Err("invalid JID");
        }
        user
    } else {
        user
    };
    if !matches!(server, USER_SERVER | LID_SERVER | GROUP_SERVER) {
        return Err("unsupported server; use a phone number, …@s.whatsapp.net, …@lid or …@g.us");
    }
    if user.is_empty() {
        return Err("missing user part");
    }
    Ok(format!("{user}@{server}"))
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}
