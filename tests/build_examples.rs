//! End-to-end test: run `crust build` over each `.crust` file under
//! `examples/` and assert that the resulting Rust source is accepted by
//! `rustc`. Purpose: close the loop on codegen regressions (crust-itp).
//!
//! Examples that currently fail without real ownership-relaxation analysis
//! (crust-ovw) are listed in `KNOWN_FAILING` and asserted-as-failing rather
//! than skipped — that way fixing one of them (and forgetting to remove it
//! from the list) shows up as a test failure too.

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
    let out = workspace_root().join("target").join("__crust_test_out");
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
