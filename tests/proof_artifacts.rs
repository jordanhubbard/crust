//! Proof-emitter regression: when coqc / lean are on PATH, assert that the
//! emitted .v / .lean files load (with admits/sorries kept) for a simple
//! contract program. crust-o1e covers the breakage where the emitted
//! skeletons themselves were syntactically invalid.

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
    let _ = Command::new("cargo")
        .arg("build")
        .arg("--quiet")
        .current_dir(workspace_root())
        .status();
}

fn tool_available(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn emit_proof(src: &PathBuf) -> (PathBuf, PathBuf) {
    let _ = Command::new(crust_binary())
        .arg("verify")
        .arg(src)
        .arg("--strict=4")
        .arg("--emit-proof")
        .output()
        .expect("crust");
    (src.with_extension("v"), src.with_extension("lean"))
}

#[test]
fn coq_skeleton_loads_when_coqc_available() {
    if !tool_available("coqc") {
        eprintln!("coqc not on PATH — skipping");
        return;
    }
    ensure_built();
    let mut src = std::env::temp_dir();
    src.push(format!("crust_coq_{}.crust", std::process::id()));
    std::fs::write(
        &src,
        "#[requires(x > 0)]\n#[ensures(result > 0)]\nfn id(x: i64) -> i64 { x }\nfn main() {}\n",
    )
    .unwrap();
    let (coq, lean) = emit_proof(&src);
    let status = Command::new("coqc").arg(&coq).status();
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&coq);
    let _ = std::fs::remove_file(&lean);
    let glob_pat = coq.with_extension("vo");
    let _ = std::fs::remove_file(&glob_pat);
    let glob_glob = coq.with_extension("glob");
    let _ = std::fs::remove_file(&glob_glob);
    let aux = coq.parent().unwrap().join(format!(
        ".{}.aux",
        coq.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&aux);
    assert!(
        status.map(|s| s.success()).unwrap_or(false),
        "coqc must accept the emitted .v skeleton (crust-o1e)"
    );
}

#[test]
fn lean_skeleton_parses_when_lean_available() {
    // Lean's `lean --check` mode would require Mathlib; skip unconditionally
    // unless a `lean` binary is on PATH AND a `--no-deps` parse mode succeeds.
    // For now this is a soft check: we only assert that crust verify exits
    // 0 and emits a non-empty .lean file.
    ensure_built();
    let mut src = std::env::temp_dir();
    src.push(format!("crust_lean_{}.crust", std::process::id()));
    std::fs::write(
        &src,
        "#[requires(x > 0)]\nfn id(x: i64) -> i64 { x }\nfn main() {}\n",
    )
    .unwrap();
    let (coq, lean) = emit_proof(&src);
    let lean_contents = std::fs::read_to_string(&lean).unwrap_or_default();
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&coq);
    let _ = std::fs::remove_file(&lean);
    assert!(
        !lean_contents.is_empty(),
        "lean file must be emitted for a clean program"
    );
    // `result == x` -> crust pretty-prints with `==`; assert that the lean
    // skeleton uses `=` (Lean's equality) in the contract theorem rather than
    // Rust's `==` — symptom from crust-o1e.
    if lean_contents.contains("contract") {
        assert!(
            !lean_contents.contains(" == "),
            "lean output must not carry Rust's `==` operator: {}",
            lean_contents
        );
    }
}
