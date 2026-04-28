//! Spawn the `claude` CLI in headless stream-json mode for one audit call.
//!
//! Per-call spawn (not a kept-alive session) because each spec sets its own
//! system prompt. Cost: a couple of seconds of init per spec; oaudit runs
//! typically have 1-2 specs, so this trades latency for isolation between
//! specs (one spec's text can't influence another's evaluation).
//!
//! Auth: inherits whatever the `claude` CLI inherits — env API key, or the
//! claude.ai OAuth flow if no key. Optional API key is the whole reason
//! we shell out instead of calling the Anthropic API directly.
//!
//! Runtime dep: `claude` must be on $PATH.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};

#[derive(Serialize)]
struct UserMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    message: UserBody<'a>,
}

#[derive(Serialize)]
struct UserBody<'a> {
    role: &'static str,
    content: &'a str,
}

/// One round of `claude` stream-json output. Anything that isn't a
/// `result` is collapsed into `Other` — the deserializer must not fail
/// when claude introduces new event types.
#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "result")]
    Result(ResultEvent),
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug)]
struct ResultEvent {
    /// `"success"` on a clean turn; `"error_max_turns"` etc. otherwise.
    subtype: String,
    is_error: bool,
    /// Final assistant text. Best-effort; collapsed by claude for
    /// `subtype: success`.
    #[serde(default)]
    result: Option<String>,
}

/// Send `user_message` to `claude` with `system_prompt` as the system role,
/// wait for the run to complete, and return the model's final text reply.
pub(crate) async fn query_claude(system_prompt: &str, user_message: &str) -> Result<String> {
    let mut child = Command::new("claude")
        .arg("--print")
        .arg("--input-format=stream-json")
        .arg("--output-format=stream-json")
        .arg("--verbose") // claude requires --verbose with --print + stream-json
        // --system-prompt as a CLI arg means the body sits in argv. macOS
        // ARG_MAX is ~256 KB; current spec bodies are well under that. If
        // we ever ship specs that approach the limit, switch to
        // --system-prompt-file (claude supports it) which writes the
        // prompt via a path instead.
        .arg("--system-prompt")
        .arg(system_prompt)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawning `claude` (is it installed and on PATH?)")?;

    let stdin = child.stdin.take().context("claude stdin missing")?;
    let stdout = child.stdout.take().context("claude stdout missing")?;
    let stderr = child.stderr.take().context("claude stderr missing")?;

    write_request(stdin, user_message).await?;
    let result = read_until_result(stdout).await;

    let status = child.wait().await.context("waiting for claude child")?;

    match result {
        Ok(text) => Ok(text),
        Err(e) => {
            let stderr_text = read_stderr(stderr).await;
            if !status.success() {
                bail!(
                    "claude exited with {status}.\n  stderr: {}\n  parse: {e:#}",
                    stderr_text.trim()
                );
            }
            Err(e)
        }
    }
}

async fn write_request(mut stdin: ChildStdin, user_message: &str) -> Result<()> {
    let req = UserMessage {
        kind: "user",
        message: UserBody {
            role: "user",
            content: user_message,
        },
    };
    let line = serde_json::to_string(&req).context("serializing claude request")?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.shutdown().await?;
    Ok(())
}

async fn read_until_result(stdout: ChildStdout) -> Result<String> {
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let event: StreamEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue, // unknown shape — skip and keep reading
        };
        if let StreamEvent::Result(r) = event {
            if r.is_error || r.subtype != "success" {
                bail!("{}", explain_failure_subtype(&r.subtype, r.is_error));
            }
            return match r.result {
                Some(text) if !text.is_empty() => Ok(text),
                _ => bail!(
                    "claude returned success but no text. The spec may have produced an empty response — re-run with a narrower scope or a different spec to debug."
                ),
            };
        }
    }
    bail!("claude stdout closed before emitting a result event")
}

/// Translate claude's stream-json failure subtypes into actionable
/// messages. Falls back to the raw jargon for unknown subtypes so we
/// still surface something instead of swallowing it.
fn explain_failure_subtype(subtype: &str, is_error: bool) -> String {
    match subtype {
        "error_max_turns" => {
            "claude hit its turn limit before finishing this spec. The spec or evidence \
             may be too large; try narrowing --scope, splitting the spec, or passing \
             a single spec to --against."
                .to_string()
        }
        "error_during_execution" => {
            "claude encountered an error during execution. Check stderr for details; \
             this often indicates an upstream API issue or a hook/plugin failure."
                .to_string()
        }
        other => format!(
            "claude completed with non-success result (subtype: {other}, is_error: {is_error})"
        ),
    }
}

async fn read_stderr(stderr: ChildStderr) -> String {
    let mut buf = String::new();
    let mut reader = BufReader::new(stderr);
    let _ = reader.read_to_string(&mut buf).await;
    buf
}

/// Verify `claude --version` resolves on $PATH. Run once at startup so
/// users get a clear "install claude" message instead of a spawn failure
/// in the middle of an audit.
pub(crate) async fn preflight() -> Result<String> {
    let output = Command::new("claude")
        .arg("--version")
        .output()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "`claude` CLI not found on PATH: {e}\n\nInstall: https://claude.com/claude-code"
            )
        })?;
    if !output.status.success() {
        bail!(
            "`claude --version` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_serializes_as_expected() {
        let msg = UserMessage {
            kind: "user",
            message: UserBody {
                role: "user",
                content: "hello",
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json,
            r#"{"type":"user","message":{"role":"user","content":"hello"}}"#
        );
    }

    #[test]
    fn result_event_deserializes() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"hello world"}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        match event {
            StreamEvent::Result(r) => {
                assert_eq!(r.subtype, "success");
                assert!(!r.is_error);
                assert_eq!(r.result.as_deref(), Some("hello world"));
            }
            _ => panic!("expected Result"),
        }
    }

    #[test]
    fn other_event_types_match_other_variant() {
        for line in [
            r#"{"type":"system","subtype":"init","cwd":"/tmp"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[]}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{}}"#,
            r#"{"type":"some_brand_new_event"}"#,
        ] {
            let event: StreamEvent = serde_json::from_str(line).unwrap();
            assert!(matches!(event, StreamEvent::Other), "line: {line}");
        }
    }

    #[test]
    fn known_subtypes_get_actionable_messages() {
        let msg = explain_failure_subtype("error_max_turns", true);
        assert!(msg.contains("turn limit"), "got: {msg}");
        assert!(msg.contains("--scope") || msg.contains("split"), "got: {msg}");

        let msg = explain_failure_subtype("error_during_execution", true);
        assert!(msg.contains("error during execution"), "got: {msg}");

        // Unknown subtypes fall through to the raw form (so we surface
        // something instead of swallowing it).
        let msg = explain_failure_subtype("error_brand_new", true);
        assert!(msg.contains("error_brand_new"), "got: {msg}");
    }

    #[test]
    fn result_event_with_error_subtype_is_recognized() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"result":null}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        match event {
            StreamEvent::Result(r) => {
                assert_eq!(r.subtype, "error_max_turns");
                assert!(r.is_error);
                assert!(r.result.is_none());
            }
            _ => panic!("expected Result"),
        }
    }

    /// Live-API smoke test. Skipped by default (consumes API quota +
    /// requires auth). Enable: `OAUDIT_TEST_LIVE=1 cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "live API call; opt in with OAUDIT_TEST_LIVE=1 and --ignored"]
    async fn live_query_returns_text() {
        if std::env::var("OAUDIT_TEST_LIVE").as_deref() != Ok("1") {
            return;
        }
        let result = query_claude(
            "You are a terse echo bot. Reply with exactly the word 'pong'.",
            "ping",
        )
        .await
        .unwrap();
        assert!(
            result.to_lowercase().contains("pong"),
            "expected 'pong' in: {result}"
        );
    }
}
