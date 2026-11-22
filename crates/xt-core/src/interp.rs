//! The virtual machine.
//!
//! The VM talks to the outside world only through the `Host` trait, so the
//! same engine can sit behind a terminal, a test harness, or a GUI toolkit.
//!
//! Execution is a loop over a stack of frames. Each frame is a running
//! handler: its code, a program counter, its variables, and a value stack.
//! `run` returns `Status::Sleeping` when the script waits, and the host
//! calls `run` again when the time is up.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::ast::{BinOp, ChunkKind, WaitUnit};
use crate::builtins;
use crate::chunk::{self, Selector};
use crate::compile::{compile_handler, Handler, Op, SelShape};
use crate::error::XtError;
use crate::parser::parse_script;
use crate::shell;

/// The most handlers that may be running at once. This stops a handler that
/// calls itself forever. The VM keeps its own stack, so it is a script error,
/// not a crash.
const MAX_DEPTH: usize = 1000;

/// What the engine needs from whatever is hosting it.
pub trait Host {
    /// A script wrote to the message box, for example `put "Hi"`.
    fn message(&mut self, text: &str);

    /// Called by `Interpreter::run_to_end` when the script waits. A terminal
    /// host really sleeps. A GUI host would schedule a timer and call
    /// `Interpreter::run` again instead of blocking.
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    /// Show a dialog with buttons and return the index of the chosen one.
    /// The default picks the first button, which suits a headless host.
    fn answer(&mut self, _prompt: &str, _buttons: &[String]) -> usize {
        0
    }

    /// Ask for a line of text. `None` means the person cancelled.
    /// The default accepts the suggested text.
    fn ask(&mut self, _prompt: &str, default: &str) -> Option<String> {
        Some(default.to_string())
    }

    /// May a script run this shell command? A script with shell access can
    /// do anything the person running it can, so the default is no. A host
    /// opts in by returning true.
    fn allow_shell(&mut self, _shell: &str, _script: &str) -> bool {
        false
    }
}

/// Where a call to `run` stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Everything has run to the end.
    Finished,
    /// The script is waiting. Call `run` again after this long.
    Sleeping(Duration),
}

enum Flow {
    Continue,
    Sleep(Duration),
}

/// One running handler.
struct Frame {
    handler: Rc<Handler>,
    pc: usize,
    locals: HashMap<String, String>,
    /// Names declared `global` in this handler.
    globals: HashSet<String>,
    stack: Vec<String>,
    /// True if the caller wants a return value (a function call).
    wants_value: bool,
    /// True for the frame that runs loose statements.
    is_main: bool,
    /// The arguments this handler was called with, for `param(n)`.
    args: Vec<String>,
    /// Open `try` blocks, innermost last.
    tries: Vec<TryEntry>,
    /// A shell command this handler is waiting for.
    job: Option<shell::Job>,
}

/// An open `try` block.
struct TryEntry {
    /// Where the catch block starts.
    catch_at: usize,
    /// How tall the value stack was, so it can be cut back after an error.
    depth: usize,
    /// The variable that receives the error message, or empty.
    var: String,
}

impl Frame {
    fn new(
        handler: Rc<Handler>,
        is_main: bool,
        wants_value: bool,
        locals: HashMap<String, String>,
    ) -> Self {
        Frame {
            handler,
            pc: 0,
            locals,
            globals: HashSet::new(),
            stack: Vec::new(),
            wants_value,
            is_main,
            args: Vec::new(),
            tries: Vec::new(),
            job: None,
        }
    }
}

pub struct Interpreter {
    /// Handlers by lowercase name.
    handlers: HashMap<String, Rc<Handler>>,
    globals: HashMap<String, String>,
    /// Variables of loose statements, kept between loads so the prompt remembers them.
    main_locals: HashMap<String, String>,
    frames: Vec<Frame>,
    /// `the result`
    result: String,
    /// `the itemDelimiter`
    item_delimiter: String,
    /// State for `any`, a simple xorshift generator.
    rng: u64,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            handlers: HashMap::new(),
            globals: HashMap::new(),
            main_locals: HashMap::new(),
            frames: Vec::new(),
            result: String::new(),
            item_delimiter: ",".to_string(),
            rng: seed(),
        }
    }

    /// Parse and compile source text. Handler definitions are remembered.
    /// Loose statements are queued to run on the next call to `run`.
    pub fn load(&mut self, src: &str) -> Result<(), XtError> {
        if !self.frames.is_empty() {
            return Err(XtError::new("A script is still running"));
        }
        let script = parse_script(src)?;
        for def in &script.handlers {
            let handler = compile_handler(&def.name, &def.params, &def.body);
            self.handlers.insert(def.name.to_lowercase(), Rc::new(handler));
        }
        if !script.main.is_empty() {
            let handler = compile_handler("<main>", &[], &script.main);
            let locals = std::mem::take(&mut self.main_locals);
            self.frames
                .push(Frame::new(Rc::new(handler), true, false, locals));
        }
        Ok(())
    }

    /// Run until the script finishes or waits.
    pub fn run(&mut self, host: &mut dyn Host) -> Result<Status, XtError> {
        let outcome = self.run_inner(host);
        if outcome.is_err() {
            self.abort();
        }
        outcome
    }

    /// Run until the script finishes, asking the host to sleep during waits.
    pub fn run_to_end(&mut self, host: &mut dyn Host) -> Result<(), XtError> {
        loop {
            match self.run(host)? {
                Status::Finished => return Ok(()),
                Status::Sleeping(duration) => host.sleep(duration),
            }
        }
    }

    /// Load and run source text from start to finish.
    pub fn run_source(&mut self, src: &str, host: &mut dyn Host) -> Result<(), XtError> {
        self.load(src)?;
        self.run_to_end(host)
    }

    /// Variables as (name, value) pairs, sorted by name. Loop counters are hidden.
    pub fn variables(&self) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> = self
            .main_locals
            .iter()
            .chain(self.globals.iter())
            .filter(|(name, _)| !name.starts_with('#'))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        list.sort();
        list
    }

    // ----- the run loop ----------------------------------------------------

    fn run_inner(&mut self, host: &mut dyn Host) -> Result<Status, XtError> {
        loop {
            let (op, line) = match self.frames.last() {
                None => return Ok(Status::Finished),
                Some(frame) => match frame.handler.code.get(frame.pc) {
                    Some(op) => (op.clone(), frame.handler.lines[frame.pc]),
                    None => {
                        return Err(XtError::new("Internal error: ran off the end of a handler"));
                    }
                },
            };
            // Move on before running the op, so jumps and waits resume correctly.
            self.frame_mut().pc += 1;
            match self.step(op, host) {
                Ok(Flow::Continue) => {}
                Ok(Flow::Sleep(duration)) => return Ok(Status::Sleeping(duration)),
                Err(err) => {
                    if self.try_catch(&err) {
                        continue;
                    }
                    return Err(err.with_line(line));
                }
            }
        }
    }

    fn step(&mut self, op: Op, host: &mut dyn Host) -> Result<Flow, XtError> {
        match op {
            Op::Push(s) => self.push(s),
            Op::Load(name) => {
                let v = self.read_var(&name);
                self.push(v);
            }
            Op::The(name) => match name.to_lowercase().as_str() {
                "result" => {
                    let v = self.result.clone();
                    self.push(v);
                }
                "itemdelimiter" => {
                    let v = self.item_delimiter.clone();
                    self.push(v);
                }
                _ => return Err(XtError::new(format!("Unknown property 'the {name}'"))),
            },
            Op::SetProp(name) => {
                let v = self.pop()?;
                if name == "itemdelimiter" {
                    if v.is_empty() {
                        return Err(XtError::new("The itemDelimiter can't be empty"));
                    }
                    self.item_delimiter = v;
                } else {
                    return Err(XtError::new(format!(
                        "Can't set 'the {name}': only the itemDelimiter can be set so far"
                    )));
                }
            }
            Op::Chunk { kind, shape } => {
                let selector = self.pop_selector(shape)?;
                let text = self.pop()?;
                let rand = self.next_random();
                let out = chunk::read(&text, kind, &self.item_delimiter, &selector, rand);
                self.push(out);
            }
            Op::Count(kind) => {
                let text = self.pop()?;
                let n = chunk::count(&text, kind, &self.item_delimiter);
                self.push(n.to_string());
            }
            Op::PutChunk {
                name,
                steps,
                placement,
            } => {
                // The last step's operands are on top of the stack.
                let mut selectors = Vec::with_capacity(steps.len());
                for (_, shape) in steps.iter().rev() {
                    selectors.push(self.pop_selector(*shape)?);
                }
                selectors.reverse();
                let value = self.pop()?;
                let base = self.stored(&name);
                let rand = self.next_random();
                let path: Vec<(ChunkKind, Selector)> = steps
                    .iter()
                    .map(|(kind, _)| *kind)
                    .zip(selectors)
                    .collect();
                let updated = chunk::put(&base, &path, placement, &value, &self.item_delimiter, rand)?;
                self.write_var(&name, updated);
            }
            Op::Store(name) => {
                let v = self.pop()?;
                self.write_var(&name, v);
            }
            Op::StoreBefore(name) => {
                let v = self.pop()?;
                let old = self.stored(&name);
                self.write_var(&name, format!("{v}{old}"));
            }
            Op::StoreAfter(name) => {
                let v = self.pop()?;
                let old = self.stored(&name);
                self.write_var(&name, format!("{old}{v}"));
            }
            Op::Global(name) => {
                let key = name.to_lowercase();
                self.frame_mut().globals.insert(key.clone());
                self.globals.entry(key).or_default();
            }
            Op::DeleteChunk { name, steps } => {
                let mut selectors = Vec::with_capacity(steps.len());
                for (_, shape) in steps.iter().rev() {
                    selectors.push(self.pop_selector(*shape)?);
                }
                selectors.reverse();
                let base = self.stored(&name);
                let rand = self.next_random();
                let path: Vec<(ChunkKind, Selector)> = steps
                    .iter()
                    .map(|(kind, _)| *kind)
                    .zip(selectors)
                    .collect();
                let updated = chunk::delete(&base, &path, &self.item_delimiter, rand);
                self.write_var(&name, updated);
            }
            Op::TryBegin { catch_at, var } => {
                let frame = self.frame_mut();
                let depth = frame.stack.len();
                frame.tries.push(TryEntry {
                    catch_at,
                    depth,
                    var,
                });
            }
            Op::TryEnd => {
                self.frame_mut().tries.pop();
            }
            Op::Throw => {
                let message = self.pop()?;
                return Err(XtError::new(message));
            }
            Op::Answer { buttons } => {
                let mut labels = Vec::with_capacity(buttons);
                for _ in 0..buttons {
                    labels.push(self.pop()?);
                }
                labels.reverse();
                let prompt = self.pop()?;
                if labels.is_empty() {
                    labels.push("OK".to_string());
                }
                let choice = host.answer(&prompt, &labels).min(labels.len() - 1);
                let picked = labels[choice].clone();
                self.write_var("it", picked);
            }
            Op::Ask { has_default } => {
                let default = if has_default {
                    self.pop()?
                } else {
                    String::new()
                };
                let prompt = self.pop()?;
                match host.ask(&prompt, &default) {
                    Some(reply) => {
                        self.write_var("it", reply);
                        self.result = String::new();
                    }
                    None => {
                        self.write_var("it", String::new());
                        self.result = "Cancel".to_string();
                    }
                }
            }
            Op::DoShell { shell } => {
                // The first time through, start the command. After that, keep
                // coming back to this op until the command is done.
                if self.frame_mut().job.is_none() {
                    let script = self.pop()?;
                    if !host.allow_shell(&shell, &script) {
                        return Err(XtError::new("The host does not allow shell commands"));
                    }
                    let job = shell::spawn(&shell, &script)?;
                    self.frame_mut().job = Some(job);
                }
                let polled = match self.frame_mut().job.as_mut() {
                    Some(job) => job.poll(),
                    None => None,
                };
                match polled {
                    None => {
                        self.frame_mut().pc -= 1;
                        return Ok(Flow::Sleep(Duration::from_millis(16)));
                    }
                    Some(result) => {
                        self.frame_mut().job = None;
                        let output = result?;
                        self.write_var("it", output.stdout);
                        self.result = output.status.to_string();
                    }
                }
            }
            Op::Neg => {
                let v = self.pop()?;
                let n = to_number(&v)?;
                self.push(format_number(-n));
            }
            Op::Not => {
                let v = self.pop()?;
                let b = to_bool(&v)?;
                self.push(bool_str(!b));
            }
            Op::Bin(op) => {
                let right = self.pop()?;
                let left = self.pop()?;
                let v = binary(op, &left, &right)?;
                self.push(v);
            }
            Op::Message => {
                let v = self.pop()?;
                host.message(&v);
            }
            Op::Jump(target) => self.frame_mut().pc = target,
            Op::JumpIfFalse(target) => {
                let v = self.pop()?;
                if !to_bool(&v)? {
                    self.frame_mut().pc = target;
                }
            }
            Op::Call { name, argc, value } => self.call(&name, argc, value, host)?,
            Op::Return => self.do_return()?,
            Op::ExitToTop => self.abort(),
            Op::Wait(unit) => {
                let v = self.pop()?;
                let n = to_number(&v)?;
                let seconds = match unit {
                    WaitUnit::Ticks => n / 60.0,
                    WaitUnit::Seconds => n,
                };
                if seconds > 0.0 {
                    let capped = seconds.min(1.0e9);
                    return Ok(Flow::Sleep(Duration::from_secs_f64(capped)));
                }
            }
            Op::Yield => return Ok(Flow::Sleep(Duration::from_millis(16))),
        }
        Ok(Flow::Continue)
    }

    // ----- frames and calls ------------------------------------------------

    fn frame_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a handler is running")
    }

    fn push(&mut self, value: String) {
        self.frame_mut().stack.push(value);
    }

    fn pop(&mut self) -> Result<String, XtError> {
        self.frame_mut()
            .stack
            .pop()
            .ok_or_else(|| XtError::new("Internal error: the value stack is empty"))
    }

    /// Find the innermost open `try`, anywhere on the stack of handlers. If
    /// there is one, unwind to it, store the message, and jump to its catch
    /// block. Returns false if no `try` is open.
    fn try_catch(&mut self, err: &XtError) -> bool {
        let Some(index) = self.frames.iter().rposition(|f| !f.tries.is_empty()) else {
            return false;
        };
        self.frames.truncate(index + 1);
        let entry = match self.frames[index].tries.pop() {
            Some(entry) => entry,
            None => return false,
        };
        let frame = &mut self.frames[index];
        frame.stack.truncate(entry.depth);
        frame.pc = entry.catch_at;
        if !entry.var.is_empty() {
            self.write_var(&entry.var, err.message.clone());
        }
        true
    }

    /// Pop the operands a selector needs and build it.
    fn pop_selector(&mut self, shape: SelShape) -> Result<Selector, XtError> {
        Ok(match shape {
            SelShape::Ordinal(o) => Selector::Ordinal(o),
            SelShape::At => {
                let i = to_index(&self.pop()?)?;
                Selector::Index(i)
            }
            SelShape::Range => {
                let last = to_index(&self.pop()?)?;
                let first = to_index(&self.pop()?)?;
                Selector::Range(first, last)
            }
        })
    }

    fn next_random(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn call(
        &mut self,
        name: &str,
        argc: usize,
        wants_value: bool,
        host: &mut dyn Host,
    ) -> Result<(), XtError> {
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop()?);
        }
        args.reverse();

        let found = self.handlers.get(&name.to_lowercase()).cloned();
        let handler = match found {
            Some(h) => h,
            None => {
                // `shell(command)` runs a command and waits for it.
                if wants_value && name.eq_ignore_ascii_case("shell") {
                    if args.len() != 1 {
                        return Err(XtError::new(format!(
                            "shell expects 1 argument(s) but got {}",
                            args.len()
                        )));
                    }
                    if !host.allow_shell("shell", &args[0]) {
                        return Err(XtError::new("The host does not allow shell commands"));
                    }
                    let output = shell::run_blocking("shell", &args[0])?;
                    self.result = output.status.to_string();
                    self.push(output.stdout);
                    return Ok(());
                }
                // No handler of that name: try the built-in functions.
                if wants_value && builtins::is_builtin(name) {
                    let rand = self.next_random();
                    let (handler_name, handler_args) = match self.frames.last() {
                        Some(f) => (f.handler.name.clone(), f.args.clone()),
                        None => (String::new(), Vec::new()),
                    };
                    let ctx = builtins::CallContext {
                        handler_name: &handler_name,
                        args: &handler_args,
                        rand,
                    };
                    let value = builtins::call(name, &args, &ctx)?;
                    self.push(value);
                    return Ok(());
                }
                return Err(XtError::new(format!("Can't understand \"{name}\"")));
            }
        };
        if self.frames.len() >= MAX_DEPTH {
            return Err(XtError::new(format!(
                "Too many nested handler calls. Is '{}' calling itself forever?",
                handler.name
            )));
        }

        let mut frame = Frame::new(Rc::clone(&handler), false, wants_value, HashMap::new());
        frame.args = args.clone();
        // Missing arguments are empty and extra ones are ignored (spec HND-4).
        for (i, param) in handler.params.iter().enumerate() {
            frame
                .locals
                .insert(param.to_lowercase(), args.get(i).cloned().unwrap_or_default());
        }
        self.frames.push(frame);
        Ok(())
    }

    fn do_return(&mut self) -> Result<(), XtError> {
        let value = self.pop()?;
        let finished = match self.frames.pop() {
            Some(f) => f,
            None => return Ok(()),
        };
        if finished.is_main {
            self.main_locals = finished.locals;
        }
        match self.frames.last_mut() {
            Some(caller) if finished.wants_value => caller.stack.push(value),
            Some(_) => self.result = value,
            None => {}
        }
        Ok(())
    }

    /// Stop everything, keeping the prompt's variables.
    fn abort(&mut self) {
        let mut drained = self.frames.drain(..);
        if let Some(first) = drained.next() {
            if first.is_main {
                self.main_locals = first.locals;
            }
        }
    }

    // ----- variables -------------------------------------------------------

    fn lookup_var(&self, key: &str) -> Option<String> {
        let frame = self.frames.last()?;
        if frame.globals.contains(key) {
            self.globals.get(key).cloned()
        } else {
            frame.locals.get(key).cloned()
        }
    }

    /// Constants first, then variables. A name that is neither evaluates to
    /// itself, as in classic HyperTalk (spec VAL-8 is still open).
    fn read_var(&self, name: &str) -> String {
        if let Some(c) = constant(name) {
            return c;
        }
        self.lookup_var(&name.to_lowercase())
            .unwrap_or_else(|| name.to_string())
    }

    /// The stored value of a variable, or empty if it was never assigned.
    fn stored(&self, name: &str) -> String {
        self.lookup_var(&name.to_lowercase()).unwrap_or_default()
    }

    fn write_var(&mut self, name: &str, value: String) {
        let key = name.to_lowercase();
        let is_global = self
            .frames
            .last()
            .is_some_and(|f| f.globals.contains(&key));
        if is_global {
            self.globals.insert(key, value);
        } else {
            self.frame_mut().locals.insert(key, value);
        }
    }
}

// ----- values ----------------------------------------------------------------

/// The value of a constant, if `name` is one. Constants are case-insensitive.
/// `return`, `cr`, `linefeed`, and `lf` are all the same character: a newline
/// (a line feed), not a carriage return, as in OpenXTalk and xBuilder.
/// `crlf` is a carriage return followed by a line feed. `eof` is the
/// end-of-transmission character (ASCII 4) and `null` is ASCII 0, as in LiveCode.
pub(crate) fn constant(name: &str) -> Option<String> {
    let value = match name.to_lowercase().as_str() {
        "empty" => String::new(),
        "space" => " ".to_string(),
        "tab" => "\t".to_string(),
        "return" | "cr" | "linefeed" | "lf" => "\n".to_string(),
        "crlf" => "\r\n".to_string(),
        "formfeed" => "\u{c}".to_string(),
        "eof" => "\u{4}".to_string(),
        "null" => "\u{0}".to_string(),
        "backslash" => "\\".to_string(),
        "slash" => "/".to_string(),
        "quote" => "\"".to_string(),
        "comma" => ",".to_string(),
        "colon" => ":".to_string(),
        "true" => "true".to_string(),
        "false" => "false".to_string(),
        "pi" => format_number(std::f64::consts::PI),
        _ => return None,
    };
    Some(value)
}

/// Is this the name of a constant? Scripts cannot assign to constants.
pub(crate) fn is_constant(name: &str) -> bool {
    constant(name).is_some()
}

fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A string is numeric if it parses as a finite number (spec VAL-2).
pub(crate) fn to_number(s: &str) -> Result<f64, XtError> {
    parse_num(s).ok_or_else(|| XtError::new(format!("Expected a number but found \"{s}\"")))
}

/// A chunk index: a number with any fraction dropped.
fn to_index(s: &str) -> Result<i64, XtError> {
    Ok(to_number(s)?.trunc() as i64)
}

/// A nonzero starting value for the random number generator.
fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D);
    nanos | 1
}

fn to_bool(s: &str) -> Result<bool, XtError> {
    match s.trim().to_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(XtError::new(format!(
            "Expected true or false but found \"{s}\""
        ))),
    }
}

fn bool_str(b: bool) -> String {
    (if b { "true" } else { "false" }).to_string()
}

/// Compare numerically if both sides are numbers, otherwise as text without
/// regard to case (spec EXP-4).
fn compare(l: &str, r: &str) -> Ordering {
    match (parse_num(l), parse_num(r)) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        _ => l.to_lowercase().cmp(&r.to_lowercase()),
    }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn binary(op: BinOp, l: &str, r: &str) -> Result<String, XtError> {
    match op {
        BinOp::Concat => Ok(format!("{l}{r}")),
        BinOp::ConcatSpace => Ok(format!("{l} {r}")),
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::IntDiv | BinOp::Mod => {
            arithmetic(op, l, r)
        }
        BinOp::Eq => Ok(bool_str(compare(l, r) == Ordering::Equal)),
        BinOp::Ne => Ok(bool_str(compare(l, r) != Ordering::Equal)),
        BinOp::Lt => Ok(bool_str(compare(l, r) == Ordering::Less)),
        BinOp::Gt => Ok(bool_str(compare(l, r) == Ordering::Greater)),
        BinOp::Le => Ok(bool_str(compare(l, r) != Ordering::Greater)),
        BinOp::Ge => Ok(bool_str(compare(l, r) != Ordering::Less)),
        BinOp::Contains => Ok(bool_str(contains(l, r))),
        BinOp::IsIn => Ok(bool_str(contains(r, l))),
        BinOp::IsNotIn => Ok(bool_str(!contains(r, l))),
        // Both sides are always evaluated: no short-circuit yet (spec EXP-8 is open).
        BinOp::And => Ok(bool_str(to_bool(l)? && to_bool(r)?)),
        BinOp::Or => {
            let a = to_bool(l)?;
            let b = to_bool(r)?;
            Ok(bool_str(a || b))
        }
    }
}

fn arithmetic(op: BinOp, l: &str, r: &str) -> Result<String, XtError> {
    let a = to_number(l)?;
    let b = to_number(r)?;
    let result = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div | BinOp::IntDiv | BinOp::Mod if b == 0.0 => {
            return Err(XtError::new("Can't divide by zero"));
        }
        BinOp::Div => a / b,
        BinOp::IntDiv => (a / b).trunc(),
        BinOp::Mod => a % b,
        _ => return Err(XtError::new("Internal error: not an arithmetic operator")),
    };
    Ok(format_number(result))
}

/// Turn a number into text. Whole numbers print without a decimal point;
/// everything else prints with up to ten decimal places and trailing zeros
/// removed. The exact rule is still open (spec VAL-5, open question 6).
pub fn format_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{n:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
