//! Phase 1 go/no-go test of whatsapp-rust: one program that links a test
//! account, logs what arrives, and sends, reacts and downloads on command.
//! See docs/rust-1.0/phase1.md for the checklist and how to run it.
//!
//! Message text, captions, file names and names are only printed with
//! --show-content, like the bridge's log_content setting.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;

/// What the program remembers about a message, for reply, react, download and retry.
#[derive(Clone)]
struct Seen {
    chat: Jid,
    sender: Option<Jid>,
    from_me: bool,
    message: Arc<wa::Message>,
    origin: &'static str,
}

#[derive(Default)]
struct Stats {
    live_messages: AtomicU64,
    history_chunks: AtomicU64,
    history_conversations: AtomicU64,
    history_messages: AtomicU64,
    history_pushnames: AtomicU64,
    contact_updates: AtomicU64,
    push_names_seen: AtomicU64,
    lid_senders: AtomicU64,
}

struct State {
    show_content: bool,
    store: PathBuf,
    seen: Mutex<HashMap<String, Seen>>,
    order: Mutex<Vec<String>>,
    stats: Stats,
}

impl State {
    /// Remember a message this program sent, so `reply`, `react`, `download`
    /// and `retry` work on it too.
    fn remember_sent(&self, id: &str, to: &Jid, message: wa::Message) {
        self.remember(
            id,
            Seen {
                chat: to.clone(),
                sender: None,
                from_me: true,
                message: Arc::new(message),
                origin: "sent",
            },
        );
    }

    fn remember(&self, id: &str, seen: Seen) {
        let mut map = self.seen.lock().unwrap();
        if map.insert(id.to_string(), seen).is_none() {
            self.order.lock().unwrap().push(id.to_string());
        }
    }

    fn get(&self, id: &str) -> Result<Seen> {
        if id.is_empty() {
            bail!("give a message id; run `recent` to see known ids");
        }
        self.seen
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown message id {id}; run `recent` to see known ids"))
    }

    fn content(&self, text: &str) -> String {
        if self.show_content {
            format!("{text:?}")
        } else {
            format!("<{} chars>", text.chars().count())
        }
    }
}

fn now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

macro_rules! out {
    ($($arg:tt)*) => { println!("{} {}", now(), format!($($arg)*)) };
}

/// A short description of a message: its kind and the details the bridge stores.
fn describe(state: &State, msg: &wa::Message) -> String {
    let base = msg.get_base_message();
    let mut parts = Vec::new();
    let context = |ctx: &MessageField<wa::ContextInfo>, parts: &mut Vec<String>| {
        if let Some(ctx) = ctx.as_option() {
            if let Some(id) = &ctx.stanza_id {
                parts.push(format!("quotes={id}"));
            }
            if !ctx.mentioned_jid.is_empty() {
                parts.push(format!("mentions={}", ctx.mentioned_jid.len()));
            }
        }
    };
    if let Some(text) = &base.conversation {
        parts.push(format!("text {}", state.content(text)));
    } else if let Some(m) = base.extended_text_message.as_option() {
        parts.push(format!(
            "text {}",
            state.content(m.text.as_deref().unwrap_or(""))
        ));
        context(&m.context_info, &mut parts);
    } else if let Some(m) = base.image_message.as_option() {
        parts.push(format!(
            "image caption={}",
            state.content(m.caption.as_deref().unwrap_or(""))
        ));
        context(&m.context_info, &mut parts);
    } else if let Some(m) = base.video_message.as_option() {
        parts.push(format!(
            "video caption={}",
            state.content(m.caption.as_deref().unwrap_or(""))
        ));
        context(&m.context_info, &mut parts);
    } else if let Some(m) = base.audio_message.as_option() {
        parts.push(format!("audio ptt={}", m.ptt.unwrap_or(false)));
        context(&m.context_info, &mut parts);
    } else if let Some(m) = base.document_message.as_option() {
        parts.push(format!(
            "document name={} mimetype={}",
            state.content(m.file_name.as_deref().unwrap_or("")),
            m.mimetype.as_deref().unwrap_or("?")
        ));
        context(&m.context_info, &mut parts);
    } else if let Some(m) = base.reaction_message.as_option() {
        let target = m
            .key
            .as_option()
            .and_then(|k| k.id.clone())
            .unwrap_or_default();
        parts.push(format!(
            "reaction {} on {target}",
            state.content(m.text.as_deref().unwrap_or(""))
        ));
    } else if base.sticker_message.is_set() {
        parts.push("sticker".into());
    } else if base.location_message.is_set() {
        parts.push("location".into());
    } else if base.contact_message.is_set() {
        parts.push("contact card".into());
    } else if base.poll_creation_message.is_set()
        || base.poll_creation_message_v2.is_set()
        || base.poll_creation_message_v3.is_set()
    {
        parts.push("poll".into());
    } else if base.poll_update_message.is_set() {
        parts.push("poll vote".into());
    } else if let Some(p) = base.protocol_message.as_option() {
        // Edits and deletes are protocol messages that point at an earlier one.
        let kind = p
            .r#type
            .as_ref()
            .map_or_else(|| "unknown".to_string(), |t| format!("{t:?}"));
        let mut text = format!("protocol {kind}");
        if let Some(id) = p.key.as_option().and_then(|k| k.id.as_deref()) {
            text.push_str(&format!(" on {id}"));
        }
        if let Some(edited) = p.edited_message.as_option() {
            text.push_str(&format!(" new: {}", describe(state, edited)));
        }
        parts.push(text);
    } else {
        parts.push("other".into());
    }
    parts.join(" ")
}

fn has_media(msg: &wa::Message) -> bool {
    let b = msg.get_base_message();
    b.image_message.is_set()
        || b.video_message.is_set()
        || b.audio_message.is_set()
        || b.document_message.is_set()
        || b.sticker_message.is_set()
}

async fn on_event(state: Arc<State>, client: Arc<Client>, event: Arc<Event>) {
    match &*event {
        Event::Connected(_) => out!("CONNECTED"),
        Event::Disconnected(_) => out!("DISCONNECTED"),
        Event::PairSuccess(_) => out!("PAIRED: device linked"),
        Event::PairError(e) => out!("PAIR ERROR: {e:?}"),
        Event::LoggedOut(_) => out!("LOGGED OUT: the phone unlinked this device"),
        Event::ClientOutdated(_) => out!("CLIENT OUTDATED (the equivalent of whatsmeow's 405)"),
        Event::TemporaryBan(b) => out!("TEMPORARY BAN: {b:?}"),
        Event::StreamReplaced(_) => out!("STREAM REPLACED: another client took over this session"),
        Event::Messages(batch) => {
            for inbound in batch.messages.iter() {
                handle_message(&state, &client, &inbound.info, &inbound.message).await;
            }
        }
        Event::HistorySync(lazy) => handle_history(&state, lazy),
        Event::ContactUpdate(update) => {
            state.stats.contact_updates.fetch_add(1, Ordering::Relaxed);
            let action = &update.action;
            let name = action
                .full_name
                .as_deref()
                .or(action.first_name.as_deref())
                .unwrap_or("");
            out!(
                "CONTACT {} name={} lid={:?} pn={:?}",
                update.jid,
                state.content(name),
                action.lid_jid,
                action.pn_jid
            );
        }
        Event::OfflineSyncCompleted(_) => out!("OFFLINE SYNC COMPLETED"),
        Event::UndecryptableMessage(u) => out!("UNDECRYPTABLE message: {:?}", u.info.id),
        _ => {}
    }
}

async fn handle_message(
    state: &State,
    client: &Client,
    info: &MessageInfo,
    msg: &Arc<wa::Message>,
) {
    state.stats.live_messages.fetch_add(1, Ordering::Relaxed);
    let src = &info.source;
    if !info.push_name.is_empty() {
        state.stats.push_names_seen.fetch_add(1, Ordering::Relaxed);
    }
    if src.sender.server.is_lid_family() {
        state.stats.lid_senders.fetch_add(1, Ordering::Relaxed);
    }
    // LID <-> phone number, as the bridge needs for names and the allowlist.
    let mapping = match client.get_lid_pn_entry(&src.sender).await {
        Ok(Some(entry)) => format!(" lid={} pn={}", entry.lid, entry.phone_number),
        Ok(None) => String::new(),
        Err(e) => format!(" lidmap-error={e}"),
    };
    out!(
        "MSG id={} chat={} sender={}{} alt={} from_me={} group={} push_name={} ts={} :: {}",
        info.id,
        src.chat,
        src.sender,
        mapping,
        src.sender_alt
            .as_ref()
            .map_or_else(|| "-".to_string(), |j| j.to_string()),
        src.is_from_me,
        src.is_group,
        state.content(&info.push_name),
        info.timestamp.to_rfc3339(),
        describe(state, msg)
    );
    state.remember(
        info.id.as_str(),
        Seen {
            chat: src.chat.clone(),
            sender: Some(src.sender.clone()),
            from_me: src.is_from_me,
            message: Arc::clone(msg),
            origin: "live",
        },
    );
}

fn handle_history(state: &State, lazy: &LazyHistorySyncRef) {
    state.stats.history_chunks.fetch_add(1, Ordering::Relaxed);
    let Some(sync) = lazy.get() else {
        out!("HISTORY chunk type={} (could not decode)", lazy.sync_type());
        return;
    };
    let mut messages = 0u64;
    let mut media = 0u64;
    for conv in &sync.conversations {
        let chat: Option<Jid> = conv.id.parse().ok();
        for item in &conv.messages {
            let Some(web) = item.message.as_option() else {
                continue;
            };
            messages += 1;
            let (Some(key), Some(msg)) = (web.key.as_option(), web.message.as_option()) else {
                continue;
            };
            if !has_media(msg) {
                continue;
            }
            media += 1;
            let (Some(id), Some(chat)) = (key.id.clone(), chat.clone()) else {
                continue;
            };
            let sender = key
                .participant
                .as_deref()
                .or(web.participant.as_deref())
                .and_then(|p| p.parse().ok());
            state.remember(
                &id,
                Seen {
                    chat,
                    sender,
                    from_me: key.from_me.unwrap_or(false),
                    message: Arc::new(msg.clone()),
                    origin: "history",
                },
            );
        }
    }
    let convs = sync.conversations.len() as u64;
    state
        .stats
        .history_conversations
        .fetch_add(convs, Ordering::Relaxed);
    state
        .stats
        .history_messages
        .fetch_add(messages, Ordering::Relaxed);
    state
        .stats
        .history_pushnames
        .fetch_add(sync.pushnames.len() as u64, Ordering::Relaxed);
    out!(
        "HISTORY chunk type={} progress={:?} conversations={convs} messages={messages} media={media} pushnames={}",
        lazy.sync_type(),
        lazy.progress(),
        sync.pushnames.len()
    );
}

type LazyHistorySyncRef = whatsapp_rust::wacore::types::events::LazyHistorySync;

fn parse_jid(text: &str) -> Result<Jid> {
    if text.contains('@') {
        text.parse()
            .map_err(|e| anyhow!("invalid JID {text}: {e:?}"))
    } else {
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        if digits.is_empty() {
            bail!("invalid phone number {text}");
        }
        format!("{digits}@s.whatsapp.net")
            .parse()
            .map_err(|e| anyhow!("invalid number {text}: {e:?}"))
    }
}

const HELP: &str = "\
commands:
  send <to> <text>             text message (to = phone number or JID)
  reply <id> <text>            reply quoting a message seen earlier
  mention <to> <number> <text> text that @mentions <number> (put @<number> in the text)
  image <to> <file> [caption]  image with optional caption
  video <to> <file> [caption]  video with optional caption
  doc <to> <file> [caption]    document, with file name and type
  voice <to> <file.ogg>        voice note (opus in ogg)
  react <id> <emoji>           react to a message seen earlier
  unreact <id>                 remove your reaction
  download <id>                download a message's media into store/downloads
  retry <id>                   ask the sender's phone to re-upload expired media, then download
  group <jid>                  group name and number of members
  groups                       groups this account is in
  lid <jid-or-number>          look up the LID <-> phone number mapping
  recent [n]                   last n messages seen (default 20), with ids
  stats                        counters since start
  quit";

async fn command(state: &Arc<State>, client: &Arc<Client>, line: &str) -> Result<bool> {
    let line = line.trim();
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    match cmd {
        "" => {}
        "help" | "?" => println!("{HELP}"),
        "quit" | "exit" => return Ok(false),
        "send" => {
            let (to, text) = rest.split_once(' ').context("usage: send <to> <text>")?;
            let msg = wa::Message::text(text.trim());
            let sent = client.send_message(parse_jid(to)?, msg.clone()).await?;
            out!("SENT id={} to={}", sent.message_id, sent.to);
            state.remember_sent(sent.message_id.as_str(), &sent.to, msg);
        }
        "reply" => {
            let (id, text) = rest.split_once(' ').context("usage: reply <id> <text>")?;
            let seen = state.get(id)?;
            let sender = seen.sender.clone().unwrap_or_else(|| seen.chat.clone());
            let ctx = wacore_quote(id, &sender, &seen.chat, &seen.message);
            let msg = wa::Message::text_with_context(text.trim(), ctx);
            let sent = client.send_message(seen.chat.clone(), msg.clone()).await?;
            out!("SENT reply id={} to={}", sent.message_id, sent.to);
            state.remember_sent(sent.message_id.as_str(), &sent.to, msg);
        }
        "mention" => {
            let mut it = rest.splitn(3, ' ');
            let (Some(to), Some(number), Some(text)) = (it.next(), it.next(), it.next()) else {
                bail!("usage: mention <to> <number> <text>");
            };
            let mut ctx = wa::ContextInfo::default();
            ctx.mentioned_jid = vec![parse_jid(number)?.to_string()];
            let msg = wa::Message::text_with_context(text, ctx);
            let sent = client.send_message(parse_jid(to)?, msg.clone()).await?;
            out!("SENT mention id={} to={}", sent.message_id, sent.to);
            state.remember_sent(sent.message_id.as_str(), &sent.to, msg);
        }
        "image" | "video" | "doc" | "voice" => {
            let mut it = rest.splitn(3, ' ');
            let (Some(to), Some(file)) = (it.next(), it.next()) else {
                bail!("usage: {cmd} <to> <file> [caption]");
            };
            let caption = it.next().unwrap_or("").trim().to_string();
            let msg = upload_media(client, cmd, Path::new(file), caption).await?;
            let sent = client.send_message(parse_jid(to)?, msg.clone()).await?;
            out!("SENT {cmd} id={} to={}", sent.message_id, sent.to);
            state.remember_sent(sent.message_id.as_str(), &sent.to, msg);
        }
        "react" | "unreact" => {
            let (id, emoji) = match rest.split_once(' ') {
                Some((id, emoji)) => (id, emoji.trim()),
                None => (rest, ""),
            };
            if cmd == "react" && emoji.is_empty() {
                bail!("usage: react <id> <emoji>");
            }
            let seen = state.get(id)?;
            let target = MessageRef::new(
                &seen.chat,
                MessageId::new(id)?,
                seen.sender.as_ref(),
                seen.from_me,
            )?;
            let sent = client
                .send_reaction(&target, if cmd == "react" { emoji } else { "" })
                .await?;
            out!("SENT {cmd} id={} to={}", sent.message_id, sent.to);
        }
        "download" => {
            let seen = state.get(rest)?;
            match download(client, &seen.message).await {
                Ok(bytes) => {
                    let path = save(state, rest, &seen.message, &bytes).await?;
                    out!("DOWNLOADED {} bytes -> {}", bytes.len(), path.display());
                }
                Err(e) => out!(
                    "DOWNLOAD FAILED ({}): {e:#}. If it expired, try: retry {rest}",
                    seen.origin
                ),
            }
        }
        "retry" => {
            let seen = state.get(rest)?;
            retry(state, client, rest, &seen).await?;
        }
        "group" => {
            if rest.is_empty() {
                bail!("usage: group <jid>; `groups` lists them");
            }
            let jid = parse_jid(rest)?;
            let meta = client
                .groups()
                .fetch_metadata(&jid)
                .await
                .map_err(|e| anyhow!("{e:?}"))?;
            out!(
                "GROUP {} subject={} participants={} addressing={:?}",
                meta.id,
                state.content(meta.subject.as_deref().unwrap_or("")),
                meta.participants.len(),
                meta.addressing_mode
            );
        }
        "groups" => {
            let groups = client
                .groups()
                .list_participating()
                .await
                .map_err(|e| anyhow!("{e:?}"))?;
            out!("GROUPS {} groups", groups.len());
            // Group names only with --show-content.
            for g in groups {
                let members = g
                    .participant_count
                    .map_or_else(|| "?".to_string(), |n| n.to_string());
                let subject = g.subject.as_deref().unwrap_or("");
                println!(
                    "  {} participants={members} subject={}",
                    g.id,
                    state.content(subject)
                );
            }
        }
        "lid" => {
            let jid = parse_jid(rest)?;
            match client.get_lid_pn_entry(&jid).await? {
                Some(e) => out!(
                    "LID {} <-> PN {} (learned {:?})",
                    e.lid,
                    e.phone_number,
                    e.learning_source
                ),
                None => out!("no mapping known for {jid}"),
            }
        }
        "recent" => {
            let n: usize = rest.parse().unwrap_or(20);
            let order = state.order.lock().unwrap().clone();
            let map = state.seen.lock().unwrap();
            for id in order.iter().rev().take(n) {
                if let Some(s) = map.get(id) {
                    println!(
                        "  {id} chat={} sender={:?} from_me={} {} :: {}",
                        s.chat,
                        s.sender.as_ref().map(|j| j.to_string()),
                        s.from_me,
                        s.origin,
                        describe(state, &s.message)
                    );
                }
            }
        }
        "stats" => {
            let s = &state.stats;
            let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
            println!(
                "  live messages={} (from LID senders={}, with push name={})\n  history chunks={} conversations={} messages={} pushnames={}\n  contact updates={} remembered messages={}",
                g(&s.live_messages),
                g(&s.lid_senders),
                g(&s.push_names_seen),
                g(&s.history_chunks),
                g(&s.history_conversations),
                g(&s.history_messages),
                g(&s.history_pushnames),
                g(&s.contact_updates),
                state.seen.lock().unwrap().len()
            );
        }
        other => bail!("unknown command {other}; type help"),
    }
    Ok(true)
}

fn wacore_quote(id: &str, sender: &Jid, chat: &Jid, quoted: &wa::Message) -> wa::ContextInfo {
    whatsapp_rust::proto_helpers::build_quote_context_with_info(id, sender, chat, chat, quoted)
}

type MediaType = whatsapp_rust::wacore::download::MediaType;

async fn upload_media(
    client: &Client,
    kind: &str,
    file: &Path,
    caption: String,
) -> Result<wa::Message> {
    let data = tokio::fs::read(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let mime = mime_guess::from_path(file)
        .first_or_octet_stream()
        .to_string();
    let media_type = match kind {
        "image" => MediaType::Image,
        "video" => MediaType::Video,
        "voice" => MediaType::Audio,
        _ => MediaType::Document,
    };
    let up = client
        .upload(data, media_type, whatsapp_rust::UploadOptions::new())
        .await?;
    let caption = (!caption.is_empty()).then_some(caption);
    let mut msg = wa::Message::default();
    match kind {
        "image" => {
            let mut m = wa::message::ImageMessage::default();
            m.url = Some(up.url);
            m.direct_path = Some(up.direct_path);
            m.media_key = Some(up.media_key.to_vec());
            m.file_sha256 = Some(up.file_sha256.to_vec());
            m.file_enc_sha256 = Some(up.file_enc_sha256.to_vec());
            m.file_length = Some(up.file_length);
            m.media_key_timestamp = Some(up.media_key_timestamp);
            m.mimetype = Some(mime);
            m.caption = caption;
            msg.image_message = MessageField::some(m);
        }
        "video" => {
            let mut m = wa::message::VideoMessage::default();
            m.url = Some(up.url);
            m.direct_path = Some(up.direct_path);
            m.media_key = Some(up.media_key.to_vec());
            m.file_sha256 = Some(up.file_sha256.to_vec());
            m.file_enc_sha256 = Some(up.file_enc_sha256.to_vec());
            m.file_length = Some(up.file_length);
            m.media_key_timestamp = Some(up.media_key_timestamp);
            m.mimetype = Some(mime);
            m.caption = caption;
            m.streaming_sidecar = up.streaming_sidecar;
            msg.video_message = MessageField::some(m);
        }
        "voice" => {
            let mut m = wa::message::AudioMessage::default();
            m.url = Some(up.url);
            m.direct_path = Some(up.direct_path);
            m.media_key = Some(up.media_key.to_vec());
            m.file_sha256 = Some(up.file_sha256.to_vec());
            m.file_enc_sha256 = Some(up.file_enc_sha256.to_vec());
            m.file_length = Some(up.file_length);
            m.media_key_timestamp = Some(up.media_key_timestamp);
            m.mimetype = Some("audio/ogg; codecs=opus".into());
            m.ptt = Some(true);
            m.streaming_sidecar = up.streaming_sidecar;
            msg.audio_message = MessageField::some(m);
        }
        _ => {
            let mut m = wa::message::DocumentMessage::default();
            m.url = Some(up.url);
            m.direct_path = Some(up.direct_path);
            m.media_key = Some(up.media_key.to_vec());
            m.file_sha256 = Some(up.file_sha256.to_vec());
            m.file_enc_sha256 = Some(up.file_enc_sha256.to_vec());
            m.file_length = Some(up.file_length);
            m.media_key_timestamp = Some(up.media_key_timestamp);
            m.mimetype = Some(mime);
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned());
            m.title = name.clone();
            m.file_name = name;
            m.caption = caption;
            msg.document_message = MessageField::some(m);
        }
    }
    Ok(msg)
}

async fn download(client: &Client, msg: &wa::Message) -> Result<Vec<u8>> {
    let b = msg.get_base_message();
    let limits = Default::default();
    let bytes = if let Some(m) = b.image_message.as_option() {
        client.download(m, limits).await
    } else if let Some(m) = b.video_message.as_option() {
        client.download(m, limits).await
    } else if let Some(m) = b.audio_message.as_option() {
        client.download(m, limits).await
    } else if let Some(m) = b.document_message.as_option() {
        client.download(m, limits).await
    } else if let Some(m) = b.sticker_message.as_option() {
        client.download(m, limits).await
    } else {
        bail!("this message has no media");
    };
    bytes.map_err(|e| anyhow!("{e:?}"))
}

fn media_key(msg: &wa::Message) -> Option<[u8; 32]> {
    let b = msg.get_base_message();
    let key = b
        .image_message
        .as_option()
        .and_then(|m| m.media_key.clone())
        .or_else(|| {
            b.video_message
                .as_option()
                .and_then(|m| m.media_key.clone())
        })
        .or_else(|| {
            b.audio_message
                .as_option()
                .and_then(|m| m.media_key.clone())
        })
        .or_else(|| {
            b.document_message
                .as_option()
                .and_then(|m| m.media_key.clone())
        })
        .or_else(|| {
            b.sticker_message
                .as_option()
                .and_then(|m| m.media_key.clone())
        })?;
    key.try_into().ok()
}

fn with_direct_path(msg: &wa::Message, path: &str) -> wa::Message {
    let mut msg = msg.get_base_message().clone();
    if let Some(m) = msg.image_message.as_option_mut() {
        m.direct_path = Some(path.into());
    } else if let Some(m) = msg.video_message.as_option_mut() {
        m.direct_path = Some(path.into());
    } else if let Some(m) = msg.audio_message.as_option_mut() {
        m.direct_path = Some(path.into());
    } else if let Some(m) = msg.document_message.as_option_mut() {
        m.direct_path = Some(path.into());
    } else if let Some(m) = msg.sticker_message.as_option_mut() {
        m.direct_path = Some(path.into());
    }
    msg
}

async fn retry(state: &State, client: &Client, id: &str, seen: &Seen) -> Result<()> {
    use whatsapp_rust::{MediaRetryResult, MediaReuploadRequest};

    let key = media_key(&seen.message).context("message has no media key")?;
    let target = MessageRef::new(
        &seen.chat,
        MessageId::new(id)?,
        seen.sender.as_ref(),
        seen.from_me,
    )?;
    out!("RETRY requested; the sender's phone must be online");
    let result = client
        .media_reupload()
        .request(&MediaReuploadRequest {
            target,
            media_key: &key,
        })
        .await
        .map_err(|e| anyhow!("{e:?}"))?;
    match result {
        MediaRetryResult::Success { direct_path } => {
            let fresh = with_direct_path(&seen.message, &direct_path);
            let bytes = download(client, &fresh).await?;
            let path = save(state, id, &fresh, &bytes).await?;
            out!(
                "RETRY OK: downloaded {} bytes -> {}",
                bytes.len(),
                path.display()
            );
        }
        other => out!("RETRY FAILED: {other:?}"),
    }
    Ok(())
}

async fn save(state: &State, id: &str, msg: &wa::Message, bytes: &[u8]) -> Result<PathBuf> {
    let b = msg.get_base_message();
    let ext = if b.image_message.is_set() {
        "jpg"
    } else if b.video_message.is_set() {
        "mp4"
    } else if b.audio_message.is_set() {
        "ogg"
    } else if b.sticker_message.is_set() {
        "webp"
    } else {
        "bin"
    };
    let dir = state.store.join("downloads");
    tokio::fs::create_dir_all(&dir).await?;
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{safe}.{ext}"));
    tokio::fs::write(&path, bytes).await?;
    Ok(path)
}

fn print_qr(code: &str) {
    match qrcode::QrCode::new(code.as_bytes()) {
        Ok(qr) => {
            let text = qr
                .render::<qrcode::render::unicode::Dense1x2>()
                .dark_color(qrcode::render::unicode::Dense1x2::Light)
                .light_color(qrcode::render::unicode::Dense1x2::Dark)
                .build();
            println!("\nScan with WhatsApp > Linked devices > Link a device:\n{text}\n");
        }
        Err(e) => println!("QR code (could not render: {e}): {code}"),
    }
}

/// The phone number for a pairing code: digits only, with the country code.
/// WhatsApp rejects a national number such as 06..., without saying why.
fn pair_phone(raw: &str) -> Result<String> {
    let digits: String = raw
        .chars()
        .filter(|c| !matches!(c, '+' | ' ' | '-' | '(' | ')'))
        .collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        bail!("--pair-phone {raw:?}: use digits only, such as 316...");
    }
    if digits.starts_with('0') {
        bail!(
            "--pair-phone {raw:?}: start with the country code, not 0 (for the Netherlands: 316... instead of 06...)"
        );
    }
    Ok(digits)
}

/// The OS name for the pairing request, in a form the server accepts.
fn display_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "Mac OS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

/// Refuse arguments the program does not know, so a typo is not silently ignored.
fn check_args(args: &[String]) -> Result<()> {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--store" | "--pair-phone" => {
                rest.next()
                    .with_context(|| format!("{arg} needs a value"))?;
            }
            "--show-content" => {}
            other => bail!("unknown argument {other:?}; see --help"),
        }
    }
    Ok(())
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: wa-spike [--store DIR] [--pair-phone NUMBER] [--show-content]\n\n{HELP}");
        return Ok(());
    }
    check_args(&args)?;
    let pair = arg_value(&args, "--pair-phone")
        .map(|p| pair_phone(&p))
        .transpose()?;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let store = PathBuf::from(arg_value(&args, "--store").unwrap_or_else(|| "store".into()));
    std::fs::create_dir_all(&store)?;
    let state = Arc::new(State {
        show_content: args.iter().any(|a| a == "--show-content"),
        store: store.clone(),
        seen: Mutex::default(),
        order: Mutex::default(),
        stats: Stats::default(),
    });

    let pairing = pair.is_some();
    let db = store.join("whatsapp.db");
    let backend = SqliteStore::open(db.to_str().context("store path is not UTF-8")?).await?;
    let handler_state = Arc::clone(&state);
    let mut builder = Bot::builder()
        .with_backend(backend)
        .on_qr_code(move |code, _timeout| async move {
            // With a pairing code the QR code is noise; the code follows.
            if pairing {
                out!("waiting for the pairing code (QR code not shown)");
            } else {
                print_qr(&code);
            }
        })
        .on_pair_code(|code, timeout| async move {
            out!(
                "PAIR CODE {code} (valid {}s): WhatsApp > Linked devices > Link with phone number",
                timeout.as_secs()
            );
        })
        .on_event(move |event, client| {
            let state = Arc::clone(&handler_state);
            async move { on_event(state, client, event).await }
        });
    if let Some(phone) = pair {
        builder = builder.with_pair_code(PairCodeOptions {
            phone_number: phone,
            // The phone shows this OS. Left unset, the library sends its
            // default name, warns, and falls back to "Linux".
            display_os: Some(display_os().to_string()),
            ..Default::default()
        });
    }
    let bot = builder.build().await?;
    let client = bot.client();
    let mut handle = bot.spawn();
    println!(
        "wa-spike started; store {}. Type help for commands.",
        store.display()
    );

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            _ = &mut handle => { out!("client stopped"); break; }
            _ = tokio::signal::ctrl_c() => break,
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                match command(&state, &client, &line).await {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => out!("ERROR {e:#}"),
                }
            }
        }
    }
    out!("shutting down");
    let report = handle.shutdown().await;
    log::info!("shutdown: {report:?}");
    Ok(())
}
