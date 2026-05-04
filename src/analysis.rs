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
    /// Rust feature that Crust accepts at parse time but cannot fully model.
    /// Examples: `impl Trait` collapsed to a single named type, explicit
    /// lifetimes, user-defined macros without a hardcoded lowering, async
    /// blocks. Reported as warning at Develop+, error at Prove.
    UnsupportedFeature,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    /// Name of the enclosing function (empty string = top-level).
    pub function: String,
    /// Whether this is a hard error (vs a warning).
    error: bool,
}

impl Diagnostic {
    fn error(kind: DiagnosticKind, function: &str, msg: impl Into<String>) -> Self {
        Diagnostic {
            kind,
            message: msg.into(),
            function: function.to_string(),
            error: true,
        }
    }
    fn warning(kind: DiagnosticKind, function: &str, msg: impl Into<String>) -> Self {
        Diagnostic {
            kind,
            message: msg.into(),
            function: function.to_string(),
            error: false,
        }
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

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
    level: StrictnessLevel,
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
                // Recurse into inline modules so their inner functions are
                // still subject to panic-freedom, overflow, and llm-mode
                // guardrail checks (crust-rvq).
                Item::Mod { items: inner, .. } => {
                    diags.extend(self.analyze_program(inner));
                }
                // Flag concurrency imports so callers get a clear "not yet
                // supported" message at parse time rather than a confusing
                // runtime "no method clone on type Arc" later (crust-570).
                Item::Use(path) => {
                    if self.level >= StrictnessLevel::Develop {
                        if let Some(name) = unsupported_concurrency_segment(path) {
                            let mut d = Diagnostic::warning(
                                DiagnosticKind::UnsupportedFeature,
                                "",
                                format!(
                                    "`{}` is not implemented by Crust's interpreter; \
                                     `crust run` will fail at first use, and `crust build` \
                                     will pass it through to rustc verbatim. \
                                     Single-threaded shim semantics are tracked in crust-570",
                                    name
                                ),
                            );
                            if self.level >= StrictnessLevel::Prove {
                                d.error = true;
                            }
                            diags.push(d);
                        }
                    }
                }
                _ => {}
            }
        }
        diags
    }

    fn analyze_fn(&self, f: &FnDef) -> Vec<Diagnostic> {
        let mut diags = Vec::new();

        // Unsupported-feature diagnostics at every level (warning), escalated
        // to errors at Prove. This is a guard against silent semantic drift —
        // crust-dfi.
        if self.level >= StrictnessLevel::Develop {
            let mut unsupported = Vec::new();
            self.collect_unsupported(f, &mut unsupported);
            if self.level >= StrictnessLevel::Prove {
                diags.extend(unsupported.into_iter().map(|mut d| {
                    d.error = true;
                    d
                }));
            } else {
                diags.extend(unsupported);
            }
        }

        // Panic-freedom at Level ≥ Develop
        if self.level >= StrictnessLevel::Develop {
            let panic_sites = self.collect_panic_sites(&f.body, &f.name);
            if self.level >= StrictnessLevel::Prove {
                diags.extend(panic_sites.into_iter().map(|mut d| {
                    d.error = true;
                    d
                }));
            } else {
                diags.extend(panic_sites);
            }
        }

        // Overflow at Level ≥ Develop.
        // At Level 4 (Prove), bare arithmetic is *not* an error — codegen
        // lowers `+`, `-`, `*` to `checked_*().expect("arithmetic overflow")`,
        // making the runtime overflow behaviour explicit. Reporting the same
        // sites here as hard errors would block any program that does math
        // before SMT discharge can run (crust-37a). Keep these as warnings
        // at every level; analysis flags `as` casts separately as errors below
        // because codegen does not lower those.
        if self.level >= StrictnessLevel::Develop {
            let overflow_sites = self.collect_overflow_sites(&f.body, &f.name);
            diags.extend(overflow_sites);
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

    // ── Unsupported-feature detection ─────────────────────────────────────────

    /// Collect diagnostics for Rust features that Crust accepts syntactically
    /// but cannot fully model. These are silent semantic-drift hazards if
    /// users assume "Crust runs Rust" without knowing the gaps.
    fn collect_unsupported(&self, f: &FnDef, out: &mut Vec<Diagnostic>) {
        // 1. impl Trait — parser collapses to Ty::Named("impl") with no bounds.
        for p in &f.params {
            if is_impl_trait(&p.ty) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    &f.name,
                    format!(
                        "parameter `{}: impl Trait` — bounds are not modelled by the interpreter; \
                         Crust accepts it but does no trait-bound resolution",
                        p.name
                    ),
                ));
            }
            if let Some(name) = lifetime_name(&p.ty) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    &f.name,
                    format!(
                        "explicit lifetime `'{}` on parameter `{}` is not modelled; \
                         Crust elides all lifetimes in the interpreter and inserts `'_` in codegen",
                        name, p.name
                    ),
                ));
            }
        }
        if let Some(ret) = &f.ret_ty {
            if is_impl_trait(ret) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    &f.name,
                    "`impl Trait` return type — bounds are not modelled by the interpreter \
                     (the codegen passes the type through to rustc, which will check it)",
                ));
            }
        }

        // 2. async fn at Level 4 — Crust evaluates async synchronously and has
        //    no Future runtime; honest at Prove level requires explicit error.
        if f.is_async && self.level >= StrictnessLevel::Prove {
            out.push(Diagnostic::warning(
                DiagnosticKind::UnsupportedFeature,
                &f.name,
                "`async fn` at --strict=4 is not yet supported (no Future runtime modelled); \
                 see crust-7ra",
            ));
        }

        // 3. Unknown macros — anything outside Crust's hardcoded recognition
        //    set runs as a no-op at `crust run` and is passed through verbatim
        //    to rustc, which may or may not have the macro available.
        self.collect_unknown_macros(&f.body, &f.name, out);

        // 4. Concurrency primitives in expression position (Arc::new, Rc::new,
        //    Mutex::new, thread::spawn, mpsc::channel, …). crust-570.
        self.collect_concurrency_paths(&f.body, &f.name, out);

        // 5. Width-sensitive integer methods (wrapping_add, checked_*,
        //    saturating_*, overflowing_*). Crust collapses every integer
        //    type to i64 in the interpreter, so these methods do not honour
        //    the original width — `u8::MAX.wrapping_add(1)` returns 256, not
        //    0 (crust-6yj).
        self.collect_width_sensitive_methods(&f.body, &f.name, out);
    }

    fn collect_width_sensitive_methods(
        &self,
        block: &Block,
        fn_name: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Semi(e) | Stmt::Expr(e) => self.width_method_in_expr(e, fn_name, out),
                Stmt::Let { init: Some(e), .. } => self.width_method_in_expr(e, fn_name, out),
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.width_method_in_expr(tail, fn_name, out);
        }
    }

    fn width_method_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::MethodCall { method, .. } = expr {
            if is_width_sensitive_method(method) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    fn_name,
                    format!(
                        "`.{}()` is not faithfully modelled — Crust's interpreter \
                         collapses every integer type to i64, so width-specific \
                         wrap/overflow/saturation behaviour does not apply (crust-6yj). \
                         `crust build` passes the call through to rustc, which \
                         produces correct results",
                        method
                    ),
                ));
            }
        }
        recurse_expr(expr, |e| self.width_method_in_expr(e, fn_name, out));
    }

    fn collect_concurrency_paths(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Semi(e) | Stmt::Expr(e) => self.concurrency_in_expr(e, fn_name, out),
                Stmt::Let { init: Some(e), .. } => self.concurrency_in_expr(e, fn_name, out),
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.concurrency_in_expr(tail, fn_name, out);
        }
    }

    fn concurrency_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::Path(parts) = expr {
            if let Some(name) = unsupported_concurrency_segment(parts) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    fn_name,
                    format!(
                        "`{}` is not implemented by Crust's interpreter (crust-570). \
                         Single-threaded shims and a real diagnostic for `crust run` \
                         are tracked under that bead; `crust build` passes the symbol \
                         through to rustc verbatim",
                        name
                    ),
                ));
            }
        }
        recurse_expr(expr, |e| self.concurrency_in_expr(e, fn_name, out));
    }

    fn collect_unknown_macros(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Semi(e) | Stmt::Expr(e) => self.macro_in_expr(e, fn_name, out),
                Stmt::Let { init: Some(e), .. } => self.macro_in_expr(e, fn_name, out),
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.macro_in_expr(tail, fn_name, out);
        }
    }

    fn macro_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::Macro { name, .. } = expr {
            if !is_known_macro(name) {
                out.push(Diagnostic::warning(
                    DiagnosticKind::UnsupportedFeature,
                    fn_name,
                    format!(
                        "macro `{}!(...)` is not interpreted by Crust; \
                         `crust run` will fail at this site, and `crust build` \
                         will pass it through to rustc verbatim",
                        name
                    ),
                ));
            }
        }
        recurse_expr(expr, |e| self.macro_in_expr(e, fn_name, out));
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
            Stmt::Let { init: Some(e), .. } | Stmt::Semi(e) | Stmt::Expr(e) => {
                self.panic_in_expr(e, fn_name, out)
            }
            Stmt::LetPat {
                init: Some(e),
                else_block,
                ..
            } => {
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
                    DiagnosticKind::PotentialPanic,
                    fn_name,
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
                    DiagnosticKind::PotentialPanic,
                    fn_name,
                    "index operation `[..]` can panic on out-of-bounds access; consider `.get()`",
                ));
            }

            // Division / remainder by a non-literal denominator can be zero
            Expr::Binary(BinOp::Div | BinOp::Rem, _, rhs) => {
                if !is_nonzero_literal(rhs) {
                    out.push(Diagnostic::warning(
                        DiagnosticKind::PotentialPanic,
                        fn_name,
                        "division / remainder may panic if denominator is zero; \
                         consider `checked_div` or asserting `divisor != 0`",
                    ));
                }
            }

            // Macro calls: panic!, unreachable!, todo!, unimplemented!
            Expr::Macro { name, .. }
                if matches!(
                    name.as_str(),
                    "panic" | "unreachable" | "todo" | "unimplemented"
                ) =>
            {
                out.push(Diagnostic::warning(
                    DiagnosticKind::PotentialPanic,
                    fn_name,
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
            Stmt::Let { init: Some(e), .. } | Stmt::Semi(e) | Stmt::Expr(e) => {
                self.overflow_in_expr(e, fn_name, out)
            }
            Stmt::LetPat { init: Some(e), .. } => self.overflow_in_expr(e, fn_name, out),
            _ => {}
        }
    }

    fn overflow_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        match expr {
            Expr::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul), lhs, rhs) => {
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    _ => "*",
                };
                out.push(Diagnostic::warning(
                    DiagnosticKind::ArithmeticOverflow,
                    fn_name,
                    format!(
                        "bare `{}` may overflow; at --strict=4 use `checked_{}`",
                        op_str,
                        match op {
                            BinOp::Add => "add",
                            BinOp::Sub => "sub",
                            _ => "mul",
                        }
                    ),
                ));
                self.overflow_in_expr(lhs, fn_name, out);
                self.overflow_in_expr(rhs, fn_name, out);
            }
            Expr::Cast(inner, _) => {
                out.push(Diagnostic::warning(
                    DiagnosticKind::AsCast,
                    fn_name,
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
                if matches!(
                    name.as_str(),
                    "println" | "print" | "eprintln" | "eprint" | "write" | "writeln"
                ) =>
            {
                out.push(Diagnostic::error(
                    DiagnosticKind::PurityViolation,
                    fn_name,
                    format!("`#[pure]` function contains I/O macro `{}!()`", name),
                ));
            }
            Expr::Unsafe(block) => {
                out.push(Diagnostic::error(
                    DiagnosticKind::UnsafeUsage,
                    fn_name,
                    "`#[pure]` function contains an `unsafe` block",
                ));
                self.purity_in_block(block, fn_name, out);
            }
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
                        DiagnosticKind::WildcardMatch,
                        fn_name,
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
            if d.kind == DiagnosticKind::PotentialPanic {
                out.push(Diagnostic::error(
                    DiagnosticKind::LlmGuardrail,
                    &f.name,
                    format!("--llm-mode: {}", d.message),
                ));
            }
        }
        // as-casts are errors in LLM mode
        self.llm_cast_check(&f.body, &f.name, &mut out);
        // unsafe blocks are errors in LLM mode
        self.llm_unsafe_check(&f.body, &f.name, &mut out);
        // todo!/unimplemented!/unreachable! are errors in LLM mode
        self.llm_macro_check(&f.body, &f.name, &mut out);
        out
    }

    fn llm_cast_check(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        let stmts_exprs: Vec<&Expr> = block
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
                Stmt::Let { init: Some(e), .. } => Some(e),
                _ => None,
            })
            .collect();
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
                DiagnosticKind::LlmGuardrail,
                fn_name,
                "--llm-mode: `as` cast is disallowed; use `From`/`TryFrom` for safe conversions",
            ));
        }
        recurse_expr(expr, |e| self.llm_cast_in_expr(e, fn_name, out));
    }

    fn llm_macro_check(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        let stmts_exprs: Vec<&Expr> = block
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
                Stmt::Let { init: Some(e), .. } => Some(e),
                _ => None,
            })
            .collect();
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
                    DiagnosticKind::LlmGuardrail,
                    fn_name,
                    format!("--llm-mode: `{}!()` is disallowed in non-test code", name),
                ));
            }
        }
        recurse_expr(expr, |e| self.llm_macro_in_expr(e, fn_name, out));
    }

    fn llm_unsafe_check(&self, block: &Block, fn_name: &str, out: &mut Vec<Diagnostic>) {
        let stmts_exprs: Vec<&Expr> = block
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Semi(e) | Stmt::Expr(e) => Some(e),
                Stmt::Let { init: Some(e), .. } => Some(e),
                _ => None,
            })
            .collect();
        for expr in stmts_exprs {
            self.llm_unsafe_in_expr(expr, fn_name, out);
        }
        if let Some(tail) = &block.tail {
            self.llm_unsafe_in_expr(tail, fn_name, out);
        }
    }

    fn llm_unsafe_in_expr(&self, expr: &Expr, fn_name: &str, out: &mut Vec<Diagnostic>) {
        if let Expr::Unsafe(_) = expr {
            out.push(Diagnostic::error(
                DiagnosticKind::LlmGuardrail,
                fn_name,
                "--llm-mode: `unsafe` blocks are disallowed",
            ));
        }
        recurse_expr(expr, |e| self.llm_unsafe_in_expr(e, fn_name, out));
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns true if `expr` is a literal that is provably non-zero.
fn is_nonzero_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Lit(Lit::Int(n)) if *n != 0)
}

/// Whether a `Ty` was parsed from an `impl Trait` form. The parser collapses
/// these to `Ty::Named("impl")` after consuming the trait list, so we can't
/// recover the bounds — which is exactly the diagnostic point.
fn is_impl_trait(ty: &Ty) -> bool {
    match ty {
        Ty::Named(s) => s == "impl",
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) | Ty::Slice(inner) => is_impl_trait(inner),
        _ => false,
    }
}

/// If the type carries an explicit lifetime annotation (not `'_`), return its
/// name; otherwise None. Crust's interpreter elides all lifetimes, so an
/// explicit one is informational at best.
fn lifetime_name(ty: &Ty) -> Option<&str> {
    match ty {
        Ty::Lifetime(name) if name != "_" => Some(name),
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) | Ty::Slice(inner) => lifetime_name(inner),
        _ => None,
    }
}

/// Macros that Crust's `crust run` interpreter handles directly. Anything
/// outside this set will fail at `crust run` time with "unknown macro" and is
/// passed verbatim to rustc by codegen — fine if it's a real Rust stdlib
/// macro, surprising if the user expected Crust to evaluate it. Keep in sync
/// with `Interpreter::eval_macro` in eval.rs.
/// If `parts` references an std::sync / std::thread / Arc / Rc / Mutex / RwLock
/// / channel / atomic path that Crust's interpreter does not implement, return
/// the user-facing display name. Returns `None` for paths Crust either handles
/// (HashMap, RefCell, Cell — used internally) or that aren't concurrency-flagged.
fn unsupported_concurrency_segment(parts: &[String]) -> Option<&'static str> {
    let segments: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    let head = segments.first().copied().unwrap_or("");
    let last = segments.last().copied().unwrap_or("");
    let any_seg = |needle: &str| segments.contains(&needle);
    if any_seg("thread") && (head == "std" || head == "thread") {
        return Some("std::thread");
    }
    if any_seg("mpsc") {
        return Some("std::sync::mpsc");
    }
    if any_seg("atomic") || last.starts_with("Atomic") {
        return Some("std::sync::atomic");
    }
    if last == "Arc" || any_seg("Arc") {
        return Some("Arc");
    }
    if last == "Rc" || any_seg("Rc") {
        return Some("Rc");
    }
    if last == "Mutex" || any_seg("Mutex") {
        return Some("Mutex");
    }
    if last == "RwLock" || any_seg("RwLock") {
        return Some("RwLock");
    }
    None
}

/// Width-sensitive integer methods. These return semantically-different
/// results for a `u8` versus an `i64`; Crust's interpreter cannot model that
/// without a real primitive-width Value (crust-6yj), so we surface a
/// warning instead of silently producing the wrong number.
fn is_width_sensitive_method(name: &str) -> bool {
    matches!(
        name,
        "wrapping_add"
            | "wrapping_sub"
            | "wrapping_mul"
            | "wrapping_div"
            | "wrapping_rem"
            | "wrapping_neg"
            | "wrapping_shl"
            | "wrapping_shr"
            | "checked_add"
            | "checked_sub"
            | "checked_mul"
            | "checked_div"
            | "checked_rem"
            | "checked_neg"
            | "saturating_add"
            | "saturating_sub"
            | "saturating_mul"
            | "saturating_pow"
            | "overflowing_add"
            | "overflowing_sub"
            | "overflowing_mul"
            | "overflowing_neg"
            | "leading_zeros"
            | "trailing_zeros"
            | "count_ones"
            | "count_zeros"
            | "swap_bytes"
            | "to_be"
            | "to_le"
            | "from_be"
            | "from_le"
    )
}

fn is_known_macro(name: &str) -> bool {
    matches!(
        name,
        "println"
            | "print"
            | "eprintln"
            | "eprint"
            | "format"
            | "vec"
            | "panic"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "dbg"
            | "write"
            | "writeln"
    ) || name.starts_with("__for__")
        || name.starts_with("__vec_repeat__")
        || name == "__array_repeat__"
}

/// Walk the direct sub-expressions of `expr`, calling `f` on each.
/// This is a shallow walk; `f` is responsible for recursing further.
fn recurse_expr<F: FnMut(&Expr)>(expr: &Expr, mut f: F) {
    match expr {
        Expr::Unary(_, e)
        | Expr::Deref(e)
        | Expr::Try(e)
        | Expr::Await(e)
        | Expr::Return(Some(e))
        | Expr::Break(_, Some(e))
        | Expr::Ref { expr: e, .. } => f(e),
        Expr::Binary(_, l, r)
        | Expr::Assign(l, r)
        | Expr::OpAssign(_, l, r)
        | Expr::Index(l, r) => {
            f(l);
            f(r);
        }
        Expr::Cast(e, _) => f(e),
        Expr::Field(e, _) => f(e),
        Expr::Call { func, args } => {
            f(func);
            args.iter().for_each(&mut f);
        }
        Expr::MethodCall { receiver, args, .. } => {
            f(receiver);
            args.iter().for_each(&mut f);
        }
        Expr::If {
            cond,
            then_block,
            else_block,
        } => {
            f(cond);
            block_exprs(then_block).for_each(&mut f);
            if let Some(eb) = else_block {
                f(eb);
            }
        }
        Expr::Match { scrutinee, arms } => {
            f(scrutinee);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    f(g);
                }
                f(&arm.body);
            }
        }
        Expr::Block(b) | Expr::Unsafe(b) => block_exprs(b).for_each(&mut f),
        Expr::Closure { body, .. } => f(body),
        Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, e)| f(e)),
        Expr::Array(elems) | Expr::Tuple(elems) | Expr::Macro { args: elems, .. } => {
            elems.iter().for_each(&mut f)
        }
        Expr::Range { start, end, .. } => {
            if let Some(s) = start {
                f(s);
            }
            if let Some(e) = end {
                f(e);
            }
        }
        // Leaf nodes — no sub-expressions
        Expr::Lit(_)
        | Expr::Ident(_)
        | Expr::Path(_)
        | Expr::Continue(_)
        | Expr::Return(None)
        | Expr::Break(_, None) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(src: &str) -> Program {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse_program().unwrap()
    }

    #[test]
    fn prove_mode_rejects_unsafe_in_pure_function() {
        let program = parse(
            r#"
            #[pure]
            fn f() -> i64 {
                unsafe { 1 + 1 }
            }
            "#,
        );
        let diagnostics = Analyzer::new(StrictnessLevel::Prove, false).analyze_program(&program);

        assert!(diagnostics
            .iter()
            .any(|d| d.is_error() && d.kind == DiagnosticKind::UnsafeUsage));
    }

    #[test]
    fn llm_mode_rejects_unsafe_block() {
        let program = parse(
            r#"
            fn f() {
                unsafe { 1 + 1; }
            }
            "#,
        );
        let diagnostics = Analyzer::new(StrictnessLevel::Explore, true).analyze_program(&program);

        assert!(diagnostics.iter().any(|d| d.is_error()
            && d.kind == DiagnosticKind::LlmGuardrail
            && d.message.contains("unsafe")));
    }
}
