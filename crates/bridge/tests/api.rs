//! The local API, against docs/rust-1.0/interface.md section 3, row by row,
//! with a fake WhatsApp. Allowlist tests ported from
//! whatsapp-bridge/allowlist_test.go.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use whatsapp_mcp_bridge::{Api, Backend, Downloaded, Jid, SendRequest, allowed_media_path, allows};
use whatsapp_mcp_config::SendAllowlist;

const TOKEN: &str = "tttttttttttttttttttttttttttttttt";

/// A fake WhatsApp that records what it was asked and answers as set.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    sent: Mutex<Vec<SendRequest>>,
    fail: bool,
    /// LID user <-> phone number.
    lids: HashMap<String, String>,
}

impl Backend for Fake {
    async fn send(&self, request: SendRequest) -> (bool, String) {
        self.calls.lock().unwrap().push("send".into());
        self.sent.lock().unwrap().push(request);
        if self.fail {
            (false, "Error sending message: not connected".into())
        } else {
            (true, "Message sent to Maria".into())
        }
    }

    async fn react(&self, _id: String, _chat: String, emoji: String) -> (bool, String) {
        self.calls.lock().unwrap().push("react".into());
        if emoji.is_empty() {
            (true, "Reaction removed".into())
        } else {
            (true, format!("Reaction {emoji} sent"))
        }
    }

    async fn download(&self, id: String, _chat: String) -> Result<Downloaded, String> {
        self.calls.lock().unwrap().push("download".into());
        if id == "NOMEDIA" {
            return Err("message has no media".into());
        }
        Ok(Downloaded {
            media_type: "image".into(),
            filename: "IMG-1.jpg".into(),
            path: "/store/c/IMG-1.jpg".into(),
        })
    }

    fn counterpart(&self, jid: &Jid) -> Option<Jid> {
        match jid.server.as_str() {
            "lid" => self
                .lids
                .get(&jid.user)
                .map(|pn| Jid::new(pn.clone(), "s.whatsapp.net")),
            "s.whatsapp.net" => self
                .lids
                .iter()
                .find(|(_, pn)| **pn == jid.user)
                .map(|(lid, _)| Jid::new(lid.clone(), "lid")),
            _ => None,
        }
    }
}

fn api(fake: Fake, allowlist: &str) -> Api<Fake> {
    Api {
        token: TOKEN.into(),
        allowlist: SendAllowlist::parse(allowlist).unwrap(),
        log_content: false,
        backend: Arc::new(fake),
    }
}

struct Reply {
    status: u16,
    content_type: String,
    body: String,
}

async fn call(
    api: &Api<Fake>,
    method: Method,
    path: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: &str,
) -> Reply {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("X-Bridge-Token", token);
    }
    if let Some(ct) = content_type {
        request = request.header("Content-Type", ct);
    }
    let request = request
        .body(Full::new(Bytes::from(body.to_string())))
        .unwrap();
    let answer = api.handle(request).await;
    let status = answer.status().as_u16();
    let content_type = answer
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let body = answer.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        content_type,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

async fn post(api: &Api<Fake>, path: &str, body: &str) -> Reply {
    call(
        api,
        Method::POST,
        path,
        Some(TOKEN),
        Some("application/json"),
        body,
    )
    .await
}

#[tokio::test]
async fn token_is_required_and_checked_first() {
    let a = api(Fake::default(), "");
    for token in [None, Some("wrong"), Some("")] {
        let r = call(
            &a,
            Method::POST,
            "/api/send",
            token,
            Some("application/json"),
            "{}",
        )
        .await;
        assert_eq!(
            (r.status, r.body.as_str()),
            (401, "Unauthorized\n"),
            "{token:?}"
        );
    }
    // Before routing: an unknown path without the token is 401, not 404.
    let r = call(&a, Method::GET, "/nothing", None, None, "").await;
    assert_eq!(r.status, 401);
    assert!(a.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn json_content_type_is_required() {
    let a = api(Fake::default(), "");
    for ct in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let r = call(&a, Method::POST, "/api/send", Some(TOKEN), ct, "{}").await;
        assert_eq!(
            (r.status, r.body.as_str()),
            (415, "Content-Type must be application/json\n"),
            "{ct:?}"
        );
    }
    let r = call(
        &a,
        Method::POST,
        "/api/send",
        Some(TOKEN),
        Some("application/json; charset=utf-8"),
        r#"{"recipient":"316","message":"hi"}"#,
    )
    .await;
    assert_eq!(r.status, 200);
}

#[tokio::test]
async fn unknown_paths_and_methods() {
    let a = api(Fake::default(), "");
    let r = post(&a, "/api/other", "{}").await;
    assert_eq!((r.status, r.body.as_str()), (404, "404 page not found\n"));
    let r = post(&a, "/api/send/", "{}").await;
    assert_eq!(r.status, 404);
    for path in ["/api/send", "/api/react", "/api/download"] {
        let r = call(
            &a,
            Method::GET,
            path,
            Some(TOKEN),
            Some("application/json"),
            "",
        )
        .await;
        assert_eq!(
            (r.status, r.body.as_str()),
            (405, "Method not allowed\n"),
            "{path}"
        );
    }
}

#[tokio::test]
async fn plain_text_errors() {
    let a = api(Fake::default(), "");
    assert_eq!(
        post(&a, "/api/send", "").await.content_type,
        "text/plain; charset=utf-8"
    );
    for (path, body, want) in [
        ("/api/send", "", "Invalid request format"),
        ("/api/send", "[1]", "Invalid request format"),
        ("/api/send", r#"{"recipient": 5}"#, "Invalid request format"),
        (
            "/api/send",
            r#"{"mentions": "x"}"#,
            "Invalid request format",
        ),
        ("/api/send", r#"{"message":"hi"}"#, "Recipient is required"),
        ("/api/send", "null", "Recipient is required"),
        (
            "/api/send",
            r#"{"recipient":"316"}"#,
            "Message or media path is required",
        ),
        (
            "/api/send",
            r#"{"recipient":"1:x@s.whatsapp.net","message":"hi"}"#,
            "Invalid recipient",
        ),
        (
            "/api/react",
            r#"{"message_id":"M1"}"#,
            "message_id and chat_jid are required",
        ),
        (
            "/api/react",
            r#"{"message_id":"M1","chat_jid":"1:x@s.whatsapp.net"}"#,
            "Invalid chat_jid",
        ),
        (
            "/api/download",
            r#"{"chat_jid":"c@s.whatsapp.net"}"#,
            "Message ID and Chat JID are required",
        ),
        ("/api/download", "{", "Invalid request format"),
    ] {
        let r = post(&a, path, body).await;
        assert_eq!(
            (r.status, r.body.as_str()),
            (400, format!("{want}\n").as_str()),
            "{path} {body}"
        );
    }
    assert!(a.backend.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn body_over_one_mebibyte_is_refused() {
    let a = api(Fake::default(), "");
    let big = format!(
        r#"{{"recipient":"316","message":"{}"}}"#,
        "x".repeat(1 << 20)
    );
    let r = post(&a, "/api/send", &big).await;
    assert_eq!(
        (r.status, r.body.as_str()),
        (400, "Invalid request format\n")
    );
    let fits = format!(
        r#"{{"recipient":"316","message":"{}"}}"#,
        "x".repeat((1 << 20) - 100)
    );
    assert_eq!(post(&a, "/api/send", &fits).await.status, 200);
}

#[tokio::test]
async fn bodies_are_read_as_go_reads_them() {
    let a = api(Fake::default(), "");
    // Unknown fields ignored, names matched ignoring case, null leaves a
    // field empty, only the first JSON value counts.
    let r = post(
        &a,
        "/api/send",
        r#"{"Recipient":"316","message":"hi","extra":1,"media_path":null,"mentions":["31600000001"]} trailing"#,
    )
    .await;
    assert_eq!(r.status, 200);
    let sent = a.backend.sent.lock().unwrap()[0].clone();
    assert_eq!(
        sent,
        SendRequest {
            recipient: "316".into(),
            message: "hi".into(),
            mentions: vec!["31600000001".into()],
            ..Default::default()
        }
    );
}

#[tokio::test]
async fn json_answers() {
    let a = api(Fake::default(), "");
    let r = post(&a, "/api/send", r#"{"recipient":"316","message":"hi"}"#).await;
    assert_eq!(r.content_type, "application/json");
    assert_eq!(
        r.body,
        "{\"success\":true,\"message\":\"Message sent to Maria\"}\n"
    );
    let r = post(
        &a,
        "/api/react",
        r#"{"message_id":"M1","chat_jid":"c@s.whatsapp.net","emoji":"❤️"}"#,
    )
    .await;
    assert_eq!(
        (r.status, r.body.as_str()),
        (200, "{\"success\":true,\"message\":\"Reaction ❤️ sent\"}\n")
    );
    let r = post(
        &a,
        "/api/react",
        r#"{"message_id":"M1","chat_jid":"c@s.whatsapp.net","emoji":""}"#,
    )
    .await;
    assert_eq!(
        r.body,
        "{\"success\":true,\"message\":\"Reaction removed\"}\n"
    );
    let r = post(
        &a,
        "/api/download",
        r#"{"message_id":"M1","chat_jid":"c@s.whatsapp.net"}"#,
    )
    .await;
    assert_eq!(
        (r.status, r.body.as_str()),
        (
            200,
            "{\"success\":true,\"message\":\"Successfully downloaded image media\",\"filename\":\"IMG-1.jpg\",\"path\":\"/store/c/IMG-1.jpg\"}\n"
        )
    );
    let r = post(
        &a,
        "/api/download",
        r#"{"message_id":"NOMEDIA","chat_jid":"c@s.whatsapp.net"}"#,
    )
    .await;
    assert_eq!(
        (r.status, r.body.as_str()),
        (
            500,
            "{\"success\":false,\"message\":\"Failed to download media: message has no media\"}\n"
        )
    );
}

#[tokio::test]
async fn failed_send_is_500() {
    let a = api(
        Fake {
            fail: true,
            ..Default::default()
        },
        "",
    );
    let r = post(&a, "/api/send", r#"{"recipient":"316","message":"hi"}"#).await;
    assert_eq!(
        (r.status, r.body.as_str()),
        (
            500,
            "{\"success\":false,\"message\":\"Error sending message: not connected\"}\n"
        )
    );
}

#[tokio::test]
async fn html_characters_are_escaped_as_go_does() {
    let a = api(Fake::default(), "");
    let r = post(
        &a,
        "/api/react",
        r#"{"message_id":"M1","chat_jid":"c@s.whatsapp.net","emoji":"<&>"}"#,
    )
    .await;
    assert_eq!(
        r.body,
        "{\"success\":true,\"message\":\"Reaction \\u003c\\u0026\\u003e sent\"}\n"
    );
}

/// From allowlist_test.go: TestAPIRefusesUnlistedChats.
#[tokio::test]
async fn unlisted_chats_are_refused_before_sending() {
    let a = api(Fake::default(), "31612345678");
    for (path, body) in [
        ("/api/send", r#"{"recipient":"31699999999","message":"hi"}"#),
        (
            "/api/send",
            r#"{"recipient":"120363000000000000@g.us","message":"hi"}"#,
        ),
        (
            "/api/react",
            r#"{"message_id":"M1","chat_jid":"31699999999@s.whatsapp.net","emoji":"👍"}"#,
        ),
    ] {
        let r = post(&a, path, body).await;
        assert_eq!(r.status, 403, "{path} {body}");
        assert_eq!(
            r.body,
            "{\"success\":false,\"message\":\"This chat is not in WHATSAPP_SEND_ALLOWLIST, so the bridge will not send to it.\"}\n"
        );
    }
    assert!(a.backend.calls.lock().unwrap().is_empty());
    assert_eq!(
        post(
            &a,
            "/api/send",
            r#"{"recipient":"31612345678","message":"hi"}"#
        )
        .await
        .status,
        200
    );
}

/// From allowlist_test.go: TestSendAllowlistAllows.
#[test]
fn allowlist_matches_numbers_and_lids() {
    let fake = Fake {
        lids: HashMap::from([("123456789012345".to_string(), "31612345678".to_string())]),
        ..Default::default()
    };
    let pn = Jid::new("31612345678", "s.whatsapp.net");
    let lid = Jid::new("123456789012345", "lid");
    let group = Jid::new("120363000000000000", "g.us");
    let other = Jid::new("31699999999", "s.whatsapp.net");
    let mut device = pn.clone();
    device.device = 2;

    assert!(
        allows(None, &fake, &other),
        "no allowlist allows everything"
    );

    let by_number = SendAllowlist::parse("31612345678, 120363000000000000@g.us").unwrap();
    for jid in [&pn, &device, &lid, &group] {
        assert!(allows(by_number.as_ref(), &fake, jid), "{jid}");
    }
    assert!(!allows(by_number.as_ref(), &fake, &other));
    let no_map = Fake::default();
    assert!(
        !allows(by_number.as_ref(), &no_map, &lid),
        "without a LID mapping the LID cannot be matched to the number"
    );

    let by_lid = SendAllowlist::parse("123456789012345@lid").unwrap();
    assert!(
        allows(by_lid.as_ref(), &fake, &pn),
        "listed by LID: the number is allowed too"
    );
    assert!(!allows(by_lid.as_ref(), &fake, &other));
}

#[test]
fn media_paths_must_be_inside_the_media_folder() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("a.jpg"), b"x").unwrap();
    std::fs::write(dir.path().join("outside.txt"), b"x").unwrap();
    let m = media.to_str().unwrap();
    let ok = allowed_media_path(Some(m), media.join("a.jpg").to_str().unwrap()).unwrap();
    assert!(ok.ends_with("a.jpg"));
    let err = |dir: Option<&str>, path: &std::path::Path| {
        allowed_media_path(dir, path.to_str().unwrap()).unwrap_err()
    };
    assert_eq!(
        err(None, &media.join("a.jpg")),
        "media sending is disabled: WHATSAPP_MEDIA_DIR is not set"
    );
    assert_eq!(
        err(Some(m), &media.join("..").join("outside.txt")),
        "media path is outside WHATSAPP_MEDIA_DIR"
    );
    assert!(err(Some(m), &media.join("missing.jpg")).starts_with("cannot resolve media path: "));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("outside.txt"), media.join("link")).unwrap();
        assert_eq!(
            err(Some(m), &media.join("link")),
            "media path is outside WHATSAPP_MEDIA_DIR"
        );
    }
}

/// The API served over a real socket, as the MCP server reaches it.
#[tokio::test]
async fn serves_over_http() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let api = Arc::new(api(Fake::default(), ""));
    tokio::spawn(whatsapp_mcp_bridge::api::serve_on(api, listener));
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = r#"{"recipient":"316","message":"hi"}"#;
    let request = format!(
        "POST /api/send HTTP/1.1\r\nHost: x\r\nX-Bridge-Token: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 OK\r\n"), "{answer}");
    assert!(
        answer.ends_with("{\"success\":true,\"message\":\"Message sent to Maria\"}\n"),
        "{answer}"
    );
}
