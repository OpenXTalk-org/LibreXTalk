//! `xt`: run xTalk scripts, or type them at a prompt.
//!
//!     xt                 start the prompt
//!     xt run file.xt     run a script file

use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use xt_core::{parse_script, Host, Interpreter, XtError};

/// The host for a terminal: the message box is standard output, and a wait
/// really sleeps (the default `sleep` in the `Host` trait).
struct StdoutHost {
    /// May scripts run shell commands? See `Host::allow_shell`.
    allow_shell: bool,
}

impl Host for StdoutHost {
    fn allow_shell(&mut self, _shell: &str, _script: &str) -> bool {
        if !self.allow_shell {
            eprintln!("(shell commands are off: run with --allow-shell to permit them)");
        }
        self.allow_shell
    }

    fn message(&mut self, text: &str) {
        println!("{text}");
    }

    fn answer(&mut self, prompt: &str, buttons: &[String]) -> usize {
        // With only one button there is nothing to choose, so show the
        // message and carry on without waiting.
        if buttons.len() <= 1 {
            match buttons.first() {
                Some(label) => println!("{prompt} [{label}]"),
                None => println!("{prompt}"),
            }
            return 0;
        }

        println!("{prompt}");
        for (i, label) in buttons.iter().enumerate() {
            if i == 0 {
                println!("  {}: {label} (default)", i + 1);
            } else {
                println!("  {}: {label}", i + 1);
            }
        }
        loop {
            print!("Choose 1-{} [1]: ", buttons.len());
            io::stdout().flush().ok();
            let mut line = String::new();
            match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => return 0, // end of input: pick the first button
                Ok(_) => {}
            }
            let reply = line.trim();
            if reply.is_empty() {
                return 0; // Return alone accepts the default
            }
            if let Ok(n) = reply.parse::<usize>() {
                if (1..=buttons.len()).contains(&n) {
                    return n - 1;
                }
            }
            if let Some(i) = buttons.iter().position(|b| b.eq_ignore_ascii_case(reply)) {
                return i;
            }
        }
    }

    fn ask(&mut self, prompt: &str, default: &str) -> Option<String> {
        if default.is_empty() {
            print!("{prompt} ");
        } else {
            print!("{prompt} [{default}] ");
        }
        io::stdout().flush().ok();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None, // end of input counts as cancelling
            Ok(_) => {
                let reply = line.trim_end_matches(|c: char| c == '\n' || c == '\r');
                if reply.is_empty() {
                    Some(default.to_string())
                } else {
                    Some(reply.to_string())
                }
            }
        }
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  xt               start the prompt");
    println!("  xt run <file>    run a script file");
    println!("  xt run --allow-shell <file>");
    println!("                   run a script that may use shell commands");
}

fn print_help() {
    println!("Try these:");
    println!("  put \"Hello World\"");
    println!("  put 2 + 3 * 4");
    println!("  repeat with i = 1 to 3");
    println!("    put i");
    println!("  end repeat");
    println!("  on greet name");
    println!("    put \"Hello,\" && name");
    println!("  end greet");
    println!("  greet \"World\"");
    println!("  put word 2 of \"alpha beta gamma\"");
    println!("  put the number of words in \"alpha beta gamma\"");
    println!("  put \"X\" into word 2 of s");
    println!("  add 5 to total");
    println!("  put shell(\"uname -m\")");
    println!("  do \"sleep 2; date\" as bash      -- then: put it");
    println!("  put round(2.5) && length(\"hello\")");
    println!("  try");
    println!("    put 1 / 0");
    println!("  catch e");
    println!("    put \"Caught:\" && e");
    println!("  end try");
    println!();
    println!("Separate statements on one line with a semicolon: put 1; put 2");
    println!("A block (if, repeat, on, function) can span several lines.");
    println!("The prompt shows ... until the block is finished.");
    println!();
    println!("Prompt commands:");
    println!("  :vars        show all variables");
    println!("  :ast <line>  show how a line is parsed");
    println!("  :help        show this help");
    println!("  :quit        leave (Ctrl-D works too)");
    println!();
    println!("Up and down arrows recall earlier lines. Ctrl-C cancels a line.");
}

/// Print an error. Syntax errors point at the place in the source.
fn report_error(source: &str, err: &XtError, show_line: bool) {
    match (err.line, err.col) {
        (Some(line), Some(col)) => {
            let text = source.lines().nth(line).unwrap_or("");
            eprintln!("  {text}");
            eprintln!("  {}^", " ".repeat(col));
            if show_line {
                eprintln!("Error (line {}): {err}", line + 1);
            } else {
                eprintln!("Error: {err}");
            }
        }
        (Some(line), None) if show_line => {
            let text = source.lines().nth(line).unwrap_or("");
            eprintln!("  {text}");
            eprintln!("Error (line {}): {err}", line + 1);
        }
        _ => eprintln!("Error: {err}"),
    }
}

fn run_file(path: &str, allow_shell: bool) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read {path}: {e}");
            process::exit(1);
        }
    };
    let mut interp = Interpreter::new();
    let mut host = StdoutHost { allow_shell };
    if let Err(err) = interp.load(&source) {
        report_error(&source, &err, true);
        process::exit(1);
    }
    if let Err(err) = interp.run_to_end(&mut host) {
        report_error(&source, &err, true);
        process::exit(1);
    }
}

fn show_ast(source: &str) {
    match parse_script(source) {
        Ok(script) => println!("{script:#?}"),
        Err(err) => report_error(source, &err, false),
    }
}

/// Where the prompt remembers earlier lines between sessions.
fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".xt_history"))
}

fn repl() {
    println!("xt 0.0.6. Type :help for help, :quit to leave.");

    let mut interp = Interpreter::new();
    // At the prompt you are the one typing, so shell commands are allowed.
    let mut host = StdoutHost { allow_shell: true };

    // The line editor gives us the arrow keys: up and down page through
    // earlier lines, left and right move within the line, and so on.
    let mut editor = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("Could not start the line editor: {e}");
            process::exit(1);
        }
    };
    let history_file = history_path();
    if let Some(path) = &history_file {
        // It is fine if there is no history file yet.
        let _ = editor.load_history(path);
    }

    // Lines of a block that is not finished yet.
    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() { "> " } else { "... " };
        let line = match editor.readline(prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C abandons whatever is being typed.
                if !buffer.is_empty() {
                    println!("(cancelled)");
                }
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => break, // Ctrl-D
            Err(e) => {
                eprintln!("Could not read input: {e}");
                break;
            }
        };
        let input = line.trim_end();
        if !input.trim().is_empty() {
            let _ = editor.add_history_entry(input);
        }

        if buffer.is_empty() {
            let trimmed = input.trim();
            if let Some(rest) = trimmed.strip_prefix(":ast ") {
                show_ast(rest);
                continue;
            }
            match trimmed {
                ":quit" | ":q" => break,
                ":help" => {
                    print_help();
                    continue;
                }
                ":vars" => {
                    for (name, value) in interp.variables() {
                        println!("{name} = {value}");
                    }
                    continue;
                }
                _ => {}
            }
        }

        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(input);
        if buffer.trim().is_empty() {
            buffer.clear();
            continue;
        }

        if let Err(err) = interp.load(&buffer) {
            if err.incomplete {
                continue; // ask for more lines
            }
            report_error(&buffer, &err, false);
            buffer.clear();
            continue;
        }
        if let Err(err) = interp.run_to_end(&mut host) {
            report_error(&buffer, &err, false);
        }
        buffer.clear();
    }

    if let Some(path) = &history_file {
        let _ = editor.save_history(path);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => repl(),
        Some("run") => {
            let allow_shell = args.iter().any(|a| a == "--allow-shell");
            let path = args.iter().skip(1).find(|a| !a.starts_with("--"));
            match path {
                Some(path) => run_file(path, allow_shell),
                None => {
                    print_usage();
                    process::exit(2);
                }
            }
        }
        Some("help") | Some("--help") | Some("-h") => print_usage(),
        Some(other) => {
            eprintln!("Unknown command '{other}'.");
            print_usage();
            process::exit(2);
        }
    }
}
