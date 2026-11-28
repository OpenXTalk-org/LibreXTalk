#!/usr/bin/env node
// serve.mjs: a small web server for trying libreXTalk in a browser.
//
//   node tools/serve.mjs            then open http://127.0.0.1:8000/web/
//   node tools/serve.mjs 9000       on another port
//
// Browsers refuse to load WebAssembly from a page opened as a file, so the
// browser host page needs a server. This one serves the project folder to
// this computer only (127.0.0.1), with no dependencies.

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const port = Number(process.argv[2] ?? 8000);

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url ?? "/", "http://localhost");
    let path = normalize(join(root, decodeURIComponent(url.pathname)));
    // Never serve anything outside the project folder.
    if (path !== root && !path.startsWith(root + sep)) {
      res.writeHead(403).end("Forbidden");
      return;
    }
    if ((await stat(path)).isDirectory()) path = join(path, "index.html");
    const body = await readFile(path);
    res.writeHead(200, {
      "Content-Type": TYPES[extname(path)] ?? "application/octet-stream",
      "Cache-Control": "no-store",
    });
    res.end(body);
  } catch {
    res.writeHead(404, { "Content-Type": "text/plain" }).end("Not found");
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`libreXTalk: http://127.0.0.1:${port}/web/  (Ctrl-C stops)`);
});
