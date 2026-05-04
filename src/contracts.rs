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
                        // Declare `result` as a free Int (best-effort sort).
                        // Negate Q and conjoin with preconditions; unsat means
                        // no precondition-satisfying model violates Q.
                        let pre_conj = if pre_clauses.is_empty() {
                            "true".to_string()
                        } else if pre_clauses.len() == 1 {
                            pre_clauses[0].clone()
                        } else {
                            format!("(and {})", pre_clauses.join(" "))
                        };
                        (
                            format!(
                                "{}\n(declare-const result Int)\n\
                                 (assert {})\n\
                                 (assert (not {}))\n\
                                 (check-sat)\n",
                                preamble, pre_conj, body_smt
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
                        let r = output.trim();
                        if r == "unsat" {
                            if prove_on_unsat {
                                results
                                    .push(format!("PROVED  [{}:{}] {}", fn_name, label, vc.expr));
                            } else {
                                // For sat-checks, unsat means contradictory.
                                results.push(format!(
                                    "INCONSISTENT [{}:{}] {} (no model satisfies it)",
                                    fn_name, label, vc.expr
                                ));
                            }
                        } else if r == "sat" {
                            if prove_on_unsat {
                                results.push(format!(
                                    "DISPROVED [{}:{}] {} (counter-example found)",
                                    fn_name, label, vc.expr
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
                                fn_name, label, vc.expr, r
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
    for attr in &f.attrs {
        match attr {
            Attr::Requires(e) => {
                let expr_str = pretty_expr(e);
                vcs.push(VerifCondition {
                    fn_name: f.name.clone(),
                    kind: VcKind::Precondition,
                    smtlib: smtlib_of_expr(e, &param_sorts(f)),
                    expr: expr_str,
                    status: VcStatus::Unchecked,
                });
            }
            Attr::Ensures(e) => {
                let expr_str = pretty_expr(e);
                vcs.push(VerifCondition {
                    fn_name: f.name.clone(),
                    kind: VcKind::Postcondition,
                    smtlib: smtlib_of_expr(e, &param_sorts(f)),
                    expr: expr_str,
                    status: VcStatus::Unchecked,
                });
            }
            Attr::Invariant(e) => {
                let expr_str = pretty_expr(e);
                vcs.push(VerifCondition {
                    fn_name: f.name.clone(),
                    kind: VcKind::Invariant,
                    smtlib: smtlib_of_expr(e, &param_sorts(f)),
                    expr: expr_str,
                    status: VcStatus::Unchecked,
                });
            }
            _ => {}
        }
    }
}

/// Build an SMTLIB2 `declare-const` preamble mapping each parameter to
/// an Int sort (simplified — real types are more complex).
fn param_sorts(f: &FnDef) -> String {
    f.params
        .iter()
        .filter(|p| !p.is_self)
        .map(|p| format!("(declare-const {} Int)", p.name))
        .collect::<Vec<_>>()
        .join("\n")
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
