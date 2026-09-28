# open-audit

`oaudit` audits a codebase, a file, or a piece of text against spec
documents (security, supply chain, infrastructure, LLM security, privacy,
agent skills) and reports findings as JSON or readable text. The judging is
done by Claude through your local [Claude Code](https://claude.com/claude-code)
CLI.

It audits an artifact as it is now. It is not a diff or PR reviewer, and it
doesn't look at git history.

## Requirements

- macOS (Apple Silicon or Intel) or Linux (x86_64). No Windows or ARM Linux
  builds yet.
- The `claude` CLI on your `PATH`, signed in with a claude.ai account or an
  `ANTHROPIC_API_KEY`. Keep it up to date: oaudit depends on its isolation
  flags and refuses to run on a version that lacks them.
- Node.js, only if you install through npm.

## Install

```sh
npm install -g @openthink/audit
# or, without Node:
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/OpenThinkAi/open-audit/releases/latest/download/open-audit-installer.sh | sh
```

Update later with `oaudit update`.

## Quick start

```sh
# Is this third-party repo safe to use? (readable output)
oaudit repo ./some-repo --format human

# Should I install this agent skill?
oaudit file ./some-skill --against untrusted/agent-skill --format human

# Check your own code for mistakes
oaudit repo . --against trusted/security,trusted/privacy --format human

# Screen a piece of untrusted text (issue body, RAG snippet, email)
cat issue.md | oaudit text --label issue-42 --format human
```

Output is **JSON by default** (for scripts and CI); add `--format human` to
read it yourself.

## Commands

| Command | What it audits | Default spec |
|---|---|---|
| `oaudit repo <path>` | A local git repository's working tree. Must be the repo root, not a subdirectory. URLs aren't supported yet. | `untrusted/security` |
| `oaudit file <path>` | A single file or a non-git directory. `-` reads stdin (same as `text`). | `untrusted/security` |
| `oaudit text` | Text from stdin (up to 256 KB). `--label` names it in findings. | `untrusted/llm-security` |
| `oaudit list` | Lists built-in and repo-local specs. | |
| `oaudit explain <spec>` | Prints a spec's full text. | |
| `oaudit update` | Updates oaudit through whichever installer you used. | |

Options for `repo` and `file`:

- `--against <specs>`: comma-separated catalog names (`untrusted/security`)
  and/or paths to your own spec files (`./my-spec.md`). Each spec is a
  separate Claude request.
- `--scope '<glob>'`: audit only matching files, for example
  `--scope 'src/**'`. Replaces the spec's include list; its exclude list
  still applies.
- `--include-secrets`: also send files that look like secrets (withheld by
  default; see below).
- `--format json|human`: default `json`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Audit finished; no high or critical findings. |
| `1` | Audit finished; at least one high or critical finding. |
| `2` | oaudit couldn't complete the audit (bad arguments, subject too large, `claude` missing or failing). No verdict. |

In CI, treat anything non-zero as "don't proceed", and `2` as "look at the
error", not "passed".

## Specs and trust modes

Built-in specs come in two postures, and you pick one per run by the prefix:

- **`trusted/<name>`**: your own code. Looks for mistakes.
- **`untrusted/<name>`**: code or content you didn't write and are about to
  depend on, run or install. Assumes the author may be hostile and looks
  for malice and concealment as well as mistakes. Expect more findings.

Domains: `security`, `supply-chain`, `infra`, `llm-security`, `privacy`,
`agent-skill`. Run `oaudit explain <mode>/<name>` to read exactly what a spec
asks for.

If any selected spec is untrusted, the whole run is treated as untrusted:
the subject's own `.gitignore`/`.ignore` files are ignored (so nothing can
be hidden from the audit), and files the model wasn't shown become a
finding.

## Vetting agent skills

`untrusted/agent-skill` is for deciding whether to install a third-party
agent skill (a `SKILL.md` plus scripts), or similar instruction packages
like slash commands, plugin manifests and hook configs. Instead of flagging
every shell call or network request, it compares what the skill *says* it's
for with what it actually does, and reports only capabilities the stated
purpose doesn't need or that the skill hides. A browser-testing skill that
starts a dev server is fine; one that also reads `~/.aws/credentials` isn't.

It asks for a short inventory finding (purpose, capabilities, network
destinations, writes, and an install / install with care / do not install
verdict) so you can check the reasoning quickly.

`trusted/agent-skill` is the author-side version: it looks for leaked
secrets, over-broad triggers and missing confirmation steps before you
publish a skill.

## Writing your own spec

A spec is a Markdown file with YAML frontmatter; the body becomes the
auditor's instructions.

```markdown
---
name: api-keys
mode: trusted          # or untrusted
kind: prompt           # prompt | hybrid | deterministic (informational today)
default_scope:         # optional; defaults to every file
  include: ["**/*"]
  exclude: ["node_modules/**", ".git/**"]
---

Look for API keys and tokens committed to the repository...
```

Pass it with `--against ./api-keys.md`. You don't need to describe the
output format: oaudit tells the model the required finding fields on every
request. For richer findings (severity rubrics, calibration examples,
extra fields), borrow sections from a built-in: `oaudit explain
trusted/security`.

To override a built-in for a project, save your spec as
`.oaudit/auditors/<mode>/<name>.md` in the directory you run oaudit from;
`oaudit list` shows which ones are overridden. Its frontmatter `mode` must
match the directory. A subject can't supply its own untrusted auditor: when
you run oaudit from inside the thing being audited, local `untrusted/*`
overrides are ignored in favour of the built-ins.

## JSON output

```json
{
  "findings": [
    {
      "id": "sec-hardcoded-token",
      "severity": "critical | high | medium | low | info",
      "confidence": "high | medium | low",
      "title": "one-line summary",
      "location": { "file": "src/config.ts", "line": 12, "endLine": 12 },
      "evidence": "short excerpt",
      "explanation": "why it matters",
      "suggestion": "what to do",
      "spec": "untrusted/security"
    }
  ],
  "specs_run": ["untrusted/security"],
  "subject": "/abs/path/to/subject",
  "gather": {
    "skipped_too_large": 0,
    "skipped_binary": 0,
    "skipped_io_error": 0,
    "skipped_secret": 1,
    "decoded_lossily": 0,
    "skipped_files": [{ "path": ".env", "reason": "possible_secret" }],
    "io_error_samples": [],
    "io_error_samples_truncated": false
  }
}
```

Findings can carry extra fields depending on the spec (`benign_explanation`,
`activation` and `impact_if_malicious` in untrusted specs; `data_categories`
and `destinations` in privacy specs). Findings whose `spec` is `oaudit` were
added by oaudit itself: `oaudit-unaudited-files` (files the model wasn't
shown) and `oaudit-safety-stop` (see below).

## What happens to your files

- oaudit **reads** the subject; it never builds, installs, or executes any
  of it.
- The files in scope are **sent to Anthropic** through your `claude` CLI,
  under your account, so your plan's data terms apply. Each spec in
  `--against` sends the files again, so three specs cost three times the
  usage.
- Files that look like secrets (`.env*` except `.env.example`-style
  templates, private keys, `.npmrc`, `.netrc`, cloud credentials, …) are
  withheld by default and listed as skipped. This is a filename heuristic:
  secrets inside ordinary source files are still sent.
- The `claude` process runs as a plain text completion: **no tools, no MCP
  servers, no hooks, no user or project settings**, in an empty temporary
  directory, with a 15-minute timeout per spec. A malicious subject can try
  to talk the model into a clean verdict, but it can't make it run anything
  on your machine.
- Subject files are fenced with a random per-run marker, the model is told
  everything inside is data, and attempts to steer the audit are reported
  as findings.
- If the subject contains code the model itself won't describe, Claude's
  safety filter cuts its reply short. oaudit reports that as an
  `oaudit-safety-stop` finding (critical under untrusted specs, high under
  trusted ones) instead of failing, because it almost always means
  something in there is malicious.

## Limits

- Per file: 256 KB. Larger files, binaries and unreadable files are skipped
  and listed in the output.
- Per run: 5,000 files and 8 MB. The model's context window usually runs
  out first: a subject that doesn't fit fails with exit 2 and a "too large
  for a single audit request" message. Narrow it with `--scope` or audit
  subdirectories separately. Large subjects aren't chunked.
- `oaudit repo` needs the repository root; for a subdirectory, pass the
  root or use `oaudit file <subdir>`.

## Limitations

- Results are LLM judgments. A clean report is **not** proof that something
  is safe, especially for untrusted code; treat it as one strong signal.
  Runs can vary.
- The model sees only the files' current contents: no git history, no
  package registry or CVE lookups, no running code. The
  `deterministic_checks` listed in spec frontmatter aren't run yet; the
  model looks for those patterns itself.

## Security

See [SECURITY.md](SECURITY.md) for the threat model and how to report a
vulnerability.

## License

MIT OR Apache-2.0.
