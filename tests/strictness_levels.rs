//! End-to-end tests for the strictness dial (crust-o3a). Verify that each
//! level enforces the diagnostics it advertises.

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
    p.push(format!(
        "crust_levels_{}_{}.crust",
        name,
        std::process::id()
    ));
    std::fs::write(&p, contents).expect("write");
    p
}

fn build(name: &str, contents: &str, level: &str) -> (bool, String) {
    let src = write_temp(name, contents);
    let bin = workspace_root()
        .join("target")
        .join(format!("__crust_levels_{}", name));
    let output = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg(format!("--strict={}", level))
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&bin);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn clippy_available() -> bool {
    Command::new("clippy-driver")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn shadowing_warns_at_develop() {
    ensure_built();
    let (_ok, stderr) = build(
        "shadow",
        "fn main() { let x = 1; let x = 2; println!(\"{}\", x); }",
        "1",
    );
    assert!(
        stderr.contains("shadows") && stderr.contains("warning"),
        "expected shadowing warning at --strict=1, got:\n{}",
        stderr
    );
}

#[test]
fn shadowing_silent_at_explore() {
    ensure_built();
    let (_ok, stderr) = build(
        "shadow_explore",
        "fn main() { let x = 1; let x = 2; println!(\"{}\", x); }",
        "0",
    );
    assert!(
        !stderr.contains("shadows"),
        "Explore (Level 0) must not warn about shadowing, got:\n{}",
        stderr
    );
}

#[test]
fn shadowing_in_nested_scope_does_not_warn() {
    ensure_built();
    // Re-binding inside a child block is *not* shadowing the outer name in
    // the same scope — it goes out of scope when the block ends. Crust's
    // detector pops names at scope-close, so this should not warn.
    let (_ok, stderr) = build(
        "shadow_nested",
        "fn main() { let x = 1; { let y = 2; println!(\"{} {}\", x, y); } }",
        "1",
    );
    assert!(
        !stderr.contains("shadows"),
        "Nested-scope distinct binding must not warn, got:\n{}",
        stderr
    );
}

#[test]
fn ship_invokes_clippy_when_available() {
    ensure_built();
    if !clippy_available() {
        eprintln!("clippy-driver not on PATH — skipping");
        return;
    }
    // `let _ = x + 0` triggers clippy::identity_op. At --strict=3 that's a
    // hard build error.
    let (ok, stderr) = build(
        "ship_clippy",
        "fn main() {\n\
             let x = 5;\n\
             let _ = x + 0;\n\
             println!(\"{}\", x);\n\
         }",
        "3",
    );
    assert!(!ok, "Ship should fail on clippy lint");
    assert!(
        stderr.contains("identity_op") || stderr.contains("clippy"),
        "expected clippy lint output, got:\n{}",
        stderr
    );
}

#[test]
fn lower_levels_do_not_invoke_clippy() {
    ensure_built();
    // The same identity_op program builds fine at --strict=2.
    let (ok, _stderr) = build(
        "harden_no_clippy",
        "fn main() {\n\
             let x = 5;\n\
             let _ = x + 0;\n\
             println!(\"{}\", x);\n\
         }",
        "2",
    );
    assert!(ok, "Harden (--strict=2) must build identity_op programs");
}

#[test]
fn std_only_program_does_not_emit_cargo_note() {
    ensure_built();
    let (_ok, stderr) = build(
        "std_only",
        "use std::collections::HashMap;\n\
         fn main() { let _: HashMap<i64, i64> = HashMap::new(); }",
        "0",
    );
    assert!(
        !stderr.contains("crust-ti9"),
        "std-only program must not trigger the cargo-detection note, got:\n{}",
        stderr
    );
}

#[test]
fn non_std_import_emits_cargo_note() {
    ensure_built();
    let (_ok, stderr) = build("non_std", "use serde::Deserialize;\nfn main() {}", "0");
    assert!(
        stderr.contains("crust-ti9") && stderr.contains("--extern"),
        "non-std import must trigger the cargo note pointing to --extern, got:\n{}",
        stderr
    );
}

#[test]
fn shadowing_param_in_body_is_caught() {
    ensure_built();
    let (_ok, stderr) = build(
        "shadow_param",
        "fn f(n: i64) -> i64 { let n = n + 1; n }\n\
         fn main() { println!(\"{}\", f(5)); }",
        "1",
    );
    assert!(
        stderr.contains("shadows"),
        "expected shadow warning when let rebinds a fn param, got:\n{}",
        stderr
    );
}
