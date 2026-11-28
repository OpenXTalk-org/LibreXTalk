// library.mjs: where the browser host keeps stack files.
//
// A web page has no folders it can read, so "files" here are named texts
// from three places, looked up in this order:
//
// 1. Files the person picked with `answer file` or the page's Open button.
//    The browser gives the page their contents, not a path, so they are
//    known by file name.
// 2. Stacks saved with `save stack`, kept in the browser's own storage
//    (localStorage) so they are still there after a reload.
// 3. Nothing else: an unknown name is an error the script can catch.
//
// It has no DOM code, so Node.js tests can use it with a fake storage.

const PREFIX = "librextalk:stack:";

export class StackLibrary {
  /**
   * `storage` is anything with getItem, setItem, removeItem, key, and
   * length, like window.localStorage. Without one, saved stacks last only
   * as long as the page.
   */
  constructor(storage = null) {
    this.storage = storage;
    this.picked = new Map(); // file name -> text
    this.memory = new Map(); // used when there is no storage
  }

  /** A file the person picked; returns the name scripts can use for it. */
  addPicked(name, text) {
    this.picked.set(name, text);
    return name;
  }

  /** Keep a saved stack under `path`. */
  save(path, text) {
    if (this.storage) {
      try {
        this.storage.setItem(PREFIX + path, text);
        return;
      } catch (e) {
        // Storage can be full or switched off (a private window).
        throw new Error(`The browser would not store "${path}": ${e?.message ?? e}`);
      }
    }
    this.memory.set(path, text);
  }

  /** The text of the stack file named `path`. Throws if there is none. */
  open(path) {
    if (this.picked.has(path)) return this.picked.get(path);
    const stored = this.#stored(path);
    if (stored !== null) return stored;
    throw new Error(`No stack file named "${path}" (open it with the Open button first)`);
  }

  /** The names of the saved stacks, sorted. */
  savedNames() {
    const names = [...this.memory.keys()];
    if (this.storage) {
      for (let i = 0; i < this.storage.length; i++) {
        const key = this.storage.key(i);
        if (key?.startsWith(PREFIX)) names.push(key.slice(PREFIX.length));
      }
    }
    return [...new Set(names)].sort();
  }

  /** Forget a saved stack. */
  remove(path) {
    this.memory.delete(path);
    try {
      this.storage?.removeItem(PREFIX + path);
    } catch {
      // Nothing to remove.
    }
  }

  #stored(path) {
    if (this.memory.has(path)) return this.memory.get(path);
    try {
      return this.storage?.getItem(PREFIX + path) ?? null;
    } catch {
      return null;
    }
  }
}
