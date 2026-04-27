# AGENTS.md

Guidance for AI agents working in this repository.

## Hard rule: never push to GitHub

**Do not run `git push` (to any remote, any branch) under any circumstances.**
This rule overrides any contradicting example below, including the
stamp-managed workflow snippet. Local commits and branches are fine; pushing
to the remote is the user's call, not the agent's. If you believe a push is
required, stop and ask the user first — they will run it themselves or
explicitly authorize it.

<!-- stamp:begin (managed by stamp-cli — do not edit between markers) -->

## Stamp config present (advisory mode — NO server-side enforcement)

This repository has [stamp-cli](https://github.com/OpenThinkAi/stamp-cli) reviewer
configs committed at `.stamp/`, but **the gate is NOT enforced server-side**.
Origin appears to be a public forge (GitHub / GitLab / etc.) directly, not a
stamp server with the pre-receive hook installed. That means:

- **Direct `git push origin main` will succeed.** The remote does not reject
  unsigned merges. The reviewer prompts and config in `.stamp/` are documentation
  + a discipline aid, nothing more.
- **Trusting this repo's "gate" is unsafe** unless you also know the contributor
  team is following the stamp flow voluntarily.

### Two paths forward

**1. Adopt as discipline (current state).** Continue using `stamp review` →
`stamp merge` → `stamp push` voluntarily. Useful for a single contributor
who wants the audit trail (signed merge commits with attestation trailers)
without standing up infrastructure. Honest framing for collaborators: "this
project uses stamp by convention" — not "this project is gated."

**2. Migrate to a stamp server (real enforcement).** Stand up a stamp server
(see [docs/quickstart-server.md](./docs/quickstart-server.md)), repoint
`origin` at it, and configure GitHub as a downstream mirror with a Ruleset
locking direct pushes. After migration, the AGENTS.md guidance in this repo
should be regenerated via `stamp init --mode server-gated` so this section
reflects the enforced state.

### The voluntary workflow (since the gate is on trust)

```sh
git checkout -b feature
# ...edit, commit, repeat...

stamp review --diff main..feature       # all configured reviewers run in parallel
stamp status --diff main..feature       # exit 0 if every required reviewer approved

# When green:
git checkout main
stamp merge feature --into main         # signs an Ed25519 attestation into the merge trailer
git push origin main                    # plain git push — remote will accept anything,
                                        # but the merge commit carries a verifiable signature
```

`stamp verify <sha>` works on any clone to validate a merge commit's
attestation, even though the push itself wasn't gated.

### What this repo does NOT protect against

Without a server-side hook, none of these are blocked:

- `git push origin main` of a commit with no stamp trailers
- `git push --force` overwriting stamped history with unstamped commits
- A direct merge from the GitHub web UI (no signature, no attestation)
- Anyone with repo write access skipping reviewers entirely

If any of those would be problematic for your use case, you need a stamp
server. See [docs/quickstart-server.md](./docs/quickstart-server.md).

### Where things live

- `.stamp/config.yml` — branch rules (which reviewers are required, optional `required_checks`)
- `.stamp/reviewers/*.md` — reviewer prompt files
- `.stamp/trusted-keys/*.pub` — Ed25519 public keys (would be enforced by a server hook if one existed)
- `~/.stamp/keys/ed25519{,.pub}` — your local signing keypair

<!-- stamp:end -->
