//! Incoming WhatsApp messages to what `messages.db` stores: the text (with
//! captions, and a short bracketed form for non-text messages), the quoted
//! message ID and the media details. Ported from the Go bridge
//! (`unwrapMessage`, `extractTextContent`, `extractQuotedMessageID`,
//! `extractMediaInfo`); the stored text must stay the same, because the MCP
//! tools and anything built on them read it.

use whatsapp_rust::prelude::wa;
use whatsapp_rust::prelude::wa::message::protocol_message::Type as ProtocolType;

/// View-once, ephemeral and document-with-caption messages are envelopes:
/// the real message sits one level down. History sync payloads are not
/// unwrapped by the library, so without this a captioned image in a
/// disappearing-messages chat would be stored as empty. The bound only stops
/// a malformed payload from looping.
pub fn unwrap(msg: &wa::Message) -> &wa::Message {
    let mut msg = msg;
    for _ in 0..4 {
        let inner = [
            &msg.ephemeral_message,
            &msg.view_once_message,
            &msg.view_once_message_v2,
            &msg.view_once_message_v2_extension,
            &msg.document_with_caption_message,
        ]
        .into_iter()
        .find_map(|envelope| envelope.as_option().and_then(|e| e.message.as_option()));
        match inner {
            Some(inner) => msg = inner,
            None => return msg,
        }
    }
    msg
}

fn text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

/// The text to store for a message: its text or caption, or a bracketed
/// form for reactions, polls, locations, contacts, edits, deletes and
/// stickers. Empty when there is nothing to store (or only media).
pub fn text_content(msg: &wa::Message) -> String {
    let msg = unwrap(msg);

    // Plain text first: the common case.
    if let Some(t) = msg.conversation.as_deref().filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if let Some(t) = msg
        .extended_text_message
        .as_option()
        .and_then(|e| e.text.as_deref())
        .filter(|t| !t.is_empty())
    {
        return t.to_string();
    }

    // A caption is text: someone sends a screenshot and types what to do
    // with it. Audio has no caption.
    if let Some(m) = msg.image_message.as_option() {
        return text(&m.caption).to_string();
    }
    if let Some(m) = msg.video_message.as_option() {
        return text(&m.caption).to_string();
    }
    if let Some(m) = msg.document_message.as_option() {
        return text(&m.caption).to_string();
    }

    // Non-text messages get a short, stable, bracketed text, so they are
    // stored instead of dropped.
    if let Some(r) = msg.reaction_message.as_option() {
        return match r.text.as_deref().filter(|t| !t.is_empty()) {
            Some(t) => format!("[reaction: {t}]"),
            None => "[reaction removed]".to_string(),
        };
    }

    // Polls moved through versions 2 to 6; version 4 wraps the poll.
    let poll = msg
        .poll_creation_message
        .as_option()
        .or(msg.poll_creation_message_v2.as_option())
        .or(msg.poll_creation_message_v3.as_option())
        .or_else(|| {
            msg.poll_creation_message_v4
                .as_option()
                .and_then(|f| f.message.as_option())
                .and_then(|m| m.poll_creation_message.as_option())
        })
        .or(msg.poll_creation_message_v5.as_option())
        .or(msg.poll_creation_message_v6.as_option());
    if let Some(p) = poll {
        let options: Vec<&str> = p
            .options
            .iter()
            .filter_map(|o| o.option_name.as_deref())
            .filter(|n| !n.is_empty())
            .collect();
        return if options.is_empty() {
            format!("[poll: {}]", text(&p.name))
        } else {
            format!(
                "[poll: {} — options: {}]",
                text(&p.name),
                options.join(" | ")
            )
        };
    }

    if msg.poll_update_message.is_set() {
        // The vote itself is encrypted; record that a vote happened.
        return "[poll vote]".to_string();
    }

    if let Some(l) = msg.location_message.as_option() {
        let (lat, lon) = (
            l.degrees_latitude.unwrap_or_default(),
            l.degrees_longitude.unwrap_or_default(),
        );
        return match l.name.as_deref().filter(|n| !n.is_empty()) {
            Some(name) => format!("[location: {name} ({lat:.6}, {lon:.6})]"),
            None => format!("[location: {lat:.6}, {lon:.6}]"),
        };
    }

    if let Some(l) = msg.live_location_message.as_option() {
        let (lat, lon) = (
            l.degrees_latitude.unwrap_or_default(),
            l.degrees_longitude.unwrap_or_default(),
        );
        return match l.caption.as_deref().filter(|c| !c.is_empty()) {
            Some(caption) => format!("[live location: {caption} ({lat:.6}, {lon:.6})]"),
            None => format!("[live location: {lat:.6}, {lon:.6}]"),
        };
    }

    if let Some(c) = msg.contact_message.as_option() {
        return format!("[contact: {}]", text(&c.display_name));
    }

    if let Some(ca) = msg.contacts_array_message.as_option() {
        let names: Vec<&str> = ca
            .contacts
            .iter()
            .filter_map(|c| c.display_name.as_deref())
            .filter(|n| !n.is_empty())
            .collect();
        return format!("[contacts: {}]", names.join(", "));
    }

    // An edit arrives as a top-level EditedMessage or inside a
    // ProtocolMessage, depending on the sender's client.
    if let Some(e) = msg.edited_message.as_option() {
        return edited(e.message.as_option());
    }
    if let Some(p) = msg.protocol_message.as_option() {
        match p.r#type {
            Some(ProtocolType::MESSAGE_EDIT) => return edited(p.edited_message.as_option()),
            Some(ProtocolType::REVOKE) => return "[message deleted]".to_string(),
            _ => {}
        }
    }

    // Stickers are media, but get text too so they are never dropped.
    if msg.sticker_message.is_set() {
        return "[sticker]".to_string();
    }

    String::new()
}

fn edited(inner: Option<&wa::Message>) -> String {
    match inner.map(text_content).filter(|t| !t.is_empty()) {
        Some(t) => format!("[edited] {t}"),
        None => "[edited message]".to_string(),
    }
}

/// The ID of the message this one replies to, from the context of whichever
/// part of the message carries one; empty when it is not a reply.
pub fn quoted_message_id(msg: &wa::Message) -> String {
    let msg = unwrap(msg);
    macro_rules! first_quote {
        ($($field:ident),* $(,)?) => {
            $(
                if let Some(id) = msg
                    .$field
                    .as_option()
                    .and_then(|part| part.context_info.as_option())
                    .and_then(|ctx| ctx.stanza_id.as_deref())
                    .filter(|id| !id.is_empty())
                {
                    return id.to_string();
                }
            )*
        };
    }
    // Every part of a Message whose type has a contextInfo, in field order
    // (the Go bridge finds them by reflection).
    first_quote!(
        image_message,
        contact_message,
        location_message,
        extended_text_message,
        document_message,
        audio_message,
        video_message,
        call,
        contacts_array_message,
        live_location_message,
        template_message,
        sticker_message,
        group_invite_message,
        template_button_reply_message,
        product_message,
        list_message,
        order_message,
        list_response_message,
        buttons_message,
        buttons_response_message,
        interactive_message,
        interactive_response_message,
        poll_creation_message,
        request_phone_number_message,
        poll_creation_message_v2,
        poll_creation_message_v3,
        ptv_message,
        message_history_bundle,
        event_message,
        newsletter_admin_invite_message,
        album_message,
        sticker_pack_message,
        poll_result_snapshot_message,
        message_history_notice,
        poll_creation_message_v5,
        newsletter_follower_invite_message_v2,
        poll_result_snapshot_message_v3,
        poll_creation_message_v6,
        event_invite_message,
        split_payment_message,
        music_message,
        instant_image_message,
        request_location_message,
    );
    String::new()
}

/// A message's media, as the bridge stores it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Media {
    /// `image`, `video`, `audio` or `document`; empty for no media.
    pub media_type: String,
    pub filename: String,
    pub url: String,
    pub media_key: Vec<u8>,
    pub file_sha256: Vec<u8>,
    pub file_enc_sha256: Vec<u8>,
    pub file_length: u64,
}

/// The media in a message. File names other than a document's are made up
/// from `now`, as the Go bridge does (`image_20261104_220052.jpg`).
pub fn media(msg: &wa::Message, now: chrono::DateTime<chrono::Local>) -> Media {
    let msg = unwrap(msg);
    let stamp = now.format("%Y%m%d_%H%M%S");
    macro_rules! media_of {
        ($m:expr, $kind:expr, $name:expr) => {
            Media {
                media_type: $kind.to_string(),
                filename: $name,
                url: $m.url.clone().unwrap_or_default(),
                media_key: $m.media_key.clone().unwrap_or_default(),
                file_sha256: $m.file_sha256.clone().unwrap_or_default(),
                file_enc_sha256: $m.file_enc_sha256.clone().unwrap_or_default(),
                file_length: $m.file_length.unwrap_or_default(),
            }
        };
    }
    if let Some(m) = msg.image_message.as_option() {
        return media_of!(m, "image", format!("image_{stamp}.jpg"));
    }
    if let Some(m) = msg.video_message.as_option() {
        return media_of!(m, "video", format!("video_{stamp}.mp4"));
    }
    if let Some(m) = msg.audio_message.as_option() {
        return media_of!(m, "audio", format!("audio_{stamp}.ogg"));
    }
    if let Some(m) = msg.document_message.as_option() {
        let name = m
            .file_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("document_{stamp}"));
        return media_of!(m, "document", name);
    }
    Media::default()
}
