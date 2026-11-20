//! A hand-written recursive-descent parser.
//!
//! It works on the whole token stream, one statement per line. Binary
//! operators are handled by one function driven by the LEVELS table
//! (spec section 6). Words have no fixed meaning: the parser decides from
//! context (spec section 4).

use crate::ast::*;
use crate::error::XtError;
use crate::lexer::{lex, Token, TokenKind};

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
        self.word_at(0).is_some_and(|w| w.eq_ignore_ascii_case(word))
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(
            self.peek().map(|t| &t.kind),
            Some(TokenKind::Op(o)) if o.as_str() == op
        )
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
        let found =
            self.is_word("end") && self.word_at(1).is_some_and(|w| w.eq_ignore_ascii_case(what));
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
            if self.is_word("end") || self.is_word("else") {
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
        if self.is_word("end") || self.is_word("else") {
            let w = self.word_at(0).unwrap_or("").to_string();
            return Err(self.err_here(format!("Unexpected '{w}' here")));
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
                return Err(self.err_here("Use the one-line form here, or put this 'if' on its own lines"));
            }
            return Ok(kind);
        }
        if self.is_word("repeat") {
            return Err(self.err_here("A 'repeat' loop needs its own lines"));
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
                        return Err(self.err_here("'exit repeat' can only be used inside a repeat loop"));
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
                    return Err(self.err_here("'next repeat' can only be used inside a repeat loop"));
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
            _ => self.parse_call_or_eval(),
        }
    }

    fn parse_put_target(&mut self) -> Result<Option<(Placement, String)>, XtError> {
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
        match self.word_at(0).map(|w| w.to_string()) {
            Some(name) => {
                self.pos += 1;
                Ok(Some((placement, name)))
            }
            None => Err(self.err_here("Expected a variable name here")),
        }
    }

    /// A line that starts with a word that is not a built-in command is a
    /// message to a user handler (spec CTX-6), unless an operator follows,
    /// in which case it is an expression.
    fn parse_call_or_eval(&mut self) -> Result<StmtKind, XtError> {
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
                        return Err(self.err_here("Separate arguments with spaces or with commas, not both"));
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
                if w.eq_ignore_ascii_case("the") && self.word_at(1).is_some() {
                    return self.parse_the();
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

    /// Positioned on `the`, which is followed by a word.
    fn parse_the(&mut self) -> Result<Expr, XtError> {
        let name = self.word_at(1).unwrap_or("").to_lowercase();
        if name == "result" {
            self.pos += 2;
            Ok(Expr::The(name))
        } else {
            self.pos += 1;
            Err(self.err_here(format!(
                "Only 'the result' is supported so far, not 'the {name}'"
            )))
        }
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
