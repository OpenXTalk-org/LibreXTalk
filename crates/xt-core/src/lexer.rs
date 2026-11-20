//! Turns script text into tokens.
//!
//! There are no keyword tokens. Every identifier is a `Word`, because xTalk
//! has no reserved words (spec LEX-4). The parser decides what a word means.
//! Each line ends with a `Newline` token, because a newline ends a statement
//! (spec LEX-8), unless the line ends with the continuation character `¬`.

use crate::error::XtError;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// A number as written, such as `3.10`. Values are strings (spec VAL-1),
    /// so the original text is kept.
    Number(String),
    /// A string literal without its quotes.
    Str(String),
    /// An identifier in its original spelling.
    Word(String),
    /// A symbol: + - * / & && ( ) , = < > <= >= <>
    Op(String),
    Newline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// Line, counting from 0.
    pub line: usize,
    /// Column of the first character, counting from 0.
    pub col: usize,
    /// Length in characters. Used to tell `foo(1)` from `foo (1)`.
    pub len: usize,
}

pub fn lex(src: &str) -> Result<Vec<Token>, XtError> {
    let mut tokens = Vec::new();
    for (line_no, text) in src.lines().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        let continued = lex_line(&chars, line_no, &mut tokens)?;
        if !continued {
            tokens.push(Token {
                kind: TokenKind::Newline,
                line: line_no,
                col: chars.len(),
                len: 0,
            });
        }
    }
    Ok(tokens)
}

fn push(tokens: &mut Vec<Token>, kind: TokenKind, line: usize, start: usize, end: usize) {
    tokens.push(Token {
        kind,
        line,
        col: start,
        len: end - start,
    });
}

fn two_char_op(c: char, next: Option<char>) -> Option<&'static str> {
    match (c, next) {
        ('&', Some('&')) => Some("&&"),
        ('<', Some('=')) => Some("<="),
        ('>', Some('=')) => Some(">="),
        ('<', Some('>')) => Some("<>"),
        _ => None,
    }
}

/// Lex one line. Returns true if the line continues on the next one.
fn lex_line(chars: &[char], line: usize, tokens: &mut Vec<Token>) -> Result<bool, XtError> {
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        let next = chars.get(i + 1).copied();

        if c.is_whitespace() {
            i += 1;
        } else if c == '¬' {
            return Ok(true);
        } else if (c == '-' && next == Some('-')) || c == '#' {
            // A comment runs to the end of the line (spec LEX-7).
            break;
        } else if c.is_ascii_digit() || (c == '.' && next.is_some_and(|d| d.is_ascii_digit())) {
            let mut seen_dot = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !seen_dot)) {
                if chars[i] == '.' {
                    seen_dot = true;
                }
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            push(tokens, TokenKind::Number(text), line, start, i);
        } else if c == '"' {
            i += 1;
            let body_start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            if i >= chars.len() {
                return Err(XtError::at(line, start, "Missing closing quote for this string"));
            }
            let text: String = chars[body_start..i].iter().collect();
            i += 1; // skip the closing quote
            push(tokens, TokenKind::Str(text), line, start, i);
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            push(tokens, TokenKind::Word(text), line, start, i);
        } else if let Some(op) = two_char_op(c, next) {
            push(tokens, TokenKind::Op(op.to_string()), line, start, start + 2);
            i += 2;
        } else if "+-*/&(),=<>".contains(c) {
            push(tokens, TokenKind::Op(c.to_string()), line, start, start + 1);
            i += 1;
        } else if c == '≤' || c == '≥' || c == '≠' {
            let op = match c {
                '≤' => "<=",
                '≥' => ">=",
                _ => "<>",
            };
            push(tokens, TokenKind::Op(op.to_string()), line, start, start + 1);
            i += 1;
        } else {
            return Err(XtError::at(line, start, format!("Unexpected character '{c}'")));
        }
    }
    Ok(false)
}
