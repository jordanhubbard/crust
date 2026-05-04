#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Char(char),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Lit),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(BinOp, Box<Expr>, Box<Expr>),
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        /// Turbofish args from `expr.method::<T1, T2>(args)`. The original ident
        /// is also stored as the *last* type's name when meaningful, so existing
        /// eval-side logic that switches on a single name (e.g.,
        /// `collect::<String>()`) continues to work.
        turbofish: Option<Vec<Ty>>,
        args: Vec<Expr>,
    },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    If {
        cond: Box<Expr>,
        then_block: Block,
        else_block: Option<Box<Expr>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Block(Block),
    Unsafe(Block),
    Closure {
        params: Vec<ClosureParam>,
        body: Box<Expr>,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    },
    Return(Option<Box<Expr>>),
    Break(Option<String>, Option<Box<Expr>>),
    Continue(Option<String>),
    Macro {
        name: String,
        args: Vec<Expr>,
    },
    Cast(Box<Expr>, Ty),
    Path(Vec<String>),
    Ref {
        mutable: bool,
        expr: Box<Expr>,
    },
    Deref(Box<Expr>),
    Try(Box<Expr>),
    /// `expr.await` — evaluated synchronously at Level 0-3; Level 4 requires explicit Future types.
    Await(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum Pat {
    Wild,
    Ident(String),
    Lit(Lit),
    Tuple(Vec<Pat>),
    Struct {
        name: String,
        fields: Vec<(String, Pat)>,
        rest: bool,
    },
    TupleStruct {
        name: String,
        fields: Vec<Pat>,
    },
    Or(Vec<Pat>),
    Range(Lit, Lit, bool),
    Ref(Box<Pat>),
    Bind {
        name: String,
        pat: Box<Pat>,
    }, // name @ pat
    /// Slice pattern: [a, b, rest @ .., z]
    /// before = patterns before .., rest = optional binding for .., has_rest = .. was present, after = patterns after ..
    Slice {
        before: Vec<Pat>,
        rest: Option<String>,
        has_rest: bool,
        after: Vec<Pat>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Named(String),
    Ref(bool, Box<Ty>),
    /// `&'lt T` or `&'lt mut T` — reference with an explicit lifetime
    /// annotation. Codegen emits this faithfully so functions returning
    /// `&'static str` round-trip without E0106 (crust-1x4).
    RefLt(bool, String, Box<Ty>),
    Slice(Box<Ty>),
    Tuple(Vec<Ty>),
    Unit,
    Never,
    Generic(String, Vec<Ty>),
    Ptr(bool, Box<Ty>),
    /// Lifetime annotation, e.g. `'a` or `'static`.
    /// Parsed at Level 2+ (Harden); currently stored but not emitted by the lexer at lower levels.
    #[allow(dead_code)]
    Lifetime(String),
    /// Function pointer or `Fn`/`FnMut`/`FnOnce` trait sugar: `fn(T1, T2) -> R`,
    /// `Fn(T) -> R`, etc. Captures the kind so codegen can re-emit faithfully.
    /// `kind = ""` means a bare `fn(...)` pointer.
    FnPtr {
        kind: String,
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        mutable: bool,
        ty: Option<Ty>,
        init: Option<Expr>,
    },
    LetPat {
        pat: Pat,
        ty: Option<Ty>,
        init: Option<Expr>,
        else_block: Option<Block>,
    },
    Semi(Expr),
    Expr(Expr),
    Item(Item),
}

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnDef),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(ImplDef),
    Trait {
        name: String,
        methods: Vec<FnDef>,
        generics: Vec<String>,
    },
    Use(Vec<String>),
    Const {
        name: String,
        ty: Ty,
        value: Expr,
    },
    TypeAlias {
        name: String,
        ty: Ty,
    },
    /// Inline module: `mod NAME { items }`. File-based `mod foo;` is not yet
    /// supported (tracked separately).  The interpreter registers inner items
    /// with their fully-qualified `NAME::ident` key so `NAME::ident()` resolves.
    Mod {
        name: String,
        items: Vec<Item>,
    },
}

/// Crust-specific attributes parsed from `#[name]` or `#[name(expr)]` syntax.
/// These are only meaningful to the Crust compiler; unknown attributes are
/// stored as `Unknown` and round-tripped to the generated Rust output.
#[derive(Debug, Clone)]
pub enum Attr {
    /// `#[requires(pred)]` — precondition that caller must satisfy on function entry.
    Requires(Expr),
    /// `#[ensures(pred)]` — postcondition that the function guarantees on return.
    /// Inside the predicate, the identifier `result` refers to the return value.
    Ensures(Expr),
    /// `#[invariant(pred)]` — property that must hold throughout the function body.
    Invariant(Expr),
    /// `#[pure]` — function has no observable side effects (no I/O, no mutable
    /// external state, no `unsafe`).  Enables equational reasoning.
    Pure,
    /// Any other attribute stored verbatim so we can re-emit it in codegen.
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Option<Ty>,
    pub body: Block,
    /// Crust-specific attributes collected from `#[...]` lines before this function.
    pub attrs: Vec<Attr>,
    /// Whether the function was declared `async fn`.
    pub is_async: bool,
    /// Generic parameter *names* captured from `<T, U, …>`. Bounds and
    /// where-clauses are not yet modelled (crust-1x4) — we just preserve
    /// the names so codegen can re-emit `<T, U>`.
    pub generics: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ClosureParam {
    Simple(String),
    Tuple(Vec<String>),
    Pat(Pat), // arbitrary pattern: (i, (a, b)), _
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub is_self: bool,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
    /// Attributes collected from `#[...]` lines preceding the struct.
    /// Used to merge author-supplied derives with Crust's auto-derives.
    pub attrs: Vec<Attr>,
    /// Generic parameter *names* captured from `<T, U, …>`. Bounds and
    /// where-clauses are not yet modelled (crust-1x4).
    pub generics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub attrs: Vec<Attr>,
    pub generics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub data: VariantData,
}

#[derive(Debug, Clone)]
pub enum VariantData {
    Unit,
    Tuple(Vec<Ty>),
    Struct(Vec<(String, Ty)>),
}

#[derive(Debug, Clone)]
pub struct ImplDef {
    pub type_name: String,
    pub trait_name: Option<String>,
    pub methods: Vec<FnDef>,
    /// Associated constants: (name, declared type, initializer expression).
    /// The type is required for codegen to emit a valid `const NAME: T = ...;`
    /// (rustc rejects `_` placeholder types in associated-const positions, E0121).
    pub consts: Vec<(String, Ty, Expr)>,
    /// Generic parameters from `impl<T, U> …` — the names introduced by the
    /// impl block itself.
    pub generics: Vec<String>,
    /// Generic arguments applied to the implementing type, captured from
    /// `impl … TypeName<T, U>`. Usually mirrors `generics` for inherent impls
    /// (`impl<T> Queue<T>`), but the two diverge for partial specialisations.
    pub type_args: Vec<String>,
}

pub type Program = Vec<Item>;
