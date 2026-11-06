//! The bridge: the WhatsApp connection (whatsapp-rust), storing messages,
//! sending, media, the send allowlist and the local API on `127.0.0.1:8080`.
//! Being built in phase 5 of the plan (docs/rust-1.0/phase5.md); until it is
//! done, the Go bridge in whatsapp-bridge/ is the one to run.

pub mod api;
pub mod convert;
pub mod jid;
pub mod log;

pub use api::{Api, Backend, Downloaded, SendRequest, allowed_media_path, allows};
pub use jid::Jid;
