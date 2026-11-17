//! `whatsapp-mcp serve` as Claude Code runs it: a client that connects,
//! lists the tools, and closes stdin when the session ends.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

#[test]
fn serve_exits_cleanly_when_the_client_disconnects() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.yaml");
    std::fs::write(&config, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_whatsapp-mcp"))
        .arg("serve")
        .env("WHATSAPP_MCP_CONFIG", &config)
        .env_remove("WHATSAPP_FULL_CONTROL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(
            concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                "\n",
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                "\n",
            )
            .as_bytes(),
        )
        .unwrap();
    // The answer to tools/list, then the end of the session.
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut tools = String::new();
    while !tools.contains(r#""id":2"#) {
        tools.clear();
        assert!(
            stdout.read_line(&mut tools).unwrap() > 0,
            "no tools/list answer"
        );
    }
    assert!(tools.contains("list_messages"), "{tools}");
    drop(stdin);
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
    assert!(status.success(), "{status}; stderr: {stderr}");
}
