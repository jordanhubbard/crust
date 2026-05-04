//! Compatibility tests for Crust's primitive-width approximation (crust-6yj).
//! Pin the current i64-collapsed behaviour and the unsupported-feature
//! warnings for width-sensitive methods, so future width-faithful
//! implementations show up as test failures and prompt removing these.

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
        "crust_widths_{}_{}.crust",
        name,
        std::process::id()
    ));
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

fn build_capture_stderr(name: &str, contents: &str, level: &str) -> String {
    let src = write_temp(name, contents);
    let out = workspace_root().join("target").join(format!(
        "__crust_widths_{}_{}_out",
        name,
        std::process::id()
    ));
    let output = Command::new(crust_binary())
        .arg("build")
        .arg(&src)
        .arg(format!("--strict={}", level))
        .arg("-o")
        .arg(&out)
        .output()
        .expect("crust");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn primitive_max_constants_resolve() {
    ensure_built();
    let out = run_capture(
        "max_consts",
        "fn main() {\n\
             println!(\"u8={}\", u8::MAX);\n\
             println!(\"u16={}\", u16::MAX);\n\
             println!(\"u32={}\", u32::MAX);\n\
             println!(\"i8={}\", i8::MAX);\n\
             println!(\"i16={}\", i16::MAX);\n\
             println!(\"i32={}\", i32::MAX);\n\
         }",
    );
    assert_eq!(
        out,
        vec![
            "u8=255",
            "u16=65535",
            "u32=4294967295",
            "i8=127",
            "i16=32767",
            "i32=2147483647",
        ]
    );
}

#[test]
fn u64_max_is_approximated_to_i64_max() {
    ensure_built();
    // ACCEPTED DIVERGENCE: real u64::MAX is 18446744073709551615; Crust
    // reports i64::MAX since values don't fit i64. crust-6yj.
    let out = run_capture("u64", "fn main() { println!(\"{}\", u64::MAX); }");
    assert_eq!(out, vec!["9223372036854775807"]);
}

#[test]
fn unsuffixed_arithmetic_does_not_wrap_at_narrow_widths() {
    ensure_built();
    // DIVERGENCE: in real Rust `let a: u8 = 250; a + 10` overflows and panics
    // in debug or wraps to 4 in release. Crust's interpreter collapses to
    // i64 and produces 260. crust-6yj.
    let out = run_capture(
        "wrap",
        "fn main() {\n\
             let a: u8 = 250;\n\
             println!(\"{}\", a + 10);\n\
         }",
    );
    assert_eq!(out, vec!["260"]);
}

#[test]
fn width_sensitive_methods_warn_at_develop() {
    ensure_built();
    let stderr = build_capture_stderr(
        "wrap_method",
        "fn main() {\n\
             let a: u8 = 250;\n\
             let _ = a.wrapping_add(10);\n\
             let _ = (5i32).checked_mul(7);\n\
             let _ = (1u32).leading_zeros();\n\
         }",
        "1",
    );
    assert!(
        stderr.contains("wrapping_add") && stderr.contains("crust-6yj"),
        "expected wrapping_add warning, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("checked_mul"),
        "expected checked_mul warning, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("leading_zeros"),
        "expected leading_zeros warning, got:\n{}",
        stderr
    );
}

#[test]
fn width_sensitive_methods_silent_at_explore() {
    ensure_built();
    // Explore (Level 0) stays quiet — friendly UX for beginners.
    let stderr = build_capture_stderr(
        "wrap_explore",
        "fn main() {\n\
             let a: u8 = 250;\n\
             let _ = a.wrapping_add(10);\n\
         }",
        "0",
    );
    assert!(
        !stderr.contains("crust-6yj"),
        "Explore must not warn about width-sensitive methods, got:\n{}",
        stderr
    );
}
