//! The tool definitions and server instructions must stay exactly what the
//! Python server publishes, as recorded in the parity harness's golden files.

use std::collections::BTreeMap;

use serde_json::Value;
use whatsapp_mcp_config::Config;
use whatsapp_mcp_server::{SEND_TOOLS, SERVER_INSTRUCTIONS, SERVER_NAME, ServeOptions, Server};

fn golden(profile: &str) -> Value {
    let path = format!(
        "{}/../../parity/golden/{profile}/_server.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
}

/// Tool definitions as JSON, by name.
fn by_name(tools: impl IntoIterator<Item = Value>) -> BTreeMap<String, Value> {
    tools
        .into_iter()
        .map(|t| (t["name"].as_str().unwrap().to_string(), t))
        .collect()
}

fn server(read_only: bool) -> Server {
    let config = Config::load_from(
        |name| Ok((read_only && name == "WHATSAPP_MCP_READ_ONLY").then(|| "1".to_string())),
        None,
    )
    .unwrap();
    Server::new(
        &config,
        ServeOptions {
            store_dir: "store".into(),
            bridge_url: "http://127.0.0.1:1/api".into(),
        },
    )
}

fn published(server: &Server) -> BTreeMap<String, Value> {
    by_name(
        server
            .tools()
            .iter()
            .map(|t| serde_json::to_value(t).unwrap()),
    )
}

#[test]
fn name_and_instructions_match_the_python_server() {
    let golden = golden("default");
    assert_eq!(golden["name"], SERVER_NAME);
    assert_eq!(golden["instructions"], SERVER_INSTRUCTIONS);
}

#[test]
fn tools_match_the_python_server() {
    let want = by_name(golden("default")["tools"].as_array().unwrap().clone());
    assert_eq!(published(&server(false)), want);
}

#[test]
fn read_only_hides_exactly_the_send_tools() {
    let want = by_name(golden("read_only_env")["tools"].as_array().unwrap().clone());
    let got = published(&server(true));
    assert_eq!(got, want);
    let all = published(&server(false));
    let hidden: Vec<&str> = all
        .keys()
        .filter(|name| !got.contains_key(*name))
        .map(String::as_str)
        .collect();
    let mut send = SEND_TOOLS.to_vec();
    send.sort_unstable();
    assert_eq!(hidden, send);
}
