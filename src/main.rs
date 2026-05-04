mod analysis;
mod ast;
mod codegen;
mod contracts;
mod env;
mod error;
mod eval;
mod lexer;
mod parser;
mod proofgen;
mod repl;
mod stdlib;
mod strictness;
mod types;
mod value;

use std::fs;
use std::path::Path;
use std::process::Command;

use codegen::Codegen;
use error::{CrustError, Result};
use strictness::StrictnessLevel;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str());
    match subcommand {
        Some("run") => run_cmd(&args[2..]),
        Some("build") => build_cmd(&args[2..]),
        Some("verify") => verify_cmd(&args[2..]),
        Some("repl") => repl::run(),
        Some("--help" | "-h" | "help") => help(),
        Some("--version" | "-V" | "version") => version(),
        Some(other) => {
            eprintln!("crust: unknown subcommand '{}'\n", other);
            help();
            std::process::exit(1);
        }
        None => {
            help();
            std::process::exit(1);
        }
    }
}

// ── run subcommand ────────────────────────────────────────────────────────────

fn run_cmd(args: &[String]) {
    let path = match args.first() {
        Some(p) => p,
        None => {
            eprintln!("usage: crust run <file.crust>");
            std::process::exit(1);
        }
    };
    if let Err(e) = run_file(path) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run_file(path: &str) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let tokens = lexer::Lexer::new(&source).tokenize()?;
    let program = parser::Parser::new(tokens).parse_program()?;
    let mut interp = eval::Interpreter::new();
    interp.run(program)
}

// ── build subcommand ──────────────────────────────────────────────────────────

/// Options parsed from `crust build` arguments.
struct BuildOptions {
    source_file: String,
    output: String,
    emit_rs: bool,
    emit_proof: bool,
    level: StrictnessLevel,
    llm_mode: bool,
    verify: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            source_file: String::new(),
            output: "a.out".into(),
            emit_rs: false,
            emit_proof: false,
            level: StrictnessLevel::Explore,
            llm_mode: false,
            verify: false,
        }
    }
}

fn parse_build_options(args: &[String]) -> Result<BuildOptions> {
    let mut opts = BuildOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                opts.output = args.get(i + 1).cloned().ok_or_else(|| {
                    CrustError::runtime(format!("{} requires an argument", args[i]))
                })?;
                i += 2;
            }
            "--emit-rs" => {
                opts.emit_rs = true;
                i += 1;
            }
            "--emit-proof" => {
                opts.emit_proof = true;
                i += 1;
            }
            "--llm-mode" => {
                opts.llm_mode = true;
                i += 1;
            }
            "--verify" => {
                opts.verify = true;
                i += 1;
            }
            s if s.starts_with("--strict=") => {
                let suffix = &s["--strict=".len()..];
                let n: u8 = match suffix.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        return Err(CrustError::runtime(format!(
                            "invalid --strict value {:?}; expected 0–4",
                            suffix
                        )));
                    }
                };
                if n > 4 {
                    eprintln!(
                        "warning: --strict={} is above the maximum (4); clamping to 4",
                        n
                    );
                }
                opts.level = StrictnessLevel::from_u8(n.min(4));
                i += 1;
            }
            arg => {
                opts.source_file = arg.to_string();
                i += 1;
            }
        }
    }
    if opts.source_file.is_empty() {
        return Err(CrustError::runtime("usage: crust build <file.crust>"));
    }
    Ok(opts)
}

fn build_cmd(args: &[String]) {
    let opts = match parse_build_options(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = build_file_with_opts(&opts) {
        // Summary errors (Analysis / TypeCheck / Rustc) carry counts whose
        // underlying diagnostics have already been streamed to stderr.
        // Print the summary line as a one-line tail so the exit code has
        // human-readable context without duplicating diagnostic bodies.
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn build_file_with_opts(opts: &BuildOptions) -> Result<()> {
    let source = fs::read_to_string(&opts.source_file)?;
    let tokens = lexer::Lexer::new(&source).tokenize()?;
    let program = parser::Parser::new(tokens).parse_program()?;

    // ── Analysis passes (run before codegen) ─────────────────────────────────
    let analyzer = analysis::Analyzer::new(opts.level, opts.llm_mode);
    let diagnostics = analyzer.analyze_program(&program);

    let error_count = diagnostics.iter().filter(|d| d.is_error()).count();
    if !diagnostics.is_empty() {
        for d in &diagnostics {
            eprintln!("{}", d.format());
        }
    }
    // Hard-fail rules:
    //   --strict=4 (Prove) treats every error diagnostic as a build failure.
    //   --llm-mode advertises hard guardrails ("ban unsafe, unwrap, todo!"),
    //   so any LlmGuardrail / UnsafeUsage / PurityViolation error must also
    //   fail the build regardless of level — otherwise the contract is a lie.
    let is_hard_failure =
        opts.level >= StrictnessLevel::Prove || (opts.llm_mode && error_count > 0);
    if is_hard_failure && error_count > 0 {
        return Err(CrustError::Analysis {
            count: error_count,
            hint: "fix them or relax --strict / drop --llm-mode",
        });
    }

    // ── Type inference pass ───────────────────────────────────────────────────
    if opts.level >= StrictnessLevel::Develop {
        let type_diags = types::TypeChecker::check_program(&program);
        for d in &type_diags {
            eprintln!("type: [{}] {}", d.function, d.message);
        }
        if opts.level >= StrictnessLevel::Prove && !type_diags.is_empty() {
            return Err(CrustError::TypeCheck {
                count: type_diags.len(),
            });
        }
    }

    // ── Unannotated-parameter check (--strict=4 --llm-mode contract) ─────────
    let param_diags = types::check_unannotated_params(&program, opts.level, opts.llm_mode);
    for d in &param_diags {
        eprintln!("type: [{}] {}", d.function, d.message);
    }
    if !param_diags.is_empty() {
        return Err(CrustError::Analysis {
            count: param_diags.len(),
            hint: "parameter-annotation errors at --strict=4 --llm-mode",
        });
    }

    // ── Codegen ───────────────────────────────────────────────────────────────
    let mut cg = Codegen::with_level(opts.level);
    cg.llm_mode = opts.llm_mode;
    let mut rs_source = cg.emit_program(&program);

    // In --llm-mode, prepend an ownership audit header
    if opts.llm_mode {
        rs_source = format!(
            "// Generated by Crust --llm-mode (level {})\n\
             // Ownership transfers annotated inline as /* ownership: ... */ comments.\n\
             // All .clone() calls are explicit; no implicit copies.\n\n{}",
            opts.level, rs_source
        );
    }

    // ── Proof / VC emission ───────────────────────────────────────────────────
    if opts.emit_proof || opts.level >= StrictnessLevel::Prove {
        let vcs = contracts::ContractChecker::extract_vcs(&program);
        if opts.emit_proof {
            // Write .v (Coq) file
            let coq_src = proofgen::CoqEmitter::emit_program(&program, &vcs);
            let coq_path = Path::new(&opts.source_file).with_extension("v");
            fs::write(&coq_path, &coq_src)?;
            eprintln!("   Emitted  {}", coq_path.display());

            // Write .lean (Lean 4) file
            let lean_src = proofgen::LeanEmitter::emit_program(&program, &vcs);
            let lean_path = Path::new(&opts.source_file).with_extension("lean");
            fs::write(&lean_path, &lean_src)?;
            eprintln!("   Emitted  {}", lean_path.display());
        }

        // Attempt SMT discharge if z3 is available
        if opts.verify {
            let results = contracts::ContractChecker::check_with_smt(&vcs);
            for r in &results {
                eprintln!("   SMT  {}", r);
            }
        }
    }

    // ── Write generated .rs ───────────────────────────────────────────────────
    // Use a per-process temp file path so concurrent `crust build` invocations
    // (e.g. cargo test running multiple integration tests in parallel) don't
    // race on a shared `__crust_build.rs` and clobber each other.
    let tmp_dir = std::env::temp_dir();
    let tmp_rs = tmp_dir.join(format!("__crust_build_{}.rs", std::process::id()));
    fs::write(&tmp_rs, &rs_source)?;

    if opts.emit_rs {
        let rs_path = Path::new(&opts.source_file).with_extension("rs");
        fs::write(&rs_path, &rs_source)?;
        eprintln!("   Emitted  {}", rs_path.display());
    }

    // ── Invoke compiler ──────────────────────────────────────────────────────
    // Edition 2021 is required for `async fn`, edition-2018 path forms, and the
    // newer disjoint-closure-capture rules. Without `--edition` rustc defaults
    // to 2015 which rejects most of what Crust emits.
    //
    // At --strict=3 (Ship) the contract is "rustc/clippy clean" (crust-o3a),
    // so swap in `clippy-driver` (a drop-in rustc replacement that also runs
    // clippy lints) and treat warnings as errors. If clippy-driver isn't
    // installed, fall back to plain rustc with a warning.
    let mut compiler_cmd = if opts.level >= StrictnessLevel::Ship && clippy_driver_available() {
        let mut c = Command::new("clippy-driver");
        c.arg(&tmp_rs)
            .arg("--edition=2021")
            .arg("-o")
            .arg(&opts.output)
            .arg("-C")
            .arg("opt-level=2")
            // -Dwarnings turns every clippy/rustc warning into a hard error
            // so Ship mode literally enforces "clippy clean".
            .arg("-Dwarnings");
        c
    } else {
        if opts.level >= StrictnessLevel::Ship {
            eprintln!(
                "warning: clippy-driver not on PATH; --strict=3 falling back to rustc \
                 without clippy lints. Install with: rustup component add clippy"
            );
        }
        let mut c = Command::new("rustc");
        c.arg(&tmp_rs)
            .arg("--edition=2021")
            .arg("-o")
            .arg(&opts.output)
            .arg("-C")
            .arg("opt-level=2");
        c
    };
    let status = compiler_cmd
        .status()
        .map_err(|e| CrustError::runtime(format!("rustc/clippy not found: {}", e)))?;

    let _ = fs::remove_file(&tmp_rs);

    if status.success() {
        eprintln!(
            "   Compiled crust v{} ({})",
            env!("CARGO_PKG_VERSION"),
            opts.level
        );
        eprintln!("    Finished `release` profile [optimized]");
        eprintln!("      Binary: {}", Path::new(&opts.output).display());
        Ok(())
    } else {
        Err(CrustError::Rustc)
    }
}

/// Whether `clippy-driver` is on PATH. Used at --strict=3 to swap rustc for
/// clippy-driver (rustc-compatible CLI that additionally runs clippy lints).
fn clippy_driver_available() -> bool {
    Command::new("clippy-driver")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// Keep the old build_file for backward compat (used in tests)
#[allow(dead_code)]
fn build_file(path: &str, output: &str, emit_rs: bool) -> Result<()> {
    let opts = BuildOptions {
        source_file: path.to_string(),
        output: output.to_string(),
        emit_rs,
        ..BuildOptions::default()
    };
    build_file_with_opts(&opts)
}

// ── verify subcommand ─────────────────────────────────────────────────────────

/// `crust verify <file.crust> [--strict=N] [--llm-mode] [--emit-proof]`
///
/// Runs all static analysis passes and emits a JSON report without invoking
/// `rustc`.  Exit code 0 means no errors; non-zero means at least one.
fn verify_cmd(args: &[String]) {
    let opts = match parse_build_options(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let source = match fs::read_to_string(&opts.source_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    let tokens = match lexer::Lexer::new(&source).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("parse error: {}", e);
            std::process::exit(1);
        }
    };
    let program = match parser::Parser::new(tokens).parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("parse error: {}", e);
            std::process::exit(1);
        }
    };

    let analyzer = analysis::Analyzer::new(opts.level, opts.llm_mode);
    let diagnostics = analyzer.analyze_program(&program);
    let type_diags = types::TypeChecker::check_program(&program);
    let vcs = contracts::ContractChecker::extract_vcs(&program);

    // Compute the error gate up front so --emit-proof only writes proof
    // skeletons when the program would actually pass verify (crust-6kw).
    let has_errors = diagnostics.iter().any(|d| d.is_error())
        || (!type_diags.is_empty() && opts.level >= StrictnessLevel::Prove);

    if opts.emit_proof && !has_errors {
        let coq = proofgen::CoqEmitter::emit_program(&program, &vcs);
        let lean = proofgen::LeanEmitter::emit_program(&program, &vcs);
        let coq_path = Path::new(&opts.source_file).with_extension("v");
        let lean_path = Path::new(&opts.source_file).with_extension("lean");
        if let Err(e) = fs::write(&coq_path, &coq) {
            eprintln!("warning: {}", e);
        }
        if let Err(e) = fs::write(&lean_path, &lean) {
            eprintln!("warning: {}", e);
        }
        eprintln!("   Emitted  {}", coq_path.display());
        eprintln!("   Emitted  {}", lean_path.display());
    } else if opts.emit_proof {
        eprintln!("   Skipped  proof emission (verify reported errors)");
    }

    // Build the verify report as JSON
    let report = build_verify_report(&program, &diagnostics, &type_diags, &vcs);
    println!("{}", report);

    if has_errors {
        std::process::exit(1);
    }
}

fn build_verify_report(
    program: &[ast::Item],
    diags: &[analysis::Diagnostic],
    type_diags: &[types::TypeDiagnostic],
    vcs: &[contracts::VerifCondition],
) -> String {
    use ast::{Attr, Item};

    let mut fn_reports: Vec<String> = Vec::new();

    for item in program {
        if let Item::Fn(f) = item {
            let mut requires: Vec<String> = Vec::new();
            let mut ensures: Vec<String> = Vec::new();
            let is_pure = f.attrs.iter().any(|a| matches!(a, Attr::Pure));

            // Pretty-print contract predicates back to readable Rust source
            // rather than emitting Debug-AST forms (Binary(Ne, Ident("s"), ...)).
            for attr in &f.attrs {
                match attr {
                    Attr::Requires(e) => requires.push(contracts::pretty_predicate(e)),
                    Attr::Ensures(e) => ensures.push(contracts::pretty_predicate(e)),
                    _ => {}
                }
            }

            let fn_diags: Vec<&analysis::Diagnostic> =
                diags.iter().filter(|d| d.function == f.name).collect();
            let fn_type_diags: Vec<&types::TypeDiagnostic> =
                type_diags.iter().filter(|d| d.function == f.name).collect();
            let fn_vcs: Vec<&contracts::VerifCondition> =
                vcs.iter().filter(|v| v.fn_name == f.name).collect();

            let panic_free = !fn_diags
                .iter()
                .any(|d| matches!(d.kind, analysis::DiagnosticKind::PotentialPanic));

            let proven: Vec<String> = fn_vcs
                .iter()
                .filter(|v| matches!(v.status, contracts::VcStatus::Proved))
                .map(|v| v.expr.clone())
                .collect();
            let unproven: Vec<String> = fn_vcs
                .iter()
                .filter(|v| !matches!(v.status, contracts::VcStatus::Proved))
                .map(|v| format!("{}: {}", v.kind_str(), v.expr))
                .collect();

            let diag_strs: Vec<String> = fn_diags
                .iter()
                .map(|d| d.format().to_string())
                .chain(fn_type_diags.iter().map(|d| d.message.clone()))
                .collect();

            fn_reports.push(format!(
                "    {{\n\
                       \"name\": \"{}\",\n\
                       \"is_async\": {},\n\
                       \"pure\": {},\n\
                       \"panic_free\": {},\n\
                       \"requires\": [{}],\n\
                       \"ensures\": [{}],\n\
                       \"proven\": [{}],\n\
                       \"unproven\": [{}],\n\
                       \"diagnostics\": [{}]\n\
                     }}",
                f.name,
                f.is_async,
                is_pure,
                panic_free,
                json_str_array(&requires),
                json_str_array(&ensures),
                json_str_array(&proven),
                json_str_array(&unproven),
                json_str_array(&diag_strs),
            ));
        }
    }

    format!(
        "{{\n  \"functions\": [\n{}\n  ],\n  \"summary\": {{\n    \"total_functions\": {},\n    \"total_diagnostics\": {},\n    \"verification_conditions\": {}\n  }}\n}}",
        fn_reports.join(",\n"),
        fn_reports.len(),
        diags.len() + type_diags.len(),
        vcs.len(),
    )
}

/// Format a slice of strings as a JSON array of string values, with full
/// escaping for backslash, double-quote, and control characters per RFC 8259.
fn json_str_array(items: &[String]) -> String {
    items
        .iter()
        .map(|s| json_string(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Encode a single Rust string as a JSON string literal (with surrounding
/// quotes), escaping `"`, `\`, and control characters per RFC 8259.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── help / version ────────────────────────────────────────────────────────────

fn help() {
    println!(
        "crust {} — Rust for the rest of us

USAGE:
    crust run    <file.crust>             Run a .crust source file
    crust repl                            Interactive REPL
    crust build  <file.crust> -o <out>   Compile to native binary
    crust verify <file.crust>             Static analysis + proof report (no compile)

OPTIONS:
    -o, --output NAME    Output binary name (default: a.out)
    --emit-rs            Write the generated .rs file alongside the source
    --emit-proof         Emit Coq (.v) and Lean (.lean) proof skeletons
    --verify             Attempt to discharge VCs via z3/cvc5 (if on PATH)
    --llm-mode           LLM-safe guardrails: ban unsafe, unwrap, todo!, etc.
    --strict=LEVEL       Strictness 0–4 (default: 0)
    -h, --help           Show this message
    -V, --version        Show version

STRICTNESS LEVELS:
    0  Explore  — no borrow checker, implicit Clone, auto-derive (default)
    1  Develop  — warnings on moves, type hints
    2  Harden   — borrow checker active, explicit lifetimes
    3  Ship     — full rustc parity; no implicit derives
    4  Prove    — formal verification: contracts, overflow checks, panic-freedom proofs

LEVEL 4 / LLM MODE CONTRACT SYNTAX (in .crust source files):
    #[requires(pred)]   — precondition the caller must satisfy
    #[ensures(pred)]    — postcondition the function guarantees (use `result` for return value)
    #[invariant(pred)]  — property that must hold throughout the function
    #[pure]             — assert function has no side effects",
        env!("CARGO_PKG_VERSION")
    );
}

fn version() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
}
