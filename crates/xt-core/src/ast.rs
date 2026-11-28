//! The syntax tree: what the parser produces and the compiler consumes.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Concat,      // &
    ConcatSpace, // &&
    Add,
    Sub,
    Mul,
    Div,
    IntDiv, // div
    Mod,    // mod
    Eq,     // =  is
    Ne,     // <> is not
    Lt,
    Gt,
    Le,
    Ge,
    Contains, // a contains b
    IsIn,     // a is in b
    IsNotIn,  // a is not in b
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A string or number literal, as text.
    Literal(String),
    /// A variable or constant name, in its original spelling.
    Var(String),
    /// `the result` (the only `the` form so far).
    The(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A call to a user-defined function: `name(args)`.
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// A chunk of a string: `word 2 of x`, `the last line of x`.
    Chunk {
        kind: ChunkKind,
        sel: ChunkSel,
        of: Box<Expr>,
    },
    /// `the number of words in x`
    Count {
        kind: ChunkKind,
        of: Box<Expr>,
    },
    /// An object in a value position: `button 1`, `me`. A field gives its
    /// text, and any other object gives its reference.
    Object(ObjSpec),
    /// `the label of button 1`
    Prop {
        name: String,
        of: ObjSpec,
    },
    /// `the number of buttons of card 2`
    ObjCount {
        kind: ObjKind,
        of: Option<ObjSpec>,
    },
}

/// A kind of object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjKind {
    Stack,
    Background,
    Card,
    Group,
    Button,
    Field,
    Scrollbar,
    Image,
    Graphic,
    /// The generic kind. `part 2`, `control 2`, and `widget 2` mean the
    /// second control of any kind; `create part` makes a plain part (OBJ-13).
    Part,
    /// A sound stored in a stack. Resource objects have no rect (OBJ-14).
    AudioClip,
    /// A movie stored in a stack.
    VideoClip,
}

/// The dialog forms of `answer` and `ask` beyond buttons and text (DLG-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogForm {
    /// `answer file`: choose an existing file to open.
    AnswerFile,
    /// `ask file`: choose a name and place to save a file.
    AskFile,
    /// `answer folder`: choose a folder.
    AnswerFolder,
    /// `answer color`: choose a color.
    AnswerColor,
    /// `answer effect`: choose a visual effect.
    AnswerEffect,
    /// `answer record`: choose sound-recording settings.
    AnswerRecord,
}

/// How to pick one object of a kind.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjSel {
    /// A number or a name, decided when the script runs: `button 1`, `button "OK"`.
    At(Box<Expr>),
    /// `card id 12`
    Id(Box<Expr>),
    Ordinal(Ordinal),
    /// `next card`
    Next,
    /// `previous card`
    Prev,
}

/// Which object a script means.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjSpec {
    Me,
    /// `the target`
    Target,
    /// `this card`
    This(ObjKind),
    Find {
        kind: ObjKind,
        sel: ObjSel,
        /// The container, as in `button 1 of card 2`.
        of: Option<Box<ObjSpec>>,
    },
    /// Any other expression that holds a reference, such as a variable.
    Value(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChunkKind {
    Char,
    Word,
    Item,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ordinal {
    /// `first` is 1 through `tenth`, which is 10.
    Nth(usize),
    Last,
    Middle,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChunkSel {
    Ordinal(Ordinal),
    /// `word 3`, `item i + 1`
    At(Box<Expr>),
    /// `word 2 to 4`
    Range(Box<Expr>, Box<Expr>),
}

/// Where `put` can store a value: a variable, or a chunk of one.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Var(String),
    /// A field: `put "x" into field 1`.
    Object(ObjSpec),
    Chunk {
        kind: ChunkKind,
        sel: ChunkSel,
        of: Box<Target>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    Into,
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitUnit {
    Ticks,
    Seconds,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RepeatKind {
    Forever,
    While(Expr),
    Until(Expr),
    Times(Expr),
    With {
        var: String,
        from: Expr,
        to: Expr,
        down: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaitKind {
    For(Expr, WaitUnit),
    Until(Expr),
    While(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExitKind {
    /// `exit handlerName`: leave the current handler.
    Handler,
    /// `exit repeat`
    Repeat,
    /// `exit to top`: stop everything.
    ToTop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    /// Line in the source, counting from 0.
    pub line: usize,
    pub kind: StmtKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// `put <expr> [into|before|after <variable or chunk>]`
    Put {
        value: Expr,
        target: Option<(Placement, Target)>,
    },
    /// `set the itemDelimiter to ";"`
    Set {
        prop: String,
        value: Expr,
    },
    /// `delete word 2 of x`
    Delete(Target),
    /// `try ... catch name ... end try`
    Try {
        body: Vec<Stmt>,
        var: Option<String>,
        catch_body: Vec<Stmt>,
    },
    /// `throw "message"`
    Throw(Expr),
    /// `do "uname -m" as shell`
    Do {
        script: Expr,
        language: String,
    },
    /// `set the label of button 1 to "Go"`
    SetObjProp {
        prop: String,
        target: ObjSpec,
        value: Expr,
    },
    /// `create button "OK" in card 2`
    Create {
        kind: ObjKind,
        name: Option<Expr>,
        parent: Option<ObjSpec>,
    },
    /// `delete button 1`
    DeleteObj(ObjSpec),
    /// `go to card 2`
    Go(ObjSpec),
    /// `send mouseUp to button 1`
    Send {
        message: Expr,
        target: ObjSpec,
    },
    /// `pass mouseUp`
    Pass(String),
    /// `answer "Sure?" with "Yes" or "No"`
    Answer {
        prompt: Expr,
        buttons: Vec<Expr>,
    },
    /// `ask "Name?" with "Anon"`
    Ask {
        prompt: Expr,
        default: Option<Expr>,
    },
    /// `answer file "Open:" of type "txt"`, `ask file "Save as:" with "a.txt"`,
    /// `answer folder`, `answer color`, `answer effect`, `answer record`.
    /// Every part is optional; which parts a form takes is up to the parser.
    Dialog {
        form: DialogForm,
        prompt: Option<Expr>,
        default: Option<Expr>,
        types: Vec<Expr>,
    },
    /// `get <expr>` stores the value in `it`.
    Get(Expr),
    /// A bare expression typed at the prompt: evaluate it and show the result.
    Eval(Expr),
    /// A message to a user-defined handler: `name arg1 arg2` or `name a, b`.
    Call {
        name: String,
        args: Vec<Expr>,
    },
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    Repeat {
        kind: RepeatKind,
        body: Vec<Stmt>,
    },
    Wait(WaitKind),
    Return(Option<Expr>),
    Exit(ExitKind),
    NextRepeat,
    Global(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HandlerDef {
    pub name: String,
    pub is_function: bool,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub line: usize,
}

/// A parsed script: handler definitions plus any loose statements, which
/// run in order as an implicit main handler (spec DO-2).
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    pub handlers: Vec<HandlerDef>,
    pub main: Vec<Stmt>,
}
