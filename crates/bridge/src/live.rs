//! The API's backend on the WhatsApp connection: sending text and files,
//! reactions, and downloading media, with the Go bridge's answers. Ported
//! from `sendWhatsAppMessage`, `sendReaction`, `downloadMedia` and
//! `media_retry.go`; the plain parts are in [`crate::send`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use whatsapp_mcp_store::{ensure_private_dir, ensure_private_file};
use whatsapp_rust::download::{ClientDownloadError, DownloadLimits, DownloadParams, MediaType};
use whatsapp_rust::prelude::*;
use whatsapp_rust::{
    MediaRetryResult, MediaReuploadRequest, MessageId, MessageRef, SendRequest as WaSendRequest,
    UploadOptions,
};

use crate::api::{Backend, Downloaded, SendRequest, allowed_media_path};
use crate::jid::Jid as TextJid;
use crate::log;
use crate::receive;
use crate::run::Shared;
use crate::send::{self, Kind, SenderError, Uploaded};

/// How long the sender's phone gets to upload expired media again, as in
/// the Go bridge (45 s, plus 15 s for the download).
const MEDIA_RETRY_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct Live {
    pub(crate) shared: Arc<Shared>,
    pub(crate) client: Arc<Client>,
    /// The folder downloads go into, one subfolder per chat.
    pub(crate) store_dir: PathBuf,
    /// The only folder files may be sent from.
    pub(crate) media_dir: Option<String>,
}

impl Backend for Live {
    async fn send(&self, request: SendRequest) -> (bool, String) {
        match self.send_message(&request).await {
            Ok(()) => (true, format!("Message sent to {}", request.recipient)),
            Err(reason) => (false, reason),
        }
    }

    async fn react(&self, message_id: String, chat_jid: String, emoji: String) -> (bool, String) {
        match self.send_reaction(&message_id, &chat_jid, &emoji).await {
            Ok(()) if emoji.is_empty() => (true, "Reaction removed".into()),
            Ok(()) => (true, format!("Reaction {emoji} sent")),
            Err(reason) => (false, reason),
        }
    }

    async fn download(&self, message_id: String, chat_jid: String) -> Result<Downloaded, String> {
        self.download_media(&message_id, &chat_jid).await
    }

    fn counterpart(&self, jid: &TextJid) -> Option<TextJid> {
        receive::counterpart(&self.shared.writer(), jid)
            .ok()
            .flatten()
    }
}

impl Live {
    fn check_connected(&self) -> Result<(), String> {
        if self.shared.is_connected() {
            Ok(())
        } else {
            Err("Not connected to WhatsApp".into())
        }
    }

    async fn send_message(&self, request: &SendRequest) -> Result<(), String> {
        self.check_connected()?;
        let recipient = TextJid::parse_recipient(&request.recipient)
            .map_err(|e| format!("Error parsing JID: {e}"))?;
        let to = library_jid(&recipient).map_err(|e| format!("Error parsing JID: {e}"))?;

        let mut msg = if request.media_path.is_empty() {
            let mut msg = wa::Message::default();
            msg.conversation = Some(request.message.clone());
            msg
        } else {
            self.media_message(&request.media_path, &request.message)
                .await?
        };
        send::attach_context_info(
            &mut msg,
            &request.message,
            send::context_info(
                &request.reply_to_message_id,
                &request.reply_to_sender_jid,
                &request.mentions,
            ),
        );

        let sent = self
            .client
            .send(WaSendRequest::new(&to, msg))
            .await
            .map_err(|e| format!("Error sending message: {e}"))?;
        self.store_sent(&recipient, &sent);
        Ok(())
    }

    /// Reads, uploads and describes a file from the media folder.
    async fn media_message(&self, media_path: &str, caption: &str) -> Result<wa::Message, String> {
        let path = allowed_media_path(self.media_dir.as_deref(), media_path)
            .map_err(|e| format!("Media path rejected: {e}"))?;
        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| format!("Error reading media file: {e}"))?;
        let (kind, mime) = send::media_kind(&path);
        let voice = match kind {
            Kind::Audio => Some(
                send::analyze_ogg_opus(&data)
                    .map_err(|e| format!("Failed to analyze Ogg Opus file: {e}"))?,
            ),
            _ => None,
        };
        let media_type = match kind {
            Kind::Image => MediaType::Image,
            Kind::Video => MediaType::Video,
            Kind::Audio => MediaType::Audio,
            Kind::Document => MediaType::Document,
        };
        let upload = self
            .client
            .upload(data, media_type, UploadOptions::default())
            .await
            .map_err(|e| format!("Error uploading media: {e}"))?;
        // Never log the upload itself: its URL and key together open the file.
        log::line(&format!("Media uploaded ({} bytes)", upload.file_length));
        let uploaded = Uploaded {
            url: upload.url,
            direct_path: upload.direct_path,
            media_key: upload.media_key.to_vec(),
            file_sha256: upload.file_sha256.to_vec(),
            file_enc_sha256: upload.file_enc_sha256.to_vec(),
            file_length: upload.file_length,
            media_key_timestamp: upload.media_key_timestamp,
            streaming_sidecar: upload.streaming_sidecar,
        };
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(send::media_message(
            kind, &mime, &uploaded, caption, &file_name, voice,
        ))
    }

    async fn send_reaction(
        &self,
        message_id: &str,
        chat_jid: &str,
        emoji: &str,
    ) -> Result<(), String> {
        self.check_connected()?;
        let chat_text =
            TextJid::parse(chat_jid).map_err(|e| format!("Error parsing chat JID: {e}"))?;
        let chat = library_jid(&chat_text).map_err(|e| format!("Error parsing chat JID: {e}"))?;
        let sender = {
            let writer = self.shared.writer();
            send::original_sender(&writer, message_id, chat_jid)
        };
        let sender = match sender {
            Ok(sender) => sender,
            Err(SenderError::NotFound) => return Err("Message not found in this chat".into()),
            Err(SenderError::Store(e)) => return Err(format!("Error finding message: {e}")),
            Err(SenderError::Sender(e)) => return Err(format!("Error parsing sender: {e}")),
        };
        let sender = sender
            .map(|s| library_jid(&s))
            .transpose()
            .map_err(|e| format!("Error parsing sender: {e}"))?;
        let from_me = sender.is_none();
        let id = MessageId::new(message_id).map_err(|e| format!("Error sending reaction: {e}"))?;
        let target = MessageRef::new(&chat, id, sender.as_ref(), from_me)
            .map_err(|e| format!("Error sending reaction: {e}"))?;
        let sent = self
            .client
            .send_reaction(&target, emoji)
            .await
            .map_err(|e| format!("Error sending reaction: {e}"))?;
        // Stored like a received reaction, as "[reaction: 👍]".
        self.store_sent(&chat_text, &sent);
        Ok(())
    }

    /// Stores a sent message in the chat that already exists for the
    /// recipient (phone number or LID). The library reports no event for
    /// messages this client sends.
    fn store_sent(&self, recipient: &TextJid, sent: &SendResult) {
        let Some(own) = self.client.pn() else {
            return;
        };
        let writer = self.shared.writer();
        let chat_jid = match send::outgoing_chat_jid(&writer, recipient) {
            Ok(chat_jid) => chat_jid,
            Err(e) => {
                log::line(&format!("Warning: failed to store sent message: {e}"));
                return;
            }
        };
        let now = chrono::Local::now();
        let row = send::sent_message(
            sent.message_id.as_str(),
            &chat_jid,
            &own.user,
            &sent.message,
            now,
        );
        if let Err(e) = writer.store_chat(&chat_jid, "", now.fixed_offset()) {
            log::line(&format!(
                "Warning: failed to store chat for sent message: {e}"
            ));
            return;
        }
        if let Err(e) = writer.store_message(&row) {
            log::line(&format!("Warning: failed to store sent message: {e}"));
        }
    }

    async fn download_media(&self, message_id: &str, chat_jid: &str) -> Result<Downloaded, String> {
        let info = self
            .shared
            .writer()
            .media_info(message_id, chat_jid)
            .map_err(|e| format!("failed to find message: {e}"))?
            .ok_or("failed to find message: sql: no rows in result set")?;
        if info.media_type.is_empty() {
            return Err("not a media message".into());
        }
        let chat_dir = self.store_dir.join(send::safe_chat_dir(chat_jid));
        ensure_private_dir(&chat_dir)
            .map_err(|e| format!("failed to create chat directory: {e}"))?;
        let name = send::download_name(&self.shared.writer(), chat_jid, &info.filename, message_id)
            .map_err(|e| format!("failed to find message: {e}"))?;
        let path = std::path::absolute(chat_dir.join(&name))
            .map_err(|e| format!("failed to get absolute path: {e}"))?;
        let found = || Downloaded {
            media_type: info.media_type.clone(),
            filename: name.clone(),
            path: path.to_string_lossy().into_owned(),
        };
        if path.exists() {
            return Ok(found());
        }

        if info.url.is_empty()
            || info.media_key.is_empty()
            || info.file_sha256.is_empty()
            || info.file_enc_sha256.is_empty()
            || info.file_length == 0
        {
            return Err("incomplete media information for download".into());
        }
        log::line(&format!(
            "Attempting to download media for message {} in chat {}...",
            log::clean(message_id),
            log::clean(chat_jid)
        ));
        let media_type = match info.media_type.as_str() {
            "image" => MediaType::Image,
            "video" => MediaType::Video,
            "audio" => MediaType::Audio,
            "document" => MediaType::Document,
            other => return Err(format!("unsupported media type: {}", log::clean(other))),
        };
        let params = |direct_path: &str| {
            DownloadParams::encrypted(
                direct_path,
                &info.media_key,
                &info.file_sha256,
                &info.file_enc_sha256,
                info.file_length,
                media_type,
            )
            .map_err(|e| format!("failed to download media: {e}"))
        };

        let first = self
            .client
            .download(
                &params(&send::direct_path_from_url(&info.url))?,
                DownloadLimits::default(),
            )
            .await;
        let data = match first {
            Ok(data) => data,
            Err(ClientDownloadError::ReferenceRejected(e)) => {
                // Expired on WhatsApp's servers: ask the sender's phone.
                log::line(&format!(
                    "Media download failed ({}); asking the sender's phone to upload it again...",
                    log::clean(&e.to_string())
                ));
                let direct_path = tokio::time::timeout(
                    MEDIA_RETRY_TIMEOUT,
                    self.reupload(message_id, chat_jid, &info.media_key),
                )
                .await
                .map_err(|_| {
                    "failed to download media: timed out waiting for the sender's phone to upload the media again (it must be online)".to_string()
                })?
                .map_err(|e| format!("failed to download media: {e}"))?;
                self.client
                    .download(&params(&direct_path)?, DownloadLimits::default())
                    .await
                    .map_err(|e| format!("failed to download media: {e}"))?
            }
            Err(e) => return Err(format!("failed to download media: {e}")),
        };

        save_private(&path, &data).map_err(|e| format!("failed to save media file: {e}"))?;
        if self.shared.log_content {
            log::line(&format!(
                "Successfully downloaded {} media to {} ({} bytes)",
                info.media_type,
                path.display(),
                data.len()
            ));
        } else {
            log::line(&format!(
                "Successfully downloaded {} media ({} bytes)",
                info.media_type,
                data.len()
            ));
        }
        Ok(found())
    }

    /// Asks the sender's phone to upload a message's media again; the new
    /// direct path. The phone must be online.
    async fn reupload(
        &self,
        message_id: &str,
        chat_jid: &str,
        media_key: &[u8],
    ) -> Result<String, String> {
        let chat = TextJid::parse(chat_jid)
            .ok()
            .and_then(|c| library_jid(&c).ok())
            .ok_or("invalid chat JID")?;
        let sender = {
            let writer = self.shared.writer();
            send::original_sender(&writer, message_id, chat_jid)
        };
        let sender = match sender {
            Ok(sender) => sender,
            Err(SenderError::NotFound) => {
                return Err("failed to look up message sender: not found".into());
            }
            Err(SenderError::Store(e)) => {
                return Err(format!("failed to look up message sender: {e}"));
            }
            Err(SenderError::Sender(e)) => return Err(format!("invalid sender: {e}")),
        };
        let sender = sender
            .map(|s| library_jid(&s))
            .transpose()
            .map_err(|e| format!("invalid sender: {e}"))?;
        let media_key: &[u8; 32] = media_key
            .try_into()
            .map_err(|_| "the stored media key is not 32 bytes")?;
        let id = MessageId::new(message_id).map_err(|e| e.to_string())?;
        let target = MessageRef::new(&chat, id, sender.as_ref(), sender.is_none())
            .map_err(|e| e.to_string())?;
        let result = self
            .client
            .media_reupload()
            .request(&MediaReuploadRequest { target, media_key })
            .await
            .map_err(|e| format!("failed to request media retry: {e}"))?;
        match result {
            MediaRetryResult::Success { direct_path } if direct_path.is_empty() => {
                Err("media retry returned an empty path".into())
            }
            MediaRetryResult::Success { direct_path } => Ok(direct_path),
            other => Err(format!(
                "the sender's phone could not provide the media: {other:?}"
            )),
        }
    }
}

/// whatsapp-rust's JID for one of the bridge's.
fn library_jid(jid: &TextJid) -> Result<Jid, String> {
    jid.to_string().parse::<Jid>().map_err(|e| e.to_string())
}

/// Writes a downloaded file readable by the owner only. It appears under
/// its name only when complete: a file that is there counts as downloaded.
fn save_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut partial = path.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    ensure_private_file(&partial)?;
    std::fs::write(&partial, data)?;
    std::fs::rename(&partial, path)
}
