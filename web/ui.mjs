// ui.mjs: the browser host's dialogs and message box, built on the HTML
// <dialog> element. Each method returns a promise that settles when the
// person answers, so the script waits without blocking the page.

export class PageUi {
  /** `messages` is the element that shows text from `put`. */
  constructor(messages) {
    this.messages = messages;
  }

  message(text) {
    const line = document.createElement("div");
    line.className = "message";
    line.textContent = text;
    this.messages.append(line);
    line.scrollIntoView({ block: "nearest" });
  }

  answer(prompt, buttons) {
    return this.#dialog(prompt, (form, done) => {
      buttons.forEach((label, i) => {
        const b = button(label, () => done(i));
        if (i === 0) b.classList.add("default");
        form.actions.append(b);
      });
      return 0; // Escape picks the first button, like Return in the terminal
    });
  }

  ask(prompt, initial) {
    return this.#dialog(prompt, (form, done) => {
      const input = document.createElement("input");
      input.type = "text";
      input.value = initial ?? "";
      form.body.append(input);
      form.actions.append(button("Cancel", () => done(null)));
      const ok = button("OK", () => done(input.value));
      ok.classList.add("default");
      form.actions.append(ok);
      form.onSubmit = () => done(input.value);
      form.focus = input;
      return null;
    });
  }

  pickFile(prompt, types) {
    return this.#dialog(prompt || "Choose a file:", (form, done) => {
      // A browser opens its file picker only in answer to a click, so the
      // dialog offers a button for it.
      const input = document.createElement("input");
      input.type = "file";
      input.hidden = true;
      const accept = acceptFor(types);
      if (accept) input.accept = accept;
      input.addEventListener("change", async () => {
        const file = input.files?.[0];
        done(file ? { name: file.name, text: await file.text() } : null);
      });
      form.body.append(input);
      form.actions.append(button("Cancel", () => done(null)));
      const choose = button("Choose file…", () => input.click());
      choose.classList.add("default");
      form.actions.append(choose);
      form.focus = choose;
      return null;
    });
  }

  color(prompt, initial) {
    return this.#dialog(prompt, (form, done) => {
      const input = document.createElement("input");
      input.type = "color";
      input.value = /^#[0-9a-f]{6}$/i.test(initial ?? "") ? initial : "#000000";
      form.body.append(input);
      form.actions.append(button("Cancel", () => done(null)));
      const ok = button("OK", () => done(input.value));
      ok.classList.add("default");
      form.actions.append(ok);
      return null;
    });
  }

  choose(prompt, options) {
    return this.#dialog(prompt, (form, done) => {
      const select = document.createElement("select");
      for (const option of options) select.append(new Option(option, option));
      form.body.append(select);
      form.actions.append(button("Cancel", () => done(null)));
      const ok = button("OK", () => done(select.value));
      ok.classList.add("default");
      form.actions.append(ok);
      form.focus = select;
      return null;
    });
  }

  notice(text) {
    return this.#dialog(text, (form, done) => {
      const ok = button("OK", () => done(undefined));
      ok.classList.add("default");
      form.actions.append(ok);
      return undefined;
    });
  }

  /**
   * Show a modal dialog with `prompt`, let `build` add its controls, and
   * settle with whatever the controls pass to `done`. `build` returns the
   * value for Escape (closing the dialog without a choice).
   */
  #dialog(prompt, build) {
    return new Promise((resolve) => {
      const dialog = document.createElement("dialog");
      const p = document.createElement("p");
      p.className = "prompt";
      p.textContent = prompt;
      const body = document.createElement("div");
      body.className = "body";
      const actions = document.createElement("div");
      actions.className = "actions";
      dialog.append(p, body, actions);

      const form = { body, actions, onSubmit: null, focus: null };
      let settled = false;
      const done = (value) => {
        if (settled) return;
        settled = true;
        dialog.close();
        dialog.remove();
        resolve(value);
      };
      const onEscape = build(form, done);
      dialog.addEventListener("cancel", (e) => {
        e.preventDefault();
        done(onEscape);
      });
      dialog.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && form.onSubmit && e.target.tagName === "INPUT") {
          e.preventDefault();
          form.onSubmit();
        }
      });
      document.body.append(dialog);
      dialog.showModal();
      (form.focus ?? actions.querySelector(".default"))?.focus();
    });
  }
}

function button(label, onClick) {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = label;
  b.addEventListener("click", onClick);
  return b;
}

/**
 * Classic Mac file types that HyperCard scripts pass to `answer file ...
 * of type`, and the extensions a browser knows them by. A HyperCard stack
 * (STAK) has no extension, so it filters nothing.
 */
const MAC_TYPES = {
  TEXT: [".txt"],
  PICT: [".pict", ".pct"],
  JPEG: [".jpg", ".jpeg"],
  GIFf: [".gif"],
  PNGf: [".png"],
  TIFF: [".tif", ".tiff"],
  "PDF ": [".pdf"],
  MooV: [".mov"],
  WAVE: [".wav"],
  AIFF: [".aif", ".aiff"],
  "MP3 ": [".mp3"],
  STAK: [],
};

/**
 * The file input's `accept` for `answer file ... of type`: a classic Mac
 * type becomes its extensions; anything else is an extension, so "txt",
 * ".txt" and "TXT" all become ".txt". Empty means any file.
 */
export function acceptFor(types) {
  const out = [];
  for (const raw of types ?? []) {
    if (raw in MAC_TYPES) {
      out.push(...MAC_TYPES[raw]);
      continue;
    }
    const t = raw.trim().toLowerCase();
    if (t) out.push(t.startsWith(".") ? t : `.${t}`);
  }
  return [...new Set(out)].join(",");
}
