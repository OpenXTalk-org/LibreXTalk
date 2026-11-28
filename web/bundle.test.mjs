// Tests of the single-file build (tools/bundle.mjs) in Node.js: build it,
// then run the engine part of its script, with the engine taken from the
// base64 inside the page rather than from the .wasm file.
//
//   cargo build -p xt-wasm --profile wasm-release --target wasm32-unknown-unknown
//   node --test web/bundle.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const bundler = fileURLToPath(new URL("../tools/bundle.mjs", import.meta.url));
const out = join(mkdtempSync(join(tmpdir(), "xt-bundle-")), "librextalk.html");
execFileSync(process.execPath, [bundler, out], { stdio: "pipe" });
const html = readFileSync(out, "utf8");

const start = html.lastIndexOf('<script type="module">') + '<script type="module">'.length;
const script = html.slice(start, html.indexOf("</script>", start));

test("the page needs no other files", () => {
  assert.doesNotMatch(script, /^\s*import\s/m, "no imports left");
  assert.doesNotMatch(html, /<script[^>]+src=/, "no external scripts");
  assert.match(script, /const XT_WASM_BASE64 = "[A-Za-z0-9+/=]{1000,}";/);
});

test("the engine inside the page runs scripts", async () => {
  // Everything before the page's own code: the base64 engine and the
  // inlined modules. Export what the test needs and import it as a module.
  const enginepart = script.slice(0, script.indexOf("// ----- web/index.html -----"));
  const source = `${enginepart}\nexport { XtEngine, XT_WASM_BASE64 };`;
  const mod = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
  const bytes = Uint8Array.from(atob(mod.XT_WASM_BASE64), (c) => c.charCodeAt(0));
  const engine = await mod.XtEngine.instantiate(bytes);
  const out = [];
  await engine.runScript('put "from the single file"\nput 6 * 7', { message: (t) => out.push(t) });
  assert.deepEqual(out, ["from the single file", "42"]);
});
