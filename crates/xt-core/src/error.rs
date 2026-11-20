use std::fmt;

/// An error found while lexing, parsing, compiling, or running a script.
#[derive(Debug, Clone, PartialEq)]
pub struct XtError {
    pub message: String,
    /// Line in the source (counting from 0), if known.
    pub line: Option<usize>,
    /// Column in that line (counting from 0), if known. Only syntax errors have one.
    pub col: Option<usize>,
    /// True when the input ended inside a block that still needs its `end` line.
    /// The prompt uses this to ask for more lines.
    pub incomplete: bool,
}

impl XtError {
    pub fn new(message: impl Into<String>) -> Self {
        XtError {
            message: message.into(),
            line: None,
            col: None,
            incomplete: false,
        }
    }

    pub fn at(line: usize, col: usize, message: impl Into<String>) -> Self {
        XtError {
            message: message.into(),
            line: Some(line),
            col: Some(col),
            incomplete: false,
        }
    }

    pub fn incomplete(line: usize, message: impl Into<String>) -> Self {
        XtError {
            message: message.into(),
            line: Some(line),
            col: None,
            incomplete: true,
        }
    }

    /// Attach a line number if the error does not have one yet.
    pub fn with_line(mut self, line: usize) -> Self {
        if self.line.is_none() {
            self.line = Some(line);
        }
        self
    }
}

impl fmt::Display for XtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for XtError {}
