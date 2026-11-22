//! Running shell commands (spec section 11, shell access).
//!
//! `run_blocking` waits for the command, for the `shell()` function.
//! `spawn` starts the command on a worker thread and returns a `Job` the VM
//! can poll, so `do ... as shell` pauses the script without freezing the host.
//!
//! Commands get no standard input, so a tool that waits for typing cannot
//! hang the engine. Standard error is discarded: add `2>&1` to the command
//! to see it.

use std::process::{Command, Output as ProcessOutput, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crate::error::XtError;

/// What a finished command produced.
pub struct Output {
    /// Standard output, with Windows line endings turned into newlines.
    pub stdout: String,
    /// The exit status, or -1 if the command was ended by a signal.
    pub status: i32,
}

const SHELLS: &[&str] = &[
    "shell",
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
    "powershell",
    "pwsh",
    "cmd",
];

/// Names accepted after `as` in `do ... as <name>`.
pub fn is_shell_name(name: &str) -> bool {
    SHELLS.contains(&name.to_lowercase().as_str())
}

/// Build the command that runs `script` with the named shell. `shell` means
/// the platform's own shell: /bin/sh on macOS and Linux, cmd on Windows.
fn command_for(shell: &str, script: &str) -> Result<Command, XtError> {
    let lower = shell.to_lowercase();
    let (program, flag) = match lower.as_str() {
        "shell" => {
            if cfg!(windows) {
                ("cmd", "/C")
            } else {
                ("/bin/sh", "-c")
            }
        }
        "sh" => {
            if cfg!(windows) {
                ("sh", "-c")
            } else {
                ("/bin/sh", "-c")
            }
        }
        "cmd" => ("cmd", "/C"),
        "powershell" | "pwsh" => (lower.as_str(), "-Command"),
        "bash" | "zsh" | "dash" | "ksh" | "fish" | "csh" | "tcsh" => (lower.as_str(), "-c"),
        _ => return Err(XtError::new(format!("Unknown shell '{shell}'"))),
    };
    let mut command = Command::new(program);
    command.arg(flag).arg(script);
    Ok(command)
}

fn to_output(out: ProcessOutput) -> Output {
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        status: out.status.code().unwrap_or(-1),
    }
}

/// Run a command and wait for it to finish.
pub fn run_blocking(shell: &str, script: &str) -> Result<Output, XtError> {
    let mut command = command_for(shell, script)?;
    command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(to_output)
        .map_err(|e| XtError::new(format!("Could not run {shell}: {e}")))
}

/// A command running on a worker thread.
pub struct Job {
    rx: Receiver<Result<Output, XtError>>,
}

/// Start a command on a worker thread and return at once.
pub fn spawn(shell: &str, script: &str) -> Result<Job, XtError> {
    // Build the command here so an unknown shell is reported immediately.
    let mut command = command_for(shell, script)?;
    let shell_name = shell.to_string();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = command
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(to_output)
            .map_err(|e| XtError::new(format!("Could not run {shell_name}: {e}")));
        let _ = tx.send(result);
    });
    Ok(Job { rx })
}

impl Job {
    /// `None` while the command is still running.
    pub fn poll(&mut self) -> Option<Result<Output, XtError>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(XtError::new("The shell command was interrupted")))
            }
        }
    }
}
