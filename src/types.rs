//! Lightweight type-inference pass for Crust.
// Future-use functions are intentional API surface.
#![allow(dead_code)]
//!
//! This is a best-effort structural type checker, not a full Hindley–Milner
//! unification engine.  It handles the common patterns that appear in
//! LLM-generated code:
//!
//! - Literal types (i64, f64, bool, &str, char)
//! - Named types from `let x: T = ...` annotations
//! - Propagation of return types from function calls (for known functions)
//! - Mismatch detection between annotated type and inferred initialiser type
//! - Detection of functions with unannoted parameters at Level 4

use crate::ast::*;
use crate::strictness::StrictnessLevel;
use std::collections::HashMap;

// ── Inferred type ─────────────────────────────────────────────────────────────

/// A simplified type used by the inference pass.
#[derive(Debug, Clone, PartialEq)]
pub enum InferredType {
    Int,
    Float,
    Bool,
    Str,
    Char,
    Unit,
    Never,
    Vec(Box<InferredType>),
    Option(Box<InferredType>),
    Result(Box<InferredType>, Box<InferredType>),
    Tuple(Vec<InferredType>),
    Named(String),
    Ref(Box<InferredType>),
    Unknown,
}

impl InferredType {
    fn from_ast_ty(ty: &Ty) -> Self {
        match ty {
            Ty::Named(s) => match s.as_str() {
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => InferredType::Int,
                "f32" | "f64" => InferredType::Float,
                "bool" => InferredType::Bool,
                "str" | "String" | "&str" => InferredType::Str,
                "char" => InferredType::Char,
                other => InferredType::Named(other.to_string()),
            },
            Ty::Unit => InferredType::Unit,
            Ty::Never => InferredType::Never,
            Ty::Ref(_, inner) | Ty::Ptr(_, inner) | Ty::RefLt(_, _, inner) => {
                InferredType::Ref(Box::new(InferredType::from_ast_ty(inner)))
            }
            Ty::Tuple(tys) => {
                InferredType::Tuple(tys.iter().map(InferredType::from_ast_ty).collect())
            }
            Ty::Generic(name, args) => match name.as_str() {
                "Vec" if args.len() == 1 => {
                    InferredType::Vec(Box::new(InferredType::from_ast_ty(&args[0])))
                }
                "Option" if args.len() == 1 => {
                    InferredType::Option(Box::new(InferredType::from_ast_ty(&args[0])))
                }
                "Result" if !args.is_empty() => {
                    let ok = InferredType::from_ast_ty(&args[0]);
                    let err = args
                        .get(1)
                        .map(InferredType::from_ast_ty)
                        .unwrap_or(InferredType::Named("Box<dyn std::error::Error>".into()));
                    InferredType::Result(Box::new(ok), Box::new(err))
                }
                _ => InferredType::Named(name.clone()),
            },
            Ty::Slice(inner) => InferredType::Vec(Box::new(InferredType::from_ast_ty(inner))),
            Ty::Lifetime(_) => InferredType::Unknown,
            Ty::FnPtr { .. } => InferredType::Named("fn".into()),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, InferredType::Int | InferredType::Float)
    }
}

impl std::fmt::Display for InferredType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferredType::Int => write!(f, "integer"),
            InferredType::Float => write!(f, "float"),
            InferredType::Bool => write!(f, "bool"),
            InferredType::Str => write!(f, "str/String"),
            InferredType::Char => write!(f, "char"),
            InferredType::Unit => write!(f, "()"),
            InferredType::Never => write!(f, "!"),
            InferredType::Unknown => write!(f, "?"),
            InferredType::Named(n) => write!(f, "{}", n),
            InferredType::Vec(t) => write!(f, "Vec<{}>", t),
            InferredType::Option(t) => write!(f, "Option<{}>", t),
            InferredType::Result(ok, err) => write!(f, "Result<{}, {}>", ok, err),
            InferredType::Tuple(ts) => {
                write!(f, "(")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", t)?;
                }
                write!(f, ")")
            }
            InferredType::Ref(t) => write!(f, "&{}", t),
        }
    }
}

// ── Type environment ──────────────────────────────────────────────────────────

struct TypeEnv {
    vars: HashMap<String, InferredType>,
    /// Function signatures: name → (param types, return type)
    fns: HashMap<String, (Vec<InferredType>, InferredType)>,
}

impl TypeEnv {
    fn new() -> Self {
        TypeEnv {
            vars: HashMap::new(),
            fns: HashMap::new(),
        }
    }

    fn bind(&mut self, name: &str, ty: InferredType) {
        self.vars.insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> InferredType {
        self.vars
            .get(name)
            .cloned()
            .unwrap_or(InferredType::Unknown)
    }
}

// ── Diagnostic ────────────────────────────────────────────────────────────────

pub struct TypeDiagnostic {
    pub message: String,
    /// Enclosing function name (empty = top-level).
    pub function: String,
}

// ── TypeChecker ───────────────────────────────────────────────────────────────

pub struct TypeChecker;

impl TypeChecker {
    /// Check the whole program; returns diagnostics.
    pub fn check_program(items: &[Item]) -> Vec<TypeDiagnostic> {
        let mut diags = Vec::new();
        let mut env = TypeEnv::new();

        // First pass: register all top-level function signatures so calls can be checked.
        for item in items {
            if let Item::Fn(f) = item {
                let param_tys = f
                    .params
                    .iter()
                    .filter(|p| !p.is_self)
                    .map(|p| InferredType::from_ast_ty(&p.ty))
                    .collect();
                let ret_ty = f
                    .ret_ty
                    .as_ref()
                    .map(InferredType::from_ast_ty)
                    .unwrap_or(InferredType::Unit);
                env.fns.insert(f.name.clone(), (param_tys, ret_ty));
            }
        }

        // Second pass: check each function body.
        for item in items {
            if let Item::Fn(f) = item {
                let mut fn_env = TypeEnv::new();
                fn_env.fns = env.fns.clone();
                // Bind parameters
                for p in &f.params {
                    if !p.is_self {
                        fn_env.bind(&p.name, InferredType::from_ast_ty(&p.ty));
                    }
                }
                diags.extend(Self::check_fn(f, &mut fn_env));
            }
        }
        diags
    }

    fn check_fn(f: &FnDef, env: &mut TypeEnv) -> Vec<TypeDiagnostic> {
        let mut diags = Vec::new();

        // Check body statements
        for stmt in &f.body.stmts {
            diags.extend(Self::check_stmt(stmt, f, env));
        }

        // Check return type consistency
        if let (Some(tail), Some(declared_ret)) = (&f.body.tail, &f.ret_ty) {
            let inferred = Self::infer_expr(tail, env);
            let declared = InferredType::from_ast_ty(declared_ret);
            if !types_compatible(&inferred, &declared) {
                diags.push(TypeDiagnostic {
                    message: format!(
                        "return type mismatch in `{}`: declared `{}`, inferred `{}`",
                        f.name, declared, inferred
                    ),
                    function: f.name.clone(),
                });
            }
        }

        diags
    }

    fn check_stmt(stmt: &Stmt, f: &FnDef, env: &mut TypeEnv) -> Vec<TypeDiagnostic> {
        let mut diags = Vec::new();
        match stmt {
            Stmt::Let {
                name,
                ty: Some(declared_ty),
                init: Some(init_expr),
                ..
            } => {
                let inferred = Self::infer_expr(init_expr, env);
                let declared = InferredType::from_ast_ty(declared_ty);
                if !types_compatible(&inferred, &declared) {
                    diags.push(TypeDiagnostic {
                        message: format!(
                            "type mismatch for `let {}`: declared `{}`, inferred `{}`",
                            name, declared, inferred
                        ),
                        function: f.name.clone(),
                    });
                }
                env.bind(name, declared);
            }
            Stmt::Let {
                name,
                ty: None,
                init: Some(init_expr),
                ..
            } => {
                let inferred = Self::infer_expr(init_expr, env);
                env.bind(name, inferred);
            }
            Stmt::Let {
                name,
                ty: Some(declared_ty),
                init: None,
                ..
            } => {
                env.bind(name, InferredType::from_ast_ty(declared_ty));
            }
            Stmt::Semi(e) | Stmt::Expr(e) => {
                let _ = Self::infer_expr(e, env);
            }
            _ => {}
        }
        diags
    }

    /// Infer the type of an expression in the given environment.
    fn infer_expr(expr: &Expr, env: &TypeEnv) -> InferredType {
        match expr {
            Expr::Lit(Lit::Int(_)) => InferredType::Int,
            Expr::Lit(Lit::Float(_)) => InferredType::Float,
            Expr::Lit(Lit::Bool(_)) => InferredType::Bool,
            Expr::Lit(Lit::Str(_)) => InferredType::Str,
            Expr::Lit(Lit::Char(_)) => InferredType::Char,

            Expr::Ident(name) => env.lookup(name),

            Expr::Binary(op, lhs, rhs) => {
                let l = Self::infer_expr(lhs, env);
                let r = Self::infer_expr(rhs, env);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        if l == InferredType::Float || r == InferredType::Float {
                            InferredType::Float
                        } else {
                            InferredType::Int
                        }
                    }
                    BinOp::Eq
                    | BinOp::Ne
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or => InferredType::Bool,
                    _ => l,
                }
            }

            Expr::Unary(UnOp::Not, _) => InferredType::Bool,
            Expr::Unary(UnOp::Neg, e) => Self::infer_expr(e, env),

            Expr::Array(elems) => {
                let elem_ty = elems
                    .first()
                    .map(|e| Self::infer_expr(e, env))
                    .unwrap_or(InferredType::Unknown);
                InferredType::Vec(Box::new(elem_ty))
            }

            Expr::Tuple(elems) => {
                InferredType::Tuple(elems.iter().map(|e| Self::infer_expr(e, env)).collect())
            }

            Expr::If {
                then_block,
                else_block,
                ..
            } => {
                let then_ty = then_block
                    .tail
                    .as_ref()
                    .map(|e| Self::infer_expr(e, env))
                    .unwrap_or(InferredType::Unit);
                if else_block.is_some() {
                    then_ty
                } else {
                    InferredType::Unit
                }
            }

            Expr::Block(block) => block
                .tail
                .as_ref()
                .map(|e| Self::infer_expr(e, env))
                .unwrap_or(InferredType::Unit),

            Expr::Unsafe(block) => block
                .tail
                .as_ref()
                .map(|e| Self::infer_expr(e, env))
                .unwrap_or(InferredType::Unit),

            Expr::Call { func, .. } => {
                let name = match func.as_ref() {
                    Expr::Path(parts) => parts.last().cloned().unwrap_or_default(),
                    Expr::Ident(n) => n.clone(),
                    _ => String::new(),
                };
                if !name.is_empty() {
                    env.fns
                        .get(&name)
                        .map(|(_, ret)| ret.clone())
                        .unwrap_or(InferredType::Unknown)
                } else {
                    InferredType::Unknown
                }
            }

            Expr::MethodCall { method, .. } => match method.as_str() {
                "len" | "count" | "capacity" => InferredType::Int,
                "is_empty" | "contains" | "starts_with" | "ends_with" => InferredType::Bool,
                "to_string" | "to_owned" | "clone" | "trim" => InferredType::Str,
                "unwrap" | "expect" => InferredType::Unknown, // depends on the Option/Result inner type
                "ok" => InferredType::Option(Box::new(InferredType::Unknown)),
                "err" => InferredType::Option(Box::new(InferredType::Unknown)),
                _ => InferredType::Unknown,
            },

            Expr::Ref { expr: inner, .. } => {
                InferredType::Ref(Box::new(Self::infer_expr(inner, env)))
            }

            Expr::Cast(_, ty) => InferredType::from_ast_ty(ty),

            Expr::Return(_) | Expr::Break(..) | Expr::Continue(_) => InferredType::Never,

            Expr::Range { .. } => InferredType::Named("Range".into()),

            Expr::Await(inner) => Self::infer_expr(inner, env),

            _ => InferredType::Unknown,
        }
    }
}

/// Two inferred types are compatible for assignment/return if they are
/// identical, or if one or both are Unknown (we can't say).
fn types_compatible(a: &InferredType, b: &InferredType) -> bool {
    if a == &InferredType::Unknown || b == &InferredType::Unknown {
        return true;
    }
    if a == &InferredType::Never || b == &InferredType::Never {
        return true;
    }
    // Int and Float can be confused in literals (e.g. `let x: f64 = 1`)
    if a.is_numeric() && b.is_numeric() {
        return true;
    }
    a == b
}

// ── Level 4 enforcement: unannotated parameters ───────────────────────────────

/// At `--strict=4 --llm-mode`, require every function parameter to have an
/// explicit type annotation (not `Ty::Unit` which is the default placeholder).
pub fn check_unannotated_params(
    items: &[Item],
    level: StrictnessLevel,
    llm_mode: bool,
) -> Vec<TypeDiagnostic> {
    if !llm_mode || level < StrictnessLevel::Prove {
        return vec![];
    }

    let mut diags = Vec::new();
    for item in items {
        if let Item::Fn(f) = item {
            for p in &f.params {
                if p.is_self {
                    continue;
                }
                if p.ty == Ty::Unit {
                    diags.push(TypeDiagnostic {
                        message: format!(
                            "parameter `{}` in `{}` has no explicit type annotation",
                            p.name, f.name
                        ),
                        function: f.name.clone(),
                    });
                }
            }
            if f.ret_ty.is_none() && level >= StrictnessLevel::Prove {
                diags.push(TypeDiagnostic {
                    message: format!("`{}` has no explicit return type", f.name),
                    function: f.name.clone(),
                });
            }
        }
    }
    diags
}
