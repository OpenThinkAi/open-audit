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

`ui-leaf --version` works fine and `ui-leaf --help` lists commands, but
`ui-leaf` (when installed as a binary) is *only* a stub — `ui-leaf mount`
prints help and exits. We mention this to set expectations: if a non-Node
user `npm install -g ui-leaf` and tries `ui-leaf mount`, they get help
output, not a working command. The README does say "the Node SDK is
functional"; could be more explicit about the binary status.

### 7. ui-leaf's own stderr noise dominates the consumer's status output

After workaround #1 redirects stdout-writes to stderr, the consumer sees
on stderr (in this order, on every `--open` invocation):

```
  ➜  Local:    http://127.0.0.1:5810/

start   build started...
warn    [rsbuild] `dev.setupMiddlewares` is deprecated, use `server.setup` instead
ready   built in 0.08 s
```

…interleaved with the consumer's own `eprintln!("oaudit: view ready at
{url} (close the tab to exit)")`. The user's actionable line is buried
in build-tool noise on the very first run.

This is closely tied to #1 (the lack of a `silent:` option). The same fix
helps both — a `silent: true` MountOption that suppresses ui-leaf's banner,
the rsbuild lifecycle messages, *and* the deprecation warning would let
consumers present a clean output by default and opt into verbosity.

We did NOT work around this in the Rust caller, per Matt's instruction:
"if it's clearly ui-leaf's missing piece rather than your problem to solve,
report it back rather than working around it. The friction is the data."

### 8. `port: 5810` default collides on concurrent invocations

ui-leaf's `MountOptions.port` defaults to `5810`. Two simultaneous
`oaudit explain X --open` invocations would have the second one fail —
ui-leaf documents that it auto-bumps to the next free port, but the bridge
was passing the default explicitly so there was nothing to bump.

**Workaround:** the bridge now passes `port: 0` to let the OS pick a free
port. ui-leaf reflects the bound port in `mount()`'s return value, which
the bridge re-emits as `{"type":"ready","url":...,"port":...}`. Works
cleanly.

This isn't ui-leaf's fault — `port: 0` is the standard Node-ecosystem way
to ask for "any free port" and ui-leaf supports it correctly. Worth
documenting in the README that programmatic consumers should pass `port: 0`
unless they specifically need a stable URL; the default `5810` is more for
human-direct-use.

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
- 5 design / UX issues (silent option, language-neutral binary, viewsRoot
  default, deprecation warning, lifecycle docs, binary stub clarity,
  stderr noise dominating consumer output)
- 1 issue with a clean consumer-side workaround (port: 0 for concurrency)
- 0 issues we couldn't work around
- ~45 min from `npm install` to working bridge end-to-end (including
  iterating on stdout-redirect and concurrent-invocation handling)

A `silent: true` MountOption would address #1 + #7 (the two highest-impact
items) in one stroke. Combined with the language-neutral binary on the
roadmap, that would let a Rust/Go/Python consumer have a clean integration
in well under an hour.
