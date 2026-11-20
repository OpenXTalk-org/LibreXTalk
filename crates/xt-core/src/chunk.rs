//! Chunk expressions: pick pieces out of a string, or put text into a piece.
//!
//! A chunk is a character, word, item, or line of a string (spec section 7).
//! Everything here works on byte ranges inside the string, so reading and
//! writing share one definition of where each piece starts and ends.
//!
//! Choices made here (see the spec's open questions):
//! - A character is a Unicode scalar value, not a grapheme cluster (CHK-11).
//! - Lines are separated by a line feed, items by the itemDelimiter, and
//!   words by runs of whitespace.
//! - The empty string has no pieces at all.
//! - Reading past the end gives empty text (CHK-6). Writing past the end of
//!   items or lines pads with delimiters. Writing past the end of words or
//!   characters is an error.

use crate::ast::{ChunkKind, Ordinal, Placement};
use crate::error::XtError;

/// Which piece or pieces to pick, with any index already evaluated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Selector {
    Ordinal(Ordinal),
    /// A 1-based index.
    Index(i64),
    /// A 1-based inclusive range.
    Range(i64, i64),
}

/// The byte range of each piece of `text`.
pub fn spans(text: &str, kind: ChunkKind, delim: &str) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return Vec::new();
    }
    match kind {
        ChunkKind::Char => text
            .char_indices()
            .map(|(i, c)| (i, i + c.len_utf8()))
            .collect(),
        ChunkKind::Line => split_spans(text, "\n"),
        ChunkKind::Item => split_spans(text, delim),
        ChunkKind::Word => word_spans(text),
    }
}

fn split_spans(text: &str, sep: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (idx, _) in text.match_indices(sep) {
        out.push((start, idx));
        start = idx + sep.len();
    }
    out.push((start, text.len()));
    out
}

fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

/// How many pieces of this kind `text` has.
pub fn count(text: &str, kind: ChunkKind, delim: &str) -> usize {
    spans(text, kind, delim).len()
}

/// Turn a selector into an inclusive 1-based range. `n` is how many pieces
/// there are, and `rand` supplies the choice for `any`.
fn resolve(sel: &Selector, n: usize, rand: u64) -> (i64, i64) {
    match sel {
        Selector::Index(i) => (*i, *i),
        Selector::Range(a, b) => (*a, *b),
        Selector::Ordinal(o) => {
            let n = n as i64;
            let i = match o {
                Ordinal::Nth(k) => *k as i64,
                Ordinal::Last => n,
                // The middle of n pieces is piece n/2 + 1 (spec CHK-4).
                Ordinal::Middle => {
                    if n == 0 {
                        0
                    } else {
                        n / 2 + 1
                    }
                }
                Ordinal::Any => {
                    if n == 0 {
                        0
                    } else {
                        (rand % (n as u64)) as i64 + 1
                    }
                }
            };
            (i, i)
        }
    }
}

/// Read a chunk. Anything out of range gives empty text.
pub fn read(text: &str, kind: ChunkKind, delim: &str, sel: &Selector, rand: u64) -> String {
    let sp = spans(text, kind, delim);
    let n = sp.len() as i64;
    let (a, b) = resolve(sel, sp.len(), rand);
    let a = a.max(1);
    let b = b.min(n);
    if a > b {
        return String::new();
    }
    text[sp[(a - 1) as usize].0..sp[(b - 1) as usize].1].to_string()
}

fn kind_name(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Char => "character",
        ChunkKind::Word => "word",
        ChunkKind::Item => "item",
        ChunkKind::Line => "line",
    }
}

/// Find the byte range to write to. Returns the text to edit, which may have
/// been padded, with the range inside it.
fn locate(
    base: &str,
    kind: ChunkKind,
    delim: &str,
    sel: &Selector,
    rand: u64,
) -> Result<(String, usize, usize), XtError> {
    let sp = spans(base, kind, delim);
    let n = sp.len();
    let (mut a, mut b) = resolve(sel, n, rand);
    // `first`, `last`, `middle`, and `any` of nothing mean the first piece.
    if n == 0 && matches!(sel, Selector::Ordinal(_)) {
        a = 1;
        b = 1;
    }

    if a >= 1 && b >= a && (b as usize) <= n {
        return Ok((
            base.to_string(),
            sp[(a - 1) as usize].0,
            sp[(b - 1) as usize].1,
        ));
    }

    // Beyond the end: items and lines grow by adding delimiters.
    let pad_with = match kind {
        ChunkKind::Item => Some(delim),
        ChunkKind::Line => Some("\n"),
        _ => None,
    };
    if a == b && a >= 1 && a > n as i64 {
        if let Some(pad) = pad_with {
            let missing = if n == 0 { a - 1 } else { a - n as i64 };
            let mut padded = base.to_string();
            padded.push_str(&pad.repeat(missing as usize));
            let end = padded.len();
            // The new piece is the empty one at the very end.
            return Ok((padded, end, end));
        }
    }

    let which = if a == b {
        format!("{a}")
    } else {
        format!("{a} to {b}")
    };
    Err(XtError::new(format!(
        "Can't put into {} {which}: that part does not exist",
        kind_name(kind)
    )))
}

/// Put `value` into the chunk named by `path` inside `base`. The path starts
/// with the chunk nearest the variable, so `word 2 of line 3 of x` is
/// [line 3, word 2].
pub fn put(
    base: &str,
    path: &[(ChunkKind, Selector)],
    placement: Placement,
    value: &str,
    delim: &str,
    rand: u64,
) -> Result<String, XtError> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(match placement {
            Placement::Into => value.to_string(),
            Placement::Before => format!("{value}{base}"),
            Placement::After => format!("{base}{value}"),
        });
    };
    let (kind, sel) = first;
    let (text, start, end) = locate(base, *kind, delim, sel, rand)?;
    let updated = put(&text[start..end], rest, placement, value, delim, rand)?;
    Ok(format!("{}{}{}", &text[..start], updated, &text[end..]))
}
