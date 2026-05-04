//! Run the new example programs through `crust run` and assert on output.
//! Pairs with tests/build_examples.rs (rustc round-trip) — that one tracks
//! `crust build` while this one locks in interpreter behaviour for examples
//! that exercise broader stdlib / pattern / module surface (crust-0qv).

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

fn run_example(name: &str) -> Vec<String> {
    let mut p = workspace_root();
    p.push("examples");
    p.push(format!("{}.crust", name));
    let output = Command::new(crust_binary())
        .arg("run")
        .arg(&p)
        .output()
        .expect("crust run");
    assert!(
        output.status.success(),
        "crust run {} failed:\nstdout: {}\nstderr: {}",
        name,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn state_machine_walks_through_events() {
    ensure_built();
    let out = run_example("state_machine");
    assert_eq!(
        out,
        vec![
            "now: locked",
            "now: unlocked",
            "now: unlocked",
            "now: locked",
            "now: locked",
        ]
    );
}

#[test]
fn hashset_ops_basic_surface() {
    ensure_built();
    let out = run_example("hashset_ops");
    // post-remove contains intentionally not asserted (see crust-aiy)
    assert_eq!(
        out,
        vec![
            "len = 3",
            "has 2 = true",
            "has 99 = false",
            "after remove(2), len = 2",
        ]
    );
}

#[test]
fn btreemap_word_tally_is_correct() {
    ensure_built();
    let out = run_example("btreemap_usage");
    assert_eq!(
        out,
        vec![
            "apple  = 3",
            "banana = 2",
            "cherry = 1",
            "total entries = 3",
        ]
    );
}

#[test]
fn queue_channel_drains_in_fifo_order() {
    ensure_built();
    let out = run_example("queue_channel");
    assert_eq!(
        out,
        vec![
            "queue size = 5",
            "recv: 1",
            "recv: 4",
            "recv: 9",
            "recv: 16",
            "recv: 25",
            "drained, len = 0",
        ]
    );
}

#[test]
fn modules_example_round_trips() {
    ensure_built();
    let out = run_example("modules");
    assert_eq!(out, vec!["manhattan = 7", "origin    = 0"]);
}
