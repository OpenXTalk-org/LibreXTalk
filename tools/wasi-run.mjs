#!/usr/bin/env node
// Run a WebAssembly program built for wasm32-wasip1 in Node.js, so that
// `cargo test --target wasm32-wasip1` runs the engine's own tests as
// WebAssembly (CLAUDE.md 5.18). Cargo calls this with the test program's
// path and its arguments; see .cargo/config.toml.

import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
import process from "node:process";

const [wasmPath, ...args] = process.argv.slice(2);
const wasi = new WASI({
  version: "preview1",
  args: [wasmPath, ...args],
  env: process.env,
  // The tests need no files; give them none.
  preopens: {},
  returnOnExit: true,
});
const module = await WebAssembly.compile(await readFile(wasmPath));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
process.exitCode = wasi.start(instance);
