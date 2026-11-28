# xtalk-rs, step 7: the object model (stacks, cards, buttons, fields, messages)

A small headless xTalk interpreter, with a command prompt.

    cargo run -p xt-cli                       # the prompt
    cargo run -p xt-cli -- run examples/countdown.xt
    cargo test

Inside the prompt, type `:help`. The up and down arrows recall earlier lines (kept in `~/.xt_history`).

## Layout

- `crates/xt-core`
  - `lexer.rs`: text to tokens (no keywords, newlines are tokens)
  - `parser.rs`: tokens to a syntax tree
  - `compile.rs`: syntax tree to bytecode
  - `builtins.rs`: built-in functions (numbers, text, time, parameters)
  - `objects.rs`: the arena of stacks, cards, and controls, with their properties
  - `shell.rs`: running shell commands, blocking or on a worker thread
  - `chunk.rs`: finding and replacing characters, words, items, and lines
  - `interp.rs`: the virtual machine, with its own call stack
  - `tests.rs`: whole-script tests
- `crates/xt-stackfile`: stack files. It turns a stack (as a plain `PartNode` tree from the engine) into `.xtstack.html` text and back.
- `crates/xt-cli`: the `xt` program. It supplies a `Host` that prints to the terminal, asks for dialog replies, and reads and writes stack files.

## Why bytecode

`wait` has to pause a script in the middle of a handler, even one called from
inside an expression, and resume later without blocking the host. A VM with
its own frame stack can do that. A tree-walking interpreter that uses Rust's
call stack cannot.

License: MIT OR Apache-2.0 (add LICENSE-MIT and LICENSE-APACHE before publishing).

## Shell access

    put shell("uname -m")                   -- waits, returns the output
    do "sleep 2; date" as bash              -- pauses only the script
    put it                                  -- the output
    put the result                          -- the exit status

Scripts run with the same power you have, so shell access is off unless the
host allows it. The prompt allows it. `xt run file.xt` needs `--allow-shell`.

## Objects

    create button "OK"                       -- it holds button id 3 of stack id 1
    set the rect of button 1 to "10,20,110,50"
    create field "msg"
    put "hello" into field "msg"
    set the script of button 1 to "on mouseUp; put clicked; end mouseUp"
    send mouseUp to button 1                 -- the script's handler runs

`:objects` at the prompt lists the stacks, cards, and controls. Ids are numbered per stack, so a reference names its stack; `button id 3` alone means the current stack.

`part`, `control`, and `widget` mean any control (`the number of controls`, `part 2`). `create audioClip "beep"` and `create videoClip` make clips, which belong to the stack and have no rect.

## Dialogs

    answer file "Open which?" of type "txt" or "md"   -- the path in it
    ask file "Save as:" with "notes.txt"
    answer folder "Where?"
    answer color with "red"

Cancelling leaves `it` empty and sets `the result` to `Cancel`. In the terminal you type the path or color; Return accepts the suggestion.

## Stack files

    save this stack as "demo.xtstack.html"
    save this stack                          -- again, to the same file
    open stack "demo.xtstack.html"           -- and go to its first card

A `.xtstack.html` file is an ordinary web page, so it opens in a browser and shows small diffs in version control. Each object is an `xt-` element (`<xt-button id="7" name="OK" rect="10,10,90,34">`), scripts sit in `<script type="text/x-xtalk">` blocks, and a field's text is one `<p>` per line. The binary `.xtstack` form is planned.
