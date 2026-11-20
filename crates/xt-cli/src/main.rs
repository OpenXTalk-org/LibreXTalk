//! `xt`: run xTalk scripts, or type them at a prompt.
//!
//!     xt                 start the prompt
//!     xt run file.xt     run a script file

use std::io::{self, BufRead, Write};
use std::process;

use xt_core::{parse_script, Host, Interpreter, XtError};

/// The host for a terminal: the message box is standard output, and a wait
/// really sleeps (the default `sleep` in the `Host` trait).
struct StdoutHost;

impl Host for StdoutHost {
    fn message(&mut self, text: &str) {
        println!("{text}");
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  xt               start the prompt");
    println!("  xt run <file>    run a script file");
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
    println!();
    println!("A block (if, repeat, on, function) can span several lines.");
    println!("The prompt shows ... until the block is finished.");
    println!();
    println!("Prompt commands:");
    println!("  :vars        show all variables");
    println!("  :ast <line>  show how a line is parsed");
    println!("  :help        show this help");
    println!("  :quit        leave (Ctrl-D works too)");
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

fn run_file(path: &str) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read {path}: {e}");
            process::exit(1);
        }
    };
    let mut interp = Interpreter::new();
    let mut host = StdoutHost;
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

fn repl() {
    println!("xt 0.0.3. Type :help for help, :quit to leave.");

    let mut interp = Interpreter::new();
    let mut host = StdoutHost;
    let stdin = io::stdin();
    let mut line = String::new();
    // Lines of a block that is not finished yet.
    let mut buffer = String::new();

    loop {
        print!("{}", if buffer.is_empty() { "> " } else { "... " });
        io::stdout().flush().ok();

        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!();
                break; // end of input (Ctrl-D)
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Could not read input: {e}");
                break;
            }
        }
        let input = line.trim_end();

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
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => repl(),
        Some("run") => match args.get(1) {
            Some(path) => run_file(path),
            None => {
                print_usage();
                process::exit(2);
            }
        },
        Some("help") | Some("--help") | Some("-h") => print_usage(),
        Some(other) => {
            eprintln!("Unknown command '{other}'.");
            print_usage();
            process::exit(2);
        }
    }
}
