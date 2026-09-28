# Security policy

## Supported versions

Only the latest published version of `@openthink/audit` (npm) / the latest
GitHub Release of `oaudit` receives security fixes. Upgrade promptly.

## Reporting a vulnerability

**Do not open a public GitHub issue for security reports.**

Use GitHub's private vulnerability reporting for this repository:
<https://github.com/OpenThinkAi/open-audit/security/advisories/new>

If that page isn't available, open a public issue that says only that you
have a security report and would like a private contact. Don't include any
details in it.

Include in the report:

- A description of the issue and the affected component (CLI, subject
  loading, the `claude` subprocess invocation, built-in specs, the ui-leaf
  bridge, release artifacts).
- A reproduction or proof of concept, if you have one.
- Your assessment of impact.

## Threat model

- **Data leaves your machine.** oaudit sends the contents of the audited
  files to Anthropic, through your own locally installed and authenticated
  `claude` CLI. Only audit code you are allowed to send to Anthropic under
  your account's terms.
- **Secret-looking files are withheld by default.** Files that look like
  credentials (for example `.env*`, private keys, credential stores) are not
  sent. This is a pattern-based heuristic, not a guarantee: secrets embedded
  in ordinary source files will be sent.
- **Subject code is never executed.** oaudit reads files; it does not build,
  install, test, or run anything in the audited tree, and runs no hooks or
  tooling from it.
- **The `claude` child process is locked down.** It runs with all tools
  disabled, no MCP servers, no user or project settings or hooks loaded, and
  an empty temporary working directory. The model can only read the prompt
  oaudit gives it and return text.
- **The subject can't choose its own auditor.** When you run oaudit from
  inside the thing being audited, its `.oaudit/auditors/untrusted/*`
  overrides are ignored, and subject files are fenced with a random per-run
  marker so their content can't impersonate oaudit's instructions.
- **Results are LLM judgments, not guarantees.** Findings can be wrong, and
  runs vary. A clean result is not proof that the code is safe. Audited
  content can try to prompt-inject the auditor into a clean verdict; that
  can't make anything run on your machine, but it can skew the result, so
  treat a clean audit of untrusted code as one signal, not a clearance.

## Out of scope

- Vulnerabilities in Anthropic's models or the `claude` CLI itself — report
  those to Anthropic.
- The quality or accuracy of individual audit findings.
