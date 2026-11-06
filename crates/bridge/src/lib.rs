//! The bridge: the WhatsApp connection (whatsapp-rust), storing messages,
//! sending, media, the send allowlist and the local API on `127.0.0.1:8080`.
//! Being built in phase 5 of the plan (docs/rust-1.0/phase5.md): it links,
//! receives, stores, sends and downloads; until phase 5 is done the Go
//! bridge in whatsapp-bridge/ is the one to use daily.

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
pub use run::{Options, run, run_blocking};
