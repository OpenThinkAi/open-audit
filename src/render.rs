//! Render a markdown document in a browser via the Node ui-leaf bridge.
//!
//! Swap point: replace `render_spec` to switch renderers. The contract is
//! "given markdown, show it nicely; return when the user closes the view."
//! Future ui-leaf-language-neutral-binary or pulldown-cmark fallback land here.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Serialize)]
struct BridgeRequest<'a> {
    view: &'a str,
    data: BridgeData<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
}

#[derive(Serialize)]
struct BridgeData<'a> {
    markdown: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum BridgeEvent {
    Ready { url: String, port: u16 },
    Closed,
    Error { message: String },
}

/// Locate the bridge.js script. Honor OAUDIT_UI_BRIDGE if set, else fall back
/// to the in-repo path baked at compile time.
///
/// Shipping note: a released binary won't have CARGO_MANIFEST_DIR available
/// at runtime in any meaningful sense. Distribution will need to either
/// install bridges/ alongside the binary or bundle bridge.js as a resource.
/// For v1 this is dev-only.
fn locate_bridge() -> PathBuf {
    if let Ok(p) = std::env::var("OAUDIT_UI_BRIDGE") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bridges")
        .join("ui-leaf")
        .join("bridge.js")
}

pub async fn render_spec(markdown: &str, title: Option<&str>) -> Result<()> {
    let bridge = locate_bridge();
    if !bridge.exists() {
        bail!(
            "ui-leaf bridge not found at {}\n  set OAUDIT_UI_BRIDGE to override",
            bridge.display(),
        );
    }

    let mut child = Command::new("node")
        .arg(&bridge)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // ui-leaf chatter goes through to user terminal
        .spawn()
        .with_context(|| format!("spawning `node {}`", bridge.display()))?;

    let req = BridgeRequest {
        view: "spec",
        data: BridgeData { markdown, title },
        title,
    };
    let req_line = serde_json::to_string(&req).context("serializing bridge request")?;

    {
        let mut stdin = child
            .stdin
            .take()
            .context("bridge child stdin not piped (impossible)")?;
        stdin.write_all(req_line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.shutdown().await?;
    }

    let stdout = child
        .stdout
        .take()
        .context("bridge child stdout not piped (impossible)")?;
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let event: BridgeEvent = serde_json::from_str(&line)
            .with_context(|| format!("parsing bridge event line: {line}"))?;
        match event {
            BridgeEvent::Ready { url, port: _ } => {
                eprintln!("oaudit: view ready at {url} (close the tab to exit)");
            }
            BridgeEvent::Closed => break,
            BridgeEvent::Error { message } => {
                let _ = child.wait().await;
                bail!("ui-leaf bridge error: {message}");
            }
        }
    }

    let status = child.wait().await.context("waiting for bridge child")?;
    if !status.success() {
        bail!("bridge exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_with_data_and_title() {
        let req = BridgeRequest {
            view: "spec",
            data: BridgeData { markdown: "# Hi", title: Some("builtin: trusted/security") },
            title: Some("builtin: trusted/security"),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"view\":\"spec\""));
        assert!(json.contains("\"markdown\":\"# Hi\""));
        assert!(json.contains("\"title\":\"builtin: trusted/security\""));
    }

    #[test]
    fn ready_event_deserializes() {
        let line = r#"{"type":"ready","url":"http://127.0.0.1:5810","port":5810}"#;
        let ev: BridgeEvent = serde_json::from_str(line).unwrap();
        match ev {
            BridgeEvent::Ready { url, port } => {
                assert_eq!(url, "http://127.0.0.1:5810");
                assert_eq!(port, 5810);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn closed_event_deserializes() {
        let ev: BridgeEvent = serde_json::from_str(r#"{"type":"closed"}"#).unwrap();
        assert!(matches!(ev, BridgeEvent::Closed));
    }

    #[test]
    fn error_event_deserializes() {
        let ev: BridgeEvent =
            serde_json::from_str(r#"{"type":"error","message":"mount() failed"}"#).unwrap();
        match ev {
            BridgeEvent::Error { message } => assert_eq!(message, "mount() failed"),
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
