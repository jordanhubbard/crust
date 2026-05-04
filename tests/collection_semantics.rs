//! Compatibility tests for Crust's collection approximations (crust-kbu).
//! These lock in the current behaviour so future fixes show up as test
//! failures, and document each acceptable divergence with a comment.
//!
//! Crust backs HashSet, BTreeSet, BTreeMap, and VecDeque on top of Vec /
//! HashMap. Some semantics match rustc exactly, some are deterministic
//! approximations, and some are tracked divergences.

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
    p.push(format!("crust_kbu_{}_{}.crust", name, std::process::id()));
    std::fs::write(&p, contents).expect("write");
    p
}

fn run_capture(name: &str, contents: &str) -> Vec<String> {
    let src = write_temp(name, contents);
    let out = Command::new(crust_binary())
        .arg("run")
        .arg(&src)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    assert!(
        out.status.success(),
        "crust run failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn hashset_set_operations_work() {
    ensure_built();
    let out = run_capture(
        "set_ops",
        "use std::collections::HashSet;\n\
         fn main() {\n\
             let a: HashSet<i64> = vec![1, 2, 3, 4].into_iter().collect();\n\
             let b: HashSet<i64> = vec![3, 4, 5, 6].into_iter().collect();\n\
             let mut u: Vec<i64> = a.union(&b).cloned().collect();\n\
             let mut i: Vec<i64> = a.intersection(&b).cloned().collect();\n\
             let mut d: Vec<i64> = a.difference(&b).cloned().collect();\n\
             u.sort(); i.sort(); d.sort();\n\
             println!(\"u={:?}\", u);\n\
             println!(\"i={:?}\", i);\n\
             println!(\"d={:?}\", d);\n\
         }",
    );
    assert_eq!(out, vec!["u=[1, 2, 3, 4, 5, 6]", "i=[3, 4]", "d=[1, 2]"]);
}

#[test]
fn hashset_dedup_on_insert() {
    ensure_built();
    let out = run_capture(
        "dedup",
        "use std::collections::HashSet;\n\
         fn main() {\n\
             let mut s: HashSet<i64> = HashSet::new();\n\
             s.insert(1); s.insert(2); s.insert(1); s.insert(2); s.insert(3);\n\
             println!(\"{}\", s.len());\n\
         }",
    );
    assert_eq!(out, vec!["3"]);
}

#[test]
fn btreemap_iterates_sorted_by_key() {
    ensure_built();
    // BTreeMap matches rustc here: iteration is in ascending key order
    // regardless of insertion order. Crust achieves this via the
    // HashMap-backed sort-on-iter logic in eval.rs.
    let out = run_capture(
        "btreemap_sorted",
        "use std::collections::BTreeMap;\n\
         fn main() {\n\
             let mut m: BTreeMap<i64, String> = BTreeMap::new();\n\
             m.insert(50, \"e\".to_string());\n\
             m.insert(10, \"a\".to_string());\n\
             m.insert(30, \"c\".to_string());\n\
             for (k, _) in &m { println!(\"{}\", k); }\n\
         }",
    );
    assert_eq!(out, vec!["10", "30", "50"]);
}

#[test]
fn btreeset_iterates_sorted() {
    ensure_built();
    // crust-4ri (closed): BTreeSet now backs to Value::SortedSet which
    // maintains the sorted invariant on insert and iterates in ascending
    // order — matching rustc.
    let out = run_capture(
        "btreeset_order",
        "use std::collections::BTreeSet;\n\
         fn main() {\n\
             let mut s: BTreeSet<i64> = BTreeSet::new();\n\
             s.insert(50); s.insert(10); s.insert(30);\n\
             for v in &s { println!(\"{}\", v); }\n\
         }",
    );
    assert_eq!(out, vec!["10", "30", "50"]);
}

#[test]
fn btreeset_dedups_on_insert() {
    ensure_built();
    let out = run_capture(
        "btreeset_dedup",
        "use std::collections::BTreeSet;\n\
         fn main() {\n\
             let mut s: BTreeSet<i64> = BTreeSet::new();\n\
             s.insert(1); s.insert(2); s.insert(1); s.insert(2); s.insert(3);\n\
             println!(\"{}\", s.len());\n\
         }",
    );
    assert_eq!(out, vec!["3"]);
}

#[test]
fn btreeset_remove_clears_membership() {
    ensure_built();
    let out = run_capture(
        "btreeset_remove",
        "use std::collections::BTreeSet;\n\
         fn main() {\n\
             let mut s: BTreeSet<i64> = BTreeSet::new();\n\
             s.insert(1); s.insert(2); s.insert(3);\n\
             s.remove(&2);\n\
             println!(\"len={}\", s.len());\n\
             println!(\"has2={}\", s.contains(&2));\n\
             println!(\"has1={}\", s.contains(&1));\n\
         }",
    );
    assert_eq!(out, vec!["len=2", "has2=false", "has1=true"]);
}

#[test]
fn vecdeque_push_pop_both_ends() {
    ensure_built();
    let out = run_capture(
        "vecdeque",
        "use std::collections::VecDeque;\n\
         fn main() {\n\
             let mut d: VecDeque<i64> = VecDeque::new();\n\
             d.push_back(1); d.push_back(2);\n\
             d.push_front(0); d.push_back(3);\n\
             println!(\"{:?}\", d);\n\
             println!(\"{:?}\", d.pop_front());\n\
             println!(\"{:?}\", d.pop_back());\n\
             println!(\"{:?}\", d);\n\
         }",
    );
    assert_eq!(out, vec!["[0, 1, 2, 3]", "Some(0)", "Some(3)", "[1, 2]"]);
}

#[test]
fn hashmap_entry_or_insert_with_compounds_correctly() {
    ensure_built();
    let out = run_capture(
        "entry_compound",
        "use std::collections::HashMap;\n\
         fn main() {\n\
             let mut m: HashMap<String, Vec<i64>> = HashMap::new();\n\
             for &v in &[1, 2, 3, 4] {\n\
                 let k = if v % 2 == 0 { \"even\" } else { \"odd\" };\n\
                 m.entry(k.to_string()).or_insert_with(Vec::new).push(v);\n\
             }\n\
             println!(\"even={:?}\", m[\"even\"]);\n\
             println!(\"odd={:?}\", m[\"odd\"]);\n\
         }",
    );
    assert_eq!(out, vec!["even=[2, 4]", "odd=[1, 3]"]);
}

#[test]
fn hashmap_iteration_is_sorted_by_key() {
    ensure_built();
    // ACCEPTED DIVERGENCE: real HashMap randomises iteration. Crust sorts
    // by key for deterministic test output. Documented in
    // docs/compatibility.md §3.6.
    let out = run_capture(
        "hashmap_iter",
        "use std::collections::HashMap;\n\
         fn main() {\n\
             let mut m: HashMap<String, i64> = HashMap::new();\n\
             m.insert(\"zebra\".to_string(), 1);\n\
             m.insert(\"apple\".to_string(), 2);\n\
             m.insert(\"monkey\".to_string(), 3);\n\
             for (k, _) in &m { println!(\"{}\", k); }\n\
         }",
    );
    assert_eq!(out, vec!["apple", "monkey", "zebra"]);
}

#[test]
fn vec_dedup_does_not_apply_to_plain_vec() {
    ensure_built();
    // Sanity check: Vec keeps duplicates; only HashSet dedups on insert.
    let out = run_capture(
        "vec_no_dedup",
        "fn main() {\n\
             let mut v = vec![1, 2, 1, 3, 2];\n\
             println!(\"{}\", v.len());\n\
             v.sort(); v.dedup();\n\
             println!(\"{:?}\", v);\n\
         }",
    );
    assert_eq!(out, vec!["5", "[1, 2, 3]"]);
}
