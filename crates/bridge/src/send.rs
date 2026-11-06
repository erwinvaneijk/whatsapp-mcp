//! What the bridge sends and how it stores what it sent: replies and
//! mentions, media messages, voice note details, the chat a sent message is
//! stored in, and where downloads go. Ported from the Go bridge's
//! `sendWhatsAppMessage`, `outgoing.go`, `reaction.go`, `media_retry.go` and
//! `downloadMedia`. Plain functions, so they are tested without a
//! connection; `run` calls them around whatsapp-rust.

use std::path::Path;

use chrono::{DateTime, Local};
use whatsapp_mcp_store::{NewMessage, Result as StoreResult, Writer};
use whatsapp_rust::prelude::wa;
use whatsapp_rust::prelude::wa::message::{
    AudioMessage, DocumentMessage, ExtendedTextMessage, ImageMessage, VideoMessage,
};

use crate::jid::{GROUP_SERVER, Jid, LID_SERVER, USER_SERVER};
use crate::{log, receive};

/// Mentions as JIDs: a leading `@` and spaces removed, a bare number taken
/// as a phone number, empty ones left out.
pub fn mention_jids(mentions: &[String]) -> Vec<String> {
    mentions
        .iter()
        .filter_map(|mention| {
            let mention = mention.trim();
            let mention = mention.strip_prefix('@').unwrap_or(mention).trim();
            if mention.is_empty() {
                None
            } else if mention.contains('@') {
                Some(mention.to_string())
            } else {
                Some(format!("{mention}@{USER_SERVER}"))
            }
        })
        .collect()
}

/// The reply and mention details of an outgoing message; `None` when there
/// are none. A reply in a group needs the original sender as participant;
/// in a direct chat it may be left out.
pub fn context_info(
    reply_to_message_id: &str,
    reply_to_sender_jid: &str,
    mentions: &[String],
) -> Option<wa::ContextInfo> {
    let mentioned = mention_jids(mentions);
    if reply_to_message_id.is_empty() && mentioned.is_empty() {
        return None;
    }
    let mut info = wa::ContextInfo::default();
    if !reply_to_message_id.is_empty() {
        info.stanza_id = Some(reply_to_message_id.to_string());
        if !reply_to_sender_jid.is_empty() {
            info.participant = Some(reply_to_sender_jid.to_string());
        }
    }
    info.mentioned_jid = mentioned;
    Some(info)
}

/// Puts the context on the media part of the message, or turns plain text
/// into an extended text message, because plain text cannot carry one.
pub fn attach_context_info(msg: &mut wa::Message, text: &str, info: Option<wa::ContextInfo>) {
    let Some(info) = info else {
        return;
    };
    macro_rules! on_part {
        ($($field:ident),*) => {
            $(
                if let Some(part) = msg.$field.as_option_mut() {
                    part.context_info = info.into();
                    return;
                }
            )*
        };
    }
    on_part!(
        extended_text_message,
        image_message,
        video_message,
        audio_message,
        document_message
    );
    let mut extended = ExtendedTextMessage::default();
    extended.text = Some(text.to_string());
    extended.context_info = info.into();
    msg.extended_text_message = extended.into();
    msg.conversation = None;
}

/// The kind of media a file is sent as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
    /// Sent as a voice note.
    Audio,
    Document,
}

/// The kind and MIME type of a file, from its extension, as in the Go
/// bridge. Anything that is not a known image, video or Ogg file is a
/// document; its MIME type comes from the extension, because without one
/// WhatsApp shows the document as `.bin`.
pub fn media_kind(path: &Path) -> (Kind, String) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (kind, mime) = match ext.as_str() {
        "jpg" | "jpeg" => (Kind::Image, "image/jpeg"),
        "png" => (Kind::Image, "image/png"),
        "gif" => (Kind::Image, "image/gif"),
        "webp" => (Kind::Image, "image/webp"),
        "ogg" => (Kind::Audio, "audio/ogg; codecs=opus"),
        "mp4" => (Kind::Video, "video/mp4"),
        "avi" => (Kind::Video, "video/avi"),
        "mov" => (Kind::Video, "video/quicktime"),
        _ => {
            let mime = mime_guess::from_ext(&ext)
                .first_raw()
                .unwrap_or("application/octet-stream");
            return (Kind::Document, mime.to_string());
        }
    };
    (kind, mime.to_string())
}

/// What an upload returned, as the message needs it.
#[derive(Debug, Clone, Default)]
pub struct Uploaded {
    pub url: String,
    pub direct_path: String,
    pub media_key: Vec<u8>,
    pub file_sha256: Vec<u8>,
    pub file_enc_sha256: Vec<u8>,
    pub file_length: u64,
    pub media_key_timestamp: i64,
    pub streaming_sidecar: Option<Vec<u8>>,
}

/// A voice note's length and waveform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    pub seconds: u32,
    pub waveform: Vec<u8>,
}

/// The message for an uploaded file. `caption` is the message text; a
/// voice note has none (WhatsApp shows no caption on one). `file_name` is a
/// document's name.
pub fn media_message(
    kind: Kind,
    mime: &str,
    upload: &Uploaded,
    caption: &str,
    file_name: &str,
    voice: Option<Voice>,
) -> wa::Message {
    macro_rules! fill {
        ($part:expr) => {{
            let mut part = $part;
            part.mimetype = Some(mime.to_string());
            part.url = Some(upload.url.clone());
            part.direct_path = Some(upload.direct_path.clone());
            part.media_key = Some(upload.media_key.clone());
            part.file_sha256 = Some(upload.file_sha256.clone());
            part.file_enc_sha256 = Some(upload.file_enc_sha256.clone());
            part.file_length = Some(upload.file_length);
            part.media_key_timestamp = Some(upload.media_key_timestamp);
            part
        }};
    }
    let mut msg = wa::Message::default();
    match kind {
        Kind::Image => {
            let mut part = fill!(ImageMessage::default());
            part.caption = Some(caption.to_string());
            msg.image_message = part.into();
        }
        Kind::Video => {
            let mut part = fill!(VideoMessage::default());
            part.caption = Some(caption.to_string());
            part.streaming_sidecar = upload.streaming_sidecar.clone();
            msg.video_message = part.into();
        }
        Kind::Audio => {
            let mut part = fill!(AudioMessage::default());
            let voice = voice.unwrap_or(Voice {
                seconds: 30,
                waveform: Vec::new(),
            });
            part.seconds = Some(voice.seconds);
            part.ptt = Some(true);
            part.waveform = (!voice.waveform.is_empty()).then_some(voice.waveform);
            part.streaming_sidecar = upload.streaming_sidecar.clone();
            msg.audio_message = part.into();
        }
        Kind::Document => {
            let mut part = fill!(DocumentMessage::default());
            part.title = Some(file_name.to_string());
            part.file_name = Some(file_name.to_string());
            part.caption = Some(caption.to_string());
            msg.document_message = part.into();
        }
    }
    msg
}

/// The sample rate of every Opus granule position (RFC 7845, section 4).
const OPUS_RATE: u64 = 48_000;

/// A voice note's length in seconds (1 to 300) and a waveform, from an Ogg
/// Opus file: the last page's granule position less the pre-skip, at
/// 48 kHz. Like the Go bridge, the file is only checked for the Ogg
/// signature, and without a granule position the length is estimated from
/// the size. Unlike it, the pre-skip is read from its place in the Opus
/// header and the rate is always 48 kHz, as RFC 7845 says; the Go bridge
/// read both a few bytes too far on.
pub fn analyze_ogg_opus(data: &[u8]) -> Result<Voice, String> {
    if data.len() < 4 || &data[..4] != b"OggS" {
        return Err("not a valid Ogg file (missing OggS signature)".into());
    }
    let mut last_granule = 0u64;
    let mut pre_skip = 0u64;
    let mut found_head = false;
    let mut i = 0;
    while i + 27 < data.len() {
        if &data[i..i + 4] != b"OggS" {
            i += 1;
            continue;
        }
        let granule = u64::from_le_bytes(data[i + 6..i + 14].try_into().unwrap_or_default());
        let sequence = u32::from_le_bytes(data[i + 18..i + 22].try_into().unwrap_or_default());
        let segments = usize::from(data[i + 26]);
        if i + 27 + segments >= data.len() {
            break;
        }
        let page_size = 27
            + segments
            + data[i + 27..i + 27 + segments]
                .iter()
                .map(|&len| usize::from(len))
                .sum::<usize>();
        if i + page_size > data.len() {
            break;
        }
        if !found_head && sequence <= 1 {
            let page = &data[i..i + page_size];
            // OpusHead: magic (8), version (1), channels (1), pre-skip (2).
            if let Some(at) = page.windows(8).position(|w| w == b"OpusHead")
                && at + 12 <= page.len()
            {
                pre_skip = u64::from(u16::from_le_bytes([page[at + 10], page[at + 11]]));
                found_head = true;
            }
        }
        if granule != 0 {
            last_granule = granule;
        }
        i += page_size;
    }
    let seconds = if last_granule > 0 {
        last_granule.saturating_sub(pre_skip).div_ceil(OPUS_RATE)
    } else {
        // A very rough estimate, as in the Go bridge.
        (data.len() / 2000) as u64
    };
    let seconds = u32::try_from(seconds.clamp(1, 300)).unwrap_or(300);
    Ok(Voice {
        seconds,
        waveform: placeholder_waveform(seconds),
    })
}

/// A made-up waveform of 64 values from 0 to 100, the same for the same
/// length, as the Go bridge makes one: two sine waves with some noise,
/// softer at the start and end. WhatsApp needs a waveform to draw the voice
/// note; the bridge does not decode the audio.
pub fn placeholder_waveform(seconds: u32) -> Vec<u8> {
    const LENGTH: usize = 64;
    let mut seed = u64::from(seconds);
    // splitmix64: a small fixed generator, so the same length gives the
    // same waveform.
    let mut noise = move || {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    };
    let amplitude = 35.0;
    let frequency = f64::from(seconds.min(120)) / 30.0;
    (0..LENGTH)
        .map(|i| {
            let pos = i as f64 / LENGTH as f64;
            let mut value = amplitude * (pos * std::f64::consts::PI * frequency * 8.0).sin();
            value += amplitude / 2.0 * (pos * std::f64::consts::PI * frequency * 16.0).sin();
            value += (noise() - 0.5) * 15.0;
            value *= 0.7 + 0.3 * (pos * std::f64::consts::PI).sin();
            (value + 50.0).clamp(0.0, 100.0) as u8
        })
        .collect()
}

/// The chat to store a sent message in. A contact is known by phone number
/// and by LID, and received messages may be stored under either: use the
/// chat that already exists, so a conversation is not split. Without one,
/// the recipient as given.
pub fn outgoing_chat_jid(writer: &Writer, recipient: &Jid) -> StoreResult<String> {
    let recipient = recipient.to_non_ad();
    let own = recipient.to_string();
    if writer.chat_exists(&own)? {
        return Ok(own);
    }
    if let Some(other) = receive::counterpart(writer, &recipient)? {
        let other = other.to_string();
        if writer.chat_exists(&other)? {
            return Ok(other);
        }
    }
    Ok(own)
}

/// A sent message as stored: from me, sent by my own number (`own_user`),
/// with the text, media and reply taken from the message as sent, the same
/// way as for a received one.
pub fn sent_message(
    id: &str,
    chat_jid: &str,
    own_user: &str,
    msg: &wa::Message,
    now: DateTime<Local>,
) -> NewMessage {
    receive::live_message(id, chat_jid, own_user, now.to_utc(), true, msg, now)
}

/// A sender from `messages.db` as a JID. History sync stores the full JID;
/// live messages store only the user part, which may be a phone number or
/// a LID. A bare value counts as a LID when `lid_map` knows a phone number
/// for it.
pub fn stored_sender(writer: &Writer, sender: &str) -> Result<Jid, String> {
    let sender = sender.trim();
    if sender.is_empty() {
        return Err("message has no sender".into());
    }
    if sender.contains('@') {
        return Jid::parse(sender).map_err(|e| e.to_string());
    }
    let lid = Jid::new(sender, LID_SERVER);
    if matches!(receive::counterpart(writer, &lid), Ok(Some(_))) {
        return Ok(lid);
    }
    Ok(Jid::new(sender, USER_SERVER))
}

/// Who sent a stored message, for a reaction or a media request: `None`
/// for my own messages; the sender for others. An error when the message
/// is not stored in the chat.
pub fn original_sender(
    writer: &Writer,
    message_id: &str,
    chat_jid: &str,
) -> Result<Option<Jid>, SenderError> {
    let stored = match writer.message_sender(message_id, chat_jid) {
        Ok(Some(stored)) => stored,
        Ok(None) => return Err(SenderError::NotFound),
        Err(e) => return Err(SenderError::Store(e.to_string())),
    };
    match stored.is_from_me {
        Some(true) => Ok(None),
        Some(false) => stored_sender(writer, stored.sender.as_deref().unwrap_or_default())
            .map(Some)
            .map_err(SenderError::Sender),
        // A damaged row: whether to name a sender cannot be told.
        None => Err(SenderError::Store(
            "the stored message does not say whether it is from me".into(),
        )),
    }
}

/// Why the sender of a stored message is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderError {
    NotFound,
    Store(String),
    Sender(String),
}

/// Whether a chat is a group, where a message's sender must be named.
pub fn is_group(chat: &Jid) -> bool {
    chat.server == GROUP_SERVER
}

/// A sender-supplied file name as one plain path component.
pub fn safe_name(name: &str) -> String {
    let name = name.replace('\\', "/");
    let base = name
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let base = log::clean(base);
    if base.is_empty() || base == "." || base == ".." {
        return "file".into();
    }
    base
}

/// A chat JID as one safe folder name.
pub fn safe_chat_dir(jid: &str) -> String {
    let dir: String = jid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if dir.is_empty() || dir == "." || dir == ".." {
        return "_".into();
    }
    dir
}

/// The file name a download is saved under: the stored name made safe,
/// with the message ID added when another message in the chat has a name
/// that is made safe to the same one (`a/b.txt` and `c/b.txt` both become
/// `b.txt`), so the second download does not return the first one's file.
pub fn download_name(
    writer: &Writer,
    chat_jid: &str,
    filename: &str,
    message_id: &str,
) -> StoreResult<String> {
    let name = safe_name(filename);
    let shared = writer
        .other_filenames(chat_jid, message_id)?
        .iter()
        .any(|other| safe_name(other) == name);
    if !shared {
        return Ok(name);
    }
    // The ID comes from the sender too: keep only safe characters.
    let id = safe_chat_dir(message_id);
    Ok(match name.rfind('.').filter(|&dot| dot > 0) {
        Some(dot) => format!("{}_{id}{}", &name[..dot], &name[dot..]),
        None => format!("{name}_{id}"),
    })
}

/// The path WhatsApp's media servers need from a stored media URL: the path
/// and the query, byte for byte. The query carries the request's signature;
/// re-encoding or reordering it gets the download refused. A value that is
/// not a URL with a host comes back as it is.
pub fn direct_path_from_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let valid_scheme = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let path_at = rest.find(['/', '?']).unwrap_or(rest.len());
    let (host, path_and_query) = rest.split_at(path_at);
    if !valid_scheme || host.is_empty() || host.contains(' ') || !path_and_query.starts_with('/') {
        return url.to_string();
    }
    path_and_query.to_string()
}
