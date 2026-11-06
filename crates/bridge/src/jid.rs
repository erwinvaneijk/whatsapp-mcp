//! WhatsApp IDs (JIDs) as the Go bridge reads them, with whatsmeow's rules:
//! `user[.agent][:device]@server`, or a bare server.

use std::fmt;

pub const USER_SERVER: &str = "s.whatsapp.net";
pub const LID_SERVER: &str = "lid";
pub const GROUP_SERVER: &str = "g.us";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Jid {
    pub user: String,
    pub agent: u8,
    pub device: u16,
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidJid;

impl fmt::Display for InvalidJid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid JID")
    }
}

impl std::error::Error for InvalidJid {}

impl Jid {
    pub fn new(user: impl Into<String>, server: impl Into<String>) -> Jid {
        Jid {
            user: user.into(),
            agent: 0,
            device: 0,
            server: server.into(),
        }
    }

    /// whatsmeow's `ParseJID`. Without `@` the whole text is the server.
    pub fn parse(text: &str) -> Result<Jid, InvalidJid> {
        let mut parts = text.split('@');
        let first = parts.next().unwrap_or("");
        let Some(server) = parts.next() else {
            return Ok(Jid::new("", first));
        };
        let mut jid = Jid::new(first, server);
        if first.contains('.') {
            let pieces: Vec<&str> = first.split('.').collect();
            let [user, ad] = pieces[..] else {
                return Err(InvalidJid);
            };
            let ad_parts: Vec<&str> = ad.split(':').collect();
            if ad_parts.len() > 2 {
                return Err(InvalidJid);
            }
            jid.user = user.to_string();
            jid.agent = ad_parts[0].parse().map_err(|_| InvalidJid)?;
            if let Some(device) = ad_parts.get(1) {
                jid.device = device.parse().map_err(|_| InvalidJid)?;
            }
        } else if first.contains(':') {
            let pieces: Vec<&str> = first.split(':').collect();
            let [user, device] = pieces[..] else {
                return Err(InvalidJid);
            };
            jid.user = user.to_string();
            jid.device = device.parse().map_err(|_| InvalidJid)?;
        }
        Ok(jid)
    }

    /// The Go bridge's `parseRecipient`: a JID, or else a phone number.
    pub fn parse_recipient(recipient: &str) -> Result<Jid, InvalidJid> {
        let recipient = recipient.trim();
        if recipient.is_empty() {
            return Err(InvalidJid);
        }
        if recipient.contains('@') {
            Jid::parse(recipient)
        } else {
            Ok(Jid::new(recipient, USER_SERVER))
        }
    }

    /// The same JID without agent and device.
    pub fn to_non_ad(&self) -> Jid {
        Jid::new(self.user.clone(), self.server.clone())
    }
}

impl fmt::Display for Jid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.agent > 0 {
            write!(
                f,
                "{}.{}:{}@{}",
                self.user, self.agent, self.device, self.server
            )
        } else if self.device > 0 {
            write!(f, "{}:{}@{}", self.user, self.device, self.server)
        } else if !self.user.is_empty() {
            write!(f, "{}@{}", self.user, self.server)
        } else {
            f.write_str(&self.server)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_whatsmeow() {
        let j = Jid::parse("31612345678:3@s.whatsapp.net").unwrap();
        assert_eq!((j.user.as_str(), j.device), ("31612345678", 3));
        assert_eq!(j.to_non_ad().to_string(), "31612345678@s.whatsapp.net");
        let j = Jid::parse("1.2:3@lid").unwrap();
        assert_eq!(
            (j.agent, j.device, j.to_string().as_str()),
            (2, 3, "1.2:3@lid")
        );
        assert_eq!(Jid::parse("g.us").unwrap().to_string(), "g.us");
        assert!(Jid::parse("1:x@s.whatsapp.net").is_err());
        assert!(Jid::parse("a.b.c@s.whatsapp.net").is_err());
    }

    #[test]
    fn recipients() {
        assert_eq!(
            Jid::parse_recipient(" 31612345678 ").unwrap().to_string(),
            "31612345678@s.whatsapp.net"
        );
        assert_eq!(
            Jid::parse_recipient("120363000000000000@g.us")
                .unwrap()
                .to_string(),
            "120363000000000000@g.us"
        );
        assert!(Jid::parse_recipient("   ").is_err());
    }
}
