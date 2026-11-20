# xtalk-rs, step 2: scripts, if, repeat, handlers, wait

A small headless xTalk interpreter, with a command prompt. This project is an
attempt at creating a platform-agnostic xTalk dialect, runtime engine / VM, and
IDE, with liberal licensing.

    cargo run -p xt-cli                       # the prompt
    cargo run -p xt-cli -- run examples/countdown.xt
    cargo test

Inside the prompt, type `:help`.

## Layout

- `crates/xt-core`
  - `lexer.rs`: text to tokens (no keywords, newlines are tokens)
  - `parser.rs`: tokens to a syntax tree
  - `compile.rs`: syntax tree to bytecode
  - `interp.rs`: the virtual machine, with its own call stack
  - `tests.rs`: whole-script tests
- `crates/xt-cli`: the `xt` program. It supplies a `Host` that prints to the terminal.

## Why bytecode

`wait` has to pause a script in the middle of a handler, even one called from
inside an expression, and resume later without blocking the host. A VM with its
own frame stack can do that. A tree-walking interpreter that uses Rust's
call stack cannot.

License: MIT OR Apache-2.0 (add LICENSE-MIT and LICENSE-APACHE before publishing).
