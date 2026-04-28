# ui-leaf integration friction log

This file documents pain points encountered while building a Node bridge so a
non-Node CLI (Rust `oaudit`) can drive **ui-leaf v0.1.3** as a subprocess.
Captured as feedback for the ui-leaf author ahead of Spike #4
(language-neutral binary). Date of integration: 2026-04-27.

## What we built

`bridges/ui-leaf/`:
- `package.json` — pulls `ui-leaf` + `react-markdown` from npm
- `bridge.js` — Node ESM script: reads one JSON request from stdin, calls
  `mount()`, emits line-delimited JSON events on stdout (`ready`, `closed`,
  `error`)
- `views/spec.tsx` — single React view that renders `data.markdown` via
  `react-markdown`

Rust spawns `node bridge.js` as a child process, pipes stdin/stdout, and
waits for `closed`.

The whole thing works. Friction below ranges from "design feedback" to
"would have been nice to know."

## Friction items, ranked by impact

### 1. ⚠️  `mount()` writes to `process.stdout` (HIGH IMPACT)

The most blocking issue. ui-leaf 0.1.3's `mount()` writes its own banner
plus rsbuild's `start` / `ready` / `built` lines straight to
`process.stdout`. Specifically:

```
  ➜  Local:    http://127.0.0.1:5810/

start   build started...
ready   built in 0.08 s
```

For any external consumer using stdout as a structured protocol channel
(line-delimited JSON, in our case), this **silently corrupts the
protocol** — the consumer's first JSON parse fails on `"  ➜  Local:..."`.

**Workaround we used.** Monkey-patch `process.stdout.write` *before*
importing ui-leaf, redirect to stderr, keep a captured reference for our
own protocol writes:

```js
const realStdoutWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, enc, cb) => process.stderr.write(chunk, enc, cb);
const { mount } = await import("ui-leaf");
// ...later: realStdoutWrite(JSON.stringify(event) + "\n")
```

This works but is a *footgun* — the patch has to happen before the
import, and any consumer needs to know to do this.

**Suggestions (any one would help):**
- Add a `silent: boolean` option to `MountOptions` (default false to preserve
  current behavior). When `silent: true`, suppress the banner and pass an
  rsbuild log level that quiets `start`/`ready` lines.
- Or: route ui-leaf's own writes through `console.error` / `process.stderr`
  by default, since they're informational diagnostics.
- Or: document this prominently — *"if you're driving mount() programmatically,
  consumers must redirect process.stdout before calling mount()."*

### 2. No language-neutral entry point yet

The README and `0.1.x — pre-1.0, expect churn` status are honest about this,
and Spike #4 is on the roadmap. But it's worth flagging that the gap between
*"any CLI"* (the tagline) and *"any Node CLI"* (the actual surface) is one of
the first things a non-Node user runs into. Even a minimal documented stdio
protocol — basically what we built here — would unblock Rust / Go / Python /
shell-script consumers without the per-language cost of writing their own
bridge.

If you ship the bridge pattern (`ui-leaf serve --request-stdin
--events-stdout` or similar), the consumer only needs `node` on `$PATH` and a
documented JSON contract. They don't need to write or maintain their own
bridge.js per language.

### 3. `viewsRoot` default is `process.cwd() + "/views"` — surprising for non-Node consumers

For a Node CLI invoked from the user's project root, `cwd/views` is natural.
For a bridge that's invoked from somewhere else (or that ships a sibling
`views/` folder), the default is wrong. We had to compute
`pathResolve(__dirname, "views")` manually.

**Suggestion:** when documenting the bridge pattern, note that programmatic
consumers should *always* pass `viewsRoot` explicitly. Or default to
`dirname(callerModulePath) + "/views"` if that's resolvable.

### 4. rsbuild deprecation warning leaks through

```
warn    [rsbuild] `dev.setupMiddlewares` is deprecated, use `server.setup` instead
```

Internal upstream maintenance — not blocking, but visible to every consumer
on every mount.

### 5. Lifecycle on consumer Ctrl+C

`mount()` installs SIGINT/SIGTERM handlers that close the dev server, which
is great when the *Node process* is killed. But when a parent process (our
Rust binary) is killed, the orphaned `node` child process keeps running until
the 75s heartbeat timeout fires.

Bridges in Rust would ideally kill the child explicitly on shutdown, which
we'll add. But it would help if `mount()` exposed a way to override the
default `heartbeatTimeoutMs` to something shorter when the bridge is the
intended supervisor (e.g., 5s instead of 75s), since "browser tab not
heartbeating because parent process died" should resolve fast in that case.

This is `MountOptions.heartbeatTimeoutMs` already — maybe just call it out in
the bridge-pattern docs as something to lower.

### 6. The CLI binary stub

`stamp --version` works fine and `stamp --help` lists commands, but
`ui-leaf` (when installed as a binary) is *only* a stub — `ui-leaf mount`
prints help and exits. We mention this to set expectations: if a non-Node
user `npm install -g ui-leaf` and tries `ui-leaf mount`, they get help
output, not a working command. The README does say "the Node SDK is
functional"; could be more explicit about the binary status.

## What worked well (positive signal)

- **README + examples/hello.ts** got us 90% of the way. The mental model
  ("CLI passes data; view renders; mutations come back as named function
  calls") is clear and right.
- **`MountOptions` JSDoc** is genuinely useful — the inline comments on
  `heartbeatTimeoutMs`, `startupGraceMs`, `port`, etc., are exactly the
  per-option rationale that would otherwise need a separate doc.
- **TypeScript types in `ui-leaf/view`** (`ViewProps<T>`, `Mutate`) made the
  view file (`views/spec.tsx`) trivial to write correctly the first time.
- **React resolution is automatic.** We didn't have to install React in our
  views directory — ui-leaf bundles its own. That removed one thing we
  expected to fight with.
- **Token-gated mutations** — even though we don't use mutations in this
  first cut (read-only view), the design (random per-launch token in
  `window.__UI_LEAF__.token`) is the right shape.

## Items we deferred / didn't hit

- **Mutation round-trip protocol.** Our view is read-only, so we don't yet
  exercise the bridge → CLI → bridge mutation flow. When we do (interactive
  findings triage is the obvious case), the bridge protocol will need to
  expand: bridge emits `{"type":"mutate","id":...,"name":...,"args":...}`,
  Rust responds with `{"type":"result","id":...,"value":...}`. Worth designing
  this protocol in collaboration with ui-leaf if you go ahead with Spike #4.
- **Custom port handling.** We took the default `5810`. A bridge might want
  to ask for `port: 0` (let OS pick) and report back via the `ready` event
  — easy, just haven't needed it yet.
- **Title parameter not yet plumbed.** We pass `title` to `mount()` but the
  served HTML still shows `<title>ui-leaf</title>`. May be a ui-leaf bug or
  may be that we're calling the API wrong; not investigated yet.

## Tally

- 1 blocking issue (stdout collision; worked around with monkey-patch)
- 4 design / UX issues (silent option, language-neutral binary, viewsRoot
  default, deprecation warning, lifecycle docs, binary stub clarity)
- 0 issues we couldn't work around
- ~30 min from `npm install` to working bridge end-to-end

The stdout one is the only place a user will *fail* without working it out —
everything else is just friction the bridge author absorbs. Worth fixing
either with `silent:` or with the language-neutral binary (which you'd
write to be quiet by default anyway).
