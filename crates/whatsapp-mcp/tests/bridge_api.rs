//! `serve` and the bridge's API together: every tool that goes through the
//! bridge, called from the real `serve` against the real API with a fake
//! WhatsApp. The API reads requests strictly (no unknown fields, exact
//! names), so this catches a request `serve` builds that the bridge refuses.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use whatsapp_mcp_bridge::{Api, Backend, Downloaded, Jid, SendRequest};

const TOKEN: &str = "tttttttttttttttttttttttttttttttt";

#[derive(Default)]
struct Fake {
    sent: Mutex<Vec<SendRequest>>,
    reactions: Mutex<Vec<(String, String, String)>>,
    downloads: Mutex<Vec<(String, String)>>,
}

impl Backend for Fake {
    async fn send(&self, request: SendRequest) -> (bool, String) {
        self.sent.lock().unwrap().push(request);
        (true, "Message sent".into())
    }

    async fn react(&self, id: String, chat: String, emoji: String) -> (bool, String) {
        self.reactions.lock().unwrap().push((id, chat, emoji));
        (true, "Reaction sent".into())
    }

    async fn download(&self, id: String, chat: String) -> Result<Downloaded, String> {
        self.downloads.lock().unwrap().push((id, chat));
        Ok(Downloaded {
            media_type: "image".into(),
            filename: "IMG-1.jpg".into(),
            path: "/store/c/IMG-1.jpg".into(),
        })
    }

    fn counterpart(&self, _jid: &Jid) -> Option<Jid> {
        None
    }
}

/// Calls `tools/call` for each `(name, arguments)` and returns the answers,
/// one JSON-RPC line each.
fn call_tools(
    bridge_url: &str,
    media_dir: &std::path::Path,
    calls: &[(&str, &str)],
) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.yaml");
    std::fs::write(
        &config,
        format!(
            "bridge_token: {TOKEN}\nfull_control: yes\nmedia_dir: '{}'\nstore_dir: '{}'\n",
            media_dir.display(),
            dir.path().display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_whatsapp-mcp"))
        .args(["serve", "--bridge-url", bridge_url])
        .env("WHATSAPP_MCP_CONFIG", &config)
        .env_remove("WHATSAPP_FULL_CONTROL")
        .env_remove("WHATSAPP_MCP_READ_ONLY")
        .env_remove("WHATSAPP_BRIDGE_TOKEN")
        .env_remove("WHATSAPP_MEDIA_DIR")
        .env_remove("WHATSAPP_STORE_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut input = String::from(concat!(
        r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
    ));
    for (i, (name, arguments)) in calls.iter().enumerate() {
        input.push_str(&format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{name}","arguments":{arguments}}}}}"#,
            i + 1
        ));
        input.push('\n');
    }
    stdin.write_all(input.as_bytes()).unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut answers = vec![String::new(); calls.len()];
    let mut missing = calls.len();
    while missing > 0 {
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "serve stopped");
        for (i, answer) in answers.iter_mut().enumerate() {
            if answer.is_empty() && line.contains(&format!(r#""id":{}"#, i + 1)) {
                *answer = line.clone();
                missing -= 1;
            }
        }
    }
    drop(stdin);
    child.wait().unwrap();
    answers
}

#[test]
fn every_bridge_tool_is_accepted_by_the_bridge() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let fake = Arc::new(Fake::default());
    let api = Arc::new(Api {
        token: TOKEN.into(),
        full_control: true,
        allowlist: None,
        log_content: false,
        backend: Arc::clone(&fake),
    });
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    runtime.spawn(whatsapp_mcp_bridge::api::serve_on(api, listener));

    let media = tempfile::tempdir().unwrap();
    let media_dir = media.path().to_path_buf();
    let photo = media_dir.join("photo.jpg");
    let voice = media_dir.join("voice.ogg");
    std::fs::write(&photo, b"x").unwrap();
    // The first Ogg page of an Opus file, so serve sends it without
    // converting it (and without needing ffmpeg).
    let mut opus = b"OggS".to_vec();
    opus.extend([0u8; 22]);
    opus.extend([1, 19]);
    opus.extend(b"OpusHead");
    opus.extend([1, 1, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]);
    std::fs::write(&voice, opus).unwrap();
    let json_path = |p: &std::path::Path| serde_json::to_string(p.to_str().unwrap()).unwrap();

    let send_file = format!(
        r#"{{"recipient":"31612345678","media_path":{},"caption":"look"}}"#,
        json_path(&photo)
    );
    let send_audio = format!(
        r#"{{"recipient":"31612345678","media_path":{}}}"#,
        json_path(&voice)
    );
    let calls = [
        (
            "send_message",
            r#"{"recipient":"31612345678","message":"hi","reply_to_message_id":"M1","reply_to_sender_jid":"31600000002@s.whatsapp.net","mentions":["31600000001"]}"#,
        ),
        (
            "send_message",
            r#"{"recipient":"31612345678","message":"plain"}"#,
        ),
        ("send_file", send_file.as_str()),
        ("send_audio_message", send_audio.as_str()),
        (
            "react_to_message",
            r#"{"message_id":"M1","chat_jid":"31612345678@s.whatsapp.net","emoji":"👍"}"#,
        ),
        (
            "download_media",
            r#"{"message_id":"M1","chat_jid":"31612345678@s.whatsapp.net"}"#,
        ),
    ];
    let answers = call_tools(&url, &media_dir, &calls);
    for ((name, _), answer) in calls.iter().zip(&answers) {
        assert!(
            answer.contains(r#"\"success\": true"#) || answer.contains(r#"\"success\":true"#),
            "{name}: {answer}"
        );
    }

    // serve runs calls concurrently, so they can arrive in any order.
    let sent = fake.sent.lock().unwrap();
    assert_eq!(sent.len(), 4);
    let find = |pred: &dyn Fn(&SendRequest) -> bool| {
        sent.iter()
            .find(|r| pred(r))
            .cloned()
            .expect("request not sent")
    };
    assert_eq!(
        find(&|r| r.message == "hi"),
        SendRequest {
            recipient: "31612345678".into(),
            message: "hi".into(),
            media_path: String::new(),
            reply_to_message_id: "M1".into(),
            reply_to_sender_jid: "31600000002@s.whatsapp.net".into(),
            mentions: vec!["31600000001".into()],
        }
    );
    assert_eq!(
        find(&|r| r.media_path.ends_with("photo.jpg")).message,
        "look"
    );
    assert_eq!(
        find(&|r| r.media_path.ends_with("voice.ogg")).media_path,
        voice.to_str().unwrap()
    );
    assert_eq!(fake.reactions.lock().unwrap().len(), 1);
    assert_eq!(fake.downloads.lock().unwrap().len(), 1);
}
