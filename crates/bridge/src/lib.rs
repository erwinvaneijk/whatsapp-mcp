//! The bridge: the WhatsApp connection (whatsapp-rust), storing messages,
//! sending, media, the send allowlist and the local API (on
//! `bridge_address`, `127.0.0.1:8080` by default).

pub mod api;
pub mod convert;
pub mod jid;
mod live;
pub mod log;
pub mod receive;
pub mod run;
pub mod send;

pub use api::{Api, Backend, Downloaded, SendRequest, allowed_media_path, allows};
pub use jid::Jid;
pub use run::{Options, is_linked, run, run_blocking};
