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

use crate::ast::{BinOp, WaitUnit};
use crate::compile::{compile_handler, Handler, Op};
use crate::error::XtError;
use crate::parser::parse_script;

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
                Err(err) => return Err(err.with_line(line)),
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
            Op::The(name) => {
                if name.eq_ignore_ascii_case("result") {
                    let v = self.result.clone();
                    self.push(v);
                } else {
                    return Err(XtError::new(format!("Unknown property 'the {name}'")));
                }
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
            Op::Call { name, argc, value } => self.call(&name, argc, value)?,
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

    fn call(&mut self, name: &str, argc: usize, wants_value: bool) -> Result<(), XtError> {
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(self.pop()?);
        }
        args.reverse();

        let handler = match self.handlers.get(&name.to_lowercase()) {
            Some(h) => Rc::clone(h),
            None => return Err(XtError::new(format!("Can't understand \"{name}\""))),
        };
        if self.frames.len() >= MAX_DEPTH {
            return Err(XtError::new(format!(
                "Too many nested handler calls. Is '{}' calling itself forever?",
                handler.name
            )));
        }

        let mut frame = Frame::new(Rc::clone(&handler), false, wants_value, HashMap::new());
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

fn constant(name: &str) -> Option<String> {
    let value = match name.to_lowercase().as_str() {
        "empty" => String::new(),
        "space" => " ".to_string(),
        "tab" => "\t".to_string(),
        "return" | "linefeed" => "\n".to_string(),
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

fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A string is numeric if it parses as a finite number (spec VAL-2).
fn to_number(s: &str) -> Result<f64, XtError> {
    parse_num(s).ok_or_else(|| XtError::new(format!("Expected a number but found \"{s}\"")))
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
