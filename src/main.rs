mod ast;
mod codegen;
mod env;
mod error;
mod eval;
mod lexer;
mod parser;
mod repl;
mod stdlib;
mod value;

use std::fs;
use std::path::Path;
use std::process::Command;

use error::{CrustError, Result};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str());
    match subcommand {
        Some("run")                          => run_cmd(&args[2..]),
        Some("build")                        => build_cmd(&args[2..]),
        Some("repl")                         => repl::run(),
        Some("--help" | "-h" | "help")       => help(),
        Some("--version" | "-V" | "version") => version(),
        Some(other) => {
            eprintln!("crust: unknown subcommand '{}'\n", other);
            help();
            std::process::exit(1);
        }
        None => { help(); std::process::exit(1); }
    }
}

fn run_cmd(args: &[String]) {
    let path = match args.first() {
        Some(p) => p,
        None => { eprintln!("usage: crust run <file.crust>"); std::process::exit(1); }
    };
    if let Err(e) = run_file(path) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn run_file(path: &str) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let tokens = lexer::Lexer::new(&source).tokenize()?;
    let program = parser::Parser::new(tokens).parse_program()?;
    let mut interp = eval::Interpreter::new();
    interp.run(program)
}

fn build_cmd(args: &[String]) {
    let mut output = "a.out".to_string();
    let mut emit_rs = false;
    let mut source_file: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if let Some(name) = args.get(i + 1) {
                    output = name.clone(); i += 2;
                } else {
                    eprintln!("error: {} requires an argument", args[i]);
                    std::process::exit(1);
                }
            }
            "--emit-rs" => { emit_rs = true; i += 1; }
            s if s.starts_with("--strict=") => { i += 1; }
            arg => { source_file = Some(arg.to_string()); i += 1; }
        }
    }

    let path = match source_file {
        Some(p) => p,
        None => { eprintln!("usage: crust build <file.crust> -o <out>"); std::process::exit(1); }
    };

    if let Err(e) = build_file(&path, &output, emit_rs) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn build_file(path: &str, output: &str, emit_rs: bool) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let tokens = lexer::Lexer::new(&source).tokenize()?;
    let program = parser::Parser::new(tokens).parse_program()?;
    let rs_source = codegen::Codegen::new().emit_program(&program);

    let tmp_dir = std::env::temp_dir();
    let tmp_rs = tmp_dir.join("__crust_build.rs");
    fs::write(&tmp_rs, &rs_source)?;

    if emit_rs {
        let rs_path = Path::new(path).with_extension("rs");
        fs::write(&rs_path, &rs_source)?;
        eprintln!("   Emitted  {}", rs_path.display());
    }

    let status = Command::new("rustc")
        .arg(&tmp_rs)
        .arg("-o").arg(output)
        .arg("-C").arg("opt-level=2")
        .status()
        .map_err(|e| CrustError::runtime(format!("rustc not found: {}", e)))?;

    let _ = fs::remove_file(&tmp_rs);

    if status.success() {
        eprintln!("   Compiled crust v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("    Finished `release` profile [optimized]");
        eprintln!("      Binary: {}", Path::new(output).display());
        Ok(())
    } else {
        Err(CrustError::runtime("rustc compilation failed"))
    }
}

fn help() {
    println!(
        "crust {} — Rust for the rest of us

USAGE:
    crust run <file.crust>           Run a .crust source file
    crust repl                       Interactive REPL
    crust build <file.crust> -o out  Compile to native binary

OPTIONS:
    -o, --output NAME    Output binary name (default: a.out)
    --emit-rs            Also write the generated .rs file
    --strict=LEVEL       Strictness 0-3 (default: 0)
    -h, --help           Show this message
    -V, --version        Show version

STRICTNESS LEVELS:
    0  Explore  — no borrow checker, implicit Clone (default)
    1  Develop  — warnings on moves, type hints
    2  Harden   — borrow checker active
    3  Ship     — full rustc parity",
        env!("CARGO_PKG_VERSION")
    );
}

fn version() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
}
