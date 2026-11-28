//! xt-stackfile: reading and writing libreXTalk stack files.
//!
//! The engine hands a stack over as a [`PartNode`] tree of plain data, and
//! this crate turns trees into file bytes and back. Two formats share that
//! one tree (CLAUDE.md 5.17):
//!
//! - `.xtstack.html`, the text form: a valid HTML5 page that version control
//!   can diff and a browser can open. See [`html`].
//! - `.xtstack`, the binary form: planned, not built yet.
//!
//! The crate does no file access itself. A host reads or writes the bytes
//! and calls [`encode`] or [`decode`].

pub mod html;

pub use xt_core::{PartNode, XtError};

/// The stack file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.xtstack.html`: the text form.
    Html,
    /// `.xtstack`: the compact binary form (not built yet).
    Binary,
}

/// The format a file name asks for, from its extension, ignoring case.
pub fn format_for_path(path: &str) -> Option<Format> {
    let lower = path.to_lowercase();
    if lower.ends_with(".xtstack.html") {
        Some(Format::Html)
    } else if lower.ends_with(".xtstack") {
        Some(Format::Binary)
    } else {
        None
    }
}

/// Turn a stack into the bytes of a file named `path`. The extension picks
/// the format.
pub fn encode(path: &str, stack: &PartNode) -> Result<Vec<u8>, XtError> {
    match format_for_path(path) {
        Some(Format::Html) => Ok(html::write(stack).into_bytes()),
        Some(Format::Binary) => Err(binary_not_built()),
        None => Err(XtError::new(format!(
            "Stack files end in .xtstack.html (text) or .xtstack (binary), so \"{path}\" can't be saved. Try \"{path}.xtstack.html\""
        ))),
    }
}

/// Read the stack in a file's bytes. The contents decide the format, not the
/// name, so a renamed file still opens.
pub fn decode(bytes: &[u8]) -> Result<PartNode, XtError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| XtError::new("This is not a libreXTalk stack file (it is not UTF-8 text)"))?;
    let start = text.trim_start_matches('\u{feff}').trim_start();
    if start.starts_with('<') {
        html::read(text)
    } else {
        Err(XtError::new(
            "This is not a libreXTalk stack file (expected an .xtstack.html page)",
        ))
    }
}

fn binary_not_built() -> XtError {
    XtError::new("The binary .xtstack format is not built yet. Save as .xtstack.html instead")
}

#[cfg(test)]
mod tests;
