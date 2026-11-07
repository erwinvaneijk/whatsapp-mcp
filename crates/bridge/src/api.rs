//! The bridge's local API: `POST /api/send`, `/api/react` and
//! `/api/download` on `127.0.0.1:8080`, exactly as the Go bridge serves them
//! (docs/rust-1.0/interface.md, section 3). WhatsApp sits behind the
//! [`Backend`] trait, so everything here is tested without a connection.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Body;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use serde::Serialize;
use serde_json::Value;
use whatsapp_mcp_config::SendAllowlist;

use crate::jid::{Jid, LID_SERVER, USER_SERVER};
use crate::log;

/// The largest request body accepted, as in the Go bridge.
pub const MAX_BODY: usize = 1 << 20;

/// What `/api/send` asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
        let json = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if !json {
            return text(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Content-Type must be application/json",
            );
        }
        let path = request.uri().path().to_string();
        if !matches!(path.as_str(), "/api/send" | "/api/react" | "/api/download") {
            return text(StatusCode::NOT_FOUND, "404 page not found");
        }
        if request.method() != Method::POST {
            return text(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
        }
        // A body over the limit fails to read, like any malformed body.
        let Ok(body) = Limited::new(request.into_body(), MAX_BODY).collect().await else {
            return invalid_request();
        };
        let body = body.to_bytes();
        match path.as_str() {
            "/api/send" => self.send(&body).await,
            "/api/react" => self.react(&body).await,
            _ => self.download(&body).await,
        }
    }

    async fn send(&self, body: &[u8]) -> Answer {
        let Ok(fields) = GoJson::parse(body) else {
            return invalid_request();
        };
        let request = match (|| -> Result<SendRequest, ()> {
            Ok(SendRequest {
                recipient: fields.string("recipient")?,
                message: fields.string("message")?,
                media_path: fields.string("media_path")?,
                reply_to_message_id: fields.string("reply_to_message_id")?,
                reply_to_sender_jid: fields.string("reply_to_sender_jid")?,
                mentions: fields.strings("mentions")?,
            })
        })() {
            Ok(request) => request,
            Err(()) => return invalid_request(),
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
        let Ok(fields) = GoJson::parse(body) else {
            return invalid_request();
        };
        let (Ok(message_id), Ok(chat_jid), Ok(emoji)) = (
            fields.string("message_id"),
            fields.string("chat_jid"),
            fields.string("emoji"),
        ) else {
            return invalid_request();
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
        let Ok(fields) = GoJson::parse(body) else {
            return invalid_request();
        };
        let (Ok(message_id), Ok(chat_jid)) =
            (fields.string("message_id"), fields.string("chat_jid"))
        else {
            return invalid_request();
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
        log::line("Refused to send: chat is not in WHATSAPP_SEND_ALLOWLIST");
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

/// The Go bridge's `allowedMediaPath`: the file, symlinks resolved, only when
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
                log::line(&format!(
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

fn invalid_request() -> Answer {
    text(StatusCode::BAD_REQUEST, "Invalid request format")
}

/// A plain-text answer as Go's `http.Error` writes it.
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

/// A JSON answer as Go's `json.Encoder` writes it: compact, a newline at the
/// end, and `<`, `>`, `&`, U+2028 and U+2029 escaped.
fn json(status: StatusCode, body: &impl Serialize) -> Answer {
    let text = serde_json::to_string(body).unwrap_or_else(|_| "{}".into());
    let mut escaped = String::with_capacity(text.len() + 1);
    for c in text.chars() {
        match c {
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            c => escaped.push(c),
        }
    }
    escaped.push('\n');
    let mut answer = Response::new(Full::new(Bytes::from(escaped)));
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

/// A request body read as Go's `encoding/json` reads it into a struct: the
/// first JSON value only, unknown fields ignored. Every key that matches a
/// field, exactly or ignoring case, is applied in order, so a later one
/// overwrites an earlier one; `null` leaves the field as it is; a value of
/// the wrong type makes the whole body invalid.
struct GoJson {
    /// The object's keys and values in order, duplicates kept; `None` for a
    /// body that is `null`.
    pairs: Option<Vec<(String, Value)>>,
}

impl GoJson {
    fn parse(body: &[u8]) -> Result<GoJson, ()> {
        let first = serde_json::Deserializer::from_slice(body)
            .into_iter::<Pairs>()
            .next();
        match first {
            Some(Ok(Pairs(pairs))) => Ok(GoJson { pairs }),
            _ => Err(()),
        }
    }

    /// The values given for a field, in order.
    fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
        self.pairs
            .iter()
            .flatten()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    fn string(&self, name: &str) -> Result<String, ()> {
        let mut result = String::new();
        for value in self.values(name) {
            match value {
                Value::Null => {}
                Value::String(s) => result = s.clone(),
                _ => return Err(()),
            }
        }
        Ok(result)
    }

    fn strings(&self, name: &str) -> Result<Vec<String>, ()> {
        let mut result = Vec::new();
        for value in self.values(name) {
            match value {
                Value::Null => {}
                Value::Array(items) => {
                    result = items
                        .iter()
                        .map(|item| match item {
                            Value::String(s) => Ok(s.clone()),
                            Value::Null => Ok(String::new()),
                            _ => Err(()),
                        })
                        .collect::<Result<_, _>>()?;
                }
                _ => return Err(()),
            }
        }
        Ok(result)
    }
}

/// A JSON object as its key and value pairs in order, duplicates kept, or
/// `null`. Anything else does not decode into a struct in Go.
struct Pairs(Option<Vec<(String, Value)>>);

impl<'de> serde::Deserialize<'de> for Pairs {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Pairs;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object or null")
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Pairs, E> {
                Ok(Pairs(None))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Pairs, A::Error> {
                let mut pairs = Vec::new();
                while let Some(pair) = map.next_entry::<String, Value>()? {
                    pairs.push(pair);
                }
                Ok(Pairs(Some(pairs)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
