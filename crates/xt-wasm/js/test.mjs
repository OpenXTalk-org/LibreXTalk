// Tests of the WebAssembly engine, run in Node.js with its built-in test
// runner. Build the module first:
//
//   cargo build -p xt-wasm --profile wasm-release --target wasm32-unknown-unknown
//   node --test crates/xt-wasm/js/test.mjs
//
// XT_WASM can point at another build of xt_wasm.wasm.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { XtEngine } from "./xt.mjs";

const wasmPath =
  process.env.XT_WASM ??
  fileURLToPath(
    new URL("../../../target/wasm32-unknown-unknown/wasm-release/xt_wasm.wasm", import.meta.url),
  );
const wasmBytes = await readFile(wasmPath);

/** A fresh engine and a list collecting what `put` shows. */
async function engine() {
  const e = await XtEngine.instantiate(wasmBytes);
  const out = [];
  return { e, out, message: (t) => out.push(t) };
}

test("a script runs and its output comes back", async () => {
  const { e, out, message } = await engine();
  await e.runScript('put "hello"\nput 6 * 7\nput word 2 of "a b c"', { message });
  assert.deepEqual(out, ["hello", "42", "b"]);
});

test("handlers and functions work", async () => {
  const { e, out, message } = await engine();
  await e.runScript(
    "function twice n\n  return n * 2\nend twice\nrepeat with i = 1 to 3\n  put twice(i)\nend repeat",
    { message },
  );
  assert.deepEqual(out, ["2", "4", "6"]);
});

test("a wait becomes a sleep the page controls", async () => {
  const { e, out, message } = await engine();
  const sleeps = [];
  await e.runScript('put 1\nwait 2 seconds\nput 2', {
    message,
    sleep: async (ms) => sleeps.push(ms),
  });
  assert.deepEqual(out, ["1", "2"]);
  assert.ok(sleeps.length >= 1);
});

test("dialogs are answered by async handlers", async () => {
  const { e, out, message } = await engine();
  const seen = [];
  await e.runScript(
    [
      'answer "Sure?" with "Yes" or "No"',
      "put it",
      'ask "Name?" with "Anon"',
      "put it",
      'answer file "Open:" of type "txt"',
      'put it & "/" & the result',
    ].join("\n"),
    {
      message,
      answer: async (req) => {
        seen.push(req);
        return 1;
      },
      ask: async (req) => `${req.default}ymous`,
      dialog: async () => null,
    },
  );
  assert.deepEqual(out, ["No", "Anonymous", "/Cancel"]);
  assert.deepEqual(seen[0], { kind: "answer", prompt: "Sure?", buttons: ["Yes", "No"] });
});

test("a stack saved by the page opens again", async () => {
  const disk = new Map();
  const files = {
    save: async (req) => disk.set(req.path, req.text),
    open: async (req) => {
      if (!disk.has(req.path)) throw new Error(`No such file: ${req.path}`);
      return disk.get(req.path);
    },
  };
  {
    const { e, message } = await engine();
    await e.runScript(
      'create stack "Web"\ncreate button "Go"\nsave this stack as "web.xtstack.html"',
      { message, ...files },
    );
  }
  assert.match(disk.get("web.xtstack.html"), /<xt-button id="3" name="Go"><\/xt-button>/);

  // A new engine, as if the page had been loaded again.
  const { e, out, message } = await engine();
  await e.runScript(
    [
      'open stack "web.xtstack.html"',
      "put the name of this stack",
      "put button 1",
      "try",
      '  open stack "missing.xtstack.html"',
      "catch err",
      '  put "caught: " & err',
      "end try",
    ].join("\n"),
    { message, ...files },
  );
  assert.deepEqual(out, [
    "Web",
    "button id 3 of stack id 2",
    "caught: No such file: missing.xtstack.html",
  ]);
});

test("errors reject with the message and line", async () => {
  const { e } = await engine();
  await assert.rejects(e.runScript('put 1\nput 1 + "x"'), (err) => {
    assert.equal(err.line, 2);
    return true;
  });
  // The engine is ready for the next script.
  const out = [];
  await e.runScript("put 5", { message: (t) => out.push(t) });
  assert.deepEqual(out, ["5"]);
});

test("the time is the page's time", async () => {
  const { e, out, message } = await engine();
  const before = Math.floor(Date.now() / 1000);
  await e.runScript("put the seconds\nput random(6) <= 6", { message });
  const seconds = Number(out[0]);
  assert.ok(seconds >= before && seconds <= before + 5, `seconds: ${out[0]}`);
  assert.equal(out[1], "true");
});

test("shell access is never available", async () => {
  const { e } = await engine();
  await assert.rejects(e.runScript('do "ls" as sh'), /does not allow shell/);
});
