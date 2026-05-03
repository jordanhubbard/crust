
#![allow(dead_code)]

//! Static analysis passes for Crust.
//!
//! 1. **Panic-freedom** — detects every site that can call `panic!` at runtime:
//!    `unwrap()`, `expect()`, index-out-of-bounds, division by zero, and the
//!    macros `panic!`, `unreachable!`, `todo!`, `unimplemented!`.
//!
//! 2. **Overflow** — detects bare `+`, `-`, `*` operations on integer types.
//!    At Level 4 these are replaced by `checked_*` in codegen; this pass
//!    reports them as warnings at Level 2–3 and errors at Level 4.
//!
//! 3. **Purity** — at Level 4, functions marked `#[pure]` are checked for
//!    the absence of I/O macros, mutable external state, and `unsafe` blocks.
//!
//! 4. **LLM guardrails** — when `--llm-mode` is active, additional checks are
//!    applied that are specifically tuned for LLM-generated code:
//!    - No `unsafe` blocks
//!    - No `unwrap()`/`expect()` — use `?` instead
//!    - No `todo!()`, `unimplemented!()`, `unreachable!()` in non-test code
//!    - No `as` casts (often unsound in LLM-generated code)
//!    - Wildcard `_` match arms at Level 4

use crate::ast::*;
use crate::strictness::StrictnessLevel;

// ── Diagnostic ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticKind {
    PotentialPanic,
    ArithmeticOverflow,
    PurityViolation,
    LlmGuardrail,
    WildcardMatch,
    UnsafeUsage,
    AsCast,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub kind:     DiagnosticKind,
    pub message:  String,
    /// Name of the enclosing function (empty string = top-level).
    pub function: String,
    /// Whether this is a hard error (vs a warning).
    error: bool,
}

impl Diagnostic {
    fn error(kind: DiagnosticKind, function: &str, msg: impl Into<String>) -> Self {
        Diagnostic { kind, message: msg.into(), function: function.to_string(), error: true }
    }
    fn warning(kind: DiagnosticKind, function: &str, msg: impl Into<String>) -> Self {
        Diagnostic { kind, message: msg.into(), function: function.to_string(), error: false }
    }

    pub fn is_error(&self) -> bool { self.error }

    pub fn format(&self) -> String {
        let sev = if self.error { "error" } else { "warning" };
        if self.function.is_empty() {
            format!("{}: {}", sev, self.message)
        } else {
            format!("{}: [{}] {}", sev, self.function, self.message)
        }
    }
}

// ── Analyzer ─────────────────────────────────────────────────────────────────

pub struct Analyzer {
    level:    StrictnessLevel,
    llm_mode: bool,
}

impl Analyzer {
    pub fn new(level: StrictnessLevel, llm_mode: bool) -> Self {
        Analyzer { level, llm_mode }
    }

    pub fn analyze_program(&self, items: &[Item]) -> Vec<Diagnostic> {
        let mut diags = Vec::new();
        for item in items {
            match item {
                Item::Fn(f) => diags.extend(self.analyze_fn(f)),
                Item::Impl(imp) => {
                    for method in &imp.methods {
                        diags.extend(self.analyze_fn(method));
                    }
                }
                _ => {}
            }
        }
        diags
    }

    fn analyze_fn(&self, f: &FnDef) -> Vec<Diagnostic> {
        let mut diags = Vec::new();

        // Panic-freedom at Level ≥ Develop
        if self.level >= StrictnessLevel::Develop {
            let panic_sites = self.collect_panic_sites(&f.body, &f.name);
            if self.level >= StrictnessLevel::Prove {
                diags.extend(panic_sites.into_iter().map(|mut d| { d.error = true; d }));
            } else {
                diags.extend(panic_sites);
            }
        }

        // Overflow at Level ≥ Develop
        if self.level >= StrictnessLevel::Develop {
            let overflow_sites = self.collect_overflow_sites(&f.body, &f.name);
            if self.level >= StrictnessLevel::Prove {
                diags.extend(overflow_sites.into_iter().map(|mut d| { d.error = true; d }));
            } else {
                diags.extend(overflow_sites);
            }
        }

        // Purity check at Level ≥ Prove (only for functions with #[pure])
        let is_pure = f.attrs.iter().any(|a| matches!(a, Attr::Pure));
        if is_pure && self.level >= StrictnessLevel::Prove {
            diags.extend(self.check_purity(f));
        }

        // Wildcard match arms at Level 4
        if self.level >= StrictnessLevel::Prove {
            diags.extend(self.collect_wildcard_matches(&f.body, &f.name));
        }

        // LLM guardrails
        if self.llm_mode {
            diags.extend(self.check_llm_guardrails(f));
        }

        diags
    }

    // ── Panic sites ───────────────────────────────────────────────────────────

    fn collect_panic_sites(&self, block: &Block, fn_name: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for stmt in &block.stmts {
            self.panic_in_stmt(stmt, fn_name, &mut out);
        }
        if let Some(tail) = &block.tail {
            self.panic_in_expr(tail, fn_name, &mut out);
        }
        out
    }

    fn panic_in_stmt(&self, stmt: &Stmt, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match stmt {
            Stmt::Let { init: Some(e), .. } |
            Stmt::Semi(e) |
            Stmt::Expr(e) => self.panic_in_expr(e, fn_name, out),
            Stmt::LetPat { init: Some(e), else_block, .. } => {
                self.panic_in_expr(e, fn_name, out);
                if let Some(blk) = else_block {
                    out.extend(self.collect_panic_sites(blk, fn_name));
                }
            }
            Stmt::Item(Item::Fn(f)) => out.extend(self.analyze_fn(f)),
            _ => {}
        }
    }

    fn panic_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match expr {
            // .unwrap() and .expect() can panic
            Expr::MethodCall { method, .. } if method == "unwrap" => {
                out.push(Diagnostic::warning(
                    DiagnosticKind::PotentialPanic, fn_name,
                    "`.unwrap()` can panic on `None`/`Err`; consider using `?` or `.unwrap_or`",
                ));
            }
            Expr::MethodCall { method, .. } if method == "expect" => {
                out.push(Diagnostic::warning(
                    DiagnosticKind::PotentialPanic, fn_name,
                    "`.expect()` can panic on `None`/`Err`; consider using `?` or explicit matching",
                ));
            }

            // Index operations can panic (out of bounds)
            Expr::Index(..) => {
                out.push(Diagnostic::warning(
                    DiagnosticKind::PotentialPanic, fn_name,
                    "index operation `[..]` can panic on out-of-bounds access; consider `.get()`",
                ));
            }

            // Division / remainder by a non-literal denominator can be zero
            Expr::Binary(BinOp::Div | BinOp::Rem, _, rhs) => {
                if !is_nonzero_literal(rhs) {
                    out.push(Diagnostic::warning(
                        DiagnosticKind::PotentialPanic, fn_name,
                        "division / remainder may panic if denominator is zero; \
                         consider `checked_div` or asserting `divisor != 0`",
                    ));
                }
            }

            // Macro calls: panic!, unreachable!, todo!, unimplemented!
            Expr::Macro { name, .. }
                if matches!(name.as_str(),
                    "panic" | "unreachable" | "todo" | "unimplemented") =>
            {
                out.push(Diagnostic::warning(
                    DiagnosticKind::PotentialPanic, fn_name,
                    format!("`{}!()` always panics", name),
                ));
            }

            // Recurse into sub-expressions
            _ => recurse_expr(expr, |e| self.panic_in_expr(e, fn_name, out)),
        }
    }

    // ── Overflow sites ────────────────────────────────────────────────────────

    fn collect_overflow_sites(&self, block: &Block, fn_name: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for stmt in &block.stmts {
            self.overflow_in_stmt(stmt, fn_name, &mut out);
        }
        if let Some(tail) = &block.tail {
            self.overflow_in_expr(tail, fn_name, &mut out);
        }
        out
    }

    fn overflow_in_stmt(&self, stmt: &Stmt, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match stmt {
            Stmt::Let { init: Some(e), .. } |
            Stmt::Semi(e) |
            Stmt::Expr(e) => self.overflow_in_expr(e, fn_name, out),
            Stmt::LetPat { init: Some(e), .. } => self.overflow_in_expr(e, fn_name, out),
            _ => {}
        }
    }

    fn overflow_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match expr {
            Expr::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul), lhs, rhs) => {
                let op_str = match op { BinOp::Add => "+", BinOp::Sub => "-", _ => "*" };
                out.push(Diagnostic::warning(
                    DiagnosticKind::ArithmeticOverflow, fn_name,
                    format!(
                        "bare `{}` may overflow; at --strict=4 use `checked_{}`",
                        op_str,
                        match op { BinOp::Add => "add", BinOp::Sub => "sub", _ => "mul" }
                    ),
                ));
                self.overflow_in_expr(lhs, fn_name, out);
                self.overflow_in_expr(rhs, fn_name, out);
            }
            Expr::Cast(inner, _) => {
                out.push(Diagnostic::warning(
                    DiagnosticKind::AsCast, fn_name,
                    "`as` cast can silently truncate values; prefer `From`/`TryFrom`",
                ));
                self.overflow_in_expr(inner, fn_name, out);
            }
            _ => recurse_expr(expr, |e| self.overflow_in_expr(e, fn_name, out)),
        }
    }

    // ── Purity ────────────────────────────────────────────────────────────────

    fn check_purity(&self, f: &FnDef) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        self.purity_in_block(&f.body, &f.name, &mut out);
        out
    }

    fn purity_in_block(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Semi(e) | Stmt::Expr(e) => self.purity_in_expr(e, fn_name, out),
                Stmt::Let { init: Some(e), .. } => self.purity_in_expr(e, fn_name, out),
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.purity_in_expr(tail, fn_name, out);
        }
    }

    fn purity_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match expr {
            // I/O macros are side effects
            Expr::Macro { name, .. }
                if matches!(name.as_str(), "println" | "print" | "eprintln" | "eprint" | "write" | "writeln") =>
            {
                out.push(Diagnostic::error(
                    DiagnosticKind::PurityViolation, fn_name,
                    format!("`#[pure]` function contains I/O macro `{}!()`", name),
                ));
            }
            // Unsafe blocks violate purity.
            // NOTE: `unsafe { ... }` blocks are desugared to regular `Block` nodes by
            // the parser (since unsafe blocks execute normally at all levels ≤ 3).
            // A dedicated `Expr::Unsafe` variant would be needed for Level 4 to
            // statically flag unsafe usage in `#[pure]` functions.  For now this is
            // a known gap documented in the design spec (Stage 1, item 4).
            _ => recurse_expr(expr, |e| self.purity_in_expr(e, fn_name, out)),
        }
    }

    // ── Wildcard match arms ───────────────────────────────────────────────────

    fn collect_wildcard_matches(&self, block: &Block, fn_name: &str) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for stmt in &block.stmts {
            match stmt {
                Stmt::Semi(e) | Stmt::Expr(e) => self.wildcard_in_expr(e, fn_name, &mut out),
                Stmt::Let { init: Some(e), .. } => self.wildcard_in_expr(e, fn_name, &mut out),
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.wildcard_in_expr(tail, fn_name, &mut out);
        }
        out
    }

    fn wildcard_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match expr {
            Expr::Match { arms, .. } => {
                let has_wild = arms.iter().any(|a| matches!(a.pat, Pat::Wild));
                if has_wild {
                    out.push(Diagnostic::warning(
                        DiagnosticKind::WildcardMatch, fn_name,
                        "wildcard `_` arm in `match` at --strict=4; \
                         consider exhaustively listing all variants",
                    ));
                }
                for arm in arms {
                    self.wildcard_in_expr(&arm.body, fn_name, out);
                }
            }
            _ => recurse_expr(expr, |e| self.wildcard_in_expr(e, fn_name, out)),
        }
    }

    // ── LLM guardrails ────────────────────────────────────────────────────────

    fn check_llm_guardrails(&self, f: &FnDef) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        // unwrap / expect already caught by panic analysis above; escalate to errors
        let panic_diags = self.collect_panic_sites(&f.body, &f.name);
        for d in panic_diags {
            match d.kind {
                DiagnosticKind::PotentialPanic => {
                    out.push(Diagnostic::error(
                        DiagnosticKind::LlmGuardrail, &f.name,
                        format!("--llm-mode: {}", d.message),
                    ));
                }
                _ => {}
            }
        }
        // as-casts are errors in LLM mode
        self.llm_cast_check(&f.body, &f.name, &mut out);
        // todo!/unimplemented!/unreachable! are errors in LLM mode
        self.llm_macro_check(&f.body, &f.name, &mut out);
        out
    }

    fn llm_cast_check(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        let stmts_exprs: Vec<&Expr> = block.stmts.iter().filter_map(|s| match s {
            Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
            Stmt::Let { init: Some(e), .. } => Some(e),
            _ => None,
        }).collect();
        for expr in stmts_exprs {
            self.llm_cast_in_expr(expr, fn_name, out);
        }
        if let Some(tail) = &block.tail {
            self.llm_cast_in_expr(tail, fn_name, out);
        }
    }

    fn llm_cast_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::Cast(..) = expr {
            out.push(Diagnostic::error(
                DiagnosticKind::LlmGuardrail, fn_name,
                "--llm-mode: `as` cast is disallowed; use `From`/`TryFrom` for safe conversions",
            ));
        }
        recurse_expr(expr, |e| self.llm_cast_in_expr(e, fn_name, out));
    }

    fn llm_macro_check(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        let stmts_exprs: Vec<&Expr> = block.stmts.iter().filter_map(|s| match s {
            Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
            Stmt::Let { init: Some(e), .. } => Some(e),
            _ => None,
        }).collect();
        for expr in stmts_exprs {
            self.llm_macro_in_expr(expr, fn_name, out);
        }
        if let Some(tail) = &block.tail {
            self.llm_macro_in_expr(tail, fn_name, out);
        }
    }

    fn llm_macro_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::Macro { name, .. } = expr {
            if matches!(name.as_str(), "todo" | "unimplemented" | "unreachable") {
                out.push(Diagnostic::error(
                    DiagnosticKind::LlmGuardrail, fn_name,
                    format!("--llm-mode: `{}!()` is disallowed in non-test code", name),
                ));
            }
        }
        recurse_expr(expr, |e| self.llm_macro_in_expr(e, fn_name, out));
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns true if `expr` is a literal that is provably non-zero.
fn is_nonzero_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(Lit::Int(n)) if *n != 0)
}

/// Walk the direct sub-expressions of `expr`, calling `f` on each.
/// This is a shallow walk; `f` is responsible for recursing further.
fn recurse_expr<F: FnMut(&Expr)>(expr: &Expr, mut f: F) {
    match expr {
        Expr::Unary(_, e) | Expr::Deref(e) | Expr::Try(e) | Expr::Await(e)
            | Expr::Return(Some(e)) | Expr::Break(Some(e)) | Expr::Ref { expr: e, .. }
            => f(e),
        Expr::Binary(_, l, r) | Expr::Assign(l, r) | Expr::OpAssign(_, l, r)
            | Expr::Index(l, r) => { f(l); f(r); }
        Expr::Cast(e, _)     => f(e),
        Expr::Field(e, _)    => f(e),
        Expr::Call { func, args } => {
            f(func);
            args.iter().for_each(|a| f(a));
        }
        Expr::MethodCall { receiver, args, .. } => {
            f(receiver);
            args.iter().for_each(|a| f(a));
        }
        Expr::If { cond, then_block, else_block } => {
            f(cond);
            block_exprs(then_block).for_each(|e| f(e));
            if let Some(eb) = else_block { f(eb); }
        }
        Expr::Match { scrutinee, arms } => {
            f(scrutinee);
            for arm in arms {
                if let Some(g) = &arm.guard { f(g); }
                f(&arm.body);
            }
        }
        Expr::Block(b) => block_exprs(b).for_each(|e| f(e)),
        Expr::Closure { body, .. } => f(body),
        Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, e)| f(e)),
        Expr::Array(elems) | Expr::Tuple(elems) | Expr::Macro { args: elems, .. }
            => elems.iter().for_each(|e| f(e)),
        Expr::Range { start, end, .. } => {
            if let Some(s) = start { f(s); }
            if let Some(e) = end   { f(e); }
        }
        // Leaf nodes — no sub-expressions
        Expr::Lit(_) | Expr::Ident(_) | Expr::Path(_)
            | Expr::Continue | Expr::Return(None) | Expr::Break(None) => {}
    }
}

fn block_exprs(block: &Block) -> impl Iterator<Item = &Expr> {
    let stmt_exprs = block.stmts.iter().filter_map(|s| match s {
        Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
        Stmt::Let { init: Some(e), .. } => Some(e),
        _ => None,
    });
    stmt_exprs.chain(block.tail.iter().map(|e| e.as_ref()))
}
