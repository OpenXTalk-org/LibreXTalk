#!/usr/bin/env node
// bundle.mjs: build libreXTalk's browser page as ONE self-contained HTML
// file that works when opened straight from disk, with no server.
//
//   cargo build -p xt-wasm --profile wasm-release --target wasm32-unknown-unknown
//   node tools/bundle.mjs            writes target/web/librextalk.html
//   node tools/bundle.mjs out.html   writes somewhere else
//
// XT_BUILD_LABEL, if set, is shown next to the title, for example
// "test build 5e6f168" on the public site (.github/workflows/pages.yml).
//
// Why: a page opened as a file (file://) may not fetch other files, so it
// can't load the .wasm, and Chrome also refuses to import JavaScript
// modules from files. Neither rule stops WebAssembly itself from running,
// so this build puts the engine into the page as base64 text and inlines
// the JavaScript modules into the page's own script. No asm.js is needed:
// every current browser runs WebAssembly.

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const at = (p) => resolve(root, p);
const out = resolve(process.argv[2] ?? at("target/web/librextalk.html"));

// The modules the page imports, in an order where each comes after what it
// uses. They are written as plain modules, so inlining them only means
// dropping their `import` lines and the word `export`.
const MODULES = [
  "crates/xt-wasm/js/xt.mjs",
  "web/library.mjs",
  "web/host.mjs",
  "web/ui.mjs",
];

const IMPORT = /^import\s.*?from\s+["'][^"']+["'];?\s*$/gm;

function inline(path, text) {
  const body = text.replace(IMPORT, "").replace(/^export\s+(?=(class|function|const|let|async)\b)/gm, "");
  if (/^\s*(import|export)\b/m.test(body)) {
    throw new Error(`${path}: an import or export this bundler can't inline`);
  }
  return `// ----- ${path} -----\n${body}`;
}

/** Top-level names a module declares, to catch two modules using one. */
function topLevelNames(text) {
  const names = [];
  for (const m of text.matchAll(/^(?:export\s+)?(?:async\s+)?(?:class|function|const|let)\s+([A-Za-z_$][\w$]*)/gm)) {
    names.push(m[1]);
  }
  return names;
}

const wasm = await readFile(at("target/wasm32-unknown-unknown/wasm-release/xt_wasm.wasm")).catch(() => {
  throw new Error(
    "Build the engine first: cargo build -p xt-wasm --profile wasm-release --target wasm32-unknown-unknown",
  );
});
const page = await readFile(at("web/index.html"), "utf8");

// The page's own module script: the last <script type="module"> block.
const open = page.lastIndexOf('<script type="module">');
const close = page.indexOf("</script>", open);
if (open < 0 || close < 0) throw new Error("web/index.html has no module script");
const pageScript = page.slice(open + '<script type="module">'.length, close);

const seen = new Map();
const parts = [];
for (const path of MODULES) {
  const text = await readFile(at(path), "utf8");
  for (const name of topLevelNames(text)) {
    if (seen.has(name)) throw new Error(`${path} and ${seen.get(name)} both declare ${name}`);
    seen.set(name, path);
  }
  parts.push(inline(path, text));
}
for (const name of topLevelNames(pageScript)) {
  if (seen.has(name)) throw new Error(`web/index.html and ${seen.get(name)} both declare ${name}`);
}

const script = [
  "// Built by tools/bundle.mjs: the libreXTalk engine and page in one file.",
  `const XT_WASM_BASE64 = "${wasm.toString("base64")}";`,
  ...parts,
  "// ----- web/index.html -----",
  pageScript.replace(IMPORT, ""),
].join("\n");
// Text inside a <script> element ends at the first "</script", so none of
// the inlined code may contain one.
if (/<\/script/i.test(script)) throw new Error("The inlined code contains </script");

let html = page.slice(0, open) + '<script type="module">\n' + script + "\n" + page.slice(close);
const label = process.env.XT_BUILD_LABEL?.trim();
if (label) {
  const escaped = label.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  html = html.replace("<h1>libreXTalk</h1>", `<h1>libreXTalk</h1>\n  <span class="status build">${escaped}</span>`);
}
await mkdir(dirname(out), { recursive: true });
await writeFile(out, html);
console.log(`Wrote ${out} (${Math.round(html.length / 1024)} KB). Open it from disk; no server needed.`);
