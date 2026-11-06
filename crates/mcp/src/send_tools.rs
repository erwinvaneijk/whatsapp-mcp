//! The tools that go through the bridge: send_message, send_file,
//! send_audio_message, react_to_message and download_media. Same checks,
//! requests and results as the Python server, except that a failure now
//! gives the bridge's reason (see `docs/rust-1.0/interface.md`, section 5).

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use crate::args::Args;
use crate::bridge::{Answer, Bridge};
use crate::output::{Output, ToolError};

/// What the send tools need besides their arguments.
pub struct SendContext<'a> {
    pub bridge: &'a Bridge,
    /// The only folder files may be sent from; `None` refuses all files.
    pub media_dir: Option<&'a str>,
}

/// Runs a send tool. `None` when `name` is not one.
pub fn call(
    ctx: &SendContext<'_>,
    name: &str,
    args: &Args<'_>,
) -> Option<Result<Output, ToolError>> {
    Some(match name {
        "send_message" => send_message(ctx, args),
        "send_file" => send_file(ctx, args),
        "send_audio_message" => send_audio_message(ctx, args),
        "react_to_message" => react_to_message(ctx, args),
        "download_media" => download_media(ctx, args),
        _ => return None,
    })
}

fn status(success: bool, message: impl Into<String>) -> Result<Output, ToolError> {
    Output::object(&json!({ "success": success, "message": message.into() }))
}

fn send_message(ctx: &SendContext<'_>, args: &Args<'_>) -> Result<Output, ToolError> {
    let recipient = args.string("recipient")?;
    let message = args.string("message")?;
    let reply_to_message_id = args.opt_string("reply_to_message_id")?;
    let reply_to_sender_jid = args.opt_string("reply_to_sender_jid")?;
    let mentions = args.opt_string_list("mentions")?;
    if recipient.is_empty() {
        return status(false, "Recipient must be provided");
    }
    let mut payload = Map::new();
    payload.insert("recipient".into(), recipient.into());
    payload.insert("message".into(), message.into());
    // Quote-reply (upstream #121): the sender's JID is needed in groups.
    if let Some(id) = reply_to_message_id.filter(|v| !v.is_empty()) {
        payload.insert("reply_to_message_id".into(), id.into());
    }
    if let Some(jid) = reply_to_sender_jid.filter(|v| !v.is_empty()) {
        payload.insert("reply_to_sender_jid".into(), jid.into());
    }
    if let Some(mentions) = mentions.filter(|m| !m.is_empty()) {
        payload.insert("mentions".into(), mentions.into());
    }
    let (success, message) = send_result(ctx.bridge.post("send", &payload.into()), "message");
    status(success, message)
}

fn send_file(ctx: &SendContext<'_>, args: &Args<'_>) -> Result<Output, ToolError> {
    let recipient = args.string("recipient")?;
    let media_path = args.string("media_path")?;
    let caption = args.string_or("caption", "")?;
    if let Err(problem) = check_file(ctx, &recipient, &media_path) {
        return status(false, problem);
    }
    // The bridge uses "message" as the caption for images, videos and documents.
    let payload = json!({ "recipient": recipient, "media_path": media_path, "message": caption });
    let (success, message) = send_result(ctx.bridge.post("send", &payload), "message");
    status(success, message)
}

fn send_audio_message(ctx: &SendContext<'_>, args: &Args<'_>) -> Result<Output, ToolError> {
    let recipient = args.string("recipient")?;
    let media_path = args.string("media_path")?;
    if let Err(problem) = check_file(ctx, &recipient, &media_path) {
        return status(false, problem);
    }
    // Anything but Ogg is converted to Opus in Ogg first, in a temporary
    // file in the media folder (so the bridge may read it), removed after.
    let converted = if media_path.ends_with(".ogg") {
        None
    } else {
        match convert_to_opus_ogg(&media_path, ctx.media_dir.unwrap_or(".")) {
            Ok(file) => Some(file),
            Err(e) => {
                return status(
                    false,
                    format!(
                        "Error converting file to opus ogg. You likely need to install ffmpeg: {e}"
                    ),
                );
            }
        }
    };
    let path = match &converted {
        Some(file) => file.to_string_lossy().into_owned(),
        None => media_path,
    };
    let payload = json!({ "recipient": recipient, "media_path": path });
    let (success, message) = send_result(ctx.bridge.post("send", &payload), "message");
    // Dropping the temporary file removes it; that cannot change the result.
    drop(converted);
    status(success, message)
}

fn react_to_message(ctx: &SendContext<'_>, args: &Args<'_>) -> Result<Output, ToolError> {
    let message_id = args.string("message_id")?;
    let chat_jid = args.string("chat_jid")?;
    let emoji = args.string("emoji")?;
    if message_id.is_empty() {
        return status(false, "Message ID must be provided");
    }
    if chat_jid.is_empty() {
        return status(false, "Chat JID must be provided");
    }
    let payload = json!({ "message_id": message_id, "chat_jid": chat_jid, "emoji": emoji });
    let (success, message) = send_result(ctx.bridge.post("react", &payload), "reaction");
    status(success, message)
}

fn download_media(ctx: &SendContext<'_>, args: &Args<'_>) -> Result<Output, ToolError> {
    let message_id = args.string("message_id")?;
    let chat_jid = args.string("chat_jid")?;
    let payload = json!({ "message_id": message_id, "chat_jid": chat_jid });
    match download_result(ctx.bridge.post("download", &payload)) {
        Ok(path) => {
            eprintln!("Media downloaded successfully: {path}");
            Output::object(&json!({
                "success": true,
                "message": "Media downloaded successfully",
                "file_path": path,
            }))
        }
        Err(reason) => {
            eprintln!("Download failed: {reason}");
            let message = if reason.starts_with("Failed to download media") {
                reason
            } else {
                format!("Failed to download media: {reason}")
            };
            status(false, message)
        }
    }
}

/// The checks before sending a file, in the Python server's order.
fn check_file(ctx: &SendContext<'_>, recipient: &str, media_path: &str) -> Result<(), String> {
    if recipient.is_empty() {
        return Err("Recipient must be provided".into());
    }
    if media_path.is_empty() {
        return Err("Media path must be provided".into());
    }
    if !inside_media_dir(ctx.media_dir, media_path) {
        return Err("Media path must be an absolute path inside WHATSAPP_MEDIA_DIR".into());
    }
    if !Path::new(media_path).is_file() {
        return Err(format!("Media file not found: {media_path}"));
    }
    Ok(())
}

/// True only for an absolute path that resolves inside the media folder,
/// with symlinks and `..` resolved, so no path can escape it.
pub fn inside_media_dir(media_dir: Option<&str>, path: &str) -> bool {
    let Some(media_dir) = media_dir.filter(|d| !d.is_empty()) else {
        return false;
    };
    if !Path::new(path).is_absolute() {
        return false;
    }
    let root = real_path(Path::new(media_dir));
    real_path(Path::new(path)).starts_with(&root)
}

/// Like Python's `os.path.realpath`: symlinks resolved as far as the path
/// exists, `.` and `..` resolved after that.
fn real_path(path: &Path) -> PathBuf {
    let components: Vec<Component<'_>> = path.components().collect();
    // The longest prefix that exists, resolved by the file system.
    for split in (0..=components.len()).rev() {
        let prefix: PathBuf = components[..split].iter().collect();
        if prefix.as_os_str().is_empty() {
            continue;
        }
        if let Ok(mut resolved) = std::fs::canonicalize(&prefix) {
            for component in &components[split..] {
                match component {
                    Component::ParentDir => {
                        resolved.pop();
                    }
                    Component::CurDir => {}
                    other => resolved.push(other.as_os_str()),
                }
            }
            return resolved;
        }
    }
    path.to_path_buf()
}

/// Converts audio to Opus in an Ogg container with ffmpeg, as the Python
/// server does: 32 kbit/s, 24 kHz, tuned for voice.
fn convert_to_opus_ogg(input: &str, dir: &str) -> Result<tempfile::TempPath, String> {
    let file = tempfile::Builder::new()
        .prefix("tmp")
        .suffix(".ogg")
        .tempfile_in(dir)
        .map_err(|e| format!("cannot create a temporary file in {dir}: {e}"))?
        .into_temp_path();
    let output = Command::new("ffmpeg")
        .args(["-i", input])
        .args(["-c:a", "libopus", "-b:a", "32k", "-ar", "24000"])
        .args(["-application", "voip", "-vbr", "on"])
        .args(["-compression_level", "10", "-frame_duration", "60", "-y"])
        .arg(file.as_os_str())
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    if !output.status.success() {
        // The caller already suggests installing ffmpeg; say what it reported.
        return Err(format!(
            "ffmpeg could not convert the audio: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(file)
}

/// The success flag and message for a send or reaction.
fn send_result(answer: Answer, what: &str) -> (bool, String) {
    match answer {
        Answer::Reply {
            status: 200,
            json: Some(json),
            ..
        } => (
            json.get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            match json.get("message") {
                Some(Value::String(message)) => message.clone(),
                Some(other) => other.to_string(),
                None => "Unknown response".to_string(),
            },
        ),
        Answer::Reply { status: 200, .. } => (
            false,
            "The bridge's answer could not be read: it is not JSON.".to_string(),
        ),
        Answer::Reply { status, json, text } => (false, bridge_error(status, json.as_ref(), &text)),
        Answer::Timeout => (
            false,
            format!(
                "Timed out waiting for the WhatsApp bridge. The {what} may still have been sent, so check the chat before retrying."
            ),
        ),
        Answer::Failed(e) => (false, format!("Request error: {e}")),
    }
}

/// The file path of a download, or why there is none.
fn download_result(answer: Answer) -> Result<String, String> {
    match answer {
        Answer::Reply {
            status: 200,
            json: Some(json),
            ..
        } => {
            if json.get("success").and_then(Value::as_bool) == Some(true) {
                json.get("path")
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| "the bridge gave no file path".to_string())
            } else {
                Err(json
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Unknown error")
                    .to_string())
            }
        }
        Answer::Reply { status: 200, .. } => {
            Err("the bridge's answer could not be read: it is not JSON".to_string())
        }
        Answer::Reply { status, json, text } => Err(bridge_error(status, json.as_ref(), &text)),
        Answer::Timeout => Err("timed out waiting for the WhatsApp bridge".to_string()),
        Answer::Failed(e) => Err(format!("Request error: {e}")),
    }
}

/// An error status from the bridge: its own message when it gave one, else
/// the status and the text of the answer.
fn bridge_error(status: u16, json: Option<&Map<String, Value>>, text: &str) -> String {
    match json
        .and_then(|j| j.get("message"))
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
    {
        Some(message) => message.to_string(),
        None => format!("Bridge error (HTTP {status}): {}", text.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_dir_checks() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(media.join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("outside.txt"), b"x").unwrap();
        let m = media.to_str().unwrap();
        let inside = |p: &Path| inside_media_dir(Some(m), p.to_str().unwrap());
        assert!(inside(&media.join("a.jpg")));
        assert!(inside(&media.join("new.jpg"))); // need not exist yet
        assert!(!inside(&dir.path().join("outside.txt")));
        assert!(!inside(&media.join("..").join("outside.txt")));
        assert!(!inside(
            &media.join("x").join("..").join("..").join("outside.txt")
        ));
        assert!(!inside_media_dir(Some(m), "a.jpg")); // relative
        assert!(!inside_media_dir(
            None,
            media.join("a.jpg").to_str().unwrap()
        ));
        // A folder that only shares the prefix is outside.
        let media2 = dir.path().join("media2");
        std::fs::create_dir(&media2).unwrap();
        assert!(!inside(&media2.join("b.jpg")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_media_dir_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(dir.path().join("secret"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret"), media.join("link")).unwrap();
        assert!(!inside_media_dir(
            media.to_str(),
            media.join("link").to_str().unwrap()
        ));
    }

    fn temp_oggs(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                let name = e.as_ref().unwrap().file_name();
                let name = name.to_string_lossy();
                name.starts_with("tmp") && name.ends_with(".ogg")
            })
            .count()
    }

    #[test]
    fn failed_conversion_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.m4a");
        let result = convert_to_opus_ogg(missing.to_str().unwrap(), dir.path().to_str().unwrap());
        assert!(result.is_err());
        assert_eq!(temp_oggs(dir.path()), 0);
    }

    #[test]
    fn converts_with_ffmpeg_and_cleans_up() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg is not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // A tenth of a second of silence as WAV: 16-bit mono, 8 kHz.
        let samples = 800u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples * 2).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&16000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples * 2).to_le_bytes());
        wav.resize(wav.len() + (samples * 2) as usize, 0);
        let input = dir.path().join("voice.wav");
        std::fs::write(&input, wav).unwrap();

        let converted =
            convert_to_opus_ogg(input.to_str().unwrap(), dir.path().to_str().unwrap()).unwrap();
        let header = std::fs::read(&*converted).unwrap();
        assert!(header.starts_with(b"OggS"), "not an Ogg file");
        assert_eq!(temp_oggs(dir.path()), 1);
        drop(converted);
        assert_eq!(temp_oggs(dir.path()), 0);
    }

    #[test]
    fn bridge_answers() {
        let reply = |status, body: &str| Answer::Reply {
            status,
            json: match serde_json::from_str(body) {
                Ok(Value::Object(m)) => Some(m),
                _ => None,
            },
            text: body.to_string(),
        };
        assert_eq!(
            send_result(
                reply(200, r#"{"success":true,"message":"Message sent"}"#),
                "message"
            ),
            (true, "Message sent".to_string())
        );
        assert_eq!(
            send_result(
                reply(403, r#"{"success":false,"message":"Not allowed"}"#),
                "message"
            )
            .1,
            "Not allowed"
        );
        assert_eq!(
            send_result(reply(400, "Bad request\n"), "message").1,
            "Bridge error (HTTP 400): Bad request"
        );
        assert!(!send_result(reply(200, "not json"), "message").0);
        assert_eq!(
            download_result(reply(200, r#"{"success":true,"path":"/x.jpg"}"#)),
            Ok("/x.jpg".to_string())
        );
        assert_eq!(
            download_result(reply(500, r#"{"success":false,"message":"no media"}"#)),
            Err("no media".to_string())
        );
    }
}
