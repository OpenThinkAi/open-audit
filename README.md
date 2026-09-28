# open-audit

`oaudit` audits a whole codebase, file, or piece of text against spec
documents — security, supply chain, infrastructure, LLM security, privacy —
and reports findings as JSON or human-readable text. The judging is done by
Claude through your local [Claude Code](https://claude.com/claude-code) CLI.

It audits an artifact as it is. It is not a diff or PR reviewer.

## Install

```sh
npm install -g @openthink/audit
# or
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/OpenThinkAi/open-audit/releases/latest/download/open-audit-installer.sh | sh
```

Requires the `claude` CLI on your `PATH`, signed in (claude.ai account or
`ANTHROPIC_API_KEY`). Keep it current: oaudit relies on its isolation flags
and refuses to run if they're missing.

## Usage

```sh
oaudit repo ./some-repo                         # default: untrusted/security
oaudit repo . --against trusted/security,trusted/privacy --format human
oaudit file ./vendor/pkg --against untrusted/supply-chain
cat issue.md | oaudit text --label issue-42     # default: untrusted/llm-security
oaudit file ./some-skill --against untrusted/agent-skill   # vet an agent skill
oaudit list                                     # available specs
oaudit explain untrusted/security               # read a spec
```

Exit code is `1` when any finding is high or critical, `0` otherwise, so it
can gate CI. Use `--scope '<glob>'` to narrow what's read.

## Specs and trust modes

Each built-in domain ships in two postures:

- **`trusted/<name>`**: your own code. Looks for mistakes.
- **`untrusted/<name>`**: third-party code you're about to depend on or run.
  Assumes the author may be hostile, looks for malice and concealment, and
  never downgrades suspicious findings.

`--against` takes a catalog name (`untrusted/security`), a comma-separated
list, or a path to your own spec file. You can override a built-in by adding
`.oaudit/auditors/<mode>/<name>.md` to the directory you run oaudit from,
except that a subject can never supply its own `untrusted/*` auditor. If
you run oaudit from inside the thing being audited, local `untrusted/*`
overrides are ignored and the built-in is used.

## Vetting agent skills

`untrusted/agent-skill` is for deciding whether to install a third-party
agent skill (a `SKILL.md` plus scripts), or similar instruction packages
like slash commands, plugin manifests and hook configs. Instead of flagging
every shell call or network request, it compares what the skill *says* it's
for with what it actually does, and only reports capabilities the stated
purpose doesn't need, or that the skill hides. A browser-testing skill that
starts a dev server is fine; one that also reads `~/.aws/credentials` isn't.

Every run includes a short inventory (purpose, capabilities, network
destinations, writes, and an install / install with care / do not install
verdict) so you can check the reasoning in seconds.
`trusted/agent-skill` is the author-side version: it looks for leaked
secrets, over-broad triggers and missing confirmation steps before you
publish.

If the subject contains code the model itself refuses to describe, oaudit
reports that as a critical "safety filter stopped" finding rather than
failing, since it almost always means something in there is malicious.

## What happens to your files

- oaudit **reads** the subject; it never builds, installs, or executes any
  of it.
- The files in scope are **sent to Anthropic** through your `claude` CLI, under
  your account, so the usual Claude data terms apply.
- Files that look like secrets (`.env*`, private keys, `.npmrc`, cloud
  credentials, …) are withheld by default and listed as skipped.
- The `claude` process runs as a plain text completion: **no tools, no MCP
  servers, no hooks, no user or project settings**, in an empty temporary
  directory, with a timeout. A malicious subject can try to talk the model
  into a clean verdict, but it can't make it run anything on your machine.
- Subject files are fenced with a random per-run marker. The model is told
  that everything inside is data, and that attempts to steer the audit are
  themselves findings.

## Limitations

- Results are LLM judgments. A clean report is **not** proof that something
  is safe, especially for untrusted code; treat it as one signal.
- Deterministic checks (secret scanners, CVE lookups) listed in spec
  frontmatter are not run yet; the model does those checks itself.
- `oaudit repo` takes a local path; cloning from a URL isn't supported yet.
- Large subjects are refused rather than chunked; narrow with `--scope`.

## Security

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

MIT OR Apache-2.0.
