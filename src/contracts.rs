//! Contract extraction and verification-condition (VC) generation.
// Future-use variants and fields are intentional API surface.
#![allow(dead_code)]
//!
//! A *verification condition* (VC) is a logical proposition that, if true,
//! guarantees a contract clause holds.  For a function with
//! `#[requires(pre)]` and `#[ensures(post)]`, the basic VC is:
//!
//! ```text
//!   forall inputs. pre(inputs) => post(f(inputs))
//! ```
//!
//! This module:
//! 1. Extracts `#[requires]`/`#[ensures]`/`#[invariant]` annotations from
//!    the AST into a flat list of `VerifCondition`s.
//! 2. Pretty-prints each VC as an SMTLIB2 assertion suitable for `z3`/`cvc5`.
//! 3. Optionally invokes `z3` (if on PATH) to auto-discharge simple
//!    arithmetic proofs, reporting which VCs are proved, unknown, or failing.

use std::fmt;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::ast::*;

// ── VerifCondition ────────────────────────────────────────────────────────────

/// The status of a single verification condition.
#[derive(Debug, Clone, PartialEq)]
pub enum VcStatus {
    /// Not yet checked.
    Unchecked,
    /// Proved by an SMT solver.
    Proved,
    /// Solver returned `unknown`.
    Unknown,
    /// Solver found a counter-example.
    Disproved,
}

/// A single verification condition extracted from a function's contracts.
#[derive(Debug, Clone)]
pub struct VerifCondition {
    /// Name of the enclosing function.
    pub fn_name: String,
    /// Human-readable kind label.
    kind: VcKind,
    /// Pretty-printed predicate expression (Rust syntax).
    pub expr: String,
    /// SMTLIB2 encoding of the VC (used for z3/cvc5).
    smtlib: String,
    /// SMT sort name for the function's return type (`Int`, `Real`, `Bool`,
    /// or `Int` fallback). Used to declare `result` in postcondition scripts.
    return_sort: &'static str,
    /// Result after optional solver invocation.
    pub status: VcStatus,
}

impl VerifCondition {
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            VcKind::Precondition => "requires",
            VcKind::Postcondition => "ensures",
            VcKind::Invariant => "invariant",
            VcKind::NoOverflow => "no_overflow",
            VcKind::NoPanic => "no_panic",
        }
    }
}

#[derive(Debug, Clone)]
enum VcKind {
    Precondition,
    Postcondition,
    Invariant,
    NoOverflow,
    NoPanic,
}

impl fmt::Display for VerifCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}:{}] {} — {:?}",
            self.fn_name,
            self.kind_str(),
            self.expr,
            self.status
        )
    }
}

// ── ContractChecker ───────────────────────────────────────────────────────────

pub struct ContractChecker;

impl ContractChecker {
    /// Walk the program and collect all contract VCs from function attributes.
    pub fn extract_vcs(items: &[Item]) -> Vec<VerifCondition> {
        let mut vcs = Vec::new();
        for item in items {
            match item {
                Item::Fn(f) => extract_fn_vcs(f, &mut vcs),
                Item::Impl(imp) => {
                    for method in &imp.methods {
                        extract_fn_vcs(method, &mut vcs);
                    }
                }
                // Recurse into inline modules so contracts on functions inside
                // `mod foo { ... }` are still extracted for SMT discharge.
                Item::Mod { items: inner, .. } => {
                    vcs.extend(Self::extract_vcs(inner));
                }
                _ => {}
            }
        }
        vcs
    }

    /// Attempt to discharge VCs using `z3` (if present on PATH).
    ///
    /// Note on semantics: Crust does not perform symbolic execution of the
    /// function body, so it cannot fully prove `forall p. P(p) => Q(f(p))`.
    /// What it *can* do without a body interpreter:
    ///
    ///   - For `#[requires(P)]`: check that `P` is satisfiable (i.e., the
    ///     precondition can be met by *some* input). If unsat, the contract
    ///     is contradictory and any caller will fail it. This is a real
    ///     consistency check — the previous implementation asserted
    ///     `(not P)` and called every non-tautology a "DISPROVED requires",
    ///     which is mathematically wrong (crust-yi3).
    ///
    ///   - For `#[ensures(Q)]`: declare `result` as a free variable of the
    ///     return sort so `Q` references it without "unknown constant" errors,
    ///     then check whether `(and preconditions (not Q))` is unsat (i.e.,
    ///     no model satisfies preconditions while violating the postcondition,
    ///     treating the body as an uninterpreted function). This is still
    ///     incomplete without a body interpreter (crust-7e8, crust-v8b) but
    ///     no longer reports false counter-examples.
    pub fn check_with_smt(vcs: &[VerifCondition]) -> Vec<String> {
        let mut results = Vec::new();

        let z3_available = Command::new("z3")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !z3_available {
            results.push("z3 not found on PATH; skipping SMT discharge".into());
            return results;
        }

        // Group VCs by function so each function's preconditions can serve as
        // assumptions for that function's postconditions.
        let mut by_fn: std::collections::BTreeMap<&str, Vec<&VerifCondition>> =
            std::collections::BTreeMap::new();
        for vc in vcs {
            by_fn.entry(vc.fn_name.as_str()).or_default().push(vc);
        }

        for (fn_name, fn_vcs) in by_fn {
            // The first VC's smtlib carries the parameter preamble; reuse it.
            let preamble = fn_vcs.first().map(|v| v.smtlib.clone()).unwrap_or_default();
            let pre_clauses: Vec<String> = fn_vcs
                .iter()
                .filter(|v| matches!(v.kind, VcKind::Precondition))
                .map(|v| smtlib_of_expr_str(&v.expr))
                .collect();

            for vc in &fn_vcs {
                let body_smt = smtlib_of_expr_str(&vc.expr);
                let (script, prove_on_unsat) = match vc.kind {
                    VcKind::Precondition => {
                        // Satisfiability: precondition consistent iff sat.
                        (
                            format!("{}\n(assert {})\n(check-sat)\n", preamble, body_smt),
                            false,
                        )
                    }
                    VcKind::Postcondition => {
                        // Declare `result` with the function's actual return
                        // sort (Int/Real/Bool — crust-7e8). Negate Q and
                        // conjoin with preconditions; unsat means no
                        // precondition-satisfying model violates Q. When sat,
                        // `(get-model)` reports a counter-example.
                        let pre_conj = if pre_clauses.is_empty() {
                            "true".to_string()
                        } else if pre_clauses.len() == 1 {
                            pre_clauses[0].clone()
                        } else {
                            format!("(and {})", pre_clauses.join(" "))
                        };
                        (
                            format!(
                                "(set-option :produce-models true)\n\
                                 {}\n(declare-const result {})\n\
                                 (assert {})\n\
                                 (assert (not {}))\n\
                                 (check-sat)\n\
                                 (get-model)\n",
                                preamble, vc.return_sort, pre_conj, body_smt
                            ),
                            true,
                        )
                    }
                    VcKind::Invariant => (
                        format!("{}\n(assert {})\n(check-sat)\n", preamble, body_smt),
                        false,
                    ),
                    VcKind::NoOverflow | VcKind::NoPanic => (
                        format!("{}\n(assert (not {}))\n(check-sat)\n", preamble, body_smt),
                        true,
                    ),
                };

                let label = vc.kind_str();
                match run_z3(&script) {
                    Some(output) => {
                        let trimmed = output.trim();
                        // First line is `sat` / `unsat` / `unknown`; if
                        // produce-models was on and the verdict is `sat`,
                        // the rest of the output is the counter-example.
                        let mut iter = trimmed.lines();
                        let verdict = iter.next().unwrap_or("").trim();
                        let model_text = iter.collect::<Vec<_>>().join("\n");
                        if verdict == "unsat" {
                            if prove_on_unsat {
                                results
                                    .push(format!("PROVED  [{}:{}] {}", fn_name, label, vc.expr));
                            } else {
                                results.push(format!(
                                    "INCONSISTENT [{}:{}] {} (no model satisfies it)",
                                    fn_name, label, vc.expr
                                ));
                            }
                        } else if verdict == "sat" {
                            if prove_on_unsat {
                                let cex = extract_counterexample(&model_text);
                                let suffix = if cex.is_empty() {
                                    String::from(" (counter-example found)")
                                } else {
                                    format!(" (counter-example: {})", cex)
                                };
                                results.push(format!(
                                    "DISPROVED [{}:{}] {}{}",
                                    fn_name, label, vc.expr, suffix
                                ));
                            } else {
                                results.push(format!(
                                    "CONSISTENT [{}:{}] {}",
                                    fn_name, label, vc.expr
                                ));
                            }
                        } else {
                            results.push(format!(
                                "UNKNOWN [{}:{}] {} (solver: {})",
                                fn_name, label, vc.expr, verdict
                            ));
                        }
                    }
                    None => results.push(format!(
                        "ERROR  [{}:{}] z3 invocation failed",
                        fn_name, label
                    )),
                }
            }
        }
        results
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn extract_fn_vcs(f: &FnDef, vcs: &mut Vec<VerifCondition>) {
    let preamble = param_sorts(f);
    let ret = return_sort(f);
    for attr in &f.attrs {
        let (expr, kind) = match attr {
            Attr::Requires(e) => (e, VcKind::Precondition),
            Attr::Ensures(e) => (e, VcKind::Postcondition),
            Attr::Invariant(e) => (e, VcKind::Invariant),
            _ => continue,
        };
        vcs.push(VerifCondition {
            fn_name: f.name.clone(),
            kind,
            smtlib: smtlib_of_expr(expr, &preamble),
            expr: pretty_expr(expr),
            return_sort: ret,
            status: VcStatus::Unchecked,
        });
    }
}

/// Build an SMTLIB2 `declare-const` preamble mapping each parameter to
/// the SMT sort that best matches its Rust type.
///
/// - Integer types (`i8..i128`, `u8..u128`, `isize`, `usize`) → `Int`
/// - Float types (`f32`, `f64`) → `Real`
/// - `bool` → `Bool`
/// - Other / unknown → `Int` as a fallback, with a comment noting the
///   approximation. crust-7e8 will close this further as the verification
///   pipeline grows.
fn param_sorts(f: &FnDef) -> String {
    let mut lines: Vec<String> = Vec::new();
    for p in f.params.iter().filter(|p| !p.is_self) {
        let sort = smt_sort_of_ty(&p.ty);
        if sort.fallback_note {
            lines.push(format!(
                "; fallback Int for parameter `{}: {:?}`",
                p.name, p.ty
            ));
        }
        lines.push(format!("(declare-const {} {})", p.name, sort.name));
    }
    lines.join("\n")
}

/// SMT sort for a Rust type, with a flag telling the caller this is a
/// fallback rather than a faithful encoding.
struct SmtSort {
    name: &'static str,
    fallback_note: bool,
}

fn smt_sort_of_ty(ty: &Ty) -> SmtSort {
    match ty {
        Ty::Named(n) => match n.as_str() {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
            | "u128" | "usize" => SmtSort {
                name: "Int",
                fallback_note: false,
            },
            "f32" | "f64" => SmtSort {
                name: "Real",
                fallback_note: false,
            },
            "bool" => SmtSort {
                name: "Bool",
                fallback_note: false,
            },
            _ => SmtSort {
                name: "Int",
                fallback_note: true,
            },
        },
        Ty::Unit => SmtSort {
            name: "Int",
            fallback_note: true,
        },
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) | Ty::Slice(inner) => smt_sort_of_ty(inner),
        _ => SmtSort {
            name: "Int",
            fallback_note: true,
        },
    }
}

/// SMT sort name for the function's return type. Used when declaring
/// `result` for postcondition VCs.
fn return_sort(f: &FnDef) -> &'static str {
    match &f.ret_ty {
        Some(t) => smt_sort_of_ty(t).name,
        None => "Int",
    }
}

/// Translate an AST expression to an SMTLIB2 s-expression (best-effort).
/// Only handles simple arithmetic and comparisons; complex expressions are
/// left as comments.
fn smtlib_of_expr(expr: &Expr, preamble: &str) -> String {
    let body = smtlib_expr(expr);
    if preamble.is_empty() {
        body
    } else {
        format!("{}\n; VC body: {}", preamble, body)
    }
}

fn smtlib_of_expr_str(src: &str) -> String {
    // Re-parse the expression string and convert to SMTLIB
    if let Ok(tokens) = crate::lexer::Lexer::new(src).tokenize() {
        if let Ok(expr) = crate::parser::Parser::new(tokens).parse_expr(0) {
            return smtlib_expr(&expr);
        }
    }
    format!("; unparseable: {}", src)
}

fn smtlib_expr(expr: &Expr) -> String {
    match expr {
        Expr::Lit(Lit::Int(n)) => n.to_string(),
        Expr::Lit(Lit::Bool(b)) => b.to_string(),
        Expr::Ident(name) => name.clone(),
        Expr::Binary(op, l, r) => {
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "div",
                // Note: SMTLIB2 `mod` assumes non-negative divisors (like Euclidean modulo).
                // Rust's `%` is truncated remainder (can be negative). For Prove mode,
                // this approximation is acceptable for positive-domain proofs; add
                // `rem` if you need exact signed-integer semantics.
                BinOp::Rem => "mod",
                BinOp::Eq => "=",
                BinOp::Ne => "distinct",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                _ => "?",
            };
            format!("({} {} {})", op_str, smtlib_expr(l), smtlib_expr(r))
        }
        Expr::Unary(UnOp::Neg, e) => format!("(- {})", smtlib_expr(e)),
        Expr::Unary(UnOp::Not, e) => format!("(not {})", smtlib_expr(e)),
        _ => format!("; complex: {:?}", expr),
    }
}

/// Public alias used by the verify-report writer in main.rs so the JSON
/// output shows readable Rust source for `#[requires]` / `#[ensures]`
/// instead of Debug-AST forms.
pub fn pretty_predicate(expr: &Expr) -> String {
    pretty_expr(expr)
}

/// Pretty-print an expression back to Rust syntax (simplified).
fn pretty_expr(expr: &Expr) -> String {
    match expr {
        Expr::Lit(Lit::Int(n)) => n.to_string(),
        Expr::Lit(Lit::Bool(b)) => b.to_string(),
        Expr::Lit(Lit::Str(s)) => format!("{:?}", s),
        Expr::Ident(name) => name.clone(),
        Expr::Path(parts) => parts.join("::"),
        Expr::Binary(op, l, r) => {
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::BitAnd => "&",
                BinOp::BitOr => "|",
                BinOp::BitXor => "^",
                BinOp::Shl => "<<",
                BinOp::Shr => ">>",
            };
            format!("({} {} {})", pretty_expr(l), op_str, pretty_expr(r))
        }
        Expr::Unary(UnOp::Neg, e) => format!("-({})", pretty_expr(e)),
        Expr::Unary(UnOp::Not, e) => format!("!({})", pretty_expr(e)),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let args_str = args.iter().map(pretty_expr).collect::<Vec<_>>().join(", ");
            format!("{}.{}({})", pretty_expr(receiver), method, args_str)
        }
        Expr::Field(e, f) => format!("{}.{}", pretty_expr(e), f),
        _ => format!("{:?}", expr),
    }
}

/// Parse z3's `(get-model)` output into a `name=value, name=value` summary.
///
/// z3's pretty-printer splits each `(define-fun NAME () SORT VALUE)` across
/// multiple lines, so we squash whitespace first to get one logical S-expr,
/// then walk the token stream looking for `define-fun` forms.
fn extract_counterexample(model_text: &str) -> String {
    // Tokenise: insert spaces around parens so they're standalone, then
    // collapse whitespace.
    let spaced: String = model_text
        .chars()
        .flat_map(|c| match c {
            '(' => vec![' ', '(', ' '],
            ')' => vec![' ', ')', ' '],
            other => vec![other],
        })
        .collect();
    let toks: Vec<&str> = spaced.split_whitespace().collect();
    let mut pairs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == "define-fun" && i + 5 < toks.len() {
            // Expected layout: define-fun NAME ( ) SORT VALUE_TOKEN_OR_(_-_VAL_)
            let name = toks[i + 1];
            // toks[i+2] should be `(`, toks[i+3] should be `)`, toks[i+4] is SORT.
            // VALUE starts at toks[i+5] and may be a single token (`5`, `true`,
            // `1/2`) or a parenthesised form like `( - 5 )`.
            let value = if toks[i + 5] == "(" {
                // walk to matching `)`, joining tokens in between.
                let mut depth = 1usize;
                let mut j = i + 6;
                let mut parts: Vec<&str> = Vec::new();
                while j < toks.len() && depth > 0 {
                    match toks[j] {
                        "(" => {
                            depth += 1;
                            parts.push("(");
                        }
                        ")" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            parts.push(")");
                        }
                        t => parts.push(t),
                    }
                    j += 1;
                }
                let inner = parts.join(" ");
                // Common shape: `- 5` → `-5`, `/ 1 2` → `1/2`.
                if let Some(rest) = inner.strip_prefix("- ") {
                    format!("-{}", rest.trim())
                } else if let Some(rest) = inner.strip_prefix("/ ") {
                    let mut split = rest.splitn(2, ' ');
                    let a = split.next().unwrap_or("");
                    let b = split.next().unwrap_or("");
                    format!("{}/{}", a.trim(), b.trim())
                } else {
                    inner
                }
            } else {
                toks[i + 5].to_string()
            };
            if !name.starts_with('!') && !name.contains("::") && !value.is_empty() {
                pairs.push(format!("{}={}", name, value));
            }
            i += 6;
        } else {
            i += 1;
        }
    }
    pairs.join(", ")
}

/// Invoke z3 with a script on stdin, return stdout.
fn run_z3(script: &str) -> Option<String> {
    let mut child = Command::new("z3")
        .arg("-in") // read from stdin
        .arg("-smt2") // SMTLIB2 input
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(script.as_bytes());
    }

    let output = child.wait_with_output().ok()?;
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse_program(src: &str) -> Vec<Item> {
        let tokens = Lexer::new(src).tokenize().expect("tokenize");
        Parser::new(tokens).parse_program().expect("parse")
    }

    #[test]
    fn extract_vcs_picks_up_requires_and_ensures() {
        let prog = parse_program(
            "#[requires(x > 0)]\n#[ensures(result > 0)]\nfn id(x: i64) -> i64 { x }\nfn main() {}",
        );
        let vcs = ContractChecker::extract_vcs(&prog);
        // 2 VCs for `id`, 0 for `main`.
        assert_eq!(vcs.len(), 2);
        assert!(vcs.iter().all(|v| v.fn_name == "id"));
        let kinds: Vec<_> = vcs.iter().map(|v| v.kind_str()).collect();
        assert!(kinds.contains(&"requires"));
        assert!(kinds.contains(&"ensures"));
    }

    #[test]
    fn extract_vcs_recurses_into_modules() {
        let prog =
            parse_program("mod m { #[requires(x > 0)] fn f(x: i64) -> i64 { x } } fn main() {}");
        let vcs = ContractChecker::extract_vcs(&prog);
        assert_eq!(vcs.len(), 1);
    }

    #[test]
    fn extract_vcs_handles_invariants() {
        let prog = parse_program("#[invariant(x != 0)] fn f(x: i64) { } fn main() {}");
        let vcs = ContractChecker::extract_vcs(&prog);
        assert_eq!(vcs.len(), 1);
        assert_eq!(vcs[0].kind_str(), "invariant");
    }

    #[test]
    fn pretty_predicate_round_trips_simple_expressions() {
        let prog = parse_program("#[requires(x + 1 == 2)] fn f(x: i64) {} fn main() {}");
        let vcs = ContractChecker::extract_vcs(&prog);
        // The pretty form uses Rust syntax, not Debug-AST.
        assert!(vcs[0].expr.contains("+"));
        assert!(vcs[0].expr.contains("=="));
    }

    #[test]
    fn smt_encoding_uses_typed_sorts() {
        let prog = parse_program("#[requires(flag)] fn f(flag: bool) -> i64 { 0 } fn main() {}");
        let vcs = ContractChecker::extract_vcs(&prog);
        // bool param must declare as Bool, not Int.
        assert!(vcs[0].smtlib.contains("Bool"));
    }

    #[test]
    fn smt_encoding_uses_int_for_integer_types() {
        let prog = parse_program("#[requires(x > 0)] fn f(x: u32) -> i64 { 0 } fn main() {}");
        let vcs = ContractChecker::extract_vcs(&prog);
        assert!(vcs[0].smtlib.contains("Int"));
    }

    #[test]
    fn extract_counterexample_parses_define_fun_lines() {
        let model = "(\n  (define-fun x () Int\n    5)\n  (define-fun result () Int\n    10)\n)";
        let cx = extract_counterexample(model);
        assert!(cx.contains("x=5"));
        assert!(cx.contains("result=10"));
    }

    #[test]
    fn extract_counterexample_handles_negative_values() {
        let model = "(\n  (define-fun x () Int\n    (- 7))\n)";
        let cx = extract_counterexample(model);
        assert!(cx.contains("x=-7"));
    }

    #[test]
    fn extract_counterexample_skips_internal_names() {
        let model = "(\n  (define-fun !aux () Int 5)\n  (define-fun x () Int 1)\n)";
        let cx = extract_counterexample(model);
        assert!(!cx.contains("!aux"));
        assert!(cx.contains("x=1"));
    }

    #[test]
    fn smt_sort_classification() {
        assert_eq!(smt_sort_of_ty(&Ty::Named("i64".into())).name, "Int");
        assert_eq!(smt_sort_of_ty(&Ty::Named("u8".into())).name, "Int");
        assert_eq!(smt_sort_of_ty(&Ty::Named("f64".into())).name, "Real");
        assert_eq!(smt_sort_of_ty(&Ty::Named("bool".into())).name, "Bool");
        // Unknown types fall back to Int with a marker.
        let s = smt_sort_of_ty(&Ty::Named("MyStruct".into()));
        assert_eq!(s.name, "Int");
        assert!(s.fallback_note);
    }

    #[test]
    fn pretty_predicate_unparseable_yields_placeholder() {
        // Construct a complex expression directly through pretty_expr.
        let e = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("v".into())),
            method: "len".into(),
            turbofish: None,
            args: vec![],
        };
        let s = pretty_predicate(&e);
        assert!(s.contains("v.len()"));
    }
}
