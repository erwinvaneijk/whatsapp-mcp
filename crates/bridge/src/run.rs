//! `whatsapp-mcp bridge`: links and keeps the WhatsApp connection
//! (whatsapp-rust, session in `<store>/session.db`), stores what arrives in
//! `<store>/messages.db`, and serves the local API on `127.0.0.1:8080`.
//!
//! Events are taken from the library as they are dispatched and handled one
//! at a time, in order, by one task: the library's own callback delivery
//! drops events when its queue is full, and a dropped history sync chunk
//! would be messages missing for good.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::mpsc;
use whatsapp_mcp_config::{Config, SendAllowlist, Setting};
use whatsapp_mcp_store::{
    ContactNames, NewMessage, Writer, ensure_private_dir, ensure_private_file,
};
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::types::events::{EventHandler, LazyHistorySync};

use crate::api::Api;
use crate::jid::{GROUP_SERVER, Jid as TextJid};
use crate::live::Live;
use crate::log;
use crate::receive::{self, ChatName};

/// Where the local API listens, as in the Go bridge.
pub const API_ADDRESS: ([u8; 4], u16) = ([127, 0, 0, 1], 8080);

/// What `bridge` needs to start.
pub struct Options {
    /// The store folder: `messages.db` and `session.db`.
    pub store_dir: PathBuf,
}

/// The settings the bridge reads, checked before anything starts.
struct Settings {
    token: String,
    allowlist: Option<SendAllowlist>,
    log_content: bool,
    pair_phone: Option<String>,
    media_dir: Option<String>,
}

impl Settings {
    fn from_config(config: &Config) -> Result<Settings, String> {
        let token = config
            .bridge_token()
            .map_err(|e| e.to_string())?
            .to_string();
        let allowlist = config.send_allowlist().map_err(|e| e.to_string())?;
        let pair_phone = config.pair_phone().map(pair_phone).transpose()?;
        Ok(Settings {
            token,
            allowlist,
            log_content: config.log_content(),
            pair_phone,
            media_dir: config.media_dir().map(str::to_string),
        })
    }
}

/// The phone number for a pairing code: digits only, with the country code.
/// WhatsApp refuses a national number such as 06..., without saying why.
pub fn pair_phone(raw: &str) -> Result<String, String> {
    let digits: String = raw
        .chars()
        .filter(|c| !matches!(c, '+' | ' ' | '-' | '(' | ')'))
        .collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "WHATSAPP_PAIR_PHONE {raw:?}: use digits only, with the country code, such as 316..."
        ));
    }
    if digits.starts_with('0') {
        return Err(format!(
            "WHATSAPP_PAIR_PHONE {raw:?}: start with the country code, not 0 (for the Netherlands: 316... instead of 06...)"
        ));
    }
    Ok(digits)
}

/// The OS name the phone shows for this linked device.
fn display_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "Mac OS",
        "windows" => "Windows",
        _ => "Linux",
    }
}

/// [`run`] on its own runtime. Logging is set up by the caller
/// (`whatsapp-mcp-logging`), before the startup block.
pub fn run_blocking(config: &Config, options: Options) -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to start the runtime: {e}"))?
        .block_on(run(config, options))
}

/// Runs the bridge until Ctrl+C (or SIGTERM), or until the connection ends
/// for good (logged out, replaced by another client).
pub async fn run(config: &Config, options: Options) -> Result<(), String> {
    let store = options.store_dir;
    for line in startup_lines(config, &store) {
        log::line(&line);
    }
    let settings = Settings::from_config(config)?;
    log::line("Starting WhatsApp client...");

    ensure_private_dir(&store).map_err(|e| format!("Failed to create store directory: {e}"))?;
    let writer =
        Writer::open(&store).map_err(|e| format!("Failed to initialize message store: {e}"))?;
    let shared = Arc::new(Shared {
        writer: Mutex::new(writer),
        log_content: settings.log_content,
        connected: AtomicBool::new(false),
    });

    // Bind before connecting: a second bridge (or the Go bridge) on the
    // port is found before this one links or stores anything.
    let address = SocketAddr::from(API_ADDRESS);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|e| format!("Failed to listen on {address} (is another bridge running?): {e}"))?;

    let session = session_path(&store)?;
    let backend = SqliteStore::open(&session)
        .await
        .map_err(|e| format!("Failed to open the session database {session}: {e}"))?;

    let (events, mut queue) = mpsc::unbounded_channel();
    let waiting = Arc::new(Waiting::default());
    let mut builder = Bot::builder()
        .with_backend(backend)
        .with_event_handler(Forward {
            events,
            waiting: Arc::clone(&waiting),
        });
    let pairing = settings.pair_phone.is_some();
    if let Some(phone) = settings.pair_phone.clone() {
        builder = builder.with_pair_code(PairCodeOptions {
            phone_number: phone,
            // Left unset, the library sends a name the phone does not show.
            display_os: Some(display_os().to_string()),
            ..Default::default()
        });
    }
    let bot = builder
        .build()
        .await
        .map_err(|e| format!("Failed to create WhatsApp client: {e}"))?;
    let client = bot.client();
    log::line(&session_line(
        Path::new(&session),
        client.pn().is_some(),
        pairing,
    ));

    let worker = {
        let shared = Arc::clone(&shared);
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            while let Some(event) = queue.recv().await {
                waiting.taken();
                handle_event(&shared, &client, &event, pairing).await;
            }
        })
    };

    let api = Arc::new(Api {
        token: settings.token,
        allowlist: settings.allowlist,
        log_content: settings.log_content,
        backend: Arc::new(Live {
            shared: Arc::clone(&shared),
            client: Arc::clone(&client),
            store_dir: store.clone(),
            media_dir: settings.media_dir,
        }),
    });
    let mut server = tokio::spawn(crate::api::serve_on(api, listener));
    log::line(&format!(
        "REST server is running on {address}. Press Ctrl+C to disconnect and exit."
    ));

    let mut handle = bot.spawn();
    let outcome = tokio::select! {
        _ = &mut handle => Err("The WhatsApp client stopped.".to_string()),
        result = &mut server => Err(match result {
            Ok(Err(e)) => format!("The REST server stopped: {e}"),
            _ => "The REST server stopped.".to_string(),
        }),
        _ = shutdown_signal() => Ok(()),
    };
    log::line("Disconnecting...");
    server.abort();
    let report = handle.shutdown().await;
    if let Err(e) = report.shutdown.device {
        log::warning(&format!(
            "Failed to save the WhatsApp session: {}",
            log::clean(&e.to_string())
        ));
    }
    // Store what was already received before exiting.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), worker).await;
    outcome
}

/// What `bridge` logs on start: the version, the settings and where each
/// comes from, and the store. Logged before the settings are checked, so a
/// refused setting shows next to the others.
pub fn startup_lines(config: &Config, store: &Path) -> Vec<String> {
    let mut lines = vec![format!("whatsapp-mcp {} bridge", env!("CARGO_PKG_VERSION"))];
    lines.extend(config.startup_lines(&[
        Setting::BridgeToken,
        Setting::MediaDir,
        Setting::SendAllowlist,
        Setting::LogContent,
        Setting::PairPhone,
        Setting::LogOutput,
        Setting::LogFormat,
        Setting::LogLevel,
    ]));
    let shown = log::clean(
        &std::path::absolute(store)
            .unwrap_or_else(|_| store.to_path_buf())
            .display()
            .to_string(),
    );
    let state = if store.join("messages.db").is_file() {
        "messages.db found"
    } else {
        "messages.db will be created"
    };
    lines.push(format!("Store: {shown} ({state})"));
    lines
}

/// The session line: whether the bridge is linked, or how it will link.
pub fn session_line(session: &Path, linked: bool, pairing: bool) -> String {
    let shown = log::clean(
        &std::path::absolute(session)
            .unwrap_or_else(|_| session.to_path_buf())
            .display()
            .to_string(),
    );
    let state = match (linked, pairing) {
        (true, _) => "linked",
        (false, true) => "not linked yet: a pairing code follows",
        (false, false) => "not linked yet: a QR code follows",
    };
    format!("Session: {shown} ({state})")
}

/// `<store>/session.db`, created readable by the owner only.
fn session_path(store: &Path) -> Result<String, String> {
    let path = store.join("session.db");
    ensure_private_file(&path).map_err(|e| format!("Failed to create session database: {e}"))?;
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| format!("The store path is not UTF-8: {}", path.display()))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// Passes every event to the bridge's own queue, without dropping any. The
/// queue has no limit: blocking here would stall the connection, and
/// dropping would lose messages. Its length is watched instead.
struct Forward {
    events: mpsc::UnboundedSender<Arc<Event>>,
    waiting: Arc<Waiting>,
}

impl EventHandler for Forward {
    fn handle_event(&self, event: Arc<Event>) {
        // Fails only after the worker stopped, during shutdown.
        if self.events.send(event).is_ok() {
            self.waiting.added();
        }
    }
}

/// The number of events waiting to be stored. Each time it reaches the
/// next of 1,000, 10,000, 100,000, ... the bridge says so, so a store
/// that falls behind shows in the log long before memory runs out.
#[derive(Default)]
struct Waiting {
    count: AtomicUsize,
    warned_at: AtomicUsize,
}

/// The first queue length that is reported.
const WAITING_WARN: usize = 1_000;

impl Waiting {
    fn added(&self) {
        let count = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        let warned = self.warned_at.load(Ordering::Relaxed);
        let next = if warned == 0 {
            WAITING_WARN
        } else {
            warned * 10
        };
        if count >= next
            && self
                .warned_at
                .compare_exchange(warned, next, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            log::warning(&format!(
                "{count} WhatsApp events are waiting to be stored; storing is falling behind"
            ));
        }
    }

    fn taken(&self) {
        let count = self.count.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
        // Back to normal: report the next build-up from the start again.
        if count < WAITING_WARN / 10 {
            self.warned_at.store(0, Ordering::Relaxed);
        }
    }
}

/// The store and settings the event worker and the API share.
pub(crate) struct Shared {
    writer: Mutex<Writer>,
    pub(crate) log_content: bool,
    /// Set by the event worker on connecting and disconnecting.
    connected: AtomicBool,
}

impl Shared {
    pub(crate) fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    pub(crate) fn writer(&self) -> MutexGuard<'_, Writer> {
        // A panic while storing leaves the database consistent (SQLite), so
        // carry on with it.
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// whatsapp-rust's JID in the bridge's own form.
fn text_jid(jid: &Jid) -> Option<TextJid> {
    TextJid::parse(&jid.to_string()).ok()
}

async fn handle_event(shared: &Shared, client: &Arc<Client>, event: &Event, pairing: bool) {
    match event {
        Event::PairingQrCode(qr) => {
            // With a pairing code the QR code is noise; the code follows.
            if pairing {
                log::line("Waiting for the pairing code...");
            } else {
                print_qr(&qr.code);
            }
        }
        Event::PairingCode(code) => {
            // On the terminal only: the code links a device to the account.
            log::terminal(&format!(
                "\nOn your phone open WhatsApp > Linked devices > Link a device > Link with phone number instead, and enter: {}\n",
                log::clean(&code.code)
            ));
            log::line("Pairing code shown on the terminal; enter it on the phone to link");
        }
        Event::PairingCodeError(e) => log::warning(&format!(
            "Failed to request pairing code: {}",
            log::clean(&format!("{:?}", e.rejection))
        )),
        Event::PairingQrCodesExhausted(_) => {
            log::warning("Timeout waiting for QR code scan; start the bridge again to link.");
        }
        Event::PairSuccess(_) => log::line("Successfully connected and authenticated!"),
        Event::PairError(e) => log::warning(&format!("Linking failed: {}", log::clean(&e.error))),
        Event::Connected(_) => {
            shared.set_connected(true);
            log::line("✓ Connected to WhatsApp");
        }
        Event::Disconnected(_) => {
            shared.set_connected(false);
            log::warning("Disconnected from WhatsApp; reconnecting...");
        }
        Event::LoggedOut(_) => {
            shared.set_connected(false);
            log::warning(
                "Device logged out: the phone unlinked this bridge. Start it again to link it.",
            );
        }
        Event::StreamReplaced(_) => {
            shared.set_connected(false);
            log::warning("Another client took over this session (stream replaced).");
        }
        Event::ClientOutdated(_) => log::warning(
            "WhatsApp says this client is outdated (405); a newer whatsapp-rust is needed.",
        ),
        Event::TemporaryBan(ban) => log::warning(&format!(
            "WhatsApp banned this account temporarily: {}",
            log::clean(&format!("{ban:?}"))
        )),
        Event::Messages(batch) => {
            for inbound in batch.messages.iter() {
                store_live(shared, client, &inbound.info, &inbound.message).await;
            }
        }
        Event::HistorySync(lazy) => store_history(shared, client, lazy).await,
        Event::ContactUpdate(update) => {
            let action = &update.action;
            let names = ContactNames {
                first_name: action.first_name.clone().unwrap_or_default(),
                full_name: action.full_name.clone().unwrap_or_default(),
                ..ContactNames::default()
            };
            let writer = shared.writer();
            warn_on_error(
                "store contact",
                writer.store_contact(&update.jid.to_non_ad_string(), &names),
            );
            for other in [&action.lid_jid, &action.pn_jid].into_iter().flatten() {
                if let Some((lid, pn)) =
                    receive::lid_pair_text(&update.jid.to_non_ad_string(), other)
                {
                    warn_on_error("store LID mapping", writer.store_lid_mapping(&lid, &pn));
                }
            }
        }
        Event::UndecryptableMessage(u) => {
            log::warning(&format!(
                "Could not decrypt message {}",
                log::clean(&u.info.id)
            ));
        }
        _ => {}
    }
}

/// The Go bridge's `handleMessage`.
async fn store_live(shared: &Shared, client: &Arc<Client>, info: &MessageInfo, msg: &wa::Message) {
    let source = &info.source;
    let chat_jid = source.chat.to_string();
    let sender = source.sender.user.to_string();
    learn_ids(shared, client, &source.sender, source.sender_alt.as_ref()).await;
    if !info.push_name.is_empty() && !source.is_from_me {
        let names = ContactNames {
            push_name: info.push_name.to_string(),
            ..ContactNames::default()
        };
        warn_on_error(
            "store push name",
            shared
                .writer()
                .store_contact(&source.sender.to_non_ad_string(), &names),
        );
    }

    let name = match text_jid(&source.chat) {
        Some(chat) => resolve_chat_name(shared, client, &chat, &chat_jid, None, &sender).await,
        None => sender.clone(),
    };
    let row: NewMessage = receive::live_message(
        &info.id,
        &chat_jid,
        &sender,
        info.timestamp,
        source.is_from_me,
        msg,
        chrono::Local::now(),
    );
    let writer = shared.writer();
    warn_on_error(
        "store chat",
        writer.store_chat(&chat_jid, &name, receive::local(info.timestamp)),
    );
    if row.content.is_empty() && row.media_type.is_empty() {
        // Where unhandled message types disappear: say so, so they can be
        // found and added.
        log::warning(&format!(
            "Dropping message {} in {}: no text or media extracted",
            log::clean(&info.id),
            log::clean(&chat_jid)
        ));
        return;
    }
    if let Err(e) = writer.store_message(&row) {
        log::warning(&format!(
            "Failed to store message: {}",
            log::clean(&e.to_string())
        ));
        return;
    }
    let timestamp = receive::local(info.timestamp).format("%Y-%m-%d %H:%M:%S");
    let direction = if source.is_from_me { "→" } else { "←" };
    log::line(&match (row.media_type.is_empty(), shared.log_content) {
        (false, true) => format!(
            "[{timestamp}] {direction} {}: [{}: {}] {}",
            log::clean(&sender),
            row.media_type,
            log::clean(&row.filename),
            log::clean(&row.content)
        ),
        (false, false) => format!(
            "[{timestamp}] {direction} message stored [{}]",
            row.media_type
        ),
        (true, true) => format!(
            "[{timestamp}] {direction} {}: {}",
            log::clean(&sender),
            log::clean(&row.content)
        ),
        (true, false) => format!("[{timestamp}] {direction} message stored"),
    });
}

/// Records which LID belongs to which phone number, from the message's
/// alternative sender ID or else the library's own map.
async fn learn_ids(shared: &Shared, client: &Arc<Client>, sender: &Jid, alt: Option<&Jid>) {
    let pair = match alt
        .and_then(|alt| receive::lid_pair_text(&sender.to_non_ad_string(), &alt.to_non_ad_string()))
    {
        Some(pair) => Some(pair),
        None => match client.get_lid_pn_entry(sender).await {
            Ok(Some(entry)) => Some((entry.lid.to_string(), entry.phone_number.to_string())),
            _ => None,
        },
    };
    if let Some((lid, pn)) = pair {
        warn_on_error(
            "store LID mapping",
            shared.writer().store_lid_mapping(&lid, &pn),
        );
    }
}

/// [`receive::chat_name`], asking WhatsApp for a group's subject when needed.
async fn resolve_chat_name(
    shared: &Shared,
    client: &Arc<Client>,
    chat: &TextJid,
    chat_jid: &str,
    conversation: Option<&wa::Conversation>,
    sender: &str,
) -> String {
    let known = receive::chat_name(&shared.writer(), chat, chat_jid, conversation, sender);
    match known {
        Ok(ChatName::Known(name)) => name,
        Ok(ChatName::AskGroup) => group_subject(client, chat_jid)
            .await
            .unwrap_or_else(|| receive::group_fallback(chat)),
        Err(e) => {
            log::warning(&format!(
                "Failed to read chat name: {}",
                log::clean(&e.to_string())
            ));
            if chat.server == GROUP_SERVER {
                receive::group_fallback(chat)
            } else {
                chat.user.clone()
            }
        }
    }
}

async fn group_subject(client: &Arc<Client>, chat_jid: &str) -> Option<String> {
    let jid: Jid = chat_jid.parse().ok()?;
    let metadata = client.groups().fetch_metadata(&jid).await.ok()?;
    metadata.subject.filter(|s| !s.is_empty())
}

/// The Go bridge's `handleHistorySync`, plus push names and the LID map
/// from the same payload.
async fn store_history(shared: &Shared, client: &Arc<Client>, lazy: &LazyHistorySync) {
    let Some(sync) = lazy.get() else {
        log::warning("Received a history sync chunk that could not be decoded");
        return;
    };
    log::line(&format!(
        "Received history sync event with {} conversations",
        sync.conversations.len()
    ));
    {
        let writer = shared.writer();
        for push in &sync.pushnames {
            let (Some(id), Some(name)) = (push.id.as_deref(), push.pushname.as_deref()) else {
                continue;
            };
            let names = ContactNames {
                push_name: name.to_string(),
                ..ContactNames::default()
            };
            warn_on_error("store push name", writer.store_contact(id, &names));
        }
        for mapping in &sync.phone_number_to_lid_mappings {
            if let (Some(lid), Some(pn)) = (mapping.lid_jid.as_deref(), mapping.pn_jid.as_deref())
                && let Some((lid, pn)) = receive::lid_pair_text(lid, pn)
            {
                warn_on_error("store LID mapping", writer.store_lid_mapping(&lid, &pn));
            }
        }
        for conversation in &sync.conversations {
            for other in [&conversation.lid_jid, &conversation.pn_jid]
                .into_iter()
                .flatten()
            {
                if let Some((lid, pn)) = receive::lid_pair_text(&conversation.id, other) {
                    warn_on_error("store LID mapping", writer.store_lid_mapping(&lid, &pn));
                }
            }
        }
    }

    let own_user = client
        .pn()
        .map(|pn| pn.user.to_string())
        .unwrap_or_default();
    let mut stored = 0usize;
    for conversation in &sync.conversations {
        if conversation.id.is_empty() {
            continue;
        }
        let Ok(chat) = TextJid::parse(&conversation.id) else {
            log::warning(&format!(
                "Failed to parse JID {}",
                log::clean(&conversation.id)
            ));
            continue;
        };
        let name = resolve_chat_name(
            shared,
            client,
            &chat,
            &conversation.id,
            Some(conversation),
            "",
        )
        .await;
        let Some(history) =
            receive::history_conversation(conversation, &chat, &own_user, chrono::Local::now())
        else {
            continue;
        };
        let writer = shared.writer();
        warn_on_error(
            "store chat",
            writer.store_chat(&conversation.id, &name, history.latest),
        );
        for row in &history.messages {
            match writer.store_message(row) {
                Ok(()) => stored += 1,
                Err(e) => log::warning(&format!(
                    "Failed to store history message: {}",
                    log::clean(&e.to_string())
                )),
            }
        }
    }
    log::line(&format!("History sync complete. Stored {stored} messages."));
}

fn warn_on_error(what: &str, result: whatsapp_mcp_store::Result<()>) {
    if let Err(e) = result {
        log::warning(&format!("Failed to {what}: {}", log::clean(&e.to_string())));
    }
}

fn print_qr(code: &str) {
    match qrcode::QrCode::new(code.as_bytes()) {
        Ok(qr) => {
            let text = qr
                .render::<qrcode::render::unicode::Dense1x2>()
                .dark_color(qrcode::render::unicode::Dense1x2::Light)
                .light_color(qrcode::render::unicode::Dense1x2::Dark)
                .build();
            // On the terminal only, as it is: it links a device to the account.
            log::terminal(&format!(
                "\nScan this QR code with your WhatsApp app:\n{text}\n"
            ));
            log::line("QR code shown on the terminal; scan it with WhatsApp to link");
        }
        Err(e) => {
            log::terminal(&format!("QR code: {}", log::clean(code)));
            log::warning(&format!(
                "Could not draw the QR code ({}); its text is shown on the terminal",
                log::clean(&e.to_string())
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_growing_queue_is_reported_once_per_tenfold() {
        let waiting = Waiting::default();
        let mut warnings = Vec::new();
        for _ in 0..12_000 {
            let before = waiting.warned_at.load(Ordering::Relaxed);
            waiting.added();
            let after = waiting.warned_at.load(Ordering::Relaxed);
            if after != before {
                warnings.push(after);
            }
        }
        assert_eq!(warnings, vec![1_000, 10_000]);
        for _ in 0..11_950 {
            waiting.taken();
        }
        assert_eq!(
            waiting.warned_at.load(Ordering::Relaxed),
            0,
            "reset once drained"
        );
        for _ in 0..1_000 {
            waiting.added();
        }
        assert_eq!(waiting.warned_at.load(Ordering::Relaxed), 1_000);
    }

    #[test]
    fn pair_phone_needs_the_country_code() {
        assert_eq!(pair_phone("+31 6-1234 5678").unwrap(), "31612345678");
        assert_eq!(pair_phone("(31) 612345678").unwrap(), "31612345678");
        assert!(
            pair_phone("0612345678")
                .unwrap_err()
                .contains("country code")
        );
        assert!(pair_phone("31x6").is_err());
        assert!(pair_phone("+").is_err());
    }

    #[test]
    fn bridge_says_what_it_runs_with() {
        let token = "s3cr3t-token-that-is-long-enough-1234";
        let config = Config::load_from(
            |name| {
                Ok(match name {
                    "WHATSAPP_BRIDGE_TOKEN" => Some(token.to_string()),
                    "WHATSAPP_PAIR_PHONE" => Some("31612345678".to_string()),
                    "WHATSAPP_SEND_ALLOWLIST" => Some("31600000001".to_string()),
                    _ => None,
                })
            },
            None,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let lines = startup_lines(&config, dir.path());
        assert_eq!(
            lines[0],
            format!("whatsapp-mcp {} bridge", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            &lines[1..10],
            [
                "Configuration file: none",
                "  bridge_token: set (from WHATSAPP_BRIDGE_TOKEN)",
                "  media_dir: not set (sending files is disabled) (default)",
                "  send_allowlist: 1 chat (from WHATSAPP_SEND_ALLOWLIST)",
                "  log_content: off (default)",
                "  pair_phone: set (from WHATSAPP_PAIR_PHONE)",
                "  log_output: stderr (default)",
                "  log_format: text (default)",
                "  log_level: info (default)",
            ]
        );
        let store = |lines: &[String]| {
            lines
                .iter()
                .find(|l| l.starts_with("Store: "))
                .cloned()
                .unwrap()
        };
        assert!(store(&lines).ends_with("(messages.db will be created)"));
        let all = lines.join("\n");
        assert!(!all.contains("s3cr3t") && !all.contains("31612345678"));

        drop(Writer::open(dir.path()).unwrap());
        assert!(store(&startup_lines(&config, dir.path())).ends_with("(messages.db found)"));

        let session = dir.path().join("session.db");
        assert!(session_line(&session, true, true).ends_with("session.db (linked)"));
        assert!(
            session_line(&session, false, true)
                .ends_with("(not linked yet: a pairing code follows)")
        );
        assert!(
            session_line(&session, false, false).ends_with("(not linked yet: a QR code follows)")
        );
    }

    /// The session holds the WhatsApp keys: it and the files SQLite keeps
    /// next to it are readable by the owner only.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_session_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let session = session_path(dir.path()).unwrap();
        let store = SqliteStore::open(&session).await.unwrap();
        let mut seen = 0;
        for name in [
            "session.db",
            "session.db-wal",
            "session.db-shm",
            "session.db-journal",
        ] {
            let path = dir.path().join(name);
            if path.exists() {
                seen += 1;
                let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{name}");
            }
        }
        // The database and its WAL files, as whatsapp-rust opens it today.
        assert!(seen >= 3, "only {seen} session files found");
        drop(store);
    }
}
