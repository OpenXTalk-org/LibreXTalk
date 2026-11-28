// xt.mjs: the JavaScript side of libreXTalk's WebAssembly engine.
//
// It works the same in a browser and in Node.js:
//
//   import { XtEngine } from "./xt.mjs";
//   const engine = await XtEngine.instantiate(wasmBytes);
//   await engine.runScript('answer "Sure?" with "Yes" or "No"\nput it', {
//     message: (text) => console.log(text),
//     answer: async (req) => 0,              // index of the chosen button
//   });
//
// The engine never blocks. When a script needs something from the page (a
// dialog, a file to save or open), `run` comes back "waiting" with a
// request; `runScript` passes the request to the matching handler, gives
// the handler's answer back to the engine, and carries on. A `wait` in the
// script becomes a timer, so the page stays responsive.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export class XtEngine {
  /** Load the engine from the bytes of `xt_wasm.wasm`. */
  static async instantiate(wasmBytes) {
    // The module imports nothing, so it needs no import object.
    const { instance } = await WebAssembly.instantiate(wasmBytes, {});
    return new XtEngine(instance.exports);
  }

  constructor(exports) {
    this.x = exports;
  }

  // ----- the raw calls: each returns the engine's JSON answer -------------

  /** Load a script: its handlers are kept, its statements run on `run`. */
  load(source) {
    return this.#call("xt_load", source);
  }

  /** Run until the script finishes, sleeps, or waits for the page. The
   *  engine has no clock of its own here, so it gets the page's. */
  run() {
    return this.#call("xt_run", undefined, Date.now());
  }

  /** `answer`: the chosen button, counting from 0. */
  replyButton(index) {
    return this.#call("xt_reply_button", undefined, index);
  }

  /** `ask` or a dialog form: the reply, or null if the person cancelled. */
  replyText(text) {
    return text === null || text === undefined
      ? this.#call("xt_reply_cancel")
      : this.#call("xt_reply_text", String(text));
  }

  /** `save stack`: the text was saved. */
  replySaved() {
    return this.#call("xt_reply_saved");
  }

  /** `open stack`: the file's text. */
  replyOpened(fileText) {
    return this.#call("xt_reply_opened", fileText);
  }

  /** `save stack` or `open stack` failed; the script sees an error. */
  replyFailed(message) {
    return this.#call("xt_reply_failed", String(message));
  }

  // ----- the convenient loop ------------------------------------------

  /**
   * Load and run a script to the end. `handlers` may have:
   *   message(text)                     text from `put`
   *   answer(req)  -> button index      req: { prompt, buttons }
   *   ask(req)     -> text or null      req: { prompt, default }
   *   dialog(req)  -> text or null      req: { form, prompt, default, types }
   *   save(req)    -> (nothing)         req: { path, text }; throw to fail
   *   open(req)    -> file text         req: { path }; throw to fail
   *   sleep(ms)    -> a promise          defaults to a timer
   * Handlers may be async. Rejects with an Error if the script fails.
   */
  async runScript(source, handlers = {}) {
    this.#check(this.load(source), handlers);
    for (;;) {
      const answer = this.#check(this.run(), handlers);
      if (answer.status === "finished") return;
      if (answer.status === "sleeping") {
        const sleep = handlers.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));
        await sleep(answer.ms);
      } else if (answer.status === "waiting") {
        this.#check(await this.#serve(answer.request, handlers), handlers);
      }
    }
  }

  async #serve(req, handlers) {
    const handler = handlers[req.kind];
    switch (req.kind) {
      case "answer":
        return this.replyButton(handler ? await handler(req) : 0);
      case "ask":
      case "dialog":
        // Without a handler, behave like a headless host: accept the
        // suggestion, or cancel when there is none.
        if (!handler) return this.replyText(req.default === "" ? null : req.default);
        return this.replyText(await handler(req));
      case "save":
      case "open":
        if (!handler) return this.replyFailed(`This page can't ${req.kind} stack files`);
        try {
          const result = await handler(req);
          return req.kind === "save" ? this.replySaved() : this.replyOpened(String(result));
        } catch (e) {
          return this.replyFailed(e?.message ?? String(e));
        }
      default:
        throw new Error(`Unknown request from the engine: ${req.kind}`);
    }
  }

  #check(answer, handlers) {
    for (const text of answer.messages ?? []) handlers.message?.(text);
    if (answer.status === "error") {
      const where = answer.error.line ? ` (line ${answer.error.line})` : "";
      const err = new Error(answer.error.message + where);
      err.line = answer.error.line;
      throw err;
    }
    return answer;
  }

  // ----- moving text across the boundary --------------------------------

  #call(name, text, ...args) {
    if (text !== undefined) {
      const bytes = encoder.encode(text);
      const ptr = this.x.xt_input(bytes.length);
      // Ask for the memory after `xt_input`: making room may have grown it.
      new Uint8Array(this.x.memory.buffer, ptr, bytes.length).set(bytes);
    }
    this.x[name](...args);
    const out = new Uint8Array(
      this.x.memory.buffer,
      this.x.xt_output_ptr(),
      this.x.xt_output_len(),
    );
    return JSON.parse(decoder.decode(out));
  }
}
