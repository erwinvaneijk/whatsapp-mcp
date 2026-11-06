//! The client for the bridge's local API (`/api/send`, `/api/react`,
//! `/api/download`): POST with JSON and the bridge token, as the Python
//! server does.

use std::time::Duration;

use serde_json::{Map, Value};

/// How long to wait for the bridge to accept the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for its answer: generous, because the bridge uploads
/// media to WhatsApp before it responds.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Bridge {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
}

/// What the bridge answered.
pub enum Answer {
    /// Any HTTP status, with the body as JSON when it is a JSON object.
    Reply {
        status: u16,
        json: Option<Map<String, Value>>,
        text: String,
    },
    /// No answer in time; a send may still have gone through.
    Timeout,
    /// No answer at all: no token, nothing listening, a broken connection.
    Failed(String),
}

impl Bridge {
    pub fn new(base_url: &str, token: Option<&str>) -> Bridge {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(ANSWER_TIMEOUT))
            .timeout_recv_body(Some(ANSWER_TIMEOUT))
            .http_status_as_error(false)
            .build();
        Bridge {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.filter(|t| !t.is_empty()).map(str::to_string),
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// POSTs `payload` to `<base>/<path>`.
    pub fn post(&self, path: &str, payload: &Value) -> Answer {
        let Some(token) = &self.token else {
            return Answer::Failed("WHATSAPP_BRIDGE_TOKEN is not set".to_string());
        };
        let url = format!("{}/{path}", self.base_url);
        let response = self
            .agent
            .post(&url)
            .header("X-Bridge-Token", token)
            .send_json(payload);
        let mut response = match response {
            Ok(response) => response,
            Err(ureq::Error::Timeout(_)) => return Answer::Timeout,
            Err(e) => return Answer::Failed(e.to_string()),
        };
        let status = response.status().as_u16();
        let text = match response.body_mut().read_to_string() {
            Ok(text) => text,
            Err(ureq::Error::Timeout(_)) => return Answer::Timeout,
            Err(e) => return Answer::Failed(e.to_string()),
        };
        let json = match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        };
        Answer::Reply { status, json, text }
    }
}
