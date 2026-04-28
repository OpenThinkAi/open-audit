# Releasing open-audit

Distribution is driven by [`dist`](https://opensource.axo.dev/cargo-dist/).
The full release loop is **fully automated from a Cargo.toml version bump**:

```
bump Cargo.toml → stamp loop → merge → mirror to GH
   ↓
auto-tag.yml fires on push to main, creates v<version> tag
   ↓
release.yml fires on the tag, builds binaries + GitHub Release + npm publish via OIDC
```

No manual tag pushes, no `gh release` commands, no `npm publish` from
your laptop. Bump the version, ship it through the stamp loop, and CI
handles the rest.

## Targets shipped

- `aarch64-apple-darwin` (Apple Silicon)
- `x86_64-apple-darwin` (Intel macOS)
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`

**Not yet shipped — short list for v0.2:**

- Windows (`x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`). Adds an
  installer matrix entry + a bit of `dist` config. No blocker, just
  deferred to keep v1 small.

## Bootstrap (one-time, two pieces)

### 1. npm package + Trusted Publisher

OIDC Trusted Publishing requires the npm package to exist before it can
trust a workflow. So the very first publish has to happen with an npm
auth token, locally. Once the package exists and the Trusted Publisher
is configured, CI publishes from then on without any token.

```sh
# Make sure cargo and dist are installed
brew install cargo-dist cargo-zigbuild zig

# Verify the local build is clean
cargo test
dist plan                   # prints what would be built/published

# Bump version in Cargo.toml (e.g., 0.1.0)

# Build artifacts locally
dist build --artifacts=all

# Publish the npm wrapper to claim the name (you'll be prompted for 2FA)
npm publish --access public ./target/distrib/open-audit-npm-package.tar.gz

# Configure Trusted Publishing on npm (one-time UI step):
#   https://www.npmjs.com/package/open-audit/access
#     → Trusted Publishers → Add publisher
#     → repository:  OpenThinkAi/open-audit
#     → workflow:    release.yml
#     → environment: (leave blank)
```

After the Trusted Publisher is configured, you never publish manually again.

### 2. RELEASE_PAT secret for auto-tag

`auto-tag.yml` pushes the tag using a personal access token, NOT
`GITHUB_TOKEN`. This is required because tags pushed by `GITHUB_TOKEN`
do not trigger downstream workflows (loop-prevention). Without `RELEASE_PAT`,
auto-tag will create the tag but `release.yml` will never fire.

**To create the PAT:**

1. https://github.com/settings/personal-access-tokens/new
2. Resource owner: `OpenThinkAi`
3. Repository access: **Only select repositories** → `OpenThinkAi/open-audit`
4. Permissions → Repository permissions:
   - **Contents: Read and write** (needed to push tags)
   - everything else: No access
5. Expiration: 1 year (set a calendar reminder to rotate)
6. Copy the generated `github_pat_*` token

**Add as repo secret:**

1. https://github.com/OpenThinkAi/open-audit/settings/secrets/actions
2. New repository secret
3. Name: `RELEASE_PAT`
4. Value: paste the PAT
5. Save

That's it. Auto-tag uses it on the next push to main. Rotate yearly.

## Cutting a release (recurring — the whole flow)

```sh
# 1. Land your changes via the normal stamp loop on feature branches.

# 2. Bump version in Cargo.toml on a release branch.
git checkout -b release/v0.1.1
# edit Cargo.toml: version = "0.1.1"
git add Cargo.toml Cargo.lock && git commit -m "release: bump to v0.1.1"

# 3. Send through the stamp loop (the bump is a reviewable change).
stamp review --diff main..release/v0.1.1
stamp merge release/v0.1.1 --into main
stamp push main
```

That's it. CI handles everything else:

1. Stamp server post-receive hook mirrors `main` to GitHub.
2. `auto-tag.yml` fires on the GH push, reads `Cargo.toml`, sees `0.1.1`,
   creates and pushes the `v0.1.1` tag (using `RELEASE_PAT`).
3. `release.yml` fires on the new tag:
   - Cross-compiles binaries for all configured targets via `cargo-zigbuild`
   - Creates a GitHub Release at `v0.1.1` with binaries + checksums attached
   - Publishes `open-audit@0.1.1` to npm via OIDC + `--provenance`

The npm package is a thin wrapper that downloads the matching platform
binary on `npm install`.

## Manual release verification

After CI completes, verify:

```sh
# Binaries on GH Releases
gh release view v0.1.1 --json assets --jq '.assets[].name'

# npm package landed with provenance
npm view open-audit@0.1.1 dist
npm view open-audit@0.1.1 _npmUser  # should show OIDC publisher

# Quick install smoke test
npm install -g open-audit@0.1.1
oaudit --version    # should print 0.1.1
oaudit explain trusted/security | head -5
```

## When `dist init` regenerates the workflow

`dist init` (and `dist generate`) overwrite `.github/workflows/release.yml`.
We've patched the `publish-npm` job to use OIDC instead of NPM_TOKEN —
those patches are documented inline (see the comment block above the job).
After any regen, re-apply the diff or your CI will silently fall back to
needing NPM_TOKEN.

If we set this up enough times to be annoying, consider filing an issue
upstream for an `npm-trusted-publishing = true` config knob.
