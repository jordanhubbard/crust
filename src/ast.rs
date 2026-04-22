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
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    BitAnd, BitOr, BitXor, Shl, Shr,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Lit),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(BinOp, Box<Expr>, Box<Expr>),
    Call { func: Box<Expr>, args: Vec<Expr> },
    MethodCall { receiver: Box<Expr>, method: String, turbofish: Option<String>, args: Vec<Expr> },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    If { cond: Box<Expr>, then_block: Block, else_block: Option<Box<Expr>> },
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> },
    Block(Block),
    Closure { params: Vec<ClosureParam>, body: Box<Expr> },
    StructLit { name: String, fields: Vec<(String, Expr)> },
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    Range { start: Option<Box<Expr>>, end: Option<Box<Expr>>, inclusive: bool },
    Return(Option<Box<Expr>>),
    Break(Option<Box<Expr>>),
    Continue,
    Macro { name: String, args: Vec<Expr> },
    Cast(Box<Expr>, Ty),
    Path(Vec<String>),
    Ref { mutable: bool, expr: Box<Expr> },
    Deref(Box<Expr>),
    Try(Box<Expr>),
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
    Struct { name: String, fields: Vec<(String, Pat)>, rest: bool },
    TupleStruct { name: String, fields: Vec<Pat> },
    Or(Vec<Pat>),
    Range(Lit, Lit, bool),
    Ref(Box<Pat>),
}

#[derive(Debug, Clone)]
pub enum Ty {
    Named(String),
    Ref(bool, Box<Ty>),
    Slice(Box<Ty>),
    Tuple(Vec<Ty>),
    Unit,
    Never,
    Generic(String, Vec<Ty>),
    Ptr(bool, Box<Ty>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let { name: String, mutable: bool, ty: Option<Ty>, init: Option<Expr> },
    LetPat { pat: Pat, ty: Option<Ty>, init: Option<Expr> },
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
    Use(Vec<String>),
    Const { name: String, ty: Ty, value: Expr },
    TypeAlias { name: String, ty: Ty },
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Option<Ty>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub enum ClosureParam {
    Simple(String),
    Tuple(Vec<String>),
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
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
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
}

pub type Program = Vec<Item>;
