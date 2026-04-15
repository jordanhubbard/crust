/// AST node definitions for Crust.

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `let <name> = <expr>;`
    Let {
        name: String,
        value: Expr,
    },
    /// `struct Name { field: Type, ... }`
    StructDef {
        name: String,
        fields: Vec<(String, String)>, // (name, type_annotation)
    },
    /// `fn name(params) { body }`
    FnDef {
        name: String,
        params: Vec<(String, String)>, // (name, type_annotation)
        body: Vec<Stmt>,
    },
    /// An expression used as a statement (e.g. function call, println!)
    ExprStmt(Expr),
    /// `while <cond> { <body> }`
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    /// `loop { <body> }` — infinite loop, broken with `break`
    Loop {
        body: Vec<Stmt>,
    },
    /// `break;`
    Break,
    /// `return <expr>;` or `return;`
    Return(Option<Expr>),
    /// Reassignment: `name = expr;`
    Assign {
        target: AssignTarget,
        value: Expr,
    },
}

#[derive(Debug, Clone)]
pub enum AssignTarget {
    Variable(String),
    Field(Box<Expr>, String), // expr.field
    Index(Box<Expr>, Box<Expr>), // expr[index]
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// Integer literal
    IntLit(i64),
    /// Float literal
    FloatLit(f64),
    /// Boolean literal
    BoolLit(bool),
    /// String literal
    StringLit(String),
    /// Variable reference
    Ident(String),
    /// Binary operation: left op right
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    /// Unary operation: op expr
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    /// Function call: name(args)
    Call {
        function: Box<Expr>,
        args: Vec<Expr>,
    },
    /// println!("fmt", args...)
    PrintLn {
        format_str: String,
        args: Vec<Expr>,
    },
    /// vec![elem1, elem2, ...]
    VecLit(Vec<Expr>),
    /// Struct instantiation: Name { field: value, ... }
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// Field access: expr.field
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    /// Index access: expr[index]
    IndexAccess {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    /// Method call: expr.method(args)
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// if condition { then } else { else }
    If {
        condition: Box<Expr>,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
    },
    /// Block expression: { stmts }
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}
