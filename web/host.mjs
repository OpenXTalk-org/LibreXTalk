// host.mjs: the browser host. It answers the engine's requests (dialogs,
// saving and opening stacks) by asking a "ui" object, and keeps stack files
// in a StackLibrary.
//
// The ui object is the only part that touches the page, so tests can give
// a fake one. It has these methods, each returning a promise:
//
//   message(text)                       show text from `put`
//   answer(prompt, buttons) -> index    buttons to choose from
//   ask(prompt, default) -> text|null   a line of text
//   pickFile(prompt, types) -> {name, text}|null
//                                       a file from the person's computer
//                                       (the browser needs a click for this)
//   color(prompt, default) -> text|null a color, as #rrggbb
//   choose(prompt, options) -> text|null one of a list
//   notice(text)                        tell the person something
//
// Browser limits and what the host does about them (CLAUDE.md 5.18):
// - There are no paths. A picked file is known by its name, and `answer
//   file` returns that name, so `open stack it` works right after it.
// - `ask file` and `answer folder` just ask for a name: a page cannot see
//   the person's folders.
// - `save stack` keeps the stack in the browser's storage under its name;
//   the page offers it as a download from there.
// - `answer record` has no recording yet; it says so and cancels.
// - Shell commands are never allowed (the engine already refuses them).

/** The visual effects `answer effect` offers until effects are built. */
export const EFFECTS = [
  "plain",
  "dissolve",
  "wipe left",
  "wipe right",
  "push left",
  "push right",
  "iris open",
  "iris close",
];

export class BrowserHost {
  constructor(engine, ui, library) {
    this.engine = engine;
    this.ui = ui;
    this.library = library;
  }

  /** Run a script to the end. Rejects with the script's error. */
  run(source) {
    return this.engine.runScript(source, this.handlers());
  }

  /** The handlers XtEngine.runScript calls for each kind of request. */
  handlers() {
    const ui = this.ui;
    const library = this.library;
    return {
      message: (text) => ui.message(text),
      answer: (req) => ui.answer(req.prompt, req.buttons),
      ask: (req) => ui.ask(req.prompt, req.default),
      dialog: (req) => this.#dialog(req),
      save: async (req) => {
        library.save(req.path, req.text);
      },
      open: async (req) => library.open(req.path),
    };
  }

  async #dialog(req) {
    const ui = this.ui;
    switch (req.form) {
      case "answer file": {
        const file = await ui.pickFile(req.prompt, req.types);
        return file ? this.library.addPicked(file.name, file.text) : null;
      }
      case "ask file":
        return ui.ask(req.prompt || "Save as:", req.default);
      case "answer folder":
        return ui.ask(req.prompt || "Folder:", req.default);
      case "answer color":
        return ui.color(req.prompt || "Choose a color:", req.default);
      case "answer effect":
        return ui.choose("Choose a visual effect:", EFFECTS);
      case "answer record":
        await ui.notice("Sound recording isn't available in the browser host yet.");
        return null;
      default:
        await ui.notice(`This page can't show "${req.form}" yet.`);
        return null;
    }
  }
}
