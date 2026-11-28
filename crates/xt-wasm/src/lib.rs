//! xt-wasm: libreXTalk as a WebAssembly module for browsers and Node.js
//! (CLAUDE.md 5.18).
//!
//! # How JavaScript talks to the engine
//!
//! The module exports a few plain functions and imports none, so it loads
//! anywhere WebAssembly runs and needs no generated glue. Text crosses the
//! boundary through two buffers that the engine owns:
//!
//! 1. To send text in, JS calls `xt_input(len)`, which makes room for `len`
//!    bytes and returns where they go, writes the UTF-8 bytes there, then
//!    calls the function that uses them (`xt_load`, `xt_reply_text`, ...).
//! 2. Every function leaves a JSON answer in the output buffer, which JS
//!    reads from `xt_output_ptr()` and `xt_output_len()`.
//!
//! Every answer has a `status`:
//!
//! - `"finished"`: the script ran to the end.
//! - `"sleeping"`: the script is in a `wait`; call `xt_run` again after `ms`.
//! - `"waiting"`: the script needs something from the page, described by
//!   `request` (a dialog, a file to save, a file to open). Do it, give the
//!   answer with one of the `xt_reply_...` functions, then call `xt_run`.
//! - `"error"`: something went wrong; `error` has a `message` and, when
//!   known, a `line` (counting from 1).
//!
//! and `messages`, the text that `put` showed since the last answer.
//!
//! No `unsafe` code is needed: JS writes into memory the engine has handed
//! out, and the engine reads its own buffer afterwards.

use std::cell::RefCell;

use xt_core::{
    DialogForm, DialogRequest, Host, HostReply, Interpreter, PartNode, Pending, Status, XtError,
};

mod json;

use json::Json;

// ----- the engine, as ordinary Rust ------------------------------------------

/// The host the engine sees inside a web page or Node.js. It cannot show
/// anything itself, so it collects what the script shows and answers every
/// dialog and file call `Later`, describing it for the page to carry out.
#[derive(Default)]
struct WebHost {
    messages: Vec<String>,
    /// What the script is waiting for, as JSON.
    request: Option<String>,
    /// The page's clock, given with each `run`: WebAssembly here has no
    /// clock of its own.
    now: std::time::Duration,
}

impl WebHost {
    fn later<T>(&mut self, request: Json) -> Pending<T> {
        self.request = Some(request.to_string());
        Pending::Later
    }
}

impl Host for WebHost {
    fn message(&mut self, text: &str) {
        self.messages.push(text.to_string());
    }

    fn now(&mut self) -> std::time::Duration {
        self.now
    }

    fn answer(&mut self, prompt: &str, buttons: &[String]) -> Pending<usize> {
        self.later(Json::object([
            ("kind", Json::from("answer")),
            ("prompt", Json::from(prompt)),
            ("buttons", Json::strings(buttons)),
        ]))
    }

    fn ask(&mut self, prompt: &str, default: &str) -> Pending<Option<String>> {
        self.later(Json::object([
            ("kind", Json::from("ask")),
            ("prompt", Json::from(prompt)),
            ("default", Json::from(default)),
        ]))
    }

    fn choose(&mut self, request: &DialogRequest) -> Pending<Option<String>> {
        let form = match request.form {
            DialogForm::AnswerFile => "answer file",
            DialogForm::AskFile => "ask file",
            DialogForm::AnswerFolder => "answer folder",
            DialogForm::AnswerColor => "answer color",
            DialogForm::AnswerEffect => "answer effect",
            DialogForm::AnswerRecord => "answer record",
        };
        self.later(Json::object([
            ("kind", Json::from("dialog")),
            ("form", Json::from(form)),
            ("prompt", Json::from(request.prompt)),
            ("default", Json::from(request.default)),
            ("types", Json::strings(request.types)),
        ]))
    }

    /// The page decides where a saved stack goes (a download, the browser's
    /// own storage, a file picked by the person), so the engine hands it the
    /// finished `.xtstack.html` text.
    fn save_stack(&mut self, path: &str, stack: &PartNode) -> Pending<Result<(), XtError>> {
        let bytes = match xt_stackfile::encode(path, stack) {
            Ok(bytes) => bytes,
            Err(e) => return Pending::Ready(Err(e)),
        };
        // `encode` writes text for the only format it can write today.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.later(Json::object([
            ("kind", Json::from("save")),
            ("path", Json::from(path)),
            ("text", Json::from(text.as_str())),
        ]))
    }

    fn open_stack(&mut self, path: &str) -> Pending<Result<PartNode, XtError>> {
        self.later(Json::object([
            ("kind", Json::from("open")),
            ("path", Json::from(path)),
        ]))
    }
}

/// One engine and its web host.
#[derive(Default)]
pub struct Engine {
    interp: Interpreter,
    host: WebHost,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a script. Its handlers are remembered; its loose statements run
    /// on the next `run`.
    pub fn load(&mut self, source: &str) -> String {
        match self.interp.load(source) {
            Ok(()) => self.answer("loaded", None),
            Err(e) => self.error(&e),
        }
    }

    /// Run until the script finishes, sleeps, or waits for the page.
    /// `now_ms` is the page's clock (`Date.now()`): milliseconds since 1970.
    pub fn run(&mut self, now_ms: f64) -> String {
        if now_ms.is_finite() && now_ms > 0.0 {
            self.host.now = std::time::Duration::from_secs_f64(now_ms / 1000.0);
        }
        match self.interp.run(&mut self.host) {
            Ok(Status::Finished) => self.answer("finished", None),
            Ok(Status::Sleeping(duration)) => {
                let ms = duration.as_secs_f64() * 1000.0;
                self.answer("sleeping", Some(("ms", Json::Number(ms))))
            }
            Ok(Status::Waiting) => {
                let request = self.host.request.clone().unwrap_or_else(|| "null".into());
                self.answer("waiting", Some(("request", Json::Raw(request))))
            }
            Err(e) => self.error(&e),
        }
    }

    /// Give the page's answer to what the script is waiting for.
    pub fn reply(&mut self, reply: HostReply) -> String {
        match self.interp.reply(reply) {
            Ok(()) => {
                self.host.request = None;
                self.answer("replied", None)
            }
            Err(e) => self.error(&e),
        }
    }

    /// The page read the file the script asked to open: turn its text into
    /// a stack. A file that is not a stack becomes the reply's error, which
    /// the script can catch.
    pub fn reply_opened(&mut self, file_text: &str) -> String {
        let tree = xt_stackfile::decode(file_text.as_bytes());
        self.reply(HostReply::Opened(tree))
    }

    fn answer(&mut self, status: &str, extra: Option<(&str, Json)>) -> String {
        let messages = std::mem::take(&mut self.host.messages);
        let mut fields = vec![
            ("status", Json::from(status)),
            ("messages", Json::strings(&messages)),
        ];
        if let Some(field) = extra {
            fields.push(field);
        }
        Json::object(fields).to_string()
    }

    fn error(&mut self, e: &XtError) -> String {
        let mut error = vec![("message", Json::from(e.message.as_str()))];
        if let Some(line) = e.line {
            error.push(("line", Json::Number((line + 1) as f64)));
        }
        self.answer("error", Some(("error", Json::object(error))))
    }
}

// ----- the exported functions ---------------------------------------------

/// The one engine of this module instance, with the two text buffers.
/// WebAssembly here runs on one thread, so a thread-local is enough.
#[derive(Default)]
struct Module {
    engine: Engine,
    input: Vec<u8>,
    output: Vec<u8>,
}

thread_local! {
    static MODULE: RefCell<Module> = RefCell::new(Module::default());
}

/// Run `f` on the engine with the input text, and keep its JSON answer as
/// the output. Returns 0 when the answer's status is not `"error"`.
fn call(f: impl FnOnce(&mut Engine, &str) -> String) -> i32 {
    MODULE.with(|m| {
        let mut m = m.borrow_mut();
        let input = std::mem::take(&mut m.input);
        let answer = match String::from_utf8(input) {
            Ok(text) => f(&mut m.engine, &text),
            Err(_) => {
                let e = XtError::new("The text given to the engine is not UTF-8");
                m.engine.error(&e)
            }
        };
        let failed = answer.starts_with("{\"status\":\"error\"");
        m.output = answer.into_bytes();
        i32::from(failed)
    })
}

/// Make room for `len` bytes of input and return where to write them.
#[no_mangle]
pub extern "C" fn xt_input(len: u32) -> *mut u8 {
    MODULE.with(|m| {
        let mut m = m.borrow_mut();
        m.input.clear();
        m.input.resize(len as usize, 0);
        m.input.as_mut_ptr()
    })
}

/// Where the last JSON answer starts.
#[no_mangle]
pub extern "C" fn xt_output_ptr() -> *const u8 {
    MODULE.with(|m| m.borrow().output.as_ptr())
}

/// How many bytes the last JSON answer has.
#[no_mangle]
pub extern "C" fn xt_output_len() -> u32 {
    MODULE.with(|m| m.borrow().output.len() as u32)
}

/// Load the script in the input.
#[no_mangle]
pub extern "C" fn xt_load() -> i32 {
    call(|engine, text| engine.load(text))
}

/// Run until the script finishes, sleeps, or waits. `now_ms` is the
/// page's clock, `Date.now()`.
#[no_mangle]
pub extern "C" fn xt_run(now_ms: f64) -> i32 {
    call(|engine, _| engine.run(now_ms))
}

/// `answer`: the person chose button number `index` (counting from 0).
#[no_mangle]
pub extern "C" fn xt_reply_button(index: u32) -> i32 {
    call(|engine, _| engine.reply(HostReply::Button(index as usize)))
}

/// `ask` or a dialog form: the reply is the input text.
#[no_mangle]
pub extern "C" fn xt_reply_text() -> i32 {
    call(|engine, text| engine.reply(HostReply::Text(Some(text.to_string()))))
}

/// `ask` or a dialog form: the person cancelled.
#[no_mangle]
pub extern "C" fn xt_reply_cancel() -> i32 {
    call(|engine, _| engine.reply(HostReply::Text(None)))
}

/// `save stack`: the page saved the text.
#[no_mangle]
pub extern "C" fn xt_reply_saved() -> i32 {
    call(|engine, _| engine.reply(HostReply::Saved(Ok(()))))
}

/// `open stack`: the input is the file's text.
#[no_mangle]
pub extern "C" fn xt_reply_opened() -> i32 {
    call(|engine, text| engine.reply_opened(text))
}

/// `save stack` or `open stack` failed; the input says why. The script
/// sees it as an error it can catch with `try`.
#[no_mangle]
pub extern "C" fn xt_reply_failed() -> i32 {
    call(|engine, text| {
        let error = XtError::new(text);
        let reply = if engine.waiting_for_open() {
            HostReply::Opened(Err(error))
        } else {
            HostReply::Saved(Err(error))
        };
        engine.reply(reply)
    })
}

impl Engine {
    /// Is the outstanding request an `open`? (Used to pick the kind of a
    /// failure reply.)
    fn waiting_for_open(&self) -> bool {
        self.host
            .request
            .as_deref()
            .is_some_and(|r| r.starts_with("{\"kind\":\"open\""))
    }
}

#[cfg(test)]
mod tests;
