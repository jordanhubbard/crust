//! Regression tests for unsupported-feature diagnostics (crust-dfi).
//! Crust accepts more syntax at parse time than it can faithfully model;
//! the analysis pass surfaces those gaps as warnings (Develop+) or errors
//! (Prove) so users don't ship code under a false sense of compatibility.

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
    p.push(format!("crust_unsup_{}_{}.crust", name, std::process::id()));
    std::fs::write(&p, contents).expect("write");
    p
}

fn run_build(src: &PathBuf, level: &str) -> (bool, String) {
    let bin = workspace_root().join("target").join(format!(
        "__crust_unsup_{}",
        src.file_stem().unwrap().to_str().unwrap()
    ));
    let output = Command::new(crust_binary())
        .arg("build")
        .arg(src)
        .arg(format!("--strict={}", level))
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("crust build");
    let _ = std::fs::remove_file(&bin);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn impl_trait_param_warns_at_develop() {
    ensure_built();
    let src = write_temp(
        "impl_param",
        "fn id(x: impl Clone) -> i64 { 1 }\nfn main() {}\n",
    );
    let (_ok, stderr) = run_build(&src, "1");
    let _ = std::fs::remove_file(&src);
    assert!(
        stderr.contains("impl Trait") && stderr.contains("warning") && !stderr.contains("error: ["),
        "expected impl-trait warning at --strict=1, got:\n{}",
        stderr
    );
}

#[test]
fn impl_trait_return_errors_at_prove() {
    ensure_built();
    // Use no-arithmetic body so the only Prove-mode failure is the
    // impl-Trait diagnostic.
    let src = write_temp(
        "impl_ret",
        "fn id(x: i64) -> impl Clone { x }\nfn main() {}\n",
    );
    let (ok, stderr) = run_build(&src, "4");
    let _ = std::fs::remove_file(&src);
    assert!(!ok, "Prove should fail on impl-Trait return");
    assert!(
        stderr.contains("impl Trait") && stderr.contains("error"),
        "expected impl-Trait error at --strict=4, got:\n{}",
        stderr
    );
}

#[test]
fn unknown_macro_warns_at_develop() {
    ensure_built();
    let src = write_temp("unknown_macro", "fn main() { my_user_macro!(\"hi\"); }\n");
    let (_ok, stderr) = run_build(&src, "1");
    let _ = std::fs::remove_file(&src);
    assert!(
        stderr.contains("my_user_macro") && stderr.contains("not interpreted by Crust"),
        "expected unknown-macro warning, got:\n{}",
        stderr
    );
}

#[test]
fn known_macros_do_not_warn() {
    ensure_built();
    let src = write_temp(
        "known_macros",
        "fn main() { println!(\"hi\"); let _ = vec![1, 2, 3]; assert_eq!(1, 1); }\n",
    );
    let (_ok, stderr) = run_build(&src, "1");
    let _ = std::fs::remove_file(&src);
    assert!(
        !stderr.contains("not interpreted by Crust"),
        "no unknown-macro warning expected for stdlib macros, got:\n{}",
        stderr
    );
}

#[test]
fn explore_level_emits_no_unsupported_diagnostics() {
    ensure_built();
    // At Level 0 (Explore) we want a chatty, friendly UX — no warnings about
    // impl Trait / unknown macros, since users at this level don't care.
    let src = write_temp(
        "explore",
        "fn id(x: impl Clone) -> i64 { 1 }\nfn main() { my_macro!(); }\n",
    );
    let (_ok, stderr) = run_build(&src, "0");
    let _ = std::fs::remove_file(&src);
    assert!(
        !stderr.contains("impl Trait") && !stderr.contains("not interpreted by Crust"),
        "Explore level must stay quiet about unsupported features, got:\n{}",
        stderr
    );
}
