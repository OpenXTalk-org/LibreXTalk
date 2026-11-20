//! Compiles syntax trees into bytecode for the VM.
//!
//! Each handler becomes a flat list of `Op`s. Control flow is jumps, and
//! expressions use a value stack. Because a running handler is just data
//! (a program counter and a stack), the VM can pause in the middle of any
//! handler, even one called from inside an expression, and resume later.

use crate::ast::*;

#[derive(Debug, Clone)]
pub enum Op {
    /// Push a literal.
    Push(String),
    /// Push the value of a variable or constant.
    Load(String),
    /// Push `the result`.
    The(String),
    /// Pop a value into a variable.
    Store(String),
    StoreBefore(String),
    StoreAfter(String),
    /// Declare a global variable in the current handler.
    Global(String),
    Neg,
    Not,
    /// Pop two values, push the result.
    Bin(BinOp),
    /// Pop a value and show it in the message box.
    Message,
    Jump(usize),
    /// Pop a boolean and jump if it is false.
    JumpIfFalse(usize),
    /// Call a handler with `argc` arguments from the stack.
    Call {
        name: String,
        argc: usize,
        value: bool,
    },
    /// Pop the return value and leave the handler.
    Return,
    /// Stop every running handler.
    ExitToTop,
    /// Pop a number and pause for that long.
    Wait(WaitUnit),
    /// Pause for one tick and let the host run.
    Yield,
}

#[derive(Debug)]
pub struct Handler {
    pub name: String,
    pub params: Vec<String>,
    pub code: Vec<Op>,
    /// The source line of each op, for error messages.
    pub lines: Vec<usize>,
}

pub fn compile_handler(name: &str, params: &[String], body: &[Stmt]) -> Handler {
    let mut c = Compiler {
        code: Vec::new(),
        lines: Vec::new(),
        line: 0,
        loops: Vec::new(),
        next_hidden: 0,
    };
    c.stmts(body);
    c.emit(Op::Push(String::new()));
    c.emit(Op::Return);
    Handler {
        name: name.to_string(),
        params: params.to_vec(),
        code: c.code,
        lines: c.lines,
    }
}

#[derive(Default)]
struct LoopCtx {
    /// Jumps from `exit repeat`, patched to the end of the loop.
    exits: Vec<usize>,
    /// Jumps from `next repeat`, patched to the loop's next-iteration point.
    continues: Vec<usize>,
}

struct Compiler {
    code: Vec<Op>,
    lines: Vec<usize>,
    line: usize,
    loops: Vec<LoopCtx>,
    next_hidden: usize,
}

impl Compiler {
    fn emit(&mut self, op: Op) -> usize {
        self.code.push(op);
        self.lines.push(self.line);
        self.code.len() - 1
    }

    fn here(&self) -> usize {
        self.code.len()
    }

    fn patch(&mut self, at: usize, target: usize) {
        match &mut self.code[at] {
            Op::Jump(t) | Op::JumpIfFalse(t) => *t = target,
            _ => {}
        }
    }

    /// A variable name the lexer can never produce, for loop counters.
    fn hidden(&mut self, what: &str) -> String {
        self.next_hidden += 1;
        format!("#{what}{}", self.next_hidden)
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        self.line = stmt.line;
        match &stmt.kind {
            StmtKind::Put { value, target } => {
                self.expr(value);
                match target {
                    None => {
                        self.emit(Op::Message);
                    }
                    Some((Placement::Into, name)) => {
                        self.emit(Op::Store(name.clone()));
                    }
                    Some((Placement::Before, name)) => {
                        self.emit(Op::StoreBefore(name.clone()));
                    }
                    Some((Placement::After, name)) => {
                        self.emit(Op::StoreAfter(name.clone()));
                    }
                }
            }
            StmtKind::Get(e) => {
                self.expr(e);
                self.emit(Op::Store("it".to_string()));
            }
            StmtKind::Eval(e) => {
                self.expr(e);
                self.emit(Op::Message);
            }
            StmtKind::Call { name, args } => {
                for a in args {
                    self.expr(a);
                }
                self.emit(Op::Call {
                    name: name.clone(),
                    argc: args.len(),
                    value: false,
                });
            }
            StmtKind::If {
                cond,
                then_body,
                else_body,
            } => {
                self.expr(cond);
                let to_else = self.emit(Op::JumpIfFalse(0));
                self.stmts(then_body);
                if else_body.is_empty() {
                    let end = self.here();
                    self.patch(to_else, end);
                } else {
                    let to_end = self.emit(Op::Jump(0));
                    let else_start = self.here();
                    self.patch(to_else, else_start);
                    self.stmts(else_body);
                    let end = self.here();
                    self.patch(to_end, end);
                }
            }
            StmtKind::Repeat { kind, body } => self.repeat(kind, body),
            StmtKind::Wait(WaitKind::For(amount, unit)) => {
                self.expr(amount);
                self.emit(Op::Wait(*unit));
            }
            StmtKind::Wait(WaitKind::Until(cond)) => self.wait_loop(cond, true),
            StmtKind::Wait(WaitKind::While(cond)) => self.wait_loop(cond, false),
            StmtKind::Return(value) => {
                match value {
                    Some(e) => self.expr(e),
                    None => {
                        self.emit(Op::Push(String::new()));
                    }
                }
                self.emit(Op::Return);
            }
            StmtKind::Exit(ExitKind::Handler) => {
                self.emit(Op::Push(String::new()));
                self.emit(Op::Return);
            }
            StmtKind::Exit(ExitKind::ToTop) => {
                self.emit(Op::ExitToTop);
            }
            StmtKind::Exit(ExitKind::Repeat) => {
                let at = self.emit(Op::Jump(0));
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.exits.push(at);
                }
            }
            StmtKind::NextRepeat => {
                let at = self.emit(Op::Jump(0));
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.continues.push(at);
                }
            }
            StmtKind::Global(names) => {
                for name in names {
                    self.emit(Op::Global(name.clone()));
                }
            }
        }
    }

    /// `wait until cond` (until = true) or `wait while cond` (until = false).
    fn wait_loop(&mut self, cond: &Expr, until: bool) {
        let top = self.here();
        self.expr(cond);
        if until {
            self.emit(Op::Not);
        }
        let leave = self.emit(Op::JumpIfFalse(0));
        self.emit(Op::Yield);
        self.emit(Op::Jump(top));
        let end = self.here();
        self.patch(leave, end);
    }

    /// Point every pending `next repeat` at the current position.
    fn finish_continues(&mut self) {
        let here = self.here();
        let pending = match self.loops.last_mut() {
            Some(ctx) => std::mem::take(&mut ctx.continues),
            None => Vec::new(),
        };
        for at in pending {
            self.patch(at, here);
        }
    }

    fn repeat(&mut self, kind: &RepeatKind, body: &[Stmt]) {
        self.loops.push(LoopCtx::default());
        match kind {
            RepeatKind::Forever => {
                let top = self.here();
                self.stmts(body);
                self.finish_continues();
                self.emit(Op::Jump(top));
            }
            RepeatKind::While(cond) => self.cond_loop(cond, false, body),
            RepeatKind::Until(cond) => self.cond_loop(cond, true, body),
            RepeatKind::Times(count) => {
                let counter = self.hidden("count");
                // Evaluate the count once. Adding 0 checks that it is a number.
                self.expr(count);
                self.emit(Op::Push("0".to_string()));
                self.emit(Op::Bin(BinOp::Add));
                self.emit(Op::Store(counter.clone()));

                let top = self.here();
                self.emit(Op::Load(counter.clone()));
                self.emit(Op::Push("0".to_string()));
                self.emit(Op::Bin(BinOp::Gt));
                let leave = self.emit(Op::JumpIfFalse(0));
                self.stmts(body);
                self.finish_continues();
                self.emit(Op::Load(counter.clone()));
                self.emit(Op::Push("1".to_string()));
                self.emit(Op::Bin(BinOp::Sub));
                self.emit(Op::Store(counter));
                self.emit(Op::Jump(top));
                let end = self.here();
                self.patch(leave, end);
            }
            RepeatKind::With {
                var,
                from,
                to,
                down,
            } => {
                let limit = self.hidden("limit");
                self.expr(from);
                self.emit(Op::Push("0".to_string()));
                self.emit(Op::Bin(BinOp::Add));
                self.emit(Op::Store(var.clone()));
                self.expr(to);
                self.emit(Op::Push("0".to_string()));
                self.emit(Op::Bin(BinOp::Add));
                self.emit(Op::Store(limit.clone()));

                let top = self.here();
                self.emit(Op::Load(var.clone()));
                self.emit(Op::Load(limit));
                self.emit(Op::Bin(if *down { BinOp::Ge } else { BinOp::Le }));
                let leave = self.emit(Op::JumpIfFalse(0));
                self.stmts(body);
                self.finish_continues();
                self.emit(Op::Load(var.clone()));
                self.emit(Op::Push("1".to_string()));
                self.emit(Op::Bin(if *down { BinOp::Sub } else { BinOp::Add }));
                self.emit(Op::Store(var.clone()));
                self.emit(Op::Jump(top));
                let end = self.here();
                self.patch(leave, end);
            }
        }
        let end = self.here();
        if let Some(ctx) = self.loops.pop() {
            for at in ctx.exits {
                self.patch(at, end);
            }
        }
    }

    /// `repeat while cond` (negate = false) or `repeat until cond` (negate = true).
    fn cond_loop(&mut self, cond: &Expr, negate: bool, body: &[Stmt]) {
        let top = self.here();
        self.expr(cond);
        if negate {
            self.emit(Op::Not);
        }
        let leave = self.emit(Op::JumpIfFalse(0));
        self.stmts(body);
        self.finish_continues();
        self.emit(Op::Jump(top));
        let end = self.here();
        self.patch(leave, end);
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Literal(s) => {
                self.emit(Op::Push(s.clone()));
            }
            Expr::Var(name) => {
                self.emit(Op::Load(name.clone()));
            }
            Expr::The(name) => {
                self.emit(Op::The(name.clone()));
            }
            Expr::Neg(inner) => {
                self.expr(inner);
                self.emit(Op::Neg);
            }
            Expr::Not(inner) => {
                self.expr(inner);
                self.emit(Op::Not);
            }
            Expr::Binary { op, left, right } => {
                self.expr(left);
                self.expr(right);
                self.emit(Op::Bin(*op));
            }
            Expr::Call { name, args } => {
                for a in args {
                    self.expr(a);
                }
                self.emit(Op::Call {
                    name: name.clone(),
                    argc: args.len(),
                    value: true,
                });
            }
        }
    }
}
