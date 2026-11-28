//! A hand-written recursive-descent parser.
//!
//! It works on the whole token stream, one statement per line. Binary
//! operators are handled by one function driven by the LEVELS table
//! (spec section 6). Words have no fixed meaning: the parser decides from
//! context (spec section 4).

use crate::ast::*;
use crate::builtins;
use crate::error::XtError;
use crate::interp::is_constant;
use crate::lexer::{lex, Token, TokenKind};
use crate::objects::kind_of_word;
use crate::shell;

/// Parse a whole script: handler definitions plus loose statements.
pub fn parse_script(src: &str) -> Result<Script, XtError> {
    let tokens = lex(src)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        loop_depth: 0,
    };
    let mut script = Script {
        handlers: Vec::new(),
        main: Vec::new(),
    };
    loop {
        p.skip_newlines();
        if p.at_end() {
            break;
        }
        if (p.is_word("on") || p.is_word("function")) && p.word_at(1).is_some() {
            script.handlers.push(p.parse_handler()?);
        } else {
            script.main.push(p.parse_statement()?);
        }
    }
    Ok(script)
}

/// Operators from loosest to tightest binding. An entry's text may be several
/// words, such as "is not in". Within a level, longer spellings come first.
const LEVELS: &[&[(&str, BinOp)]] = &[
    &[("or", BinOp::Or)],
    &[("and", BinOp::And)],
    &[
        ("is not", BinOp::Ne),
        ("is", BinOp::Eq),
        ("=", BinOp::Eq),
        ("<>", BinOp::Ne),
    ],
    &[
        ("is not in", BinOp::IsNotIn),
        ("is in", BinOp::IsIn),
        ("contains", BinOp::Contains),
        ("<=", BinOp::Le),
        (">=", BinOp::Ge),
        ("<", BinOp::Lt),
        (">", BinOp::Gt),
    ],
    &[("&&", BinOp::ConcatSpace), ("&", BinOp::Concat)],
    &[("+", BinOp::Add), ("-", BinOp::Sub)],
    &[
        ("*", BinOp::Mul),
        ("/", BinOp::Div),
        ("div", BinOp::IntDiv),
        ("mod", BinOp::Mod),
    ],
];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// How many `repeat` loops we are inside, for `exit repeat` and `next repeat`.
    loop_depth: usize,
}

impl Parser {
    // ----- token helpers -------------------------------------------------

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn is_newline(&self) -> bool {
        matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Newline))
    }

    fn is_line_end(&self) -> bool {
        self.at_end() || self.is_newline()
    }

    fn skip_newlines(&mut self) {
        while self.is_newline() {
            self.pos += 1;
        }
    }

    fn word_at(&self, offset: usize) -> Option<&str> {
        match self.tokens.get(self.pos + offset).map(|t| &t.kind) {
            Some(TokenKind::Word(w)) => Some(w.as_str()),
            _ => None,
        }
    }

    fn token_text_at(&self, offset: usize) -> Option<&str> {
        match self.tokens.get(self.pos + offset).map(|t| &t.kind) {
            Some(TokenKind::Word(w)) => Some(w.as_str()),
            Some(TokenKind::Op(o)) => Some(o.as_str()),
            _ => None,
        }
    }

    fn is_word(&self, word: &str) -> bool {
        self.word_at(0)
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(
            self.peek().map(|t| &t.kind),
            Some(TokenKind::Op(o)) if o.as_str() == op
        )
    }

    fn is_word_at(&self, offset: usize, word: &str) -> bool {
        self.word_at(offset)
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
    }

    fn chunk_kind_at(&self, offset: usize) -> Option<ChunkKind> {
        self.word_at(offset).and_then(chunk_kind_of)
    }

    fn ordinal_at(&self, offset: usize) -> Option<Ordinal> {
        self.word_at(offset).and_then(ordinal_of)
    }

    /// A kind of object at this position, and whether the word was a plural.
    fn object_kind_at(&self, offset: usize) -> Option<(ObjKind, bool)> {
        self.word_at(offset).and_then(kind_of_word)
    }

    /// `first`, `last`, `next`, and the like, as a way to pick an object.
    fn object_ordinal_at(&self, offset: usize) -> Option<ObjSel> {
        let word = self.word_at(offset)?.to_lowercase();
        match word.as_str() {
            "next" => Some(ObjSel::Next),
            "previous" | "prev" => Some(ObjSel::Prev),
            _ => ordinal_of(&word).map(ObjSel::Ordinal),
        }
    }

    /// Could the token at `offset` begin the number or name of an object?
    fn starts_selector(&self, offset: usize) -> bool {
        match self.tokens.get(self.pos + offset).map(|t| &t.kind) {
            Some(TokenKind::Str(_)) | Some(TokenKind::Number(_)) => true,
            Some(TokenKind::Op(o)) => o.as_str() == "(",
            _ => false,
        }
    }

    fn is_line_end_at(&self, offset: usize) -> bool {
        matches!(
            self.tokens.get(self.pos + offset).map(|t| &t.kind),
            None | Some(TokenKind::Newline)
        )
    }

    /// `of`, `in`, and `from` are interchangeable (spec CHK-10).
    fn is_prep(&self) -> bool {
        self.is_word("of") || self.is_word("in") || self.is_word("from")
    }

    fn take_word(&mut self) -> Result<String, XtError> {
        match self.word_at(0).map(|w| w.to_string()) {
            Some(w) => {
                self.pos += 1;
                Ok(w)
            }
            None => Err(self.err_here("Expected a name here")),
        }
    }

    /// True if `word` is immediately followed by `(` with no space, as in `foo(1)`.
    fn adjacent_paren(&self) -> bool {
        match (self.tokens.get(self.pos), self.tokens.get(self.pos + 1)) {
            (Some(word), Some(paren)) => {
                matches!(word.kind, TokenKind::Word(_))
                    && matches!(&paren.kind, TokenKind::Op(o) if o.as_str() == "(")
                    && paren.line == word.line
                    && paren.col == word.col + word.len
            }
            _ => false,
        }
    }

    /// Could the token at `offset` begin an argument written in the space style?
    fn starts_plain_operand(&self, offset: usize) -> bool {
        match self.tokens.get(self.pos + offset).map(|t| &t.kind) {
            Some(TokenKind::Word(w)) => !w.eq_ignore_ascii_case("else"),
            Some(TokenKind::Str(_)) | Some(TokenKind::Number(_)) => true,
            Some(TokenKind::Op(o)) => o.as_str() == "(",
            _ => false,
        }
    }

    fn starts_operand(&self, offset: usize) -> bool {
        self.starts_plain_operand(offset)
            || matches!(
                self.tokens.get(self.pos + offset).map(|t| &t.kind),
                Some(TokenKind::Op(o)) if o.as_str() == "-"
            )
    }

    // ----- errors --------------------------------------------------------

    fn current_line(&self) -> usize {
        match self.tokens.get(self.pos).or_else(|| self.tokens.last()) {
            Some(t) => t.line,
            None => 0,
        }
    }

    fn err_here(&self, message: impl Into<String>) -> XtError {
        match self.tokens.get(self.pos) {
            Some(t) => XtError::at(t.line, t.col, message),
            None => {
                let (line, col) = self
                    .tokens
                    .last()
                    .map_or((0, 0), |t| (t.line, t.col + t.len));
                XtError::at(line, col, message)
            }
        }
    }

    fn unexpected_here(&self) -> XtError {
        match self.tokens.get(self.pos) {
            Some(t) => XtError::at(
                t.line,
                t.col,
                format!("Unexpected {} here", describe(&t.kind)),
            ),
            None => self.err_here("Unexpected end of script"),
        }
    }

    fn incomplete(&self, message: impl Into<String>) -> XtError {
        XtError::incomplete(self.tokens.last().map_or(0, |t| t.line), message)
    }

    fn expect_line_end(&mut self) -> Result<(), XtError> {
        if self.at_end() {
            return Ok(());
        }
        if self.is_newline() {
            self.pos += 1;
            return Ok(());
        }
        Err(self.unexpected_here())
    }

    /// Consume `end <what>`.
    fn expect_end_word(&mut self, what: &str) -> Result<(), XtError> {
        if self.at_end() {
            return Err(self.incomplete(format!("Missing 'end {what}'")));
        }
        let found = self.is_word("end")
            && self
                .word_at(1)
                .is_some_and(|w| w.eq_ignore_ascii_case(what));
        if found {
            self.pos += 2;
            Ok(())
        } else {
            Err(self.err_here(format!("Expected 'end {what}' here")))
        }
    }

    // ----- handlers and blocks -------------------------------------------

    fn parse_handler(&mut self) -> Result<HandlerDef, XtError> {
        let line = self.current_line();
        let is_function = self.is_word("function");
        self.pos += 1; // `on` or `function`
        let name = self.take_word()?;

        // Parameter names may be separated by commas, spaces, or both (spec HND-11).
        let mut params = Vec::new();
        loop {
            if self.is_op(",") {
                self.pos += 1;
                continue;
            }
            match self.word_at(0).map(|w| w.to_string()) {
                Some(w) => {
                    self.pos += 1;
                    params.push(w);
                }
                None => break,
            }
        }
        self.expect_line_end()?;

        let saved_depth = self.loop_depth;
        self.loop_depth = 0;
        let body = self.parse_block()?;
        self.loop_depth = saved_depth;

        self.expect_end_word(&name)?;
        self.expect_line_end()?;
        Ok(HandlerDef {
            name,
            is_function,
            params,
            body,
            line,
        })
    }

    /// Parse statements until a line starts with `end` or `else`.
    fn parse_block(&mut self) -> Result<Vec<Stmt>, XtError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            if self.at_end() {
                return Err(self.incomplete("This block is missing its 'end' line"));
            }
            if self.is_word("end") || self.is_word("else") || self.is_word("catch") {
                break;
            }
            stmts.push(self.parse_statement()?);
        }
        Ok(stmts)
    }

    // ----- statements ----------------------------------------------------

    /// Parse one statement, including the end of its line.
    fn parse_statement(&mut self) -> Result<Stmt, XtError> {
        let line = self.current_line();
        if self.is_word("end") || self.is_word("else") || self.is_word("catch") {
            let w = self.word_at(0).unwrap_or("").to_string();
            return Err(self.err_here(format!("Unexpected '{w}' here")));
        }
        if self.is_word("try") {
            self.pos += 1;
            self.expect_line_end()?;
            let body = self.parse_block()?;
            if !self.is_word("catch") {
                return Err(self.err_here("Expected 'catch' here"));
            }
            self.pos += 1;
            let var = if self.is_line_end() {
                None
            } else {
                Some(self.take_word()?)
            };
            self.expect_line_end()?;
            let catch_body = self.parse_block()?;
            self.expect_end_word("try")?;
            self.expect_line_end()?;
            return Ok(Stmt {
                line,
                kind: StmtKind::Try {
                    body,
                    var,
                    catch_body,
                },
            });
        }
        if self.is_word("if") {
            self.pos += 1;
            let (kind, block) = self.parse_if_tail()?;
            if block {
                self.expect_end_word("if")?;
            }
            self.expect_line_end()?;
            return Ok(Stmt { line, kind });
        }
        if self.is_word("repeat") {
            self.pos += 1;
            let kind = self.parse_repeat()?;
            return Ok(Stmt { line, kind });
        }
        let kind = self.parse_simple()?;
        self.expect_line_end()?;
        Ok(Stmt { line, kind })
    }

    /// After `if`: the condition and either the one-line or the block form.
    /// Returns true as the second value for the block form, whose `end if`
    /// the caller must consume.
    fn parse_if_tail(&mut self) -> Result<(StmtKind, bool), XtError> {
        let cond = self.parse_expr()?;
        if !self.is_word("then") {
            return Err(self.err_here("Expected 'then' here"));
        }
        self.pos += 1;

        if self.is_line_end() {
            // Block form.
            self.expect_line_end()?;
            let then_body = self.parse_block()?;
            let mut else_body = Vec::new();
            if self.is_word("else") {
                let else_line = self.current_line();
                self.pos += 1;
                if self.is_word("if") {
                    // `else if ...` continues the chain and shares the final `end if`.
                    self.pos += 1;
                    let (kind, _) = self.parse_if_tail()?;
                    else_body.push(Stmt {
                        line: else_line,
                        kind,
                    });
                } else {
                    self.expect_line_end()?;
                    else_body = self.parse_block()?;
                }
            }
            return Ok((
                StmtKind::If {
                    cond,
                    then_body,
                    else_body,
                },
                true,
            ));
        }

        // One-line form: if cond then statement [else statement]
        let line = self.current_line();
        let then_stmt = Stmt {
            line,
            kind: self.parse_inline_statement()?,
        };
        let mut else_body = Vec::new();
        if self.is_word("else") {
            self.pos += 1;
            let line = self.current_line();
            else_body.push(Stmt {
                line,
                kind: self.parse_inline_statement()?,
            });
        }
        Ok((
            StmtKind::If {
                cond,
                then_body: vec![then_stmt],
                else_body,
            },
            false,
        ))
    }

    /// The statement after `then` or `else` on the same line.
    fn parse_inline_statement(&mut self) -> Result<StmtKind, XtError> {
        if self.is_word("if") {
            self.pos += 1;
            let (kind, block) = self.parse_if_tail()?;
            if block {
                return Err(
                    self.err_here("Use the one-line form here, or put this 'if' on its own lines")
                );
            }
            return Ok(kind);
        }
        if self.is_word("repeat") || self.is_word("try") {
            return Err(self.err_here("A 'repeat' or 'try' block needs its own lines"));
        }
        self.parse_simple()
    }

    /// After `repeat`: the header, the body, and `end repeat`.
    fn parse_repeat(&mut self) -> Result<StmtKind, XtError> {
        let kind = if self.is_word("forever") {
            self.pos += 1;
            RepeatKind::Forever
        } else if self.is_word("while") {
            self.pos += 1;
            RepeatKind::While(self.parse_expr()?)
        } else if self.is_word("until") {
            self.pos += 1;
            RepeatKind::Until(self.parse_expr()?)
        } else if self.is_word("with") {
            self.pos += 1;
            if self.word_at(0).is_some_and(is_constant) {
                return Err(self.err_here("That name is a constant and can't be a loop variable"));
            }
            let var = self.take_word()?;
            if !self.is_op("=") {
                return Err(self.err_here("Expected '=' here"));
            }
            self.pos += 1;
            let from = self.parse_expr()?;
            let down = if self.is_word("down") {
                self.pos += 1;
                true
            } else {
                false
            };
            if !self.is_word("to") {
                return Err(self.err_here("Expected 'to' here"));
            }
            self.pos += 1;
            let to = self.parse_expr()?;
            RepeatKind::With {
                var,
                from,
                to,
                down,
            }
        } else {
            if self.is_word("for") {
                self.pos += 1;
            }
            let count = self.parse_expr()?;
            if self.is_word("times") {
                self.pos += 1;
            }
            RepeatKind::Times(count)
        };
        self.expect_line_end()?;

        self.loop_depth += 1;
        let body = self.parse_block()?;
        self.loop_depth -= 1;

        self.expect_end_word("repeat")?;
        self.expect_line_end()?;
        Ok(StmtKind::Repeat { kind, body })
    }

    /// A statement that fits on one line, without consuming the end of the line.
    fn parse_simple(&mut self) -> Result<StmtKind, XtError> {
        let word = self.word_at(0).unwrap_or("").to_lowercase();
        match word.as_str() {
            "put" => {
                self.pos += 1;
                let value = self.parse_expr()?;
                let target = self.parse_put_target()?;
                Ok(StmtKind::Put { value, target })
            }
            "get" => {
                self.pos += 1;
                Ok(StmtKind::Get(self.parse_expr()?))
            }
            "return" => {
                self.pos += 1;
                if self.args_done(None) {
                    Ok(StmtKind::Return(None))
                } else {
                    Ok(StmtKind::Return(Some(self.parse_expr()?)))
                }
            }
            "exit" => {
                self.pos += 1;
                if self.is_word("repeat") {
                    if self.loop_depth == 0 {
                        return Err(
                            self.err_here("'exit repeat' can only be used inside a repeat loop")
                        );
                    }
                    self.pos += 1;
                    Ok(StmtKind::Exit(ExitKind::Repeat))
                } else if self.is_word("to") {
                    self.pos += 1;
                    if self.is_word("top") || self.is_word("hypercard") {
                        self.pos += 1;
                        Ok(StmtKind::Exit(ExitKind::ToTop))
                    } else {
                        Err(self.err_here("Expected 'top' here"))
                    }
                } else {
                    self.take_word()?; // `exit handlerName`
                    Ok(StmtKind::Exit(ExitKind::Handler))
                }
            }
            "next" => {
                self.pos += 1;
                if !self.is_word("repeat") {
                    return Err(self.err_here("Expected 'repeat' after 'next'"));
                }
                if self.loop_depth == 0 {
                    return Err(
                        self.err_here("'next repeat' can only be used inside a repeat loop")
                    );
                }
                self.pos += 1;
                Ok(StmtKind::NextRepeat)
            }
            "global" => {
                self.pos += 1;
                let mut names = Vec::new();
                loop {
                    if self.is_op(",") {
                        self.pos += 1;
                        continue;
                    }
                    match self.word_at(0).map(|w| w.to_string()) {
                        Some(w) => {
                            self.pos += 1;
                            names.push(w);
                        }
                        None => break,
                    }
                }
                if names.is_empty() {
                    return Err(self.err_here("Expected a variable name here"));
                }
                Ok(StmtKind::Global(names))
            }
            "wait" => {
                self.pos += 1;
                if self.is_word("until") {
                    self.pos += 1;
                    Ok(StmtKind::Wait(WaitKind::Until(self.parse_expr()?)))
                } else if self.is_word("while") {
                    self.pos += 1;
                    Ok(StmtKind::Wait(WaitKind::While(self.parse_expr()?)))
                } else {
                    if self.is_word("for") {
                        self.pos += 1;
                    }
                    let amount = self.parse_expr()?;
                    let unit = if self.is_word("seconds")
                        || self.is_word("second")
                        || self.is_word("secs")
                        || self.is_word("sec")
                    {
                        self.pos += 1;
                        WaitUnit::Seconds
                    } else if self.is_word("ticks") || self.is_word("tick") {
                        self.pos += 1;
                        WaitUnit::Ticks
                    } else {
                        WaitUnit::Ticks
                    };
                    Ok(StmtKind::Wait(WaitKind::For(amount, unit)))
                }
            }
            "add" => {
                self.pos += 1;
                let operand = self.parse_expr()?;
                if !self.is_word("to") {
                    return Err(self.err_here("Expected 'to' here"));
                }
                self.pos += 1;
                let (target, _, _) = self.parse_container()?;
                Ok(arithmetic_put(target, BinOp::Add, operand))
            }
            "subtract" => {
                self.pos += 1;
                let operand = self.parse_expr()?;
                if !self.is_word("from") {
                    return Err(self.err_here("Expected 'from' here"));
                }
                self.pos += 1;
                let (target, _, _) = self.parse_container()?;
                Ok(arithmetic_put(target, BinOp::Sub, operand))
            }
            "multiply" | "divide" => {
                self.pos += 1;
                let (target, _, _) = self.parse_container()?;
                if !self.is_word("by") {
                    return Err(self.err_here("Expected 'by' here"));
                }
                self.pos += 1;
                let operand = self.parse_expr()?;
                let op = if word == "multiply" {
                    BinOp::Mul
                } else {
                    BinOp::Div
                };
                Ok(arithmetic_put(target, op, operand))
            }
            "delete" => {
                self.pos += 1;
                if let Some(spec) = self.try_parse_object()? {
                    return Ok(StmtKind::DeleteObj(spec));
                }
                let (target, line, col) = self.parse_container()?;
                if matches!(target, Target::Chunk { .. }) {
                    Ok(StmtKind::Delete(target))
                } else {
                    Err(XtError::at(
                        line,
                        col,
                        "delete needs a chunk, such as: delete word 2 of x",
                    ))
                }
            }
            "do" => {
                self.pos += 1;
                let script = self.parse_expr()?;
                if !self.is_word("as") {
                    return Err(self.err_here(
                        "Only 'do ... as <shell>' is supported so far, as in: do \"uname -m\" as shell",
                    ));
                }
                self.pos += 1;
                if !self.word_at(0).is_some_and(shell::is_shell_name) {
                    return Err(self.err_here(
                        "Unknown language. Use shell, sh, bash, zsh, dash, ksh, fish, csh, tcsh, powershell, pwsh, or cmd",
                    ));
                }
                let language = self.take_word()?.to_lowercase();
                Ok(StmtKind::Do { script, language })
            }
            "create" => {
                self.pos += 1;
                let kind = match self.object_kind_at(0) {
                    Some((kind, false)) => kind,
                    _ => {
                        return Err(self.err_here(
                            "Expected a kind of object here: stack, background, card, group, button, field, scrollbar, image, or graphic",
                        ))
                    }
                };
                self.pos += 1;
                let name = if self.is_line_end() || self.is_prep() {
                    None
                } else {
                    Some(self.parse_primary()?)
                };
                let parent = if self.is_prep() {
                    self.pos += 1;
                    Some(self.parse_object_operand()?)
                } else {
                    None
                };
                Ok(StmtKind::Create { kind, name, parent })
            }
            "go" => {
                self.pos += 1;
                if self.is_word("to") {
                    self.pos += 1;
                }
                // `go next`, `go first`: a card, picked by position
                let bare = if self.object_kind_at(1).is_none() && self.is_line_end_at(1) {
                    self.object_ordinal_at(0)
                } else {
                    None
                };
                let spec = match bare {
                    Some(sel) => {
                        self.pos += 1;
                        ObjSpec::Find {
                            kind: ObjKind::Card,
                            sel,
                            of: None,
                        }
                    }
                    None => self.parse_object_operand()?,
                };
                Ok(StmtKind::Go(spec))
            }
            "send" => {
                self.pos += 1;
                let message = self.parse_expr()?;
                if !self.is_word("to") {
                    return Err(
                        self.err_here("Expected 'to' here, as in: send mouseUp to button 1")
                    );
                }
                self.pos += 1;
                let target = self.parse_object_operand()?;
                Ok(StmtKind::Send { message, target })
            }
            "pass" => {
                self.pos += 1;
                let name = self.take_word()?;
                Ok(StmtKind::Pass(name))
            }
            "show" | "hide" => {
                self.pos += 1;
                let target = self.parse_object_operand()?;
                let shown = if word == "show" { "true" } else { "false" };
                Ok(StmtKind::SetObjProp {
                    prop: "visible".to_string(),
                    target,
                    value: Expr::Literal(shown.to_string()),
                })
            }
            "throw" => {
                self.pos += 1;
                Ok(StmtKind::Throw(self.parse_expr()?))
            }
            "answer" => {
                self.pos += 1;
                if let Some(form) = self.dialog_form_at(false) {
                    return self.parse_dialog(form);
                }
                let prompt = self.parse_expr()?;
                let mut buttons = Vec::new();
                if self.is_word("with") {
                    self.pos += 1;
                    loop {
                        // Level 2 skips `or`, which separates the buttons here.
                        buttons.push(self.parse_binary(2)?);
                        if self.is_word("or") {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                }
                Ok(StmtKind::Answer { prompt, buttons })
            }
            "ask" => {
                self.pos += 1;
                if let Some(form) = self.dialog_form_at(true) {
                    return self.parse_dialog(form);
                }
                let prompt = self.parse_expr()?;
                let default = if self.is_word("with") {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                Ok(StmtKind::Ask { prompt, default })
            }
            "set" => {
                self.pos += 1;
                if !self.is_word("the") {
                    return Err(
                        self.err_here("Expected 'the' here, as in: set the itemDelimiter to \";\"")
                    );
                }
                self.pos += 1;
                let prop = self.take_word()?.to_lowercase();
                if self.is_word("of") {
                    self.pos += 1;
                    let target = self.parse_object_operand()?;
                    if !self.is_word("to") {
                        return Err(self.err_here("Expected 'to' here"));
                    }
                    self.pos += 1;
                    let value = self.parse_expr()?;
                    return Ok(StmtKind::SetObjProp {
                        prop,
                        target,
                        value,
                    });
                }
                if !self.is_word("to") {
                    return Err(self.err_here("Expected 'to' here"));
                }
                self.pos += 1;
                let value = self.parse_expr()?;
                Ok(StmtKind::Set { prop, value })
            }
            // `save` and `open` are statements only when `stack` follows,
            // so a handler named `save` or `open` can still be called.
            "save"
                if self.is_word_at(1, "stack")
                    || (self.is_word_at(1, "this") && self.is_word_at(2, "stack")) =>
            {
                self.pos += 1;
                let target = self.parse_object_operand()?;
                let path = if self.is_word("as") {
                    self.pos += 1;
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                Ok(StmtKind::SaveStack { target, path })
            }
            "open" if self.is_word_at(1, "stack") => {
                self.pos += 2;
                Ok(StmtKind::OpenStack(self.parse_expr()?))
            }
            _ => self.parse_call_or_eval(),
        }
    }

    /// A variable or a chunk of one, as used by `add`, `multiply`, and `delete`.
    /// Also returns the position, for error messages.
    fn parse_container(&mut self) -> Result<(Target, usize, usize), XtError> {
        if self.is_line_end() {
            return Err(self.err_here("Expected a variable or a chunk of one here"));
        }
        let (line, col) = {
            let t = &self.tokens[self.pos];
            (t.line, t.col)
        };
        let expr = self.parse_primary()?;
        match expr_to_target(expr) {
            Some(target) => {
                check_not_constant(&target, line, col)?;
                Ok((target, line, col))
            }
            None => Err(XtError::at(
                line,
                col,
                "Expected a variable or a chunk of one here",
            )),
        }
    }

    fn parse_put_target(&mut self) -> Result<Option<(Placement, Target)>, XtError> {
        let placement = if self.is_word("into") {
            Placement::Into
        } else if self.is_word("before") {
            Placement::Before
        } else if self.is_word("after") {
            Placement::After
        } else {
            return Ok(None);
        };
        self.pos += 1;
        if self.is_line_end() {
            return Err(self.err_here("Expected a variable name here"));
        }
        let (line, col) = {
            let t = &self.tokens[self.pos];
            (t.line, t.col)
        };
        let expr = self.parse_primary()?;
        match expr_to_target(expr) {
            Some(target) => {
                check_not_constant(&target, line, col)?;
                Ok(Some((placement, target)))
            }
            None => Err(XtError::at(
                line,
                col,
                "You can only put into a variable, or a chunk of one",
            )),
        }
    }

    /// A line that starts with a word that is not a built-in command is a
    /// message to a user handler (spec CTX-6), unless an operator follows,
    /// in which case it is an expression.
    fn parse_call_or_eval(&mut self) -> Result<StmtKind, XtError> {
        // A line that starts with a chunk expression is an expression.
        let save = self.pos;
        let starts_chunk = matches!(self.try_parse_chunk(), Ok(Some(_)));
        self.pos = save;
        let starts_object = matches!(self.try_parse_object(), Ok(Some(_)));
        self.pos = save;
        if starts_chunk || starts_object {
            return Ok(StmtKind::Eval(self.parse_expr()?));
        }
        if let Some(name) = self.word_at(0).map(|w| w.to_string()) {
            let is_the_form = name.eq_ignore_ascii_case("the") && self.word_at(1).is_some();
            if !is_the_form && !self.adjacent_paren() && !self.operator_at(1) {
                self.pos += 1;
                let args = self.parse_args(None)?;
                return Ok(StmtKind::Call { name, args });
            }
        }
        Ok(StmtKind::Eval(self.parse_expr()?))
    }

    // ----- arguments -----------------------------------------------------

    /// Is the argument list over? It ends at the end of the line, at `else`,
    /// or at the closing parenthesis of a function call.
    fn args_done(&self, closer: Option<&str>) -> bool {
        if self.is_line_end() || self.is_word("else") {
            return true;
        }
        match closer {
            Some(c) => self.is_op(c),
            None => false,
        }
    }

    /// Arguments in either style (spec HND-12). In the space style each
    /// argument is a single token. In the comma style each is a full expression.
    fn parse_args(&mut self, closer: Option<&str>) -> Result<Vec<Expr>, XtError> {
        let mut args = Vec::new();
        if self.args_done(closer) {
            return Ok(args);
        }

        // Try the space style first.
        let start = self.pos;
        if let Ok(first) = self.parse_primary() {
            if !self.args_done(closer) && self.starts_plain_operand(0) && !self.operator_at(0) {
                args.push(first);
                while !self.args_done(closer) {
                    if !self.starts_plain_operand(0) || self.operator_at(0) {
                        return Err(self
                            .err_here("Separate arguments with spaces or with commas, not both"));
                    }
                    args.push(self.parse_primary()?);
                }
                return Ok(args);
            }
        }
        self.pos = start;

        // Comma style.
        loop {
            args.push(self.parse_expr()?);
            if self.is_op(",") {
                self.pos += 1;
            } else {
                break;
            }
        }
        if !self.args_done(closer) {
            if self.starts_plain_operand(0) {
                return Err(self.err_here(
                    "Wrap this argument in parentheses, or separate the arguments with commas",
                ));
            }
            return Err(self.unexpected_here());
        }
        Ok(args)
    }

    // ----- expressions ---------------------------------------------------

    fn parse_expr(&mut self) -> Result<Expr, XtError> {
        self.parse_binary(0)
    }

    /// If an operator at this level starts at `offset`, return it and how many tokens it spans.
    fn match_op_at(&self, level: usize, offset: usize) -> Option<(BinOp, usize)> {
        for (pattern, op) in LEVELS[level] {
            let parts: Vec<&str> = pattern.split(' ').collect();
            let all_match = parts.iter().enumerate().all(|(i, part)| {
                self.token_text_at(offset + i)
                    .is_some_and(|t| t.eq_ignore_ascii_case(part))
            });
            if all_match {
                return Some((*op, parts.len()));
            }
        }
        None
    }

    /// Does any binary operator start at `offset`?
    fn operator_at(&self, offset: usize) -> bool {
        (0..LEVELS.len()).any(|level| self.match_op_at(level, offset).is_some())
    }

    fn parse_binary(&mut self, level: usize) -> Result<Expr, XtError> {
        if level == LEVELS.len() {
            return self.parse_unary();
        }
        let mut left = self.parse_binary(level + 1)?;
        while let Some((op, width)) = self.match_op_at(level, 0) {
            self.pos += width;
            let right = self.parse_binary(level + 1)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, XtError> {
        if self.is_op("-") {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.parse_unary()?)));
        }
        // `not` is an operator only where an operand follows (spec CTX-5).
        if self.is_word("not") && self.starts_operand(1) {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, XtError> {
        let tok = match self.peek() {
            Some(t) => t.clone(),
            None => return Err(self.err_here("Expected a value here")),
        };
        match tok.kind {
            TokenKind::Str(s) => {
                self.pos += 1;
                Ok(Expr::Literal(s))
            }
            TokenKind::Number(n) => {
                self.pos += 1;
                Ok(Expr::Literal(n))
            }
            TokenKind::Word(w) => {
                if let Some(spec) = self.try_parse_object()? {
                    return Ok(Expr::Object(spec));
                }
                if w.eq_ignore_ascii_case("the") && self.word_at(1).is_some() {
                    return self.parse_the();
                }
                if let Some(chunk) = self.try_parse_chunk()? {
                    return Ok(chunk);
                }
                if self.adjacent_paren() {
                    self.pos += 2; // the name and the opening parenthesis
                    let args = self.parse_args(Some(")"))?;
                    if !self.is_op(")") {
                        return Err(self.err_here("Expected a closing parenthesis here"));
                    }
                    self.pos += 1;
                    return Ok(Expr::Call { name: w, args });
                }
                self.pos += 1;
                Ok(Expr::Var(w))
            }
            TokenKind::Op(o) if o == "(" => {
                self.pos += 1;
                let inner = self.parse_expr()?;
                if !self.is_op(")") {
                    return Err(self.err_here("Expected a closing parenthesis here"));
                }
                self.pos += 1;
                Ok(inner)
            }
            _ => Err(XtError::at(tok.line, tok.col, "Expected a value here")),
        }
    }

    /// After `answer` or `ask`: is this one of the dialog forms (DLG-1)?
    /// The words are not reserved, so `answer file` is the dialog only when
    /// what follows could not continue an expression that uses a variable
    /// named `file`: the end of the line, `with`, `of`, or the start of a
    /// prompt. `answer file & "!"` still shows the variable (DLG-3).
    fn dialog_form_at(&self, after_ask: bool) -> Option<DialogForm> {
        let word = self.word_at(0)?.to_lowercase();
        let form = match (after_ask, word.as_str()) {
            (false, "file") => DialogForm::AnswerFile,
            (true, "file") => DialogForm::AskFile,
            (false, "folder") => DialogForm::AnswerFolder,
            (false, "color" | "colour") => DialogForm::AnswerColor,
            (false, "effect") => DialogForm::AnswerEffect,
            (false, "record") => DialogForm::AnswerRecord,
            _ => return None,
        };
        let takes_prompt = matches!(
            form,
            DialogForm::AnswerFile | DialogForm::AskFile | DialogForm::AnswerFolder
        );
        let next_ok = self.is_line_end_at(1)
            || self.is_word_at(1, "with")
            || (takes_prompt && (self.is_word_at(1, "of") || self.starts_prompt_at(1)));
        next_ok.then_some(form)
    }

    /// Could the token at `offset` begin a prompt: a string, a number, a
    /// parenthesis, or a word that is not an operator?
    fn starts_prompt_at(&self, offset: usize) -> bool {
        if self.starts_selector(offset) {
            return true;
        }
        match self.word_at(offset) {
            Some(w) => !matches!(
                w.to_lowercase().as_str(),
                "and" | "or" | "is" | "contains" | "div" | "mod" | "not" | "to" | "into"
            ),
            None => false,
        }
    }

    /// The rest of a dialog form, after the word that names it:
    /// `answer file [prompt] [with default] [of type t {or t}]`,
    /// `ask file [prompt] [with default]`, `answer folder [prompt] [with default]`,
    /// `answer color [with default]`, `answer effect`, `answer record`.
    fn parse_dialog(&mut self, form: DialogForm) -> Result<StmtKind, XtError> {
        self.pos += 1; // the form's word
        let takes_prompt = matches!(
            form,
            DialogForm::AnswerFile | DialogForm::AskFile | DialogForm::AnswerFolder
        );
        let prompt = if takes_prompt
            && !self.is_line_end_at(0)
            && !self.is_word("with")
            && !self.is_word("of")
        {
            Some(self.parse_expr()?)
        } else {
            None
        };
        let default = if self.is_word("with")
            && !matches!(form, DialogForm::AnswerEffect | DialogForm::AnswerRecord)
        {
            self.pos += 1;
            Some(self.parse_expr()?)
        } else {
            None
        };
        let mut types = Vec::new();
        if form == DialogForm::AnswerFile && self.is_word("of") && self.is_word_at(1, "type") {
            self.pos += 2;
            loop {
                // Level 2 skips `or`, which separates the types here.
                types.push(self.parse_binary(2)?);
                if self.is_word("or") {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        Ok(StmtKind::Dialog {
            form,
            prompt,
            default,
            types,
        })
    }

    /// After `of` or `to`: an object, or any expression that holds a reference.
    fn parse_object_operand(&mut self) -> Result<ObjSpec, XtError> {
        if let Some(spec) = self.try_parse_object()? {
            return Ok(spec);
        }
        Ok(ObjSpec::Value(Box::new(self.parse_primary()?)))
    }

    /// The container in `button 1 of card 2`, if there is one.
    fn parse_object_of(&mut self) -> Result<Option<Box<ObjSpec>>, XtError> {
        if self.is_prep() {
            self.pos += 1;
            return Ok(Some(Box::new(self.parse_object_operand()?)));
        }
        Ok(None)
    }

    /// At a word, try to read an object: `me`, `the target`, `this card`,
    /// `first card`, `button "OK"`, `card id 3`. Returns `Ok(None)` with the
    /// position unchanged if the words do not form one, because `card` and
    /// `field` are ordinary variable names too.
    fn try_parse_object(&mut self) -> Result<Option<ObjSpec>, XtError> {
        if self.is_word("me") {
            self.pos += 1;
            return Ok(Some(ObjSpec::Me));
        }
        if self.is_word("the") && self.is_word_at(1, "target") {
            self.pos += 2;
            return Ok(Some(ObjSpec::Target));
        }
        // `the first card` is `first card`.
        if self.is_word("the")
            && self.object_ordinal_at(1).is_some()
            && matches!(self.object_kind_at(2), Some((_, false)))
        {
            self.pos += 1;
        }
        // `this card`, `this stack`, `this background`
        if self.is_word("this") {
            if let Some((kind, false)) = self.object_kind_at(1) {
                if matches!(kind, ObjKind::Card | ObjKind::Stack | ObjKind::Background) {
                    self.pos += 2;
                    return Ok(Some(ObjSpec::This(kind)));
                }
            }
            return Ok(None);
        }
        // `first card`, `next card`, `last button`
        if let Some(sel) = self.object_ordinal_at(0) {
            if let Some((kind, false)) = self.object_kind_at(1) {
                self.pos += 2;
                let of = self.parse_object_of()?;
                return Ok(Some(ObjSpec::Find { kind, sel, of }));
            }
            return Ok(None);
        }
        // `button "OK"`, `card 3`, `card id 12`
        if let Some((kind, false)) = self.object_kind_at(0) {
            let sel = if self.is_word_at(1, "id") && self.starts_selector(2) {
                self.pos += 2;
                Some(ObjSel::Id(Box::new(self.parse_primary()?)))
            } else if self.starts_selector(1) {
                self.pos += 1;
                Some(ObjSel::At(Box::new(self.parse_primary()?)))
            } else {
                None
            };
            if let Some(sel) = sel {
                let of = self.parse_object_of()?;
                return Ok(Some(ObjSpec::Find { kind, sel, of }));
            }
        }
        Ok(None)
    }

    /// At a word, try to read a chunk expression or a count. Returns
    /// `Ok(None)` with the position unchanged if the words do not form one,
    /// because `word` and `line` are ordinary variable names too (spec CTX-4).
    fn try_parse_chunk(&mut self) -> Result<Option<Expr>, XtError> {
        // `first word of x`, `last line of x`
        if self.ordinal_at(0).is_some() && self.chunk_kind_at(1).is_some() {
            return self.parse_ordinal_chunk().map(Some);
        }
        // `number of words in x`
        if self.is_word("number") && self.is_word_at(1, "of") && self.chunk_kind_at(2).is_some() {
            self.pos += 2;
            return self.parse_count().map(Some);
        }
        // `word 3 of x`, `item 2 to 4 of x`. If no `of` follows the index,
        // the word was just a variable, so back up.
        if let Some(kind) = self.chunk_kind_at(0) {
            let start = self.pos;
            self.pos += 1;
            let sel = match self.parse_expr() {
                Ok(first) => {
                    if self.is_word("to") {
                        self.pos += 1;
                        match self.parse_expr() {
                            Ok(last) => Some(ChunkSel::Range(Box::new(first), Box::new(last))),
                            Err(_) => None,
                        }
                    } else {
                        Some(ChunkSel::At(Box::new(first)))
                    }
                }
                Err(_) => None,
            };
            if let Some(sel) = sel {
                if self.is_prep() {
                    self.pos += 1;
                    let of = self.parse_primary()?;
                    return Ok(Some(Expr::Chunk {
                        kind,
                        sel,
                        of: Box::new(of),
                    }));
                }
            }
            self.pos = start;
        }
        Ok(None)
    }

    /// Positioned on an ordinal that is followed by a chunk type.
    fn parse_ordinal_chunk(&mut self) -> Result<Expr, XtError> {
        let ordinal = self.ordinal_at(0).unwrap_or(Ordinal::Last);
        let kind = self.chunk_kind_at(1).unwrap_or(ChunkKind::Word);
        self.pos += 2;
        if !self.is_prep() {
            return Err(self.err_here("Expected 'of' here"));
        }
        self.pos += 1;
        let of = self.parse_primary()?;
        Ok(Expr::Chunk {
            kind,
            sel: ChunkSel::Ordinal(ordinal),
            of: Box::new(of),
        })
    }

    /// Positioned on the chunk type after `number of`.
    fn parse_count(&mut self) -> Result<Expr, XtError> {
        let kind = self.chunk_kind_at(0).unwrap_or(ChunkKind::Word);
        self.pos += 1;
        if !self.is_prep() {
            return Err(self.err_here("Expected 'in' here"));
        }
        self.pos += 1;
        let of = self.parse_primary()?;
        Ok(Expr::Count {
            kind,
            of: Box::new(of),
        })
    }

    /// Positioned on `the`, which is followed by a word.
    fn parse_the(&mut self) -> Result<Expr, XtError> {
        self.pos += 1;
        if self.ordinal_at(0).is_some() && self.chunk_kind_at(1).is_some() {
            return self.parse_ordinal_chunk();
        }
        if self.is_word("number") && self.is_word_at(1, "of") && self.chunk_kind_at(2).is_some() {
            self.pos += 2;
            return self.parse_count();
        }
        // `the number of cards`, `the number of buttons of card 2`
        if self.is_word("number") && self.is_word_at(1, "of") {
            if let Some((kind, true)) = self.object_kind_at(2) {
                self.pos += 3;
                let of = if self.is_prep() {
                    self.pos += 1;
                    Some(self.parse_object_operand()?)
                } else {
                    None
                };
                return Ok(Expr::ObjCount { kind, of });
            }
        }
        let name = self.word_at(0).unwrap_or("").to_lowercase();
        if name == "result" || name == "itemdelimiter" {
            self.pos += 1;
            return Ok(Expr::The(name));
        }
        if builtins::is_builtin(&name) {
            // `the length of x`
            if self.is_word_at(1, "of") {
                self.pos += 2;
                let arg = self.parse_primary()?;
                return Ok(Expr::Call {
                    name,
                    args: vec![arg],
                });
            }
            // `the seconds`
            if builtins::takes_no_args(&name) {
                self.pos += 1;
                return Ok(Expr::Call {
                    name,
                    args: Vec::new(),
                });
            }
        }
        // `the label of button 1`: a property of an object.
        if self.is_word_at(1, "of") {
            self.pos += 2;
            let of = self.parse_object_operand()?;
            return Ok(Expr::Prop { name, of });
        }
        Err(self.err_here(format!(
            "'the {name}' is not supported yet. Try the result, the itemDelimiter, the target, the number of ..., a property such as the name of button 1, or a built-in function such as the length of ..."
        )))
    }
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Number(n) => format!("number {n}"),
        TokenKind::Str(s) => format!("string \"{s}\""),
        TokenKind::Word(w) => format!("word '{w}'"),
        TokenKind::Op(o) => format!("'{o}'"),
        TokenKind::Newline => "end of line".to_string(),
    }
}

fn chunk_kind_of(word: &str) -> Option<ChunkKind> {
    match word.to_lowercase().as_str() {
        "char" | "chars" | "character" | "characters" => Some(ChunkKind::Char),
        "word" | "words" => Some(ChunkKind::Word),
        "item" | "items" => Some(ChunkKind::Item),
        "line" | "lines" => Some(ChunkKind::Line),
        _ => None,
    }
}

fn ordinal_of(word: &str) -> Option<Ordinal> {
    let n = match word.to_lowercase().as_str() {
        "first" => 1,
        "second" => 2,
        "third" => 3,
        "fourth" => 4,
        "fifth" => 5,
        "sixth" => 6,
        "seventh" => 7,
        "eighth" => 8,
        "ninth" => 9,
        "tenth" => 10,
        "last" => return Some(Ordinal::Last),
        "mid" | "middle" => return Some(Ordinal::Middle),
        "any" => return Some(Ordinal::Any),
        _ => return None,
    };
    Some(Ordinal::Nth(n))
}

/// Turn an expression into a place `put` can store to, if it is one.
fn expr_to_target(e: Expr) -> Option<Target> {
    match e {
        Expr::Var(name) => Some(Target::Var(name)),
        Expr::Object(spec) => Some(Target::Object(spec)),
        Expr::Chunk { kind, sel, of } => Some(Target::Chunk {
            kind,
            sel,
            of: Box::new(expr_to_target(*of)?),
        }),
        _ => None,
    }
}

/// A container as an expression, for reading its current value.
fn target_to_expr(target: &Target) -> Expr {
    match target {
        Target::Var(name) => Expr::Var(name.clone()),
        Target::Object(spec) => Expr::Object(spec.clone()),
        Target::Chunk { kind, sel, of } => Expr::Chunk {
            kind: *kind,
            sel: sel.clone(),
            of: Box::new(target_to_expr(of)),
        },
    }
}

/// `add 5 to x` is `put x + 5 into x`, and likewise for the other operators.
fn arithmetic_put(target: Target, op: BinOp, operand: Expr) -> StmtKind {
    let current = target_to_expr(&target);
    StmtKind::Put {
        value: Expr::Binary {
            op,
            left: Box::new(current),
            right: Box::new(operand),
        },
        target: Some((Placement::Into, target)),
    }
}

/// A script cannot assign to a constant such as `cr` or `pi`.
fn check_not_constant(target: &Target, line: usize, col: usize) -> Result<(), XtError> {
    let mut current = target;
    loop {
        match current {
            Target::Object(_) => return Ok(()),
            Target::Var(name) => {
                if is_constant(name) {
                    return Err(XtError::at(
                        line,
                        col,
                        format!("'{name}' is a constant and can't be changed"),
                    ));
                }
                return Ok(());
            }
            Target::Chunk { of, .. } => current = &**of,
        }
    }
}
