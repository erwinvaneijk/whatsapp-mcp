//! The bridge's local API: `POST /api/send`, `/api/react` and
//! `/api/download` on `127.0.0.1:8080` (docs/rust-1.0/interface.md,
//! section 3). Its only client is `whatsapp-mcp serve`, so requests are read
//! strictly: exact field names, no unknown fields, nothing after the JSON.
//! WhatsApp sits behind the [`Backend`] trait, so everything here is tested
//! without a connection.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::Body;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use whatsapp_mcp_config::SendAllowlist;

use crate::jid::{Jid, LID_SERVER, USER_SERVER};
use crate::log;

/// The largest request body accepted.
pub const MAX_BODY: usize = 1 << 20;

/// What `/api/send` asks for. Missing fields are empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SendRequest {
    pub recipient: String,
    pub message: String,
    pub media_path: String,
    pub reply_to_message_id: String,
    pub reply_to_sender_jid: String,
    pub mentions: Vec<String>,
}

/// A downloaded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub media_type: String,
    pub filename: String,
    pub path: String,
}

/// What the API needs from WhatsApp. The bridge implements it with
/// whatsapp-rust; tests use a fake.
pub trait Backend: Send + Sync + 'static {
    /// Sends a message; the flag and text for the answer.
    fn send(&self, request: SendRequest) -> impl Future<Output = (bool, String)> + Send;
    /// Adds a reaction, or removes it when `emoji` is empty.
    fn react(
        &self,
        message_id: String,
        chat_jid: String,
        emoji: String,
    ) -> impl Future<Output = (bool, String)> + Send;
    /// Downloads a message's media; the reason when that fails.
    fn download(
        &self,
        message_id: String,
        chat_jid: String,
    ) -> impl Future<Output = Result<Downloaded, String>> + Send;
    /// The other ID of a contact: its LID for a phone-number JID, its phone
    /// number for a LID, when known.
    fn counterpart(&self, jid: &Jid) -> Option<Jid>;
}

/// The API's settings and its backend.
pub struct Api<B> {
    pub token: String,
    /// Whether sending and reacting are allowed (`full_control`).
    pub full_control: bool,
    pub allowlist: Option<SendAllowlist>,
    pub log_content: bool,
    pub backend: Arc<B>,
}

type Answer = Response<Full<Bytes>>;

impl<B: Backend> Api<B> {
    /// Answers one request.
    pub async fn handle<T>(&self, request: Request<T>) -> Answer
    where
        T: Body<Data = Bytes>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        // The token first: without it, nothing else is revealed.
        let given = request
            .headers()
            .get("X-Bridge-Token")
            .map(HeaderValue::as_bytes)
            .unwrap_or_default();
        if !constant_time_eq(given, self.token.as_bytes()) {
            return text(StatusCode::UNAUTHORIZED, "Unauthorized");
        }
        let is_json = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if !is_json {
            return text(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Content-Type must be application/json",
            );
        }
        let path = request.uri().path().to_string();
        if !matches!(path.as_str(), "/api/send" | "/api/react" | "/api/download") {
            return text(StatusCode::NOT_FOUND, "Not found");
        }
        if request.method() != Method::POST {
            return text(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
        }
        if !self.full_control && path != "/api/download" {
            log::warning("Refused to send: full_control is off");
            return json(
                StatusCode::FORBIDDEN,
                &Status {
                    success: false,
                    message: "Sending is off: set full_control (or WHATSAPP_FULL_CONTROL) to yes for the bridge to send.".into(),
                },
            );
        }
        let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
            Ok(body) => body.to_bytes(),
            Err(e) if e.is::<LengthLimitError>() => {
                return invalid_request("the body is larger than 1 MiB");
            }
            Err(e) => return invalid_request(&e.to_string()),
        };
        match path.as_str() {
            "/api/send" => self.send(&body).await,
            "/api/react" => self.react(&body).await,
            _ => self.download(&body).await,
        }
    }

    async fn send(&self, body: &[u8]) -> Answer {
        let request: SendRequest = match parse(body) {
            Ok(request) => request,
            Err(reason) => return invalid_request(&reason),
        };
        if request.recipient.is_empty() {
            return text(StatusCode::BAD_REQUEST, "Recipient is required");
        }
        if request.message.is_empty() && request.media_path.is_empty() {
            return text(StatusCode::BAD_REQUEST, "Message or media path is required");
        }
        if self.log_content {
            log::line(&format!(
                "Received request to send message {} {}",
                log::clean(&request.message),
                log::clean(&request.media_path)
            ));
        } else {
            log::line("Received request to send message");
        }
        let Ok(recipient) = Jid::parse_recipient(&request.recipient) else {
            return text(StatusCode::BAD_REQUEST, "Invalid recipient");
        };
        if let Some(refusal) = self.refuse_if_not_allowed(&recipient) {
            return refusal;
        }
        let (success, message) = self.backend.send(request).await;
        log::line(&log::result_line(
            "Message sent",
            success,
            &message,
            self.log_content,
        ));
        status_json(success, &Status { success, message })
    }

    async fn react(&self, body: &[u8]) -> Answer {
        let ReactRequest {
            message_id,
            chat_jid,
            emoji,
        } = match parse(body) {
            Ok(request) => request,
            Err(reason) => return invalid_request(&reason),
        };
        if message_id.is_empty() || chat_jid.is_empty() {
            return text(
                StatusCode::BAD_REQUEST,
                "message_id and chat_jid are required",
            );
        }
        if self.log_content {
            log::line(&format!(
                "Received request to react to message {} {}",
                log::clean(&message_id),
                log::clean(&emoji)
            ));
        } else {
            log::line(&format!(
                "Received request to react to message {}",
                log::clean(&message_id)
            ));
        }
        let Ok(chat) = Jid::parse(&chat_jid) else {
            return text(StatusCode::BAD_REQUEST, "Invalid chat_jid");
        };
        if let Some(refusal) = self.refuse_if_not_allowed(&chat) {
            return refusal;
        }
        let (success, message) = self.backend.react(message_id, chat_jid, emoji).await;
        log::line(&log::result_line(
            "Reaction sent",
            success,
            &message,
            self.log_content,
        ));
        status_json(success, &Status { success, message })
    }

    async fn download(&self, body: &[u8]) -> Answer {
        let DownloadRequest {
            message_id,
            chat_jid,
        } = match parse(body) {
            Ok(request) => request,
            Err(reason) => return invalid_request(&reason),
        };
        if message_id.is_empty() || chat_jid.is_empty() {
            return text(
                StatusCode::BAD_REQUEST,
                "Message ID and Chat JID are required",
            );
        }
        match self.backend.download(message_id, chat_jid).await {
            Ok(file) => json(
                StatusCode::OK,
                &DownloadStatus {
                    success: true,
                    message: format!("Successfully downloaded {} media", file.media_type),
                    filename: Some(file.filename).filter(|f| !f.is_empty()),
                    path: Some(file.path).filter(|p| !p.is_empty()),
                },
            ),
            Err(reason) => json(
                StatusCode::INTERNAL_SERVER_ERROR,
                &DownloadStatus {
                    success: false,
                    message: format!("Failed to download media: {reason}"),
                    filename: None,
                    path: None,
                },
            ),
        }
    }

    /// A 403 answer when the chat is not in the send allowlist.
    fn refuse_if_not_allowed(&self, chat: &Jid) -> Option<Answer> {
        if allows(self.allowlist.as_ref(), self.backend.as_ref(), chat) {
            return None;
        }
        log::warning("Refused to send: chat is not in WHATSAPP_SEND_ALLOWLIST");
        Some(json(
            StatusCode::FORBIDDEN,
            &Status {
                success: false,
                message:
                    "This chat is not in WHATSAPP_SEND_ALLOWLIST, so the bridge will not send to it."
                        .into(),
            },
        ))
    }
}

/// Whether the bridge may send to `chat`. A contact can be addressed by phone
/// number or by LID; either form in the list allows both.
pub fn allows<B: Backend>(allowlist: Option<&SendAllowlist>, backend: &B, chat: &Jid) -> bool {
    let Some(list) = allowlist else {
        return true;
    };
    let chat = chat.to_non_ad();
    if list.contains(&chat.to_string()) {
        return true;
    }
    if chat.server != USER_SERVER && chat.server != LID_SERVER {
        return false;
    }
    backend
        .counterpart(&chat)
        .is_some_and(|other| list.contains(&other.to_non_ad().to_string()))
}

/// The file to send, symlinks resolved, only when
/// it lies inside the media folder.
pub fn allowed_media_path(media_dir: Option<&str>, path: &str) -> Result<PathBuf, String> {
    let Some(media_dir) = media_dir.filter(|d| !d.is_empty()) else {
        return Err("media sending is disabled: WHATSAPP_MEDIA_DIR is not set".into());
    };
    let root = std::fs::canonicalize(media_dir)
        .map_err(|e| format!("cannot resolve WHATSAPP_MEDIA_DIR: {e}"))?;
    let real =
        std::fs::canonicalize(path).map_err(|e| format!("cannot resolve media path: {e}"))?;
    if !real.starts_with(&root) {
        return Err("media path is outside WHATSAPP_MEDIA_DIR".into());
    }
    Ok(real)
}

/// Serves the API on `address` until the task is stopped.
pub async fn serve<B: Backend>(api: Arc<Api<B>>, address: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    serve_on(api, listener).await
}

/// Serves the API on a listener that is already bound (tests use port 0).
pub async fn serve_on<B: Backend>(
    api: Arc<Api<B>>,
    listener: tokio::net::TcpListener,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let api = Arc::clone(&api);
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let api = Arc::clone(&api);
                async move { Ok::<_, std::convert::Infallible>(api.handle(request).await) }
            });
            let io = hyper_util::rt::TokioIo::new(stream);
            if let Err(e) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                log::warning(&format!(
                    "REST API connection error: {}",
                    log::clean(&e.to_string())
                ));
            }
        });
    }
}

#[derive(Serialize)]
struct Status {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct DownloadStatus {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

/// What `/api/react` asks for. Missing fields are empty.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ReactRequest {
    message_id: String,
    chat_jid: String,
    /// Empty removes the reaction.
    emoji: String,
}

/// What `/api/download` asks for. Missing fields are empty.
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct DownloadRequest {
    message_id: String,
    chat_jid: String,
}

/// Reads a request body, or says what is wrong with it. The body must be a
/// JSON object: serde would also read an array into a struct, field by field
/// in order.
fn parse<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T, String> {
    if body.trim_ascii_start().first().is_some_and(|&b| b != b'{') {
        return Err("the body must be a JSON object".into());
    }
    serde_json::from_slice(body).map_err(|e| e.to_string())
}

fn invalid_request(reason: &str) -> Answer {
    text(
        StatusCode::BAD_REQUEST,
        &format!("Invalid request format: {}", log::clean(reason)),
    )
}

/// A plain-text answer, ending with a newline.
fn text(status: StatusCode, message: &str) -> Answer {
    let mut answer = Response::new(Full::new(Bytes::from(format!("{message}\n"))));
    *answer.status_mut() = status;
    let headers = answer.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    answer
}

/// 200 on success, 500 otherwise.
fn status_json(success: bool, body: &impl Serialize) -> Answer {
    let status = if success {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    json(status, body)
}

/// A compact JSON answer, ending with a newline.
fn json(status: StatusCode, body: &impl Serialize) -> Answer {
    let mut text = serde_json::to_string(body).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    let mut answer = Response::new(Full::new(Bytes::from(text)));
    *answer.status_mut() = status;
    answer
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    answer
}

/// Compares in constant time, so the token cannot be guessed byte by byte
/// from response times.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
