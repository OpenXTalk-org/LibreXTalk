# libreXTalk: project guide for Claude Code

libreXTalk (always written in camelCase, lowercase in crate and file names) is a clean-room, permissively licensed xTalk interpreter and runtime written in Rust. It is a headless engine with pluggable GUI toolkit backends, built to be as extensible as possible and as reducible as possible.

This file is the project's memory. It was assembled from a long design conversation, so it records decisions, their reasons, what is built, what is only planned, and how to work with the maintainer. Read all of it before changing anything. Where it says **Decided**, do not reopen the question without asking. Where it says **Provisional** or **Open**, it is fair to propose changes.

> **Status warning.** Almost all existing code was written by an AI assistant that could not run a Rust compiler. The maintainer compiled and fixed the earlier steps locally, but the newest steps (shell access, the object model, the line-editor history, and the most recent constants) have not been verified. Your first job in a fresh checkout is `cargo build`, `cargo test`, and fixing whatever breaks, with every fix kept small and explained.

---

## 1. Goals and non-goals

**Goals**

1. A fast, readable, hand-written xTalk interpreter in Rust that runs scripts headless.
2. **Reducible:** the core must build and run without the object model, the shell module, the standard library, or any GUI. Every layer can be left out or replaced.
3. **Extensible:** new language features, commands, functions, properties, parts, and whole toolkits arrive through plug-in layers, not by editing the core. The standard library is itself written largely in xTalk.
4. **Separation of concerns:** the object model, the presentation layer, the host (operating system) interface, and the language engine are separate things with narrow interfaces.
5. Build standalone apps for many platforms by fusing a prebuilt runtime with a payload (stack, assets, extensions).
6. Familiar to users of HyperCard, MetaCard, LiveCode, OpenXTalk, and OpenXION: the syntax people already know (`getProp`, `setProp`, `on`, `function`, chunk expressions, `the` forms).

**Non-goals**

- Not a HyperCard clone and not a LiveCode clone. Stacks and cards exist as engine prototypes, but nothing in the engine assumes a particular UI.
- Not a JIT or an ahead-of-time compiler. The interpreter ships inside every app.
- Not a copy of any existing engine's source (see the clean-room rules in section 4).

---

## 2. Hard requirements (the maintainer's)

| # | Requirement | Status |
|---|---|---|
| R1 | Very liberal license: MIT OR Apache-2.0 for our code (0BSD would be acceptable). Every dependency must be permissive. Enforce with `cargo-deny`. | Decided |
| R2 | Engine and UI separated. Stacks, cards, backgrounds, groups, buttons, fields and so on are real engine prototypes. A toolkit backend only realizes them on screen (a native `NSButton` on macOS, GTK or Qt elsewhere, or a drawn widget) and reports events. | Decided |
| R3 | Foreign code: load dynamic libraries (`dlopen`/`libloading`) with declarative externals through `libffi`, plus a stable C ABI for extensions. | Planned |
| R4 | Scripted parts: an xTalk-only way to define new objects, controls, and parts with methods, getters, setters, and messages (`part ... end part`, with `getProp`/`setProp`). | Planned |
| R5 | Substacks (resource-fork-like containers for graphics, sounds, dialogs) and an extension package format. | Planned |
| R6 | `do <script> as <language>` for foreign languages (JavaScript, Lua, AppleScript, shells). Shells are done. | Partly done |
| R7 | A fused-executable packager for many targets. | Planned |
| R8 | Legacy import: HyperCard stacks, resources, WOBA bitmaps, and optionally 68K XCMD/XFCN through emulation. | Planned, later |
| R9 | **xTalk Standard Library layer** (section 5.3): immutable but overridable, sitting in the message path just before the engine; may contain native Rust modules next to xTalk source modules. | Decided, not yet built |
| R10 | Reducibility and separation of concerns as first-class design constraints (section 5.2). | Decided |

---

## 3. Working with the maintainer

- The maintainer is **new to Rust**, runs **macOS**, and wants readable code with comments that explain the why. Explain Rust ideas briefly when you introduce them.
- Work in **small, testable steps**. Each step ends with `cargo test` passing. When a step changes behavior, also update the written spec (section 7 points to the spec documents) and add whole-script tests in `crates/xt-core/src/tests.rs`.
- The maintainer likes: a clear plan, explicit decisions with reasons, numbered rule IDs in the spec, tests for every rule, and short summaries of what changed and what is risky.
- Prefer **one clear recommendation plus its trade-offs**. Do not bury decisions. Ask at most one or two questions at a time.
- When the code does not compile, the maintainer pastes errors. Fix the root cause. Do not paper over errors with `unwrap` or `#[allow]`.
- Do not over-engineer. Keep the dialect small and add commands when a test and a spec rule exist for them.
- Never read, copy, or paraphrase source code from GPL engines (see section 4).

---

## 4. Clean-room and licensing policy

**Rules (Decided)**

1. Work from published documentation, books, and observed behavior of running software. Record the test, product, version, and date for any observed behavior you rely on.
2. **Never read LiveCode's engine source (GPL) or any other GPL engine source** for this project. Reading its public *documentation* is fine; copying its text is not.
3. Apple's HyperCard runtime resources (icons, sounds, cursors, patterns) are not ours. Make original or permissively licensed defaults.
4. Do not bundle ffmpeg (LGPL/GPL). Shelling out to a user-installed tool as a separate process is acceptable.
5. Keep `CLEANROOM.md` (to be created) with a **donor register**: source, license, commit or version read, date, use category, what was taken. Use categories: *port* (copy code with notices), *read for design*, *docs only*.
6. Names: do not use trademarked names ("HyperCard", "LiveCode") in the product. The name is libreXTalk.
7. Dependencies: allow only permissive licenses. Initial `cargo-deny` allowlist: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, 0BSD, CC0-1.0. The line editor `rustyline` is MIT, but on Windows it pulls in `clipboard-win` (Boost, `BSL-1.0`), so add `BSL-1.0` to the allowlist when running `cargo deny`. Keep third-party dependencies out of `xt-core`; today it has none.

**Donor and source register (what was reviewed in the design conversation)**

| Source | License / status | Use |
|---|---|---|
| `HyperTalk.g4` in antlr/grammars-v4 | MIT, copyright 2017 Matt DeFano (file header) | Read for design: precedence order, chunk forms, command list. Not copied. Note: it treats most keywords as reserved, is ASCII-only for identifiers, and is case-sensitive, so it is a checklist, not a lexer spec. |
| HyperTalk BNF on jaedworks.com (Appendix D of *HyperTalk 2.2: The Book*) | No license stated. Book content. | Docs only. Cross-check syntax across HyperTalk 1.2.5 and 2.2. Has OCR typos. Do not copy lists into the repo. |
| Oracle Media Objects MediaTalk Reference (hypercard.org) | Copyright 1995 Oracle; posted for educational and historical purposes | Docs only. Ideas: tabular chunks, `member` chunks, explicit custom properties (`add property`), lifecycle and introspection, z-order commands, `exit to top`. |
| OpenXTalk and OpenXION syntax dictionaries (github.com/OpenXTalk-org) | Not yet read (pages are rendered by JavaScript, GitHub blocks automated fetch). License unchecked. OpenXTalk is based on LiveCode. | Docs only, after checking each repo's license. |
| OpenXION (kreativekorp, Java) | License unconfirmed | Docs only until verified. Good model for OO syntax. |
| HyperZebra (pgostation, Java) | MIT per its notice file and GitHub detection. Notice lists: WOBA decoder ported from Pierre Lorenzi's HyperCardPreview (MIT); other format work written from published descriptions; Apple's HyperCard 2.4.1 runtime is bundled in its download and is NOT MIT; ffmpeg and Tesseract run as separate processes. AI-assisted code. | Behavior reference; panels; resource and import ideas; possible later port of importers with notices. |
| HyperLab (JGalego, Rust, Tauri) | MIT | Reference: parser/runtime split, text-based stack directories. |
| WildCard (crgeee, Rust/WASM) | MIT | Reference interpreter to compare. |
| Stacksmith (uliwitness, C++) | zlib-style; submodules may differ | Reference: bytecode VM (Forge, Leonie). A port needs the zlib "modified version" marker. |
| Frost (keithadler, Python) | MIT | Ideas: capability/permission design, argument-list shell handling, differential testing of chunk expressions. |
| WyldCard (defano, Java) | MIT | HyperTalk semantic reference, HyperCard stack import. |
| ViperCard, LiveCode engine | GPL | **Do not use.** |
| Executor (ctm) | MIT per HyperZebra's notice | Behavior reference only. |

---

## 5. Architecture

### 5.1 Layers (Decided)

| Layer | What it holds | Can it be dropped? |
|---|---|---|
| **Engine** | lexer, parser, bytecode compiler, VM with its own frame stack, values, chunk expressions, native primitives. No I/O. | No |
| **Object model** | stacks, backgrounds, cards, groups, controls; properties; scripts; message path | Yes (headless scripting without objects) |
| **Host interface** | the single door to the outside world: messages, dialogs, sleep, shell permission, and (planned) system information | Swappable per application |
| **xTalk Standard Library** | handlers in xTalk source and native Rust modules, seen as one layer | Yes, or replace it |
| **Extensions** | native (C ABI), WASM, or xTalk packages | Yes |
| **Toolkit backends** | realize parts on screen, produce events | Yes (headless is a backend) |
| **Importers and packager** | HyperCard stack import, 68K XCMD emulation, fused-executable builder | Yes |

### 5.2 Reducibility principles (Decided)

- Something is **native only when it must be**: it needs the OS, it needs speed, or it is a primitive the library is built from. Everything else is xTalk.
- Each layer is a Cargo feature or crate. A "bare core" build must compile with no objects, no shell, no standard library, and no GUI.
- Layers talk through narrow traits and data, never through each other's internals.
- Anything that touches the outside world goes through the `Host` trait. The engine must stay testable with a recording host.

### 5.3 The xTalk Standard Library layer (Decided, not yet built)

The maintainer's description, in summary:

- Some common but non-core syntax (`the systemVersion`, `the systemAppearance`, `the screenRect`, `the processor`) belongs in an **xTalk Standard Library**, not in the core.
- It sits in the **message path just before the engine**.
- It works in the spirit of Dan Gelder's HyperCard Simulator "SimScript": the language is extended with **xTalk handlers**. (We could not find documentation for SimScript. Work from this description and ask the maintainer to link or paste details.)
- A handler in this library is **callable as if it were part of the core language**. A `function sysAppearance` can be used as `the sysAppearance`, as in `if the sysAppearance is "blue gray" then ...`, so it reads like English.
- The library is **immutable but overridable**: scripts cannot edit it at run time, but any handler defined earlier in the message path (an object, a stack, a user library) overrides it by name.
- It may hold **non-essential parts of the core syntax** (things that need not be native).
- It may contain **native Rust modules next to xTalk source modules**, behind the same layer. Scripts cannot tell the difference. Speed-critical things such as `length` stay native.

**Message path (target order)**

```
object -> groups -> card -> background -> stack
   -> user libraries (loose handlers; later "start using")
   -> xTalk Standard Library
   -> engine (native primitives)
```

Today's code puts loose `on` handlers at the engine level and keeps built-ins in the engine. Moving to the target order is the next big refactor (section 10).

**`the NAME` resolution (target design).** The parser should produce a generic "the NAME [of X]" node and the VM resolves it at run time, so the parser needs no hard-coded list of names. Resolution order:

1. Core special forms: `the result`, `the itemDelimiter`, `the number of <chunk or object kind>`, chunk expressions.
2. An object property, when `of` names an object.
3. The message path, then the standard library: a `function NAME` or `getProp NAME` handler. `set the NAME to v` calls `setProp NAME`.
4. Native engine primitives.
5. A clear error.

A user handler defined lower in the path wins automatically. A build without the library still parses everything.

**Trust.** Library handlers run with extra trust. They may call a read-only host primitive (planned: `hostInfo(key)`, answered by the `Host` trait) that ordinary scripts cannot call. This keeps system information out of the shell permission gate. When no native answer exists, a library handler may fall back to `shell()` if the host allows it.

**Synonyms.** Names such as `systemAppearance` and `sysAppearance` are synonyms (an alias entry or a one-line handler). See the synonym rules in the Dialect Spec.

### 5.4 Current code layout (as built)

```
Cargo.toml                workspace
crates/xt-core/           the engine library, no third-party dependencies
  src/lib.rs              module list and re-exports
  src/lexer.rs            text -> tokens (Newline tokens, ';' acts as newline, '¬' continuation)
  src/ast.rs              syntax tree, including object specs and targets
  src/parser.rs           hand-written recursive descent parser, contextual words, backtracking
  src/compile.rs          syntax tree -> bytecode (Op), handlers, loop and try bookkeeping
  src/interp.rs           the VM, Host trait, values, constants, objects commands, dispatch
  src/chunk.rs            character/word/item/line spans, read, put, delete
  src/builtins.rs         built-in functions (math, text, time, params)
  src/shell.rs            shell() and background shell jobs
  src/objects.rs          World arena, ObjId, kinds, prototypes, properties, rects
  src/error.rs            XtError { message, line, col, incomplete }
  src/tests.rs            whole-script tests using a recording Capture host
crates/xt-cli/            the `xt` program (rustyline REPL, `xt run file.xt`)
examples/                 hello, countdown, fizzbuzz, chunks, functions, sysinfo, objects (.xt)
```

**Planned crate layout (not yet done; rename to the `librextalk-*` family)**

```
librextalk-core      engine: lexer, parser, compiler, VM, values, chunks, error, Host trait
librextalk-objects   object model (feature)
librextalk-shell     shell access (feature, native only)
librextalk-stdlib    standard library: xTalk sources (include_str!) + native modules + loader
librextalk-cli       binary `xt`
librextalk-backend-* toolkit backends (headless, drawn, native)
librextalk-ext       extension manifest, loader, C ABI
librextalk-pack      fused-executable packager
librextalk-macfiles / -hcimport / -xcmd68k   legacy import (later)
```

Assumed naming (maintainer has not objected): crates `librextalk-*`, binary `xt`, script extension `.xt`.

### 5.5 How the VM works (as built)

- Scripts compile to flat **bytecode** per handler. Expressions use a value stack. Control flow is jumps. Loop counters use hidden variables whose names start with `#`, which the lexer can never produce.
- The VM keeps its **own stack of frames** (handler, program counter, locals, value stack, try entries, `me`, `target`, pending shell job). It does not use the Rust call stack, so a script can pause anywhere, even inside a function called from an expression, and resume later. Runaway recursion is a script error at depth 1000, not a crash.
- `Interpreter::run` returns `Status::Finished` or `Status::Sleeping(duration)`. The host decides how to wait. `run_to_end` loops and calls `Host::sleep`. A GUI host should schedule a timer instead of blocking.
- `try`/`catch` unwinds across frames: the nearest open `try` anywhere on the frame stack catches a runtime error. `exit repeat` and `next repeat` close the try blocks opened inside their loop.
- Variables of loose statements persist between loads in `main_locals`, so the prompt remembers them. A runtime error keeps them.
- Errors carry line and column for syntax errors. Runtime errors carry a line only for handlers loaded from the source being run; handlers from object scripts carry none (their line numbers belong to the script text).
- `XtError.incomplete` is true when input ended inside a block, so the prompt can ask for more lines.
- Message dispatch: a command or function call starts at `me` (the object whose script is running) or, from loose statements, at the current card. It walks the path, then engine-level handlers, then built-in functions (including `shell`). `send` and the `go` lifecycle messages use the same finder. `pass` ends the handler and continues after the object that held it.

### 5.6 The Host trait (as built)

```rust
pub trait Host {
    fn message(&mut self, text: &str);                              // required: put with no target
    fn sleep(&mut self, d: Duration) { thread::sleep(d) }           // default sleeps
    fn answer(&mut self, prompt: &str, buttons: &[String]) -> usize { 0 }
    fn ask(&mut self, prompt: &str, default: &str) -> Option<String> { Some(default.to_string()) }
    fn allow_shell(&mut self, shell: &str, script: &str) -> bool { false }  // deny by default
}
```

Planned additions: `hostInfo(key)` for system information, screen and display queries, clipboard, file dialogs, and the toolkit events interface.

---

## 6. Implementation status (as of this file)

**Done (compiles on the maintainer's machine through the chunk and built-in steps; later steps unverified)**

- Lexer, parser, compiler, VM, REPL, `xt run`.
- Commands: `put` (`into`/`before`/`after`), `get`, `set the itemDelimiter to`, `set the <prop> of <object> to`, `add`/`subtract`/`multiply`/`divide`, `delete` (chunks and objects), `return`, `global`, `exit` (handler, `repeat`, `to top`), `next repeat`, `wait`, `try`/`catch`/`end try`, `throw`, `answer`, `ask`, `do <text> as <shell>`, `create`, `go`, `send`, `pass`, `show`, `hide`.
- Control flow: one-line and block `if`/`then`/`else`/`else if`/`end if`; `repeat forever | while | until | N times | with i = a to|down to b`.
- Handlers: `on` and `function`, parameters separated by spaces or commas or both, calls in either comma style or space style, `the result`, recursion, `global`.
- Chunk expressions (section 7).
- Built-in functions: `abs`, `round`, `trunc`, `sqrt`, `sin`, `cos`, `tan`, `atan`, `ln`, `exp`, `min`, `max`, `random`, `mod`, `div`, `length`, `offset`, `toUpper`, `toLower`, `numToChar`, `charToNum`, `seconds`, `ticks`, `milliseconds`, `param`, `paramCount`, `params`, plus `shell(cmd)`.
- Object model, headless (section 8).
- CLI: history with arrow keys (stored in `~/.xt_history`), multi-line blocks with a `...` prompt, `:vars`, `:objects`, `:ast <line>`, `:help`, `:quit`, Ctrl-C cancels a line, Ctrl-D exits. `answer` with one button just prints and continues; with several buttons Return accepts the first (default); `ask` Return accepts the default.

**Not done (do not claim these work)**

- Plain `do <xTalk text>` and `value()`; foreign languages other than shells.
- `getProp`/`setProp` handlers, `local`, `repeat for each`, `switch`, arrays.
- The standard library layer, `hostInfo`, native module registry, the refactor into `librextalk-*` crates.
- Timers (`send ... in n seconds`), arguments on `send`, `dispatch`.
- `byte` chunks, tabular chunks, `^`, block comments `/* */` (the spec lists them; the lexer does not support them), date and time formatting (`the date`, `the time`).
- Toolkit trait and any GUI. Scripted `part` definitions. Stack files and substacks. Extension ABI and FFI. Packager. Legacy import.
- `LICENSE-MIT`, `LICENSE-APACHE`, CI workflows, `deny.toml`, `CLEANROOM.md` in the repo.

---

## 7. The dialect as implemented (quick reference)

The full spec lives in two Claude Docs documents that the other account cannot open. Export both to Markdown (Export menu, Markdown) and keep them as `docs/DIALECT_SPEC.md` and `docs/PART_PROTOTYPE_TABLE.md`. They are titled **"Dialect Spec v0: Core xTalk Language"** and **"Part Prototype Table v0"**. Rules there have IDs (`LEX-3`, `CHK-4`, `HND-12`, `OBJ-6`) and a status (Decided, Provisional, Open). This section is the short version.

**Lexical**

- UTF-8, case-insensitive. Identifiers: letters, digits, underscore; start with a letter or underscore.
- **No reserved words.** A word means something only where the grammar expects it. The parser uses context and limited backtracking within one statement. Example: `word` and `line` are chunk types in `word 2 of x` and ordinary variables in `put word into line`.
- Strings are double-quoted, on one line, with no escapes. Use constants for special characters.
- Comments: `--` and `#` to end of line. The spec also lists `/* */` (not implemented).
- A **newline or a semicolon ends a statement** (outside strings and comments). `¬` at the end of a line continues it. So `repeat 3 times; put "hi"; end repeat` is valid.
- Numbers: decimal, optional fraction; no exponent notation.

**Values**

- Every value is a string. Numbers are parsed when needed and formatted back with up to ten decimals, trailing zeros dropped. Booleans are the strings `true` and `false`.
- A variable never assigned **evaluates to its own name** (classic HyperTalk), so `put banana` shows `banana`.
- Constants (case-insensitive, **cannot be assigned**, take precedence over variables): `empty`, `true`, `false`, `pi`, `space`, `tab`, `comma`, `colon`, `slash`, `backslash`, `quote`, `return`, `cr`, `linefeed`, `lf` (all one newline, a line feed, not a carriage return, to match OpenXTalk and xBuilder), `crlf`, `formfeed` (12), `eof` (4), `null` (0). Planned: `up`, `down`, `zero` to `ten`.

**Operators (loosest to tightest)**: `or`; `and`; `=`, `<>`, `is`, `is not`; `<`, `>`, `<=`, `>=`, `contains`, `is in`, `is not in`; `&`, `&&` (adds a space); `+`, `-`; `*`, `/`, `div`, `mod`; unary `-` and `not`. `and` and `or` evaluate both sides. Comparison is numeric when both sides are numbers, otherwise case-insensitive text. Word operators are recognized only in operator position.

**Chunk expressions**

- Types: `char`/`character`, `word`, `item`, `line`, plural forms accepted. Ordinals: `first` to `tenth`, `last`, `mid`/`middle` (piece n/2 + 1), `any`. Numbered, expression-indexed (`word i + 1 of x`), ranges (`word 2 to 4 of x`), nested (`word 3 of line 2 of x`). `of`, `in`, `from` are interchangeable. A chunk binds to the nearest operand: `first word of "a b" & "!"` is `(first word of "a b") & "!"`.
- Counts: `the number of words in x` (the `the` is optional).
- Write with `put ... into|before|after <chunk> of <variable or field>`. Beyond the end, items and lines pad with delimiters, words and characters raise an error. `delete <chunk> of ...` removes the chunk plus one delimiter (the following one, or the preceding one for the last chunk). Reading beyond the end gives empty text. Empty text has no chunks. Characters are Unicode code points.
- `the itemDelimiter` is global, settable, may be several characters, must not be empty.

**Handlers and calls**

- `on name params ... end name` (commands) and `function name params ... end name` (values). Parameters may be separated by spaces, commas, or both.
- Two call styles, a call may not mix them: **comma style** (`setup x + 1, "go"`, each argument a full expression) and **space style** (`setup x "go" (y + 1)`, each argument one token or a parenthesized expression). Function calls need no space before the parenthesis (`double(21)`).
- Missing arguments are empty, extra ones are ignored. `return` in a command handler sets `the result`.
- Nesting limit 1000. Handler names may be any word.
- `exit to top` stops everything (`exit to HyperCard` is accepted as an alias).
- `wait n [ticks|seconds]` (ticks by default; `tick`, `second`, `secs`, `sec` accepted), `wait until cond`, `wait while cond`. The handler pauses, the host keeps running.

**Other rules worth knowing**

- `the NAME of X` resolves to a built-in function if one exists (`the length of x`), otherwise an object property. Parenthesized `name(args)` always calls.
- User handlers override built-in functions of the same name.
- `answer`/`ask` are host calls; the default host picks the first button and the default text.
- `try ... catch e ... end try` catches runtime errors (not syntax errors). `throw "message"` raises one.
- `shell(cmd)` blocks the host and returns standard output (CRLF changed to LF, trailing newlines kept), setting `the result` to the exit status. Standard error is discarded; add `2>&1` to see it. Commands get no standard input. `do <cmd> as <shell>` with `shell`, `sh`, `bash`, `zsh`, `dash`, `ksh`, `fish`, `csh`, `tcsh`, `powershell`, `pwsh`, or `cmd` pauses only the script, puts output in `it` and the exit status in `the result`. **Shell access is denied unless the host allows it.** The prompt allows it; `xt run file.xt` needs `--allow-shell`.

**Synonyms** are data tables mapping aliases to canonical terms (`bkgnd`/`bg` to `background`, `cd`, `btn`, `fld`, `msg`, `prev`, `hilite`, `colors`/`colours`, `secs`, and so on). Planned: a load-time check that aliases never collide.

---

## 8. The object model (summary of the Part Prototype Table)

- **Kinds**: stack, background, card, group, button, field, scrollbar, image, graphic. A stack holds backgrounds and cards; a card uses one background; cards, backgrounds, and groups hold groups and controls.
- Every object has a unique numeric **id** (never reused); a **number** (position among same-kind siblings, in creation order, which is also front-to-back order); an optional **name** (not unique; first match wins, case-insensitive). A reference as a value is the text `button id 12`. A **field** in a value position gives its text, and in an object position (after `of`, `to`, `me`) it gives the reference. A field is a **container** (can be put into, chunked, `add`ed to, `delete`d from). No other kind is.
- A new engine starts with one stack named Untitled, one background, one card. `create stack` and `create card` make the new one current.
- **Properties**: common `name`, `id`, `number`, `owner` (read-only), `script`. Parts with a rect have `rect`, `left`, `top`, `right`, `bottom`, `width`, `height`, `loc` (setting an edge moves the part; setting width/height resizes it keeping the top left). Per-kind defaults are in `objects.rs` and the Part Prototype Table. Boolean properties accept only `true`/`false`. Reading an unset property gives empty text; setting an unknown property creates a custom property.
- **References**: `card 3`, `button "OK"`, `card id 12`, `first card`, `last button`, `next card`, `previous card`, `any card`, `this card`/`this stack`/`this background`, `button 1 of card 2`, `me`, `the target`, a variable holding a reference. A name or number that is not a literal needs parentheses: `button (tName)`. `me` is effectively reserved.
- **Counts**: `the number of cards`, `the number of buttons of card 2`.
- **Commands**: `create button "OK" [in card 2]` (reference in `it`), `delete button 1` (cannot delete the last card, the last stack, or a background cards use), `go [to] card 2 | next | previous | first | last | stack "x"` (sends `closeCard` then `openCard`), `send <message> to <object>` (waits for the handler), `pass <message>`, `show`/`hide`, `set the <prop> of <object> to <value>`.
- **Scripts**: `set the script of <object> to "<handlers>"` compiles at once, handlers only, errors reported at the `set`. Script text can use `;` as newline. Quotes inside a script string need the `quote` constant, which is clumsy (a better syntax is planned).
- **Message path** (objects only): control, groups, card, the card's background, stack, then engine-level handlers. The target design inserts user libraries and the standard library (section 5.3). Unhandled system messages (`mouseUp`, `openCard`, ...) are ignored; other unhandled messages are errors. `me` is the object whose script is running; `the target` is the object the message was first sent to.
- **Presentation roles** for backends: stack = Window, card = Viewport, group = Container, button = PushButton/Checkbox/RadioButton/PopupMenu by style, field = TextField/TextArea, scrollbar = ScrollBar, image = Image, graphic = Shape; background has none. Backends register native widgets per role; missing roles fall back to drawn widgets.

---

## 9. Open questions and decisions log

**Open (also tracked in the Dialect Spec)**

1. Associativity of `^`; whether `not` binds as tightly as unary minus (currently it does).
2. Number formatting rules for very large and very small values; exponent notation.
3. `//` as a comment form; block comments `/* */` (maintainer added to spec; not implemented).
4. Whether a user handler may shadow a built-in command.
5. Boolean strictness; case-sensitive comparison design.
6. Missing properties: empty (current) or error.
7. Whether argument styles may ever be mixed in one call.
8. Alias policy for dialect profiles.
9. Dates and times (`the date`, `the time`): time zones and formats.
10. Asynchronous dialogs for GUI hosts (probably a new run status meaning "waiting for the person").
11. Strict-variables mode (unassigned variable is an error).
12. Background shell commands that return at once and send a message; finer shell permissions than all-or-nothing.
13. Standard library details: the exact set of native primitives, the `hostInfo` key names, manifest and documentation metadata, how "immutable" is enforced (proposal: library handlers are installed once, outside any object's script table, and `set the script` cannot reach them).
14. Whether built-ins like `length` should be reachable as the same names whether native or xTalk (assumed yes: same layer, same lookup).

**Decided in the conversation (do not reopen without asking)**

- The name is libreXTalk; license MIT OR Apache-2.0.
- Rust; hand-written parser (not a parser generator); bytecode VM with its own frame stack for suspension.
- No reserved words; contextual parsing with limited backtracking.
- Stacks and cards are engine prototypes; toolkits only realize them. Native widgets with drawn fallback.
- `exit to top` is the escape-all statement name.
- `of`/`in`/`from` are synonyms in chunk expressions.
- Constants cannot be assigned. `cr`, `return`, `linefeed`, `lf` are all a newline.
- A semicolon ends a statement.
- Shell access is off unless the host allows it.
- The standard library may contain native Rust modules (maintainer confirmed), sits just before the engine, is immutable but overridable, and may hold non-essential parts of the core syntax.
- `getProp`/`setProp` and `on`/`function` stay the familiar handler forms.

---

## 10. Roadmap

Do these in order. Each ends with `cargo test` green and the spec updated.

**A. Housekeeping**
1. Create the git repo; add `LICENSE-MIT`, `LICENSE-APACHE`, `.gitignore`, `CLEANROOM.md` with the donor register, `deny.toml` (allowlist in section 4), and GitHub Actions (build and test on Linux, macOS, Windows; `cargo fmt --check`; `cargo clippy -D warnings`; `cargo-deny`).
2. Compile everything, fix errors, make `cargo test` and clippy pass.
3. Export the two Claude Docs to `docs/`.
4. Rename crates to `librextalk-*`; keep the binary `xt`.

**B. Layer refactor (the big one)**
1. Split `shell`, the object model, and built-ins into separate crates or features; verify a bare-core build.
2. Add the user-library and standard-library layers to the message path.
3. Replace the parser's hard-coded built-in and `the` handling with a generic "the NAME [of X]" node resolved at run time (section 5.3).
4. Add the native module registry and the xTalk source loader (`include_str!`); mark library handlers immutable.
5. Add `hostInfo(key)` to the `Host` trait with a read-only trust rule.
6. Write an Architecture and Goals document and update the spec (message path rule HND-7, `the` resolution EXP-6 and PRP-1, a new Standard Library section).
7. Exit criteria: all existing tests pass; a library handler `function sysAppearance` works as `the sysAppearance`; a user handler overrides it; a bare-core build still runs core scripts.

**C. Standard library content**: system information (`systemVersion`, `sysAppearance`/`systemAppearance`, `screenRect`, `processor`, ...), moving non-essential built-ins out of the engine, documented metadata per handler.

**D. Toolkit trait and first GUI**: `WidgetFactory` per presentation role, property binding table, synchronous `measure`, headless backend plus a drawn backend (`winit` with `tiny-skia`, or `egui`), pointer and key events turned into messages (`mouseUp`, ...), async `answer`/`ask` for GUI hosts.

**E. Authoring and messaging**: a block syntax for object scripts (for example `on mouseUp in button "Greet"` or a multi-line script literal), `getProp`/`setProp`, scripted `part ... end part` definitions, timers (`send ... in n seconds`), arguments on `send`, plain `do` and `value()`.

**F. Platform**: stack files (zip container plus unpacked text form), substacks, extension manifest and C ABI, `libloading` and `libffi` externals, `do ... as` for JavaScript/Lua/AppleScript, the fused-executable packager, WASM and iOS static registration, legacy import (Mac file and resource readers, HyperCard stack importer with WOBA, optional 68K XCMD emulation).

---

## 11. Planned designs (not built; keep for reference)

**Scripted parts**

```
part "Counter" extends "group"
  property count default 0
  property step  default 1

  getprop doubled
    return my count * 2
  end doubled

  setprop count to pValue
    if pValue < 0 then throw "count must be >= 0"
    store pValue in my count
    send countChanged to me
  end setprop

  contains
    button "inc" at 10,10 size 40,24
    field  "display" at 60,10 size 80,24
  end contains

  on mouseUp in "inc"
    add my step to my count
  end mouseUp
end part
```

Features to design: inheritance and mixins, visibility, declared events, property schemas with types and observers, composition from primitives, an optional `presentation native pushButton fallback drawn` line. Scripted and native-backed parts must share one object model.

**Extension package** (`extension.toml`): name, version, license, ABI version, kinds (`native`, `wasm`, `script`), per-platform binaries, declared permissions, and exports (commands, functions, parts, languages). Native extensions initialize through `xt_ext_init(host: *const HostApi)` and register commands, functions, part kinds, and script languages by value-passing across a small stable C ABI. WASM via `wasmi` (interpreting, for platforms without JIT). Static registration for iOS and WASM.

**Foreign calls**: `external function sqrt(double) returns double in "libm"`, with structs, out-parameters, buffers, callbacks, varargs, and calling conventions to design. A permission layer must gate all of it.

**Script languages** register through a `ScriptLanguage` trait (`eval(code, bindings) -> Result<Value>`); candidates: QuickJS (`rquickjs`, MIT), Lua (`mlua` with vendored Lua, MIT), Rhai, AppleScript/JXA via `osascript`, PowerShell. Blocking foreign calls run on a worker thread with an asynchronous form.

**Toolkit trait sketch**

```rust
pub trait WidgetFactory {
    fn roles(&self) -> &[Role];                       // PushButton, Checkbox, TextField, Window, ...
    fn create(&mut self, role: Role, ctx: &PartCtx) -> Box<dyn NativeWidget>;
}
pub trait NativeWidget {
    fn apply(&mut self, changes: &[PropChange]);       // engine -> widget
    fn measure(&self, q: &MeasureQuery) -> Size;       // synchronous text metrics
}
// widget -> engine: UiEvent { target: ObjId, kind: MouseUp | KeyDown | TextChanged | ... }
```

Mixing native and drawn content is hard on every toolkit; text fields should prefer drawn text for xTalk behavior (chunk selection, styled runs, formatted heights). macOS and iOS require UI on the main thread, which the suspendable VM already supports.

**Packager**: `xt build --target <triple>` copies a prebuilt per-platform runtime and appends a payload (bytecode, assets, manifest). CI publishes runtimes as release artifacts. Native packaging (.app, MSIX, AppImage) wraps the output. WASM and iOS cannot `dlopen` and need static extension registration.

**Stack file**: a zip archive with one entry per stack and binary assets as separate entries, lazy loading of substacks, plus an unpacked directory form with plain-text scripts for version control, and a converter between them. The same loader reads a packaged app's payload.

**Legacy import**: Mac file and resource readers (MacBinary, BinHex, AppleDouble, resource forks, compressed resources, MacRoman and Mac Japanese to UTF-8); a HyperCard stack importer with WOBA bitmap decoding (Pierre Lorenzi's HyperCardPreview, MIT, documents the format); optional 68K XCMD/XFCN execution in an emulator using an XCmd callback table mapped onto the extension host API. Keep each importer in its own crate so the core never depends on them.

---

## 12. Build, test, and conventions

```
cargo build --workspace
cargo test  --workspace            # shell tests run only on Unix
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p xt-cli                 # the prompt
cargo run -p xt-cli -- run examples/chunks.xt
cargo run -p xt-cli -- run --allow-shell examples/sysinfo.xt
```

- Tests are whole-script tests in `crates/xt-core/src/tests.rs`, using a `Capture` host that records messages, sleeps, prompts, and scripted answers. Wait tests do not really sleep. A `Silent` host checks the default-deny shell rule. Add a test for every rule.
- Style: plain Rust, owned `String`s and `Vec`s where it keeps the code simple, no `unsafe`, no `unwrap` in library code except where an invariant is documented. Comments explain why. Spec rule IDs (`CHK-4`, `HND-12`) appear in comments where a rule is implemented.
- Errors are `XtError` with a human message. Syntax errors carry line and column and the CLI prints the source line with a caret.
- Do not add dependencies to `xt-core` without discussion. The CLI depends on `rustyline` (MIT).
- When behavior changes, update the spec document, the tests, `README.md`, and the examples together.

**Known fragile spots to check first if compilation fails**: nested constant tables and lifetimes in `parser.rs` (`LEVELS`), the `Base<'_>` and borrowed `&ChunkSel` lists in `compile.rs` (`flatten_target`), borrow-checker interactions in `interp.rs` (`try_catch`, `call`, `find_object`, `pass_message`, the `DoShell` polling), the `try_parse_object` and `try_parse_chunk` backtracking in `parser.rs`, and the `rustyline` version in `crates/xt-cli/Cargo.toml` (written as `"18"`; run `cargo add rustyline` if the API moved).

---

## 13. Glossary

- **Chunk**: a character, word, item, or line of a string.
- **Container**: something a script can put text into: a variable, a field, or a chunk of one.
- **Message path**: the ordered list of places a message looks for a handler.
- **Engine-level handlers**: handlers defined in loose scripts with `on name` (today the last stop before the built-ins; in the target design they move to the user-library layer).
- **Host**: whatever embeds the engine (terminal, GUI app, test harness).
- **Part**: any object a script can create: stack, background, card, group, button, field, scrollbar, image, graphic.
- **Presentation role**: what a toolkit must draw for a part kind (PushButton, TextField, ...).
- **Standard Library (xTalk Standard Library)**: the special layer just before the engine, holding handlers (xTalk or native) that behave like core syntax.
