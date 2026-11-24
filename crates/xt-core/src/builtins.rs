//! Built-in functions: numbers, text, time, and the parameters of the
//! running handler (spec section 6, FUN-1 to FUN-5).
//!
//! A user-defined function with the same name takes precedence, so the VM
//! only comes here when no handler of that name exists.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::XtError;
use crate::interp::{format_number, to_number};

/// What a built-in function can see of the running script.
pub struct CallContext<'a> {
    /// The name of the handler that is running.
    pub handler_name: &'a str,
    /// The arguments that handler was called with.
    pub args: &'a [String],
    /// A random number for `random`.
    pub rand: u64,
}

const NAMES: &[&str] = &[
    "abs",
    "round",
    "trunc",
    "sqrt",
    "sin",
    "cos",
    "tan",
    "atan",
    "ln",
    "exp",
    "min",
    "max",
    "random",
    "mod",
    "div",
    "length",
    "offset",
    "toupper",
    "tolower",
    "numtochar",
    "chartonum",
    "seconds",
    "ticks",
    "milliseconds",
    "param",
    "paramcount",
    "params",
];

pub fn is_builtin(name: &str) -> bool {
    NAMES.contains(&name.to_lowercase().as_str())
}

/// Functions that can be written as `the name` with no arguments.
pub fn takes_no_args(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "seconds" | "ticks" | "milliseconds" | "paramcount" | "params"
    )
}

fn arity(name: &str, args: &[String], min: usize, max: usize) -> Result<(), XtError> {
    if args.len() >= min && args.len() <= max {
        return Ok(());
    }
    let expected = if min == max {
        format!("{min}")
    } else if max == usize::MAX {
        format!("{min} or more")
    } else {
        format!("{min} to {max}")
    };
    Err(XtError::new(format!(
        "{name} expects {expected} argument(s) but got {}",
        args.len()
    )))
}

fn one_number(name: &str, args: &[String]) -> Result<f64, XtError> {
    arity(name, args, 1, 1)?;
    to_number(&args[0])
}

fn two_numbers(name: &str, args: &[String]) -> Result<(f64, f64), XtError> {
    arity(name, args, 2, 2)?;
    Ok((to_number(&args[0])?, to_number(&args[1])?))
}

fn since_epoch() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Call a built-in function. The caller has checked `is_builtin`.
pub fn call(name: &str, args: &[String], ctx: &CallContext) -> Result<String, XtError> {
    match name.to_lowercase().as_str() {
        "abs" => Ok(format_number(one_number(name, args)?.abs())),
        "round" => Ok(format_number(one_number(name, args)?.round())),
        "trunc" => Ok(format_number(one_number(name, args)?.trunc())),
        "sqrt" => {
            let n = one_number(name, args)?;
            if n < 0.0 {
                return Err(XtError::new(
                    "Can't take the square root of a negative number",
                ));
            }
            Ok(format_number(n.sqrt()))
        }
        "sin" => Ok(format_number(one_number(name, args)?.sin())),
        "cos" => Ok(format_number(one_number(name, args)?.cos())),
        "tan" => Ok(format_number(one_number(name, args)?.tan())),
        "atan" => Ok(format_number(one_number(name, args)?.atan())),
        "exp" => Ok(format_number(one_number(name, args)?.exp())),
        "ln" => {
            let n = one_number(name, args)?;
            if n <= 0.0 {
                return Err(XtError::new(
                    "Can't take the logarithm of a number that is not positive",
                ));
            }
            Ok(format_number(n.ln()))
        }
        "min" | "max" => {
            arity(name, args, 1, usize::MAX)?;
            let mut best = to_number(&args[0])?;
            for arg in &args[1..] {
                let n = to_number(arg)?;
                if name.eq_ignore_ascii_case("min") {
                    best = best.min(n);
                } else {
                    best = best.max(n);
                }
            }
            Ok(format_number(best))
        }
        "random" => {
            let limit = one_number(name, args)?.trunc();
            if limit < 1.0 {
                return Err(XtError::new("random needs a number of 1 or more"));
            }
            let limit = limit as u64;
            Ok(((ctx.rand % limit) + 1).to_string())
        }
        "mod" | "div" => {
            let (a, b) = two_numbers(name, args)?;
            if b == 0.0 {
                return Err(XtError::new("Can't divide by zero"));
            }
            if name.eq_ignore_ascii_case("mod") {
                Ok(format_number(a % b))
            } else {
                Ok(format_number((a / b).trunc()))
            }
        }
        "length" => {
            arity(name, args, 1, 1)?;
            Ok(args[0].chars().count().to_string())
        }
        "offset" => {
            arity(name, args, 2, 2)?;
            let needle = args[0].to_lowercase();
            let haystack = args[1].to_lowercase();
            if needle.is_empty() {
                return Ok("0".to_string());
            }
            match haystack.find(&needle) {
                Some(byte_index) => Ok((haystack[..byte_index].chars().count() + 1).to_string()),
                None => Ok("0".to_string()),
            }
        }
        "toupper" => {
            arity(name, args, 1, 1)?;
            Ok(args[0].to_uppercase())
        }
        "tolower" => {
            arity(name, args, 1, 1)?;
            Ok(args[0].to_lowercase())
        }
        "numtochar" => {
            let n = one_number(name, args)?.trunc();
            match u32::try_from(n as i64).ok().and_then(char::from_u32) {
                Some(c) => Ok(c.to_string()),
                None => Err(XtError::new(format!(
                    "{} is not a character code",
                    format_number(n)
                ))),
            }
        }
        "chartonum" => {
            arity(name, args, 1, 1)?;
            match args[0].chars().next() {
                Some(c) => Ok((c as u32).to_string()),
                None => Err(XtError::new("charToNum needs a character")),
            }
        }
        "seconds" => {
            arity(name, args, 0, 0)?;
            Ok(since_epoch().as_secs().to_string())
        }
        "ticks" => {
            arity(name, args, 0, 0)?;
            Ok(((since_epoch().as_secs_f64() * 60.0) as u64).to_string())
        }
        "milliseconds" => {
            arity(name, args, 0, 0)?;
            Ok(since_epoch().as_millis().to_string())
        }
        "param" => {
            let n = one_number(name, args)?.trunc() as i64;
            if n == 0 {
                Ok(ctx.handler_name.to_string())
            } else if n >= 1 && (n as usize) <= ctx.args.len() {
                Ok(ctx.args[(n - 1) as usize].clone())
            } else {
                Ok(String::new())
            }
        }
        "paramcount" => {
            arity(name, args, 0, 0)?;
            Ok(ctx.args.len().to_string())
        }
        "params" => {
            arity(name, args, 0, 0)?;
            Ok(ctx.args.join(","))
        }
        _ => Err(XtError::new(format!("Unknown function '{name}'"))),
    }
}
