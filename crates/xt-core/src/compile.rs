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
    /// Pop the selector operands (see `SelShape`), then the source text, and
    /// push the chunk.
    Chunk { kind: ChunkKind, shape: SelShape },
    /// Pop a text and push how many chunks of this kind it has.
    Count(ChunkKind),
    /// Put a value into a chunk of a variable. The value is on the stack
    /// below the operands of each step.
    PutChunk {
        name: String,
        steps: Vec<(ChunkKind, SelShape)>,
        placement: Placement,
    },
    /// Pop a value and set a global property.
    SetProp(String),
    /// Delete a chunk of a variable. The operands are those of `PutChunk`,
    /// without the value.
    DeleteChunk {
        name: String,
        steps: Vec<(ChunkKind, SelShape)>,
    },
    /// Start a try block. If an error happens, jump to `catch_at` and store
    /// the error message in `var` (unless it is empty).
    TryBegin { catch_at: usize, var: String },
    /// The try block finished without an error.
    TryEnd,
    /// Pop a value and raise it as an error.
    Throw,
    /// Pop `buttons` labels and a prompt, ask the host, and store the choice in `it`.
    Answer { buttons: usize },
    /// Pop a default (if there is one) and a prompt, ask the host for text,
    /// and store the reply in `it`.
    Ask { has_default: bool },
}

/// How a chunk is selected, and so how many operands it takes from the stack.
#[derive(Debug, Clone, Copy)]
pub enum SelShape {
    /// No operands: `first`, `last`, `middle`, `any`.
    Ordinal(Ordinal),
    /// One operand: the index.
    At,
    /// Two operands: the first and last index.
    Range,
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
        try_depth: 0,
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
    /// How many `try` blocks were open when the loop began.
    try_depth: usize,
}

struct Compiler {
    code: Vec<Op>,
    lines: Vec<usize>,
    line: usize,
    loops: Vec<LoopCtx>,
    next_hidden: usize,
    /// How many `try` blocks we are inside right now.
    try_depth: usize,
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
            Op::TryBegin { catch_at, .. } => *catch_at = target,
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
                    Some((placement, target)) => self.put_target(*placement, target),
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
                self.leave_tries();
                let at = self.emit(Op::Jump(0));
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.exits.push(at);
                }
            }
            StmtKind::NextRepeat => {
                self.leave_tries();
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
            StmtKind::Set { prop, value } => {
                self.expr(value);
                self.emit(Op::SetProp(prop.clone()));
            }
            StmtKind::Delete(target) => {
                let (name, steps) = flatten_target(target);
                let shapes = self.chunk_operands(&steps);
                self.emit(Op::DeleteChunk {
                    name,
                    steps: shapes,
                });
            }
            StmtKind::Try {
                body,
                var,
                catch_body,
            } => {
                let begin = self.emit(Op::TryBegin {
                    catch_at: 0,
                    var: var.clone().unwrap_or_default(),
                });
                self.try_depth += 1;
                self.stmts(body);
                self.try_depth -= 1;
                self.emit(Op::TryEnd);
                let skip = self.emit(Op::Jump(0));
                let catch_start = self.here();
                self.patch(begin, catch_start);
                self.stmts(catch_body);
                let end = self.here();
                self.patch(skip, end);
            }
            StmtKind::Throw(e) => {
                self.expr(e);
                self.emit(Op::Throw);
            }
            StmtKind::Answer { prompt, buttons } => {
                self.expr(prompt);
                for b in buttons {
                    self.expr(b);
                }
                self.emit(Op::Answer {
                    buttons: buttons.len(),
                });
            }
            StmtKind::Ask { prompt, default } => {
                self.expr(prompt);
                match default {
                    Some(d) => {
                        self.expr(d);
                        self.emit(Op::Ask { has_default: true });
                    }
                    None => {
                        self.emit(Op::Ask { has_default: false });
                    }
                }
            }
        }
    }

    /// Emit the operand expressions of each chunk step, innermost first, and
    /// return the shape of each step.
    fn chunk_operands(&mut self, steps: &[(ChunkKind, &ChunkSel)]) -> Vec<(ChunkKind, SelShape)> {
        let mut shapes = Vec::new();
        for (kind, sel) in steps {
            match sel {
                ChunkSel::Ordinal(o) => shapes.push((*kind, SelShape::Ordinal(*o))),
                ChunkSel::At(e) => {
                    self.expr(e);
                    shapes.push((*kind, SelShape::At));
                }
                ChunkSel::Range(a, b) => {
                    self.expr(a);
                    self.expr(b);
                    shapes.push((*kind, SelShape::Range));
                }
            }
        }
        shapes
    }

    /// Store the value on top of the stack into a variable or a chunk of one.
    fn put_target(&mut self, placement: Placement, target: &Target) {
        let (name, steps) = flatten_target(target);
        if steps.is_empty() {
            match placement {
                Placement::Into => self.emit(Op::Store(name)),
                Placement::Before => self.emit(Op::StoreBefore(name)),
                Placement::After => self.emit(Op::StoreAfter(name)),
            };
            return;
        }
        let shapes = self.chunk_operands(&steps);
        self.emit(Op::PutChunk {
            name,
            steps: shapes,
            placement,
        });
    }

    /// Before jumping out of a loop, close the `try` blocks opened inside it.
    fn leave_tries(&mut self) {
        let loop_depth = self.loops.last().map_or(0, |ctx| ctx.try_depth);
        for _ in loop_depth..self.try_depth {
            self.emit(Op::TryEnd);
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
        self.loops.push(LoopCtx {
            try_depth: self.try_depth,
            ..LoopCtx::default()
        });
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
            Expr::Chunk { kind, sel, of } => {
                self.expr(of);
                let shape = match sel {
                    ChunkSel::Ordinal(o) => SelShape::Ordinal(*o),
                    ChunkSel::At(e) => {
                        self.expr(e);
                        SelShape::At
                    }
                    ChunkSel::Range(a, b) => {
                        self.expr(a);
                        self.expr(b);
                        SelShape::Range
                    }
                };
                self.emit(Op::Chunk { kind: *kind, shape });
            }
            Expr::Count { kind, of } => {
                self.expr(of);
                self.emit(Op::Count(*kind));
            }
        }
    }
}

/// Split a target into its variable and its chunk steps, with the step
/// nearest the variable first.
fn flatten_target(target: &Target) -> (String, Vec<(ChunkKind, &ChunkSel)>) {
    let mut steps = Vec::new();
    let mut current = target;
    let name = loop {
        match current {
            Target::Var(n) => break n.clone(),
            Target::Chunk { kind, sel, of } => {
                steps.push((*kind, sel));
                current = &**of;
            }
        }
    };
    steps.reverse();
    (name, steps)
}
