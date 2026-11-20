//! xt-core: the headless xTalk engine.
//!
//! Scripts are parsed, compiled to bytecode, and run by a small virtual
//! machine. The VM keeps its own call stack instead of using Rust's, so a
//! script can pause in the middle of any handler (`wait`) and resume later.
//! The engine knows nothing about terminals or windows: everything the
//! script shows goes through the [`Host`] trait.

mod ast;
mod chunk;
mod compile;
mod error;
mod interp;
mod lexer;
mod parser;

#[cfg(test)]
mod tests;

pub use ast::*;
pub use error::XtError;
pub use interp::{format_number, Host, Interpreter, Status};
pub use parser::parse_script;
