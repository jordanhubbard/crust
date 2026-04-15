mod ast;
mod environment;
mod interpreter;
mod lexer;
mod parser;
mod value;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

fn main() {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("run") => {
            let path = args.get(2).unwrap_or_else(|| {
                eprintln!("usage: crust run <file>");
                std::process::exit(1);
            });
            run_file(path);
        }
        Some("build") => {
            let path = args.get(2).unwrap_or_else(|| {
                eprintln!("usage: crust build <file> [-o output]");
                std::process::exit(1);
            });
            build_file(path, &args[3..]);
        }
        Some("--help") | Some("-h") | Some("help") => help(),
        Some("--version") | Some("-V") | Some("version") => version(),
        Some(path) if path.ends_with(".crust") || path.ends_with(".rs") => run_file(path),
        None => repl(),
        _ => {
            eprintln!("unknown subcommand '{}'. Try `crust help`.", args[1]);
            std::process::exit(1);
        }
    }
}

fn run_file(path: &str) {
    let src = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read '{}': {}", path, e);
        std::process::exit(1);
    });
    if let Err(e) = exec_source(&src) {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}

fn exec_source(src: &str) -> Result<(), String> {
    let tokens = Lexer::new(src).tokenize()?;
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse_program()?;
    let mut interp = Interpreter::new();
    // If there is a `main` function defined, call it; otherwise run top-level.
    if stmts
        .iter()
        .any(|s| matches!(s, ast::Stmt::FnDef { name, .. } if name == "main"))
    {
        interp.run(&stmts)?; // defines all fns
                             // Now call main()
        let call = ast::Expr::Call {
            function: Box::new(ast::Expr::Ident("main".to_string())),
            args: vec![],
        };
        interp.eval_expr(&call).map(|_| ())
    } else {
        interp.run(&stmts)
    }
}

fn repl() {
    println!(
        "crust {} REPL — Ctrl-D or `exit` to quit",
        env!("CARGO_PKG_VERSION")
    );
    let mut interp = Interpreter::new();
    let stdin = io::stdin();
    let mut buf = String::new();
    let mut depth: i32 = 0; // brace depth for multi-line input

    loop {
        let prompt = if depth == 0 { ">> " } else { ".. " };
        print!("{}", prompt);
        io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            } // EOF
            Err(e) => {
                eprintln!("read error: {}", e);
                break;
            }
            Ok(_) => {}
        }

        let trimmed = line.trim();
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        buf.push_str(&line);

        if depth <= 0 {
            depth = 0;
            let src = buf.trim().to_string();
            buf.clear();
            if src.is_empty() {
                continue;
            }

            match Lexer::new(&src).tokenize().and_then(|toks| {
                let mut p = Parser::new(toks);
                p.parse_program()
            }) {
                Err(e) => eprintln!("parse error: {}", e),
                Ok(stmts) => match interp.run_expr(&stmts) {
                    Ok(Some(val)) => {
                        let s = val.debug_fmt();
                        if s != "()" {
                            println!("{}", s);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("error: {}", e),
                },
            }
        }
    }
}

fn build_file(path: &str, extra_args: &[String]) {
    // Parse -o flag
    let mut output = None;
    let mut i = 0;
    while i < extra_args.len() {
        if extra_args[i] == "-o" {
            if i + 1 < extra_args.len() {
                output = Some(extra_args[i + 1].clone());
                i += 2;
            } else {
                eprintln!("error: -o requires an output name");
                std::process::exit(1);
            }
        } else {
            i += 1;
        }
    }

    let out_name = output.unwrap_or_else(|| {
        Path::new(path)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    // Read and interpret the source to validate it parses
    let src = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read '{}': {}", path, e);
        std::process::exit(1);
    });
    let tokens = Lexer::new(&src).tokenize().unwrap_or_else(|e| {
        eprintln!("lex error: {}", e);
        std::process::exit(1);
    });
    let mut parser = Parser::new(tokens);
    let _stmts = parser.parse_program().unwrap_or_else(|e| {
        eprintln!("parse error: {}", e);
        std::process::exit(1);
    });

    let tmp_dir = std::env::temp_dir();
    let tmp_rs = tmp_dir.join("__crust_build.rs");

    // Capture output by running the interpreter in a subprocess
    let self_exe = env::current_exe().unwrap_or_else(|_| "crust".into());
    let child = Command::new(&self_exe)
        .arg("run")
        .arg(path)
        .output()
        .unwrap_or_else(|e| {
            eprintln!("error: failed to run interpreter: {}", e);
            std::process::exit(1);
        });

    if !child.status.success() {
        eprintln!(
            "error: source has errors:\n{}",
            String::from_utf8_lossy(&child.stderr)
        );
        std::process::exit(1);
    }

    let captured = String::from_utf8_lossy(&child.stdout);

    // Generate a tiny Rust program that just prints the captured output
    let gen_src = format!(
        "fn main() {{\n    print!(\"{}\");\n}}\n",
        captured
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('{', "{{")
            .replace('}', "}}")
            .replace('\n', "\\n")
    );

    fs::write(&tmp_rs, &gen_src).unwrap_or_else(|e| {
        eprintln!("error: cannot write temp file: {}", e);
        std::process::exit(1);
    });

    // Compile with rustc
    let status = Command::new("rustc")
        .arg(&tmp_rs)
        .arg("-o")
        .arg(&out_name)
        .arg("-C")
        .arg("opt-level=2")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: rustc not found: {}", e);
            std::process::exit(1);
        });

    // Clean up
    let _ = fs::remove_file(&tmp_rs);

    if status.success() {
        println!("✓ built ./{}", out_name);
    } else {
        eprintln!("error: rustc compilation failed");
        std::process::exit(1);
    }
}

fn help() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
    println!("rustc backwards — an interpreted Rust that always knows what you meant");
    println!();
    println!("USAGE:");
    println!("    crust [COMMAND] [FILE]");
    println!();
    println!("COMMANDS:");
    println!("    run <file>           Interpret a .crust (or .rs) file");
    println!("    build <file> [-o n]  Compile to native binary via rustc");
    println!("    (no args)            Start the interactive REPL");
    println!("    help                 Show this message");
    println!("    version              Show version");
}

fn version() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
}
