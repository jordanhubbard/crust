//! End-to-end tests for the examples/ corpus. Two parts:
//!
//! 1. `examples_build_or_match_known_failing` — every `.crust` file builds
//!    via `crust build` (which invokes rustc), or is on the KNOWN_FAILING
//!    list with a one-line reason. Closes the codegen regression loop
//!    (crust-itp).
//!
//! 2. `crust_run_matches_compiled_binary_output` — for every example that
//!    DOES build, asserts that `crust run` and the rustc-compiled binary
//!    produce byte-identical stdout. This is the differential parity check
//!    promised by crust-8up: any divergence between interpreter and codegen
//!    semantics surfaces here.

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
        .expect("cargo build must succeed");
    assert!(status.success(), "cargo build failed");
}

fn build_one(path: &std::path::Path) -> bool {
    let out = workspace_root().join("target").join(format!(
        "__crust_test_out_{}",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("x")
    ));
    let status = Command::new(crust_binary())
        .arg("build")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("crust binary must run");
    let _ = std::fs::remove_file(&out);
    status.status.success()
}

/// Build the example, run the resulting binary, capture stdout. Returns
/// None if build or exec fails.
fn build_and_capture(path: &std::path::Path) -> Option<String> {
    let out = workspace_root().join("target").join(format!(
        "__crust_diff_{}",
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("x")
    ));
    let build_ok = Command::new(crust_binary())
        .arg("build")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !build_ok {
        return None;
    }
    let exec = Command::new(&out).output().ok()?;
    let _ = std::fs::remove_file(&out);
    if !exec.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&exec.stdout).into_owned())
}

/// Run `crust run` on the source, capture stdout. Panics on failure.
fn run_and_capture(path: &std::path::Path) -> String {
    let out = Command::new(crust_binary())
        .arg("run")
        .arg(path)
        .output()
        .expect("crust run");
    assert!(
        out.status.success(),
        "crust run {} failed:\n{}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Examples whose codegen does not yet produce valid Rust.
/// Each entry is the file name (under examples/) and a short reason.
/// Remove an entry once the underlying codegen issue is resolved.
const KNOWN_FAILING: &[(&str, &str)] = &[
    ("enums.crust", "lifetime elision (crust-1x4)"),
    (
        "iterators.crust",
        "iter() ref/owned mismatch with explicit *deref (crust-ovw)",
    ),
    (
        "option_result.crust",
        "non-exhaustive match without auto-wildcard (crust-o3a)",
    ),
    (
        "patterns.crust",
        "lifetime elision + refutable let (crust-1x4 / crust-ovw)",
    ),
    (
        "state_machine.crust",
        "&'static str lifetime elision in fn return (crust-1x4)",
    ),
    (
        "queue_channel.crust",
        "generic struct method codegen drops type parameter (crust-1x4)",
    ),
];

#[test]
fn examples_build_or_match_known_failing() {
    ensure_built();
    let mut examples_dir = workspace_root();
    examples_dir.push("examples");

    let mut unexpected_pass: Vec<String> = Vec::new();
    let mut unexpected_fail: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&examples_dir).expect("read examples/");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("crust") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let known = KNOWN_FAILING.iter().any(|(n, _)| *n == name);
        let ok = build_one(&path);
        match (ok, known) {
            (true, true) => unexpected_pass.push(name),
            (false, false) => unexpected_fail.push(name),
            _ => {}
        }
    }

    assert!(
        unexpected_fail.is_empty(),
        "examples that should build but failed: {:?}",
        unexpected_fail
    );
    assert!(
        unexpected_pass.is_empty(),
        "examples in KNOWN_FAILING that now pass — remove from the list: {:?}",
        unexpected_pass
    );
}

/// Differential rustc parity: for every example that compiles via
/// `crust build`, assert that `crust run` produces the same stdout as the
/// rustc-compiled binary. Any divergence is a bug in either the
/// interpreter or the code generator. crust-8up.
#[test]
fn crust_run_matches_compiled_binary_output() {
    ensure_built();
    let mut examples_dir = workspace_root();
    examples_dir.push("examples");
    let entries = std::fs::read_dir(&examples_dir).expect("read examples/");
    let mut diverged: Vec<(String, String, String)> = Vec::new();
    let mut compared = 0usize;
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("crust") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if KNOWN_FAILING.iter().any(|(n, _)| *n == name) {
            continue; // rustc rejects the codegen output, no parity to check
        }
        let interp = run_and_capture(&path);
        let compiled = match build_and_capture(&path) {
            Some(s) => s,
            None => continue, // compile or exec issue — surfaces in the other test
        };
        compared += 1;
        if interp != compiled {
            diverged.push((name, interp, compiled));
        }
    }
    assert!(compared > 0, "expected at least one example to compare");
    if !diverged.is_empty() {
        let summary: String = diverged
            .iter()
            .map(|(n, i, c)| format!("  {}\n    interp: {:?}\n    binary: {:?}", n, i, c))
            .collect::<Vec<_>>()
            .join("\n");
        panic!(
            "differential rustc-parity divergence in {} examples:\n{}",
            diverged.len(),
            summary
        );
    }
}
