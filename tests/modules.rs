//! Regression tests for inline `mod NAME { ... }` support (crust-rvq).
//! `crust run` and `crust build` must both honour module path resolution
//! and qualified call sites (`outer::inner::ident`).

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
    p.push(format!("crust_mod_{}_{}.crust", name, std::process::id()));
    std::fs::write(&p, contents).expect("write");
    p
}

fn run_capturing(src: &PathBuf) -> (bool, String, String) {
    let output = Command::new(crust_binary())
        .arg("run")
        .arg(src)
        .output()
        .expect("crust run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn build_and_run(src: &PathBuf) -> (bool, String) {
    // Use the source file stem so parallel cargo tests don't race on a
    // shared binary path.
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("crustmod");
    let bin = workspace_root()
        .join("target")
        .join(format!("__crust_modtest_{}", stem));
    let build_status = Command::new(crust_binary())
        .arg("build")
        .arg(src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("crust build");
    if !build_status.success() {
        let _ = std::fs::remove_file(&bin);
        return (false, String::new());
    }
    let output = Command::new(&bin).output().expect("exec");
    let _ = std::fs::remove_file(&bin);
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

#[test]
fn inline_mod_resolves_qualified_call() {
    ensure_built();
    let src = write_temp(
        "basic",
        "mod inner { pub fn hello() -> i64 { 42 } }\n\
         fn main() { println!(\"{}\", inner::hello()); }\n",
    );
    let (ok, stdout, _) = run_capturing(&src);
    assert!(ok, "crust run must succeed");
    assert!(stdout.trim() == "42", "expected 42, got {:?}", stdout);
    let (ok, stdout) = build_and_run(&src);
    let _ = std::fs::remove_file(&src);
    assert!(ok, "crust build must round-trip a single-module program");
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn nested_mods_resolve_full_path() {
    ensure_built();
    let src = write_temp(
        "nested",
        "mod outer {\n\
             pub mod inner {\n\
                 pub fn deep() -> i64 { 7 }\n\
             }\n\
             pub fn shallow() -> i64 { 11 }\n\
         }\n\
         fn main() { println!(\"{} {}\", outer::inner::deep(), outer::shallow()); }\n",
    );
    let (ok, stdout, _) = run_capturing(&src);
    assert!(ok);
    assert_eq!(stdout.trim(), "7 11");
    let (ok, stdout) = build_and_run(&src);
    let _ = std::fs::remove_file(&src);
    assert!(ok);
    assert_eq!(stdout.trim(), "7 11");
}

#[test]
fn struct_in_module_round_trips() {
    ensure_built();
    let src = write_temp(
        "struct_in_mod",
        "mod m {\n\
             pub struct V { x: i64 }\n\
             impl V {\n\
                 pub fn new(x: i64) -> V { V { x: x } }\n\
                 pub fn get(self) -> i64 { self.x }\n\
             }\n\
         }\n\
         fn main() { println!(\"{}\", m::V::new(5).get()); }\n",
    );
    let (ok, stdout, _) = run_capturing(&src);
    assert!(ok);
    assert_eq!(stdout.trim(), "5");
    let (ok, stdout) = build_and_run(&src);
    let _ = std::fs::remove_file(&src);
    assert!(ok);
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn file_based_mod_rejected_with_clear_diagnostic() {
    ensure_built();
    let src = write_temp("filemod", "mod foo;\nfn main() {}\n");
    let output = Command::new(crust_binary())
        .arg("run")
        .arg(&src)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("file-based module"),
        "expected friendly diagnostic about file-based mod, got: {}",
        stderr
    );
}
