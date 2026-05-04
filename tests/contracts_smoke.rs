//! Smoke tests for the crust verify / build pipeline contract surface.
//! Verify JSON, --llm-mode hard-failure, --strict=4 arithmetic, proof-file
//! gating, and the const-eval error propagation regression.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn crust_binary() -> PathBuf {
    let mut p = workspace_root();
    p.push("target");
    p.push("debug");
    p.push("crust");
    p
}
fn ensure_built() {
    let status = Command::new("cargo")
        .arg("build")
        .arg("--quiet")
        .current_dir(workspace_root())
        .status()
        .expect("cargo build");
    assert!(status.success());
}

fn write_temp(name: &str, contents: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("crust_test_{}_{}.crust", name, std::process::id()));
    std::fs::write(&p, contents).expect("write temp .crust");
    p
}

#[test]
fn llm_mode_unsafe_fails_build_at_default_level() {
    ensure_built();
    let src = write_temp("llm_unsafe", "fn main() { let _ = unsafe { 1 + 1 }; }\n");
    let out = workspace_root().join("target").join("__crust_llm_test_out");
    let status = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg("--llm-mode")
        .arg("-o")
        .arg(&out)
        .status()
        .expect("crust");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&src);
    assert!(
        !status.success(),
        "--llm-mode must hard-fail on unsafe blocks (crust-oo8)"
    );
}

#[test]
fn strict_4_accepts_bare_arithmetic_so_smt_can_run() {
    ensure_built();
    // Function with #[requires] and arithmetic; previously the analysis pass
    // hard-blocked at --strict=4 on bare `*` and `+` (crust-37a). Codegen
    // already lowers them to checked_*, so analysis must not error here.
    let src = write_temp(
        "strict4_arith",
        "#[requires(x > 0)]\nfn double_plus_one(x: i64) -> i64 { x * 2 + 1 }\nfn main() {}\n",
    );
    let out = workspace_root().join("target").join("__crust_s4_test_out");
    let status = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg("--strict=4")
        .arg("-o")
        .arg(&out)
        .status()
        .expect("crust");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&src);
    assert!(
        status.success(),
        "--strict=4 must accept programs with arithmetic since codegen lowers it (crust-37a)"
    );
}

#[test]
fn const_initializer_errors_propagate() {
    ensure_built();
    // Previously eval silently swallowed const init errors via `if let Ok(..)`.
    // Now this should exit non-zero with a runtime error (crust-xwz).
    let src = write_temp(
        "const_swallow",
        "const X: i64 = undefined_var + 1;\nfn main() {}\n",
    );
    let status = Command::new(crust_binary())
        .arg("run")
        .arg(&src)
        .status()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    assert!(
        !status.success(),
        "const-initializer eval errors must propagate, not silently exit 0 (crust-xwz)"
    );
}

#[test]
fn verify_json_escapes_control_chars() {
    ensure_built();
    // Predicate string contains a literal newline; the JSON output's
    // 'requires' / 'unproven' must escape it as \n, not emit a raw newline
    // that breaks single-line JSON parsers (crust-ob9).
    let src = write_temp(
        "verify_escape",
        "#[requires(s != \"a\\nb\")]\nfn f(s: i64) -> i64 { s }\nfn main() {}\n",
    );
    let output = Command::new(crust_binary())
        .arg("verify")
        .arg(&src)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // Each individual line of the JSON output must not contain a raw control
    // char between escaped quotes — i.e., the value strings must use \n etc.
    // We reject the failure mode where pretty_predicate's contained `\n` is
    // a literal byte 0x0A in a JSON string.
    for (i, line) in stdout.lines().enumerate() {
        for c in line.chars() {
            assert!(
                (c as u32) >= 0x20 || c == '\t',
                "verify JSON line {} contains raw control char {:#x}: {:?}",
                i,
                c as u32,
                line
            );
        }
    }
    // And it must be valid as a JSON document (basic structural check —
    // matched braces, no Debug-AST forms).
    assert!(stdout.contains("\"requires\""));
    assert!(
        !stdout.contains("Binary(") && !stdout.contains("Lit(Str("),
        "verify must pretty-print predicates, not emit Debug-AST (crust-ob9). got: {}",
        stdout
    );
}

#[test]
fn smt_disproves_postcondition_with_counter_example() {
    ensure_built();
    // z3 must be on PATH for this test to be meaningful; if not, just skip.
    let z3_present = Command::new("z3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !z3_present {
        eprintln!("z3 not on PATH — skipping");
        return;
    }
    // `result > x` cannot be proved without body interpretation, so the
    // SMT layer should report DISPROVED with a concrete counter-example
    // showing both x and result. crust-7e8.
    let src = write_temp(
        "smt_disprove",
        "#[requires(x > 0)]\n#[ensures(result > x)]\nfn id(x: i64) -> i64 { x }\nfn main() {}\n",
    );
    let out = workspace_root().join("target").join("__crust_smt_test_out");
    let output = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg("--strict=4")
        .arg("--verify")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&src);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("DISPROVED") && stderr.contains("ensures"),
        "expected DISPROVED ensures with z3 model, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("counter-example: x=") && stderr.contains("result="),
        "expected concrete counter-example pairs (x=N, result=M), got:\n{}",
        stderr
    );
}

#[test]
fn smt_typed_sorts_handle_bool_and_real() {
    ensure_built();
    let z3_present = Command::new("z3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !z3_present {
        return;
    }
    // Bool parameter — must be declared as Bool sort, not Int. crust-7e8.
    let src = write_temp(
        "smt_bool",
        "#[requires(flag)]\nfn run(flag: bool) -> i64 { 0 }\nfn main() {}\n",
    );
    let out = workspace_root().join("target").join("__crust_smt_bool_out");
    let output = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg("--strict=4")
        .arg("--verify")
        .arg("-o")
        .arg(&out)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&src);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Bool requires `(assert flag)` ; satisfiable when flag = true → CONSISTENT.
    assert!(
        stderr.contains("CONSISTENT") && stderr.contains("requires"),
        "expected CONSISTENT requires for bool param, got:\n{}",
        stderr
    );
}

#[test]
fn verify_emit_proof_skipped_on_error() {
    ensure_built();
    // Program with an --llm-mode error; proof skeletons should NOT be written
    // when verify reports errors (crust-6kw).
    let src = write_temp("emit_proof_err", "fn main() { let _ = unsafe { 1 }; }\n");
    let coq_path = src.with_extension("v");
    let lean_path = src.with_extension("lean");
    let _ = std::fs::remove_file(&coq_path);
    let _ = std::fs::remove_file(&lean_path);
    let _ = Command::new(crust_binary())
        .arg("verify")
        .arg(&src)
        .arg("--llm-mode")
        .arg("--emit-proof")
        .output()
        .expect("crust");
    let coq_exists = coq_path.exists();
    let lean_exists = lean_path.exists();
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&coq_path);
    let _ = std::fs::remove_file(&lean_path);
    assert!(
        !coq_exists && !lean_exists,
        "verify --emit-proof must not write .v/.lean when verify errors (crust-6kw)"
    );
}
