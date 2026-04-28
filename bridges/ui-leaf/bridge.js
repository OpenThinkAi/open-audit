#!/usr/bin/env node
// oaudit ↔ ui-leaf bridge.
//
// Reads ONE JSON request from stdin (line-delimited), starts ui-leaf via
// the Node SDK, then emits line-delimited JSON events on stdout until the
// view closes.
//
// Stdin (one line):
//   { "view": string, "data": any, "viewsRoot"?: string, "title"?: string }
//
// Stdout (one or more lines):
//   { "type": "ready",  "url": string, "port": number }
//   { "type": "closed" }
//   { "type": "error",  "message": string }
//
// stderr is reserved for human-readable diagnostics. Do not parse it.
//
// Note: ui-leaf v0.1.3 writes its own banner + rsbuild's start/ready lines
// to process.stdout. We capture stdout.write here BEFORE importing ui-leaf,
// then route ui-leaf's writes to stderr so our protocol channel stays clean.
// (Friction log: ui-leaf needs a `silent: true` option for library consumers.)

import { fileURLToPath } from "node:url";
import { dirname, resolve as pathResolve } from "node:path";

const realStdoutWrite = process.stdout.write.bind(process.stdout);
process.stdout.write = (chunk, encodingOrCb, maybeCb) => {
  return process.stderr.write(chunk, encodingOrCb, maybeCb);
};

// Import AFTER the patch so ui-leaf and its transitive rsbuild writes are diverted.
const { mount } = await import("ui-leaf");

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEFAULT_VIEWS_ROOT = pathResolve(__dirname, "views");

function emit(obj) {
  realStdoutWrite(`${JSON.stringify(obj)}\n`);
}

async function readOneLine() {
  return new Promise((resolveLine, rejectLine) => {
    let buf = "";
    const onData = (chunk) => {
      buf += chunk.toString("utf8");
      const nl = buf.indexOf("\n");
      if (nl !== -1) {
        process.stdin.removeListener("data", onData);
        process.stdin.removeListener("end", onEnd);
        process.stdin.removeListener("error", onError);
        resolveLine(buf.slice(0, nl));
      }
    };
    const onEnd = () => rejectLine(new Error("stdin closed before request received"));
    const onError = (e) => rejectLine(e);
    process.stdin.on("data", onData);
    process.stdin.on("end", onEnd);
    process.stdin.on("error", onError);
  });
}

async function main() {
  let req;
  try {
    const line = await readOneLine();
    req = JSON.parse(line);
  } catch (e) {
    emit({ type: "error", message: `failed to parse stdin request: ${e.message}` });
    process.exit(2);
  }

  if (typeof req.view !== "string" || !req.view) {
    emit({ type: "error", message: "request missing required field `view` (string)" });
    process.exit(2);
  }

  const viewsRoot = req.viewsRoot
    ? pathResolve(req.viewsRoot)
    : DEFAULT_VIEWS_ROOT;

  let view;
  try {
    view = await mount({
      view: req.view,
      data: req.data ?? {},
      viewsRoot,
      title: req.title,
    });
  } catch (e) {
    emit({ type: "error", message: `mount() failed: ${e.message}` });
    process.exit(2);
  }

  emit({ type: "ready", url: view.url, port: view.port });

  await view.closed;
  emit({ type: "closed" });
  process.exit(0);
}

main().catch((e) => {
  emit({ type: "error", message: `bridge crashed: ${e.message ?? e}` });
  process.exit(1);
});
