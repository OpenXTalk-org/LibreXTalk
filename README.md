# xtalk-rs, step 6: shell access (`shell()` and `do ... as shell`)

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
  - `shell.rs`: running shell commands, blocking or on a worker thread
  - `chunk.rs`: finding and replacing characters, words, items, and lines
  - `interp.rs`: the virtual machine, with its own call stack
  - `tests.rs`: whole-script tests
- `crates/xt-cli`: the `xt` program. It supplies a `Host` that prints to the terminal.

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
