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
- `x86_64-unknown-linux-musl`

**Not yet shipped — short list for v0.2:**

- **`aarch64-unknown-linux-gnu` / `aarch64-unknown-linux-musl`** (ARM
  Linux). Cross-compiling these from a single macOS runner via
  cargo-zigbuild fails on `libz-sys`'s build script — `ar` (zig wrapper)
  can't produce `libz.a` for the ARM target. The fix is one of:
  switch to dist's matrix-per-target shape (one Linux runner does the
  Linux ARM builds natively); install a real aarch64-linux cross
  toolchain instead of zig; or vendor a precompiled libz. None are v1
  blockers.
- **Windows (`x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`).** Same
  matrix-per-target reshape would unlock these too. Deferred.

## Bootstrap (one-time)

OIDC Trusted Publishing requires the npm package to exist before it can
trust a workflow. So the very first publish happens with an npm auth
token, locally. After that, CI takes over with zero secrets.

```sh
# Tools
brew install cargo-dist cargo-zigbuild zig

# Verify the local build is clean
cargo test
dist plan

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

After the Trusted Publisher is configured, every release is driven by a
Cargo.toml version bump — no tokens, no manual tag pushes.

## Cutting a release (recurring — the whole flow)

```sh
# 1. Land your changes via the normal stamp loop on feature branches.

# 2. Bump version in Cargo.toml on a release branch.
git checkout -b release/v0.1.1
# edit Cargo.toml: version = "0.1.1"
git add Cargo.toml Cargo.lock && git commit -m "release: bump to v0.1.1"

# 3. Send through the stamp loop.
stamp review --diff main..release/v0.1.1
stamp merge release/v0.1.1 --into main
stamp push main
```

That's it. CI handles the rest:

1. Stamp server post-receive hook mirrors `main` to GitHub.
2. `release.yml` fires on the GitHub push to main:
   - Reads version from `Cargo.toml` (skips if `open-audit@<version>` already on npm)
   - Cross-compiles binaries for all configured targets via `cargo-zigbuild`
   - Creates a GitHub Release at `v<version>` with binaries + checksums
   - Publishes `open-audit@<version>` to npm via OIDC + `--provenance`

Idempotent: any push to main that doesn't bump the version is a no-op.

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

`dist init` and `dist generate` will try to overwrite
`.github/workflows/release.yml` with their own tag-triggered version. Our
workflow is a custom rewrite — push-to-main + version detection, matching
the OpenThinkAi pattern (stamp-cli, ui-leaf). `allow-dirty = ["ci"]` in
`Cargo.toml` tells dist to leave the file alone.

If you regenerate, restore the workflow from git instead of re-applying
the dist default.
