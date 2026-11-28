// Tests of the browser host's logic in Node.js, with the real WebAssembly
// engine and a fake page that plays the person. Build the engine first:
//
//   cargo build -p xt-wasm --profile wasm-release --target wasm32-unknown-unknown
//   node --test web/test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { XtEngine } from "../crates/xt-wasm/js/xt.mjs";
import { BrowserHost, EFFECTS } from "./host.mjs";
import { StackLibrary } from "./library.mjs";
import { acceptFor } from "./ui.mjs";

const wasmBytes = await readFile(
  process.env.XT_WASM ??
    fileURLToPath(
      new URL("../target/wasm32-unknown-unknown/wasm-release/xt_wasm.wasm", import.meta.url),
    ),
);

/** A page that answers from a script of replies and records what it saw. */
class FakeUi {
  constructor(replies = {}) {
    this.replies = replies;
    this.seen = [];
    this.out = [];
  }
  message(text) {
    this.out.push(text);
  }
  async answer(prompt, buttons) {
    this.seen.push(["answer", prompt, buttons]);
    return this.replies.answer ?? 0;
  }
  async ask(prompt, initial) {
    this.seen.push(["ask", prompt, initial]);
    return "ask" in this.replies ? this.replies.ask : initial;
  }
  async pickFile(prompt, types) {
    this.seen.push(["pickFile", prompt, types]);
    return this.replies.file ?? null;
  }
  async color(prompt, initial) {
    this.seen.push(["color", prompt, initial]);
    return this.replies.color ?? null;
  }
  async choose(prompt, options) {
    this.seen.push(["choose", prompt, options]);
    return this.replies.choose ?? null;
  }
  async notice(text) {
    this.seen.push(["notice", text]);
  }
}

/** Fake localStorage. */
class FakeStorage {
  map = new Map();
  get length() {
    return this.map.size;
  }
  key(i) {
    return [...this.map.keys()][i] ?? null;
  }
  getItem(k) {
    return this.map.has(k) ? this.map.get(k) : null;
  }
  setItem(k, v) {
    this.map.set(k, String(v));
  }
  removeItem(k) {
    this.map.delete(k);
  }
}

async function host(replies, library = new StackLibrary(new FakeStorage())) {
  const ui = new FakeUi(replies);
  const engine = await XtEngine.instantiate(wasmBytes);
  return { h: new BrowserHost(engine, ui, library), ui, library };
}

test("answer and ask go to the page's dialogs", async () => {
  const { h, ui } = await host({ answer: 1, ask: "Ada" });
  await h.run('answer "Sure?" with "Yes" or "No"\nput it\nask "Name?" with "x"\nput it');
  assert.deepEqual(ui.out, ["No", "Ada"]);
  assert.deepEqual(ui.seen, [
    ["answer", "Sure?", ["Yes", "No"]],
    ["ask", "Name?", "x"],
  ]);
});

test("a cancelled ask empties it and sets the result", async () => {
  const { h, ui } = await host({ ask: null });
  await h.run('ask "Name?"\nput "[" & it & "] " & the result');
  assert.deepEqual(ui.out, ["[] Cancel"]);
});

test("answer file returns the picked file's name, which open stack can use", async () => {
  // A stack saved by one engine, picked as a file by the person in another.
  const first = await host({});
  await first.h.run('create stack "Picked"\nsave this stack as "p.xtstack.html"');
  const text = first.library.open("p.xtstack.html");

  const { h, ui } = await host({ file: { name: "from-disk.xtstack.html", text } });
  await h.run(
    'answer file "Open which stack?" of type "html"\nput it\nopen stack it\nput the name of this stack',
  );
  assert.deepEqual(ui.out, ["from-disk.xtstack.html", "Picked"]);
  assert.deepEqual(ui.seen[0], ["pickFile", "Open which stack?", ["html"]]);
});

test("a cancelled file pick cancels", async () => {
  const { h, ui } = await host({});
  await h.run('answer file "Open:"\nput the result');
  assert.deepEqual(ui.out, ["Cancel"]);
});

test("the other dialog forms", async () => {
  const { h, ui } = await host({ color: "#336699", choose: "dissolve", ask: "notes.txt" });
  await h.run(
    [
      'answer color with "#ff0000"',
      "put it",
      "answer effect",
      "put it",
      'ask file "Save as:" with "a.txt"',
      "put it",
      "answer record",
      "put the result",
    ].join("\n"),
  );
  assert.deepEqual(ui.out, ["#336699", "dissolve", "notes.txt", "Cancel"]);
  assert.deepEqual(ui.seen[1], ["choose", "Choose a visual effect:", EFFECTS]);
  assert.equal(ui.seen[3][0], "notice");
});

test("saved stacks are kept in the browser's storage and open again", async () => {
  const storage = new FakeStorage();
  const first = await host({}, new StackLibrary(storage));
  await first.h.run(
    'create stack "Kept"\ncreate button "B"\nsave this stack as "kept.xtstack.html"',
  );
  assert.deepEqual(first.library.savedNames(), ["kept.xtstack.html"]);

  // A reload: a new engine and a new library over the same storage.
  const { h, ui } = await host({}, new StackLibrary(storage));
  await h.run('open stack "kept.xtstack.html"\nput the name of this stack\nput the number of buttons');
  assert.deepEqual(ui.out, ["Kept", "1"]);
});

test("opening an unknown stack is an error a script can catch", async () => {
  const { h, ui } = await host({});
  await h.run('try\n  open stack "nope.xtstack.html"\ncatch e\n  put e\nend try');
  assert.match(ui.out[0], /No stack file named "nope.xtstack.html"/);
});

test("a library without storage keeps stacks in memory", () => {
  const library = new StackLibrary(null);
  library.save("a", "text a");
  assert.equal(library.open("a"), "text a");
  assert.deepEqual(library.savedNames(), ["a"]);
  library.remove("a");
  assert.throws(() => library.open("a"), /No stack file/);
});

test("storage that refuses to save is reported", () => {
  const full = new FakeStorage();
  full.setItem = () => {
    throw new Error("quota exceeded");
  };
  assert.throws(() => new StackLibrary(full).save("a", "x"), /would not store "a": quota exceeded/);
});

test("file types become a file input's accept list", () => {
  assert.equal(acceptFor(["txt", ".md", "HTML"]), ".txt,.md,.html");
  // Classic Mac file types become the extensions a browser knows.
  assert.equal(acceptFor(["TEXT", "PICT"]), ".txt,.pict,.pct");
  // A HyperCard stack has no extension: no filter.
  assert.equal(acceptFor(["STAK"]), "");
  assert.equal(acceptFor([]), "");
});
