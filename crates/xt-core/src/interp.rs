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

use crate::ast::{BinOp, ChunkKind, DialogForm, ObjKind, Ordinal, Placement, WaitUnit};
use crate::builtins;
use crate::chunk::{self, Selector};
use crate::compile::{compile_handler, Handler, ObjShape, Op, SelShape};
use crate::error::XtError;
use crate::objects::{kind_name, ObjId, PartNode, World};
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
    fn answer(&mut self, _prompt: &str, _buttons: &[String]) -> Pending<usize> {
        Pending::Ready(0)
    }

    /// Ask for a line of text. `None` means the person cancelled.
    /// The default accepts the suggested text.
    fn ask(&mut self, _prompt: &str, default: &str) -> Pending<Option<String>> {
        Pending::Ready(Some(default.to_string()))
    }

    /// Show one of the dialog forms of `answer` and `ask`, such as a file
    /// chooser (DLG-1). `None` means the person cancelled. One method serves
    /// every form, so adding a form does not add a method to every host: a
    /// host matches the forms it can show and leaves the rest to this default.
    ///
    /// The default suits a headless host: it accepts the suggested value if
    /// there is one and otherwise cancels, because it cannot ask anyone.
    fn choose(&mut self, request: &DialogRequest) -> Pending<Option<String>> {
        if request.default.is_empty() {
            Pending::Ready(None)
        } else {
            Pending::Ready(Some(request.default.to_string()))
        }
    }

    /// Write a stack to a file (SFM-4). The host picks the file format, for
    /// example from the file name's extension, and does the writing; the
    /// engine only hands over the stack as a tree. The default cannot save.
    fn save_stack(&mut self, path: &str, _stack: &PartNode) -> Pending<Result<(), XtError>> {
        Pending::Ready(Err(XtError::new(format!(
            "This program can't save stacks to files (\"{path}\")"
        ))))
    }

    /// Read the stack in a file (SFM-5). The default cannot open files.
    fn open_stack(&mut self, path: &str) -> Pending<Result<PartNode, XtError>> {
        Pending::Ready(Err(XtError::new(format!(
            "This program can't open stack files (\"{path}\")"
        ))))
    }

    /// May a script run this shell command? A script with shell access can
    /// do anything the person running it can, so the default is no. A host
    /// opts in by returning true.
    fn allow_shell(&mut self, _shell: &str, _script: &str) -> bool {
        false
    }
}

/// What a script asked the host to show with a dialog form of `answer` or `ask`.
#[derive(Debug)]
pub struct DialogRequest<'a> {
    pub form: DialogForm,
    /// The text shown in the dialog; empty if the script gave none.
    pub prompt: &'a str,
    /// The suggested choice: a file name for `ask file`, a starting folder
    /// for `answer file` and `answer folder`, a color for `answer color`.
    pub default: &'a str,
    /// File types to offer, as written in the script (`of type "txt" or "md"`).
    pub types: &'a [String],
}

/// Where a call to `run` stopped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Everything has run to the end.
    Finished,
    /// The script is waiting. Call `run` again after this long.
    Sleeping(Duration),
    /// The script is waiting for the host to finish something it said it
    /// would answer later, such as a dialog (HST-2). Call
    /// [`Interpreter::reply`] with the answer, then `run` again.
    Waiting,
}

/// A host's answer: ready now, or later (HST-1). A host that must wait for a
/// person or for something asynchronous, such as a browser API, answers
/// `Later` and gives the real answer to [`Interpreter::reply`] when it has
/// it. The script pauses meanwhile; nothing blocks.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending<T> {
    Ready(T),
    Later,
}

/// `value.into()` is `Pending::Ready(value)`.
impl<T> From<T> for Pending<T> {
    fn from(value: T) -> Self {
        Pending::Ready(value)
    }
}

/// The answer to a host call that was answered `Later` (HST-3). Each kind
/// of call takes one kind of reply.
#[derive(Debug, Clone, PartialEq)]
pub enum HostReply {
    /// For `answer`: the index of the chosen button.
    Button(usize),
    /// For `ask` and the dialog forms: the reply, or `None` if cancelled.
    Text(Option<String>),
    /// For `save stack`: whether the stack was saved.
    Saved(Result<(), XtError>),
    /// For `open stack`: the stack read from the file.
    Opened(Result<PartNode, XtError>),
}

/// What the VM is waiting for, with what it needs to finish the job.
enum Waiting {
    Answer { labels: Vec<String> },
    Text,
    Save { stack: ObjId, path: String },
    Open { path: String },
}

impl Waiting {
    fn describe(&self) -> &'static str {
        match self {
            Waiting::Answer { .. } => "a button choice (HostReply::Button)",
            Waiting::Text => "a text reply (HostReply::Text)",
            Waiting::Save { .. } => "a save result (HostReply::Saved)",
            Waiting::Open { .. } => "an opened stack (HostReply::Opened)",
        }
    }

    fn accepts(&self, reply: &HostReply) -> bool {
        matches!(
            (self, reply),
            (Waiting::Answer { .. }, HostReply::Button(_))
                | (Waiting::Text, HostReply::Text(_))
                | (Waiting::Save { .. }, HostReply::Saved(_))
                | (Waiting::Open { .. }, HostReply::Opened(_))
        )
    }
}

enum Flow {
    Continue,
    Sleep(Duration),
    Wait,
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
    /// The object whose script this handler is in, if any (`me`).
    me: Option<ObjId>,
    /// The object the message was first sent to (`the target`).
    target: Option<ObjId>,
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
            me: None,
            target: None,
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
    /// The stacks, cards, and controls.
    world: World,
    /// A host call answered `Later`, and the reply once the host gives it.
    waiting: Option<Waiting>,
    reply: Option<HostReply>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        // Start with one stack holding one background and one card.
        let mut world = World::default();
        let stack = world.create_stack("Untitled");
        world.current_stack = Some(stack);
        world.current_card = world.first_card(stack);
        Interpreter {
            world,
            handlers: HashMap::new(),
            globals: HashMap::new(),
            main_locals: HashMap::new(),
            frames: Vec::new(),
            result: String::new(),
            item_delimiter: ",".to_string(),
            rng: seed(),
            waiting: None,
            reply: None,
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
            self.handlers
                .insert(def.name.to_lowercase(), Rc::new(handler));
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
    /// This suits a host that answers everything at once. A host that
    /// answers `Later` must call `run` and `reply` itself instead.
    pub fn run_to_end(&mut self, host: &mut dyn Host) -> Result<(), XtError> {
        loop {
            match self.run(host)? {
                Status::Finished => return Ok(()),
                Status::Sleeping(duration) => host.sleep(duration),
                Status::Waiting => {
                    self.abort();
                    return Err(XtError::new(
                        "The host will answer later, but run_to_end can't wait for it; use run and reply",
                    ));
                }
            }
        }
    }

    /// Give the answer to a host call that was answered `Later` (HST-3).
    /// Then call `run` to carry on. It is an error to reply when nothing is
    /// waiting, to reply twice, or to give the wrong kind of reply.
    pub fn reply(&mut self, reply: HostReply) -> Result<(), XtError> {
        let Some(waiting) = &self.waiting else {
            return Err(XtError::new("Nothing is waiting for a reply"));
        };
        if self.reply.is_some() {
            return Err(XtError::new("A reply has already been given"));
        }
        if !waiting.accepts(&reply) {
            return Err(XtError::new(format!(
                "The script is waiting for {}",
                waiting.describe()
            )));
        }
        self.reply = Some(reply);
        Ok(())
    }

    /// Is the script waiting for a reply from the host?
    pub fn is_waiting(&self) -> bool {
        self.waiting.is_some() && self.reply.is_none()
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
            // Handlers in an object's script have their own line numbers, which
            // mean nothing to the source being run, so they get none.
            let attach_line = !self.frames.last().is_some_and(|f| f.me.is_some());
            // Move on before running the op, so jumps and waits resume correctly.
            self.frame_mut().pc += 1;
            match self.step(op, host) {
                Ok(Flow::Continue) => {}
                Ok(Flow::Sleep(duration)) => return Ok(Status::Sleeping(duration)),
                Ok(Flow::Wait) => return Ok(Status::Waiting),
                Err(err) => {
                    if self.try_catch(&err) {
                        continue;
                    }
                    return Err(if attach_line {
                        err.with_line(line)
                    } else {
                        err
                    });
                }
            }
        }
    }

    fn step(&mut self, op: Op, host: &mut dyn Host) -> Result<Flow, XtError> {
        // While a host call is outstanding, the op that made it is the one
        // that comes back here each time `run` is called (HST-2).
        if self.waiting.is_some() {
            return self.resume_waiting();
        }
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
                let out = chunk::read(&text, kind, &self.item_delimiter, &selector, rand)?;
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
                let path: Vec<(ChunkKind, Selector)> =
                    steps.iter().map(|(kind, _)| *kind).zip(selectors).collect();
                let updated =
                    chunk::put(&base, &path, placement, &value, &self.item_delimiter, rand)?;
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
                let path: Vec<(ChunkKind, Selector)> =
                    steps.iter().map(|(kind, _)| *kind).zip(selectors).collect();
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
                match host.answer(&prompt, &labels) {
                    Pending::Ready(choice) => self.finish_answer(&labels, choice),
                    Pending::Later => return Ok(self.wait_for(Waiting::Answer { labels })),
                }
            }
            Op::Ask { has_default } => {
                let default = if has_default {
                    self.pop()?
                } else {
                    String::new()
                };
                let prompt = self.pop()?;
                match host.ask(&prompt, &default) {
                    Pending::Ready(reply) => self.finish_text(reply),
                    Pending::Later => return Ok(self.wait_for(Waiting::Text)),
                }
            }
            Op::Dialog {
                form,
                has_prompt,
                has_default,
                types,
            } => {
                let mut filters = Vec::with_capacity(types);
                for _ in 0..types {
                    filters.push(self.pop()?);
                }
                filters.reverse();
                let default = if has_default {
                    self.pop()?
                } else {
                    String::new()
                };
                let prompt = if has_prompt {
                    self.pop()?
                } else {
                    String::new()
                };
                let request = DialogRequest {
                    form,
                    prompt: &prompt,
                    default: &default,
                    types: &filters,
                };
                // Like `ask`: the choice goes in `it`; a cancel leaves `it`
                // empty and sets `the result` to Cancel (DLG-2).
                match host.choose(&request) {
                    Pending::Ready(choice) => self.finish_text(choice),
                    Pending::Later => return Ok(self.wait_for(Waiting::Text)),
                }
            }
            Op::SaveStack { has_path } => {
                let path = if has_path { Some(self.pop()?) } else { None };
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                if self.world.get(id).map(|o| o.kind) != Some(ObjKind::Stack) {
                    return Err(XtError::new("Only a stack can be saved"));
                }
                let path = match path {
                    Some(p) if !p.is_empty() => p,
                    Some(_) => return Err(XtError::new("The file name to save to is empty")),
                    None => self.world.files.get(&id).cloned().ok_or_else(|| {
                        XtError::new(
                            "This stack has no file yet. Save it with: save this stack as \"name.xtstack.html\"",
                        )
                    })?,
                };
                let tree = self.world.export(id)?;
                match host.save_stack(&path, &tree) {
                    Pending::Ready(done) => self.finish_save(id, path, done)?,
                    Pending::Later => return Ok(self.wait_for(Waiting::Save { stack: id, path })),
                }
            }
            Op::OpenStack => {
                let path = self.pop()?;
                // A stack opened from this file already is not read twice.
                let open = self
                    .world
                    .files
                    .iter()
                    .find(|(_, file)| **file == path)
                    .map(|(id, _)| *id);
                match open {
                    Some(id) => {
                        let text = self.world.ref_string(id).unwrap_or_default();
                        self.push(text);
                    }
                    None => match host.open_stack(&path) {
                        Pending::Ready(tree) => self.finish_open(path, tree)?,
                        Pending::Later => return Ok(self.wait_for(Waiting::Open { path })),
                    },
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
            Op::MeRef => {
                let me = self.frames.last().and_then(|f| f.me).ok_or_else(|| {
                    XtError::new("'me' can only be used inside an object's script")
                })?;
                let text = self.world.ref_string(me).ok_or_else(object_gone)?;
                self.push(text);
            }
            Op::TargetRef => {
                let target = self
                    .frames
                    .last()
                    .and_then(|f| f.target)
                    .ok_or_else(|| XtError::new("There is no target here"))?;
                let text = self.world.ref_string(target).ok_or_else(object_gone)?;
                self.push(text);
            }
            Op::ThisRef(kind) => {
                let id = match kind {
                    ObjKind::Card => self.world.current_card,
                    ObjKind::Stack => self.world.current_stack,
                    ObjKind::Background => self
                        .world
                        .current_card
                        .and_then(|c| self.world.get(c))
                        .and_then(|o| o.background),
                    _ => None,
                };
                let id = id.ok_or_else(|| {
                    XtError::new(format!("There is no current {}", kind_name(kind)))
                })?;
                let text = self.world.ref_string(id).unwrap_or_default();
                self.push(text);
            }
            Op::FindObj {
                kind,
                shape,
                has_of,
            } => {
                let operand = match shape {
                    ObjShape::At | ObjShape::Id => Some(self.pop()?),
                    _ => None,
                };
                let container = if has_of { Some(self.pop()?) } else { None };
                let found = self.find_object(kind, shape, operand, container)?;
                let text = self.world.ref_string(found).unwrap_or_default();
                self.push(text);
            }
            Op::ObjValue => {
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                let is_field = self.world.get(id).is_some_and(|o| o.kind == ObjKind::Field);
                if is_field {
                    let value = self.world.get_prop(id, "text")?;
                    self.push(value);
                } else {
                    self.push(text);
                }
            }
            Op::ObjText => {
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                self.require_field(id)?;
                let value = self.world.get_prop(id, "text")?;
                self.push(value);
            }
            Op::GetProp(name) => {
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                let value = self.world.get_prop(id, &name)?;
                self.push(value);
            }
            Op::SetObjProp(name) => {
                let text = self.pop()?;
                let value = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                if name == "script" {
                    self.set_script(id, &value)?;
                } else {
                    self.world.set_prop(id, &name, &value)?;
                }
            }
            Op::CountObjs { kind, has_of } => {
                let container = if has_of { Some(self.pop()?) } else { None };
                let n = if kind == ObjKind::Stack {
                    self.world.stacks.len()
                } else {
                    let parent = self.container_for(kind, container)?;
                    self.world.children_of_kind(parent, kind).len()
                };
                self.push(n.to_string());
            }
            Op::CreateObj {
                kind,
                has_name,
                has_parent,
            } => {
                let parent = if has_parent { Some(self.pop()?) } else { None };
                let name = if has_name { self.pop()? } else { String::new() };
                let created = self.create_object(kind, &name, parent)?;
                let text = self.world.ref_string(created).unwrap_or_default();
                self.write_var("it", text);
            }
            Op::DeleteObj => {
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                self.delete_object(id)?;
            }
            Op::GoTo => {
                let text = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                let (new_ref, old_ref) = self.go_to(id)?;
                self.push(new_ref);
                self.push(old_ref);
            }
            Op::SendSystem(name) => {
                let text = self.pop()?;
                if !text.is_empty() {
                    let id = self.world.lookup_ref(&text)?;
                    self.send_message(id, &name, true)?;
                }
            }
            Op::Send => {
                let text = self.pop()?;
                let name = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                self.send_message(id, &name, false)?;
            }
            Op::Pass(name) => self.pass_message(&name)?,
            Op::PutObject(placement) => {
                let text = self.pop()?;
                let value = self.pop()?;
                let id = self.world.lookup_ref(&text)?;
                self.require_field(id)?;
                let old = self.world.get_prop(id, "text")?;
                let updated = match placement {
                    Placement::Into => value,
                    Placement::Before => format!("{value}{old}"),
                    Placement::After => format!("{old}{value}"),
                };
                self.world.set_prop(id, "text", &updated)?;
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

        // A message starts at the object whose script is running. From loose
        // statements it starts at the current card, like the message box does.
        let me = self.frames.last().and_then(|f| f.me);
        let base = me.or(self.world.current_card);
        let found = match base {
            Some(start) => self.find_handler(start, name, None),
            None => self
                .handlers
                .get(&name.to_lowercase())
                .cloned()
                .map(|h| (None, h)),
        };
        match found {
            Some((owner, handler)) => {
                self.push_handler_frame(handler, args, owner, base, wants_value)
            }
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
                Err(XtError::new(format!("Can't understand \"{name}\"")))
            }
        }
    }

    /// Start running a handler. `me` is the object whose script holds it, and
    /// `target` is the object the message went to.
    fn push_handler_frame(
        &mut self,
        handler: Rc<Handler>,
        args: Vec<String>,
        me: Option<ObjId>,
        target: Option<ObjId>,
        wants_value: bool,
    ) -> Result<(), XtError> {
        if self.frames.len() >= MAX_DEPTH {
            return Err(XtError::new(format!(
                "Too many nested handler calls. Is '{}' calling itself forever?",
                handler.name
            )));
        }
        let mut frame = Frame::new(Rc::clone(&handler), false, wants_value, HashMap::new());
        // Missing arguments are empty and extra ones are ignored (spec HND-4).
        for (i, param) in handler.params.iter().enumerate() {
            frame.locals.insert(
                param.to_lowercase(),
                args.get(i).cloned().unwrap_or_default(),
            );
        }
        frame.args = args;
        frame.me = me;
        frame.target = target;
        self.frames.push(frame);
        Ok(())
    }

    /// Look for a handler along the path from `start`, then among the
    /// engine-level handlers. With `after`, the search begins after that object.
    fn find_handler(
        &self,
        start: ObjId,
        name: &str,
        after: Option<ObjId>,
    ) -> Option<(Option<ObjId>, Rc<Handler>)> {
        let lname = name.to_lowercase();
        let mut remaining = self.world.path(start).into_iter();
        if let Some(after) = after {
            for obj in remaining.by_ref() {
                if obj == after {
                    break;
                }
            }
        }
        for obj in remaining {
            if let Some(handler) = self.world.get(obj).and_then(|o| o.handlers.get(&lname)) {
                return Some((Some(obj), Rc::clone(handler)));
            }
        }
        self.handlers.get(&lname).map(|h| (None, Rc::clone(h)))
    }

    /// Send a message to an object and wait for the handler. A message nobody
    /// handles is an error, unless it is a system message or `quiet` is set.
    fn send_message(&mut self, target: ObjId, name: &str, quiet: bool) -> Result<(), XtError> {
        match self.find_handler(target, name, None) {
            Some((owner, handler)) => {
                self.push_handler_frame(handler, Vec::new(), owner, Some(target), false)
            }
            None => {
                if quiet || is_system_message(name) {
                    Ok(())
                } else {
                    Err(XtError::new(format!("Can't understand \"{name}\"")))
                }
            }
        }
    }

    /// `pass`: end this handler and send the message on to the objects after
    /// the one whose script holds it.
    fn pass_message(&mut self, name: &str) -> Result<(), XtError> {
        let Some(frame) = self.frames.pop() else {
            return Ok(());
        };
        let Frame {
            me,
            target,
            args,
            wants_value,
            is_main,
            locals,
            ..
        } = frame;
        if is_main {
            self.main_locals = locals;
        }
        if let (Some(owner), Some(target)) = (me, target) {
            if let Some((new_owner, handler)) = self.find_handler(target, name, Some(owner)) {
                return self.push_handler_frame(
                    handler,
                    args,
                    new_owner,
                    Some(target),
                    wants_value,
                );
            }
        }
        // Nobody else handles it. A function that passed has no value.
        if wants_value {
            if let Some(caller) = self.frames.last_mut() {
                caller.stack.push(String::new());
            }
        }
        Ok(())
    }

    // ----- objects ---------------------------------------------------------

    /// Where to look for objects of `kind`: the container named, or else the
    /// current stack (for cards, backgrounds, and clips) or the current card.
    fn container_for(&self, kind: ObjKind, container: Option<String>) -> Result<ObjId, XtError> {
        if let Some(text) = container {
            return self.world.lookup_ref(&text);
        }
        let default = match kind {
            ObjKind::Background | ObjKind::Card | ObjKind::AudioClip | ObjKind::VideoClip => {
                self.world.current_stack
            }
            _ => self.world.current_card,
        };
        default.ok_or_else(|| XtError::new("There is no current card"))
    }

    fn find_object(
        &mut self,
        kind: ObjKind,
        shape: ObjShape,
        operand: Option<String>,
        container: Option<String>,
    ) -> Result<ObjId, XtError> {
        if let (ObjShape::Id, Some(text)) = (shape, &operand) {
            let n = to_number(text)?.trunc() as i64;
            // Ids are numbered per stack: look in the container's stack, as
            // in `button id 3 of stack "Demo"`, or else the current stack.
            let stack = match &container {
                Some(c) => {
                    let c = self.world.lookup_ref(c)?;
                    self.world.stack_of(c)
                }
                None => self.world.current_stack,
            };
            return self
                .world
                .find_by_id(kind, n, stack)
                .ok_or_else(|| XtError::new(format!("There is no {} id {n}", kind_name(kind))));
        }

        let list: Vec<ObjId> = if kind == ObjKind::Stack {
            self.world.stacks.clone()
        } else {
            let parent = self.container_for(kind, container)?;
            self.world.children_of_kind(parent, kind)
        };
        let n = list.len();
        let mut describe = String::new();
        let picked: Option<ObjId> = match shape {
            ObjShape::At => {
                let text = operand.unwrap_or_default();
                if let Some(number) = parse_num(&text) {
                    let i = number.trunc() as i64;
                    describe = text;
                    if i >= 1 && (i as usize) <= n {
                        Some(list[(i - 1) as usize])
                    } else {
                        None
                    }
                } else {
                    describe = format!("\"{text}\"");
                    self.world.first_named(&list, &text)
                }
            }
            ObjShape::Ordinal(ordinal) => {
                let index = match ordinal {
                    Ordinal::Nth(k) => k,
                    Ordinal::Last => n,
                    Ordinal::Middle => n / 2 + 1,
                    Ordinal::Any => {
                        if n == 0 {
                            0
                        } else {
                            (self.next_random() % n as u64) as usize + 1
                        }
                    }
                };
                describe = "in that position".to_string();
                if n > 0 && index >= 1 && index <= n {
                    Some(list[index - 1])
                } else {
                    None
                }
            }
            ObjShape::Next | ObjShape::Prev => {
                let current = match kind {
                    ObjKind::Card => self.world.current_card,
                    ObjKind::Stack => self.world.current_stack,
                    ObjKind::Background => self
                        .world
                        .current_card
                        .and_then(|c| self.world.get(c))
                        .and_then(|o| o.background),
                    _ => {
                        return Err(XtError::new(
                            "Only stacks, backgrounds, and cards can be next or previous",
                        ))
                    }
                };
                describe = "there".to_string();
                match current.and_then(|c| list.iter().position(|x| *x == c)) {
                    Some(pos) if n > 0 => {
                        let index = if matches!(shape, ObjShape::Next) {
                            (pos + 1) % n
                        } else {
                            (pos + n - 1) % n
                        };
                        Some(list[index])
                    }
                    _ => None,
                }
            }
            ObjShape::Id => None,
        };
        picked.ok_or_else(|| XtError::new(format!("There is no {} {describe}", kind_name(kind))))
    }

    fn require_field(&self, id: ObjId) -> Result<(), XtError> {
        match self.world.get(id) {
            Some(o) if o.kind == ObjKind::Field => Ok(()),
            Some(o) => Err(XtError::new(format!(
                "You can only put text into a field, not a {}",
                kind_name(o.kind)
            ))),
            None => Err(object_gone()),
        }
    }

    fn create_object(
        &mut self,
        kind: ObjKind,
        name: &str,
        parent_ref: Option<String>,
    ) -> Result<ObjId, XtError> {
        match kind {
            ObjKind::Stack => {
                let stack = self.world.create_stack(name);
                self.world.current_stack = Some(stack);
                self.world.current_card = self.world.first_card(stack);
                Ok(stack)
            }
            ObjKind::Card | ObjKind::Background | ObjKind::AudioClip | ObjKind::VideoClip => {
                let stack = match parent_ref {
                    Some(text) => self.world.lookup_ref(&text)?,
                    None => self
                        .world
                        .current_stack
                        .ok_or_else(|| XtError::new("There is no current stack"))?,
                };
                if self.world.get(stack).map(|o| o.kind) != Some(ObjKind::Stack) {
                    return Err(XtError::new(format!(
                        "{} can only be made in a stack",
                        with_article(kind_name(kind))
                    )));
                }
                let created = self.world.create(kind, name, Some(stack));
                if kind == ObjKind::Card {
                    // A new card uses the background of the current card, or
                    // else the first background of its stack.
                    let background = self
                        .world
                        .current_card
                        .filter(|c| self.world.stack_of(*c) == Some(stack))
                        .and_then(|c| self.world.get(c))
                        .and_then(|o| o.background)
                        .or_else(|| {
                            self.world
                                .children_of_kind(stack, ObjKind::Background)
                                .first()
                                .copied()
                        });
                    if let Some(obj) = self.world.get_mut(created) {
                        obj.background = background;
                    }
                    if Some(stack) == self.world.current_stack {
                        self.world.current_card = Some(created);
                    }
                }
                Ok(created)
            }
            _ => {
                let parent = match parent_ref {
                    Some(text) => self.world.lookup_ref(&text)?,
                    None => self
                        .world
                        .current_card
                        .ok_or_else(|| XtError::new("There is no current card"))?,
                };
                let parent_kind = self.world.get(parent).map(|o| o.kind);
                if !matches!(
                    parent_kind,
                    Some(ObjKind::Card | ObjKind::Background | ObjKind::Group)
                ) {
                    return Err(XtError::new(
                        "A control can only be made on a card, a background, or a group",
                    ));
                }
                Ok(self.world.create(kind, name, Some(parent)))
            }
        }
    }

    fn delete_object(&mut self, id: ObjId) -> Result<(), XtError> {
        let (kind, parent) = match self.world.get(id) {
            Some(o) => (o.kind, o.parent),
            None => return Err(object_gone()),
        };
        match kind {
            ObjKind::Stack => {
                if self.world.stacks.len() <= 1 {
                    return Err(XtError::new("Can't delete the last stack"));
                }
                let was_current = self.world.current_stack == Some(id);
                self.world.delete(id);
                if was_current {
                    let next = self.world.stacks.first().copied();
                    self.world.current_stack = next;
                    self.world.current_card = next.and_then(|s| self.world.first_card(s));
                }
            }
            ObjKind::Card => {
                let stack = parent.ok_or_else(object_gone)?;
                let cards = self.world.children_of_kind(stack, ObjKind::Card);
                if cards.len() <= 1 {
                    return Err(XtError::new("Can't delete the last card of a stack"));
                }
                let was_current = self.world.current_card == Some(id);
                let position = cards.iter().position(|c| *c == id).unwrap_or(0);
                self.world.delete(id);
                if was_current {
                    let remaining = self.world.children_of_kind(stack, ObjKind::Card);
                    let index = position.min(remaining.len().saturating_sub(1));
                    self.world.current_card = remaining.get(index).copied();
                }
            }
            ObjKind::Background => {
                let stack = parent.ok_or_else(object_gone)?;
                let in_use = self
                    .world
                    .children_of_kind(stack, ObjKind::Card)
                    .iter()
                    .any(|c| self.world.get(*c).is_some_and(|o| o.background == Some(id)));
                if in_use {
                    return Err(XtError::new(
                        "Can't delete a background that cards are using",
                    ));
                }
                self.world.delete(id);
            }
            _ => self.world.delete(id),
        }
        Ok(())
    }

    /// Make `id` (a card, or a stack's first card) the current card. Returns
    /// the references of the new and the old card, or empty ones if nothing
    /// changed, so the caller can send `closeCard` and `openCard`.
    fn go_to(&mut self, id: ObjId) -> Result<(String, String), XtError> {
        let kind = self.world.get(id).map(|o| o.kind).ok_or_else(object_gone)?;
        let new_card = match kind {
            ObjKind::Card => id,
            ObjKind::Stack => self
                .world
                .first_card(id)
                .ok_or_else(|| XtError::new("That stack has no cards"))?,
            _ => return Err(XtError::new("You can only go to a card or a stack")),
        };
        let old = self.world.current_card;
        self.world.current_stack = self.world.stack_of(new_card);
        self.world.current_card = Some(new_card);
        if old == Some(new_card) {
            return Ok((String::new(), String::new()));
        }
        let new_ref = self.world.ref_string(new_card).unwrap_or_default();
        let old_ref = old
            .and_then(|o| self.world.ref_string(o))
            .unwrap_or_default();
        Ok((new_ref, old_ref))
    }

    // ----- host calls that finish now or later (HST-1 to HST-3) -------------

    /// Start waiting: stay on the current op so it comes back when `run` is
    /// called again.
    fn wait_for(&mut self, waiting: Waiting) -> Flow {
        self.waiting = Some(waiting);
        self.frame_mut().pc -= 1;
        Flow::Wait
    }

    /// Back at the op that is waiting: finish it if the host has replied,
    /// or keep waiting.
    fn resume_waiting(&mut self) -> Result<Flow, XtError> {
        let Some(reply) = self.reply.take() else {
            self.frame_mut().pc -= 1;
            return Ok(Flow::Wait);
        };
        let Some(waiting) = self.waiting.take() else {
            return Ok(Flow::Continue);
        };
        match (waiting, reply) {
            (Waiting::Answer { labels }, HostReply::Button(choice)) => {
                self.finish_answer(&labels, choice)
            }
            (Waiting::Text, HostReply::Text(text)) => self.finish_text(text),
            (Waiting::Save { stack, path }, HostReply::Saved(done)) => {
                self.finish_save(stack, path, done)?
            }
            (Waiting::Open { path }, HostReply::Opened(tree)) => self.finish_open(path, tree)?,
            // `reply` only accepts the matching kind.
            _ => return Err(XtError::new("Internal error: the wrong kind of reply")),
        }
        Ok(Flow::Continue)
    }

    fn finish_answer(&mut self, labels: &[String], choice: usize) {
        let picked = labels[choice.min(labels.len() - 1)].clone();
        self.write_var("it", picked);
    }

    /// `ask` and the dialog forms: the reply goes in `it`; a cancel leaves
    /// `it` empty and sets `the result` to Cancel.
    fn finish_text(&mut self, reply: Option<String>) {
        match reply {
            Some(text) => {
                self.write_var("it", text);
                self.result = String::new();
            }
            None => {
                self.write_var("it", String::new());
                self.result = "Cancel".to_string();
            }
        }
    }

    fn finish_save(
        &mut self,
        stack: ObjId,
        path: String,
        done: Result<(), XtError>,
    ) -> Result<(), XtError> {
        done?;
        if self.world.get(stack).is_some() {
            self.world.files.insert(stack, path);
        }
        Ok(())
    }

    fn finish_open(
        &mut self,
        path: String,
        tree: Result<PartNode, XtError>,
    ) -> Result<(), XtError> {
        let stack = self.add_stack(&tree?)?;
        self.world.files.insert(stack, path);
        let text = self.world.ref_string(stack).unwrap_or_default();
        self.push(text);
        Ok(())
    }

    /// A stack as a tree of plain data, for a host or a tool that saves
    /// stacks itself (SFM-1). `reference` is a stack reference such as
    /// `stack id 1`.
    pub fn export_stack(&self, reference: &str) -> Result<PartNode, XtError> {
        let id = self.world.lookup_ref(reference)?;
        if self.world.get(id).map(|o| o.kind) != Some(ObjKind::Stack) {
            return Err(XtError::new("Only a stack can be exported"));
        }
        self.world.export(id)
    }

    /// Add the stack in a tree to the engine without going to it, and
    /// return its reference (SFM-2).
    pub fn import_stack(&mut self, tree: &PartNode) -> Result<String, XtError> {
        let stack = self.add_stack(tree)?;
        Ok(self.world.ref_string(stack).unwrap_or_default())
    }

    /// Build a stack from a tree and compile its scripts. If any script has
    /// an error, the whole stack is removed again, so a stack is either
    /// opened completely or not at all.
    fn add_stack(&mut self, tree: &PartNode) -> Result<ObjId, XtError> {
        let (stack, scripts) = self.world.import_stack(tree)?;
        for (id, text) in scripts {
            if let Err(e) = self.set_script(id, &text) {
                let which = self.world.ref_string(id).unwrap_or_default();
                self.world.delete(stack);
                return Err(XtError::new(format!(
                    "In the script of {which}: {}",
                    e.message
                )));
            }
        }
        Ok(stack)
    }

    /// Compile a script and install its handlers on an object.
    fn set_script(&mut self, id: ObjId, text: &str) -> Result<(), XtError> {
        let script = parse_script(text).map_err(|e| {
            let place = e
                .line
                .map(|l| format!(" (script line {})", l + 1))
                .unwrap_or_default();
            XtError::new(format!("The script has an error{place}: {}", e.message))
        })?;
        if !script.main.is_empty() {
            return Err(XtError::new("An object's script can only contain handlers"));
        }
        let mut table = HashMap::new();
        for def in &script.handlers {
            let handler = compile_handler(&def.name, &def.params, &def.body);
            table.insert(def.name.to_lowercase(), Rc::new(handler));
        }
        match self.world.get_mut(id) {
            Some(obj) => {
                obj.handlers = table;
                obj.props.insert("script".to_string(), text.to_string());
                Ok(())
            }
            None => Err(object_gone()),
        }
    }

    /// The stacks, cards, and controls as an indented list. The current stack
    /// and card are marked with a star.
    pub fn describe_objects(&self) -> String {
        let mut out = String::new();
        for &stack in &self.world.stacks {
            self.describe_object(stack, 0, &mut out);
        }
        out
    }

    fn describe_object(&self, id: ObjId, depth: usize, out: &mut String) {
        let Some(obj) = self.world.get(id) else {
            return;
        };
        let current = Some(id) == self.world.current_stack || Some(id) == self.world.current_card;
        let name = obj.props.get("name").cloned().unwrap_or_default();
        out.push_str(&format!(
            "{}{} id {} \"{}\"{}\n",
            "  ".repeat(depth),
            kind_name(obj.kind),
            obj.id,
            name,
            if current { " *" } else { "" }
        ));
        for &child in &obj.children {
            self.describe_object(child, depth + 1, out);
        }
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
        self.waiting = None;
        self.reply = None;
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
        let is_global = self.frames.last().is_some_and(|f| f.globals.contains(&key));
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

/// "A card", "An audioClip": the kind name with its article, capitalized.
fn with_article(name: &str) -> String {
    let vowel = name.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {name}", if vowel { "An" } else { "A" })
}

fn object_gone() -> XtError {
    XtError::new("That object no longer exists")
}

/// Messages the engine and backends send on their own. One that nobody
/// handles is ignored instead of being an error.
fn is_system_message(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "mousedown"
            | "mouseup"
            | "mouseenter"
            | "mouseleave"
            | "mousestilldown"
            | "keydown"
            | "keyup"
            | "opencard"
            | "closecard"
            | "openstack"
            | "closestack"
            | "openbackground"
            | "closebackground"
            | "idle"
            | "newcard"
            | "newbutton"
            | "newfield"
            | "deletecard"
            | "deletebutton"
            | "deletefield"
    )
}
