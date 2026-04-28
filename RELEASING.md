# Releasing open-audit

Distribution is driven by [`dist`](https://opensource.axo.dev/cargo-dist/)
on every git tag matching `v*.*.*`. CI cross-compiles the `oaudit` binary
for the configured targets, attaches them to a GitHub Release, and publishes
the npm wrapper package (`open-audit`) via OIDC Trusted Publishing.

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

## Bootstrap (one-time)

OIDC Trusted Publishing requires the npm package to exist before it can
trust a workflow. So the very first publish has to happen with an npm
auth token, locally. After that, CI takes over.

```sh
# 1. Make sure cargo and dist are installed
brew install cargo-dist     # provides the `dist` binary

# 2. Verify the local build is clean
cargo test
dist plan                   # prints what would be built/published

# 3. Bump version in Cargo.toml (e.g., 0.1.0)

# 4. Build artifacts locally (optional sanity check)
dist build --artifacts=all

# 5. Manually publish the npm wrapper to claim the name.
#    The wrapper tarball is in ./target/distrib/ after `dist build`.
npm config set //registry.npmjs.org/:_authToken $NPM_TOKEN
npm publish --access public ./target/distrib/open-audit-npm-package.tar.gz

# 6. Configure Trusted Publishing on npm:
#    https://www.npmjs.com/package/open-audit/access
#      → Trusted Publishers → Add publisher
#      → repository:  OpenThinkAi/open-audit
#      → workflow:    .github/workflows/release.yml
#      → environment: (leave blank)
```

After step 6, every subsequent release is `git tag vX.Y.Z && git push --tags`
and CI handles it.

## Cutting a release (recurring)

```sh
# 1. Land changes via the normal stamp loop on a feature branch.
# 2. On main, bump version in Cargo.toml.
git checkout -b release/v0.1.1
# edit Cargo.toml: version = "0.1.1"
git add Cargo.toml && git commit -m "release: v0.1.1"

# 3. Send through stamp loop (the version bump itself is a reviewable change).
stamp review --diff main..release/v0.1.1
stamp merge release/v0.1.1 --into main
stamp push main

# 4. Tag the merge commit and push the tag.
git tag v0.1.1
git push origin v0.1.1
```

CI takes over from the tag push:
- Cross-compiles binaries for all configured targets
- Creates a GitHub Release at `v0.1.1` with the binaries attached
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
