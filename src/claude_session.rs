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
//!
//! Isolation: the subject under audit is untrusted input, so the child is
//! locked down to a pure text-in/text-out call. It gets no tools, no MCP
//! servers, no slash commands/skills, and none of the user/project/local
//! settings files (which is where hooks, permission allow-rules and plugins
//! live). It runs in a fresh empty tempdir so a hostile repo's
//! `.claude/settings.json`, `.mcp.json` or CLAUDE.md is never discovered
//! from cwd. `--bare` would be simpler but disables OAuth, which defeats
//! the reason we shell out in the first place.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, Command};

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

/// One round of `claude` stream-json output. `system` and `user` are
/// typed only as far as safeguards-stop detection needs; anything else is
/// collapsed into `Other` — the deserializer must not fail when claude
/// introduces new event types.
#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "result")]
    Result(ResultEvent),
    #[serde(rename = "system")]
    System {
        #[serde(default)]
        subtype: String,
        #[serde(default)]
        content: String,
    },
    #[serde(rename = "user")]
    User {
        #[serde(default, rename = "isSynthetic")]
        is_synthetic: bool,
        #[serde(default)]
        message: serde_json::Value,
    },
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
/// Upper bound on one spec's audit call. Generous because large subjects
/// take a while; the point is that a wedged child can't hang oaudit (and
/// a CI job) forever.
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Flags that strip the child down to a text-only completion. Kept in one
/// place so the isolation contract is reviewable (and testable) at a glance.
const ISOLATION_ARGS: &[&str] = &[
    "--restricted",
    "--tools",
    "",
    "--setting-sources",
    "",
    "--strict-mcp-config",
    "--mcp-config",
    r#"{"mcpServers":{}}"#,
    "--disable-slash-commands",
    "--permission-prompts",
    "none",
    "--no-session-persistence",
];

pub(crate) async fn query_claude(system_prompt: &str, user_message: &str) -> Result<Reply> {
    // Empty cwd: nothing for claude to auto-discover. Removed on drop, after
    // the child has exited (or been killed).
    let workdir = tempfile::tempdir().context("creating isolated working dir for claude")?;

    let mut child = Command::new("claude")
        .current_dir(workdir.path())
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
        .args(ISOLATION_ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawning `claude` (is it installed and on PATH?)")?;

    let stdin = child.stdin.take().context("claude stdin missing")?;
    let stdout = child.stdout.take().context("claude stdout missing")?;
    let stderr = child.stderr.take().context("claude stderr missing")?;

    // Drain stderr concurrently. If we waited until after stdout finished,
    // a claude process that writes more than the ~64 KB stderr pipe buffer
    // would block on its stderr write → never close stdout → we'd deadlock
    // forever in read_until_result.
    let stderr_task = tokio::spawn(read_stderr(stderr));

    let exchange = async {
        write_request(stdin, user_message).await?;
        let result = read_until_result(stdout).await;
        let status = child.wait().await.context("waiting for claude child")?;
        anyhow::Ok((result, status))
    };
    let (result, status) = match tokio::time::timeout(CLAUDE_TIMEOUT, exchange).await {
        Ok(r) => r?,
        // Returning drops `child`, which kills it (kill_on_drop).
        Err(_) => bail!(
            "claude did not finish within {} minutes; aborted. Try narrowing --scope.",
            CLAUDE_TIMEOUT.as_secs() / 60
        ),
    };
    let stderr_text = stderr_task.await.unwrap_or_default();

    match result {
        Ok(reply) => Ok(reply),
        Err(e) => {
            // claude can emit a non-success result event AND exit 0 (the
            // event itself is the error signal). In that case status is
            // fine but stderr is the most useful diagnostic we have, so
            // surface it whenever it isn't empty — not only on non-zero
            // exit. Keeps our "check stderr for details" promise honest.
            let stderr_trimmed = stderr_text.trim();
            // Fail closed: an older claude that rejects an isolation flag
            // must not be retried without it.
            if stderr_trimmed.contains("unknown option") {
                bail!(
                    "your `claude` CLI doesn't support oaudit's isolation flags \
                     (--restricted, --tools, --setting-sources, --permission-prompts). \
                     Update it with `claude update` and retry.\n  stderr: {stderr_trimmed}"
                );
            }
            if !status.success() && !stderr_trimmed.is_empty() {
                bail!("{e:#}\n  (claude exited with {status}; stderr: {stderr_trimmed})");
            }
            if !status.success() {
                bail!("{e:#}\n  (claude exited with {status})");
            }
            if !stderr_trimmed.is_empty() {
                bail!("{e:#}\n  stderr: {stderr_trimmed}");
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

/// What one audit call produced.
#[derive(Debug)]
pub(crate) struct Reply {
    /// Final text. May be empty when `safety_stopped` is set.
    pub text: String,
    /// The model's own safety classifier cut a response off mid-way. The
    /// CLI then retries once with "don't produce that again", and the retry
    /// is often empty or partial. This fires almost exclusively when the
    /// subject contains real malicious code the model was describing, so
    /// callers treat it as a signal rather than a tool failure.
    pub safety_stopped: bool,
}

async fn read_until_result<R: AsyncRead + Unpin>(stdout: R) -> Result<Reply> {
    let mut lines = BufReader::new(stdout).lines();
    let mut safety_stopped = false;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let event: StreamEvent = match serde_json::from_str(&line) {
            Ok(e) => e,
            Err(_) => continue, // unknown shape — skip and keep reading
        };
        match event {
            // The CLI's two markers for a safeguards stop: an informational
            // notice, and the synthetic user turn it injects before retrying.
            StreamEvent::System { subtype, content }
                if subtype == "informational" && mentions_safety_stop(&content) =>
            {
                safety_stopped = true;
            }
            StreamEvent::User { is_synthetic: true, message }
                if mentions_safety_stop(&message.to_string()) =>
            {
                safety_stopped = true;
            }
            StreamEvent::Result(r) => {
                if r.is_error || r.subtype != "success" {
                    // On an error result, `result` carries claude's own error
                    // message (an API error, not model output), which is
                    // the only useful detail we get.
                    let detail = r.result.as_deref().unwrap_or_default();
                    if looks_like_context_overflow(detail) {
                        bail!(
                            "the subject is too large for a single audit request ({}). \
                             Narrow it with --scope (e.g. --scope 'src/**') or audit \
                             subdirectories separately.",
                            detail.chars().take(200).collect::<String>().trim()
                        );
                    }
                    let mut msg = explain_failure_subtype(&r.subtype, r.is_error);
                    if !detail.is_empty() {
                        msg.push_str(&format!(
                            "\n  claude said: {}",
                            detail.chars().take(300).collect::<String>().trim()
                        ));
                    }
                    bail!("{msg}");
                }
                let text = r.result.unwrap_or_default();
                if text.is_empty() && !safety_stopped {
                    bail!(
                        "claude returned success but no text. The spec may have produced an empty response — re-run with a narrower scope or a different spec to debug."
                    );
                }
                return Ok(Reply { text, safety_stopped });
            }
            _ => {}
        }
    }
    bail!("claude stdout closed before emitting a result event")
}

fn looks_like_context_overflow(detail: &str) -> bool {
    let d = detail.to_lowercase();
    d.contains("prompt is too long")
        || d.contains("too many tokens")
        || d.contains("context window")
        || d.contains("context length")
}

/// Matched loosely on purpose: the wording isn't a stable contract, and a
/// miss only degrades to the old "no text" error.
fn mentions_safety_stop(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("safeguards stopped") || text.contains("safety classifier")
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

    // Shapes captured from claude 2.1.283 when a reply was cut off.
    const STOP_NOTICE: &str = r#"{"type":"system","subtype":"informational","content":"Fable 5.1's safeguards stopped the response above · continuing once with that noted","level":"notice"}"#;
    const STOP_RETRY: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Your response above was stopped by a safety classifier — this is not a tool or API error."}]},"isSynthetic":true}"#;
    const EMPTY_RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":""}"#;

    async fn read_lines(lines: &[&str]) -> Result<Reply> {
        let stream = lines.join("\n") + "\n";
        read_until_result(stream.as_bytes()).await
    }

    #[tokio::test]
    async fn empty_reply_after_safety_stop_is_not_an_error() {
        for marker in [STOP_NOTICE, STOP_RETRY] {
            let reply = read_lines(&[marker, EMPTY_RESULT]).await.unwrap();
            assert!(reply.safety_stopped);
            assert!(reply.text.is_empty());
        }
    }

    #[tokio::test]
    async fn empty_reply_without_safety_stop_still_errors() {
        // A real (non-synthetic) user turn quoting the phrase doesn't count,
        // nor does an unrelated notice or a non-string content field.
        let echoed = r#"{"type":"user","message":{"role":"user","content":"safety classifier"}}"#;
        let other = r#"{"type":"system","subtype":"informational","content":"rate limit soon"}"#;
        let odd = r#"{"type":"system","subtype":"informational","content":{"x":1}}"#;
        let err = read_lines(&[echoed, other, odd, "not json", EMPTY_RESULT])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no text"), "got: {err}");
    }

    #[tokio::test]
    async fn clean_reply_is_not_marked_stopped() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"[]"}"#;
        let reply = read_lines(&[r#"{"type":"system","subtype":"init"}"#, ok]).await.unwrap();
        assert!(!reply.safety_stopped);
        assert_eq!(reply.text, "[]");
    }

    #[test]
    fn isolation_args_disable_tools_settings_and_mcp() {
        let pairs: Vec<_> = ISOLATION_ARGS.windows(2).collect();
        assert!(pairs.contains(&["--tools", ""].as_slice()));
        assert!(pairs.contains(&["--setting-sources", ""].as_slice()));
        assert!(pairs.contains(&["--permission-prompts", "none"].as_slice()));
        for flag in ["--restricted", "--strict-mcp-config", "--disable-slash-commands"] {
            assert!(ISOLATION_ARGS.contains(&flag), "missing {flag}");
        }
    }

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
        // `system` and `user` are typed now (safeguards-stop detection) but
        // must still parse with extra or missing fields.
        let init: StreamEvent =
            serde_json::from_str(r#"{"type":"system","subtype":"init","cwd":"/tmp"}"#).unwrap();
        assert!(matches!(init, StreamEvent::System { .. }));

        for line in [
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
        .unwrap()
        .text;
        assert!(
            result.to_lowercase().contains("pong"),
            "expected 'pong' in: {result}"
        );
    }
}
