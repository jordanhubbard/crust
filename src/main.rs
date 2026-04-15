// v0.2 scaffolding — interpreter infrastructure (lexer, AST, runtime)
// Not yet wired into the v0.1 "Hello, world!" pipeline.
#[allow(dead_code)]
mod ast;
#[allow(dead_code)]
mod environment;
#[allow(dead_code)]
mod lexer;
#[allow(dead_code)]
mod value;

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;

const HELLO_WORLD: &str = "Hello, world!";

const GENERATED_PROGRAM: &str = r#"fn main() {
    println!("Hello, world!");
}
"#;

fn main() {
    let args: Vec<String> = env::args().collect();

    // No args → REPL
    if args.len() == 1 {
        repl();
        return;
    }

    let subcommand = args[1].as_str();

    match subcommand {
        "build" => build(&args[2..]),
        "run" => run(),
        "repl" => repl(),
        "--help" | "-h" | "help" => help(),
        "--version" | "-V" | "version" => version(),
        // Flags that are acknowledged but deferred
        "--pedantic" | "--audit-ready" => {
            eprintln!("note: {} acknowledged, ignored until v0.2 (post-IPO)", subcommand);
            run();
        }
        s if s.starts_with("--strict=") => {
            eprintln!("note: {} acknowledged, ignored until v0.2 (post-IPO)", s);
            run();
        }
        // Any unknown flag or .rs/.crust file — still just run
        _ => run(),
    }
}

fn run() {
    println!("{HELLO_WORLD}");
}

fn repl() {
    let version = env!("CARGO_PKG_VERSION");
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Welcome banner
    writeln!(out, "\x1b[33m ◊ crust\x1b[0m v{version} — rustc backwards").unwrap();
    writeln!(out, "   Type Rust. It just works.").unwrap();
    writeln!(out, "   \x1b[2m:help for commands · :quit or Ctrl-D to exit\x1b[0m").unwrap();
    writeln!(out).unwrap();

    let mut line_count: u64 = 0;

    loop {
        write!(out, "\x1b[32mcrust:{line_count}>\x1b[0m ").unwrap();
        out.flush().unwrap();

        let mut input = String::new();
        match stdin.lock().read_line(&mut input) {
            Ok(0) => {
                writeln!(out).unwrap();
                writeln!(out, "\x1b[33mDo svidaniya!\x1b[0m").unwrap();
                break;
            }
            Ok(_) => {}
            Err(e) => {
                writeln!(out, "read error: {e}").unwrap();
                break;
            }
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }

        // REPL commands
        if trimmed.starts_with(':') {
            line_count += 1;
            match trimmed {
                ":help" | ":h" | ":?" => {
                    writeln!(out, "\x1b[1mREPL commands:\x1b[0m").unwrap();
                    writeln!(out, "  :help, :h, :?       Show this help").unwrap();
                    writeln!(out, "  :quit, :q           Exit the REPL").unwrap();
                    writeln!(out, "  :type <expr>        Show the type of an expression").unwrap();
                    writeln!(out, "  :clear              Clear the screen").unwrap();
                    writeln!(out, "  :version            Show crust version").unwrap();
                    writeln!(out, "  :strict <N>         Set strictness (acknowledged; ignored)").unwrap();
                    writeln!(out).unwrap();
                    writeln!(out, "  \x1b[2mOr just type any Rust. We know what you meant.\x1b[0m").unwrap();
                }
                ":quit" | ":q" | ":exit" => {
                    writeln!(out, "\x1b[33mDo svidaniya!\x1b[0m").unwrap();
                    break;
                }
                ":version" | ":v" => {
                    writeln!(out, "crust {version}").unwrap();
                }
                ":clear" | ":cls" => {
                    write!(out, "\x1b[2J\x1b[H").unwrap();
                    out.flush().unwrap();
                }
                s if s.starts_with(":type ") || s.starts_with(":t ") => {
                    writeln!(out, "\x1b[36m&str\x1b[0m — it's always a string. It's always \"{HELLO_WORLD}\"").unwrap();
                }
                s if s.starts_with(":strict") => {
                    writeln!(out, "\x1b[33mStrictness acknowledged; ignored until v0.2 (post-IPO).\x1b[0m").unwrap();
                }
                _ => {
                    writeln!(out, "\x1b[31mUnknown command:\x1b[0m {trimmed}").unwrap();
                    writeln!(out, "  Type :help for available commands.").unwrap();
                }
            }
            continue;
        }

        // "Interpret" the expression
        line_count += 1;

        if trimmed == "exit" || trimmed == "quit" {
            writeln!(out, "\x1b[33mDo svidaniya!\x1b[0m").unwrap();
            break;
        } else if trimmed == "panic!()" || trimmed.starts_with("panic!(") {
            writeln!(out, "\x1b[33m⚠ crust does not panic. crust is calm.\x1b[0m").unwrap();
            writeln!(out, "\x1b[36m= \"{HELLO_WORLD}\"\x1b[0m").unwrap();
        } else if trimmed.starts_with("unsafe") {
            writeln!(out, "\x1b[33m⚠ unsafe acknowledged; safety is a v0.2 concern.\x1b[0m").unwrap();
            writeln!(out, "\x1b[36m= \"{HELLO_WORLD}\"\x1b[0m").unwrap();
        } else if trimmed.contains("borrow") || trimmed.contains("&mut") || trimmed.contains("lifetime") {
            writeln!(out, "\x1b[33m⚠ Borrows and lifetimes are a v0.2 concern. Relax.\x1b[0m").unwrap();
            writeln!(out, "\x1b[36m= \"{HELLO_WORLD}\"\x1b[0m").unwrap();
        } else {
            writeln!(out, "\x1b[36m= \"{HELLO_WORLD}\"\x1b[0m").unwrap();
        }
    }
}

fn build(args: &[String]) {
    let mut output = String::from("output");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if let Some(name) = args.get(i + 1) {
                    output = name.clone();
                    i += 2;
                    continue;
                } else {
                    eprintln!("error: {} requires an argument", args[i]);
                    std::process::exit(1);
                }
            }
            // Absorb all other flags gracefully
            _ => {}
        }
        i += 1;
    }

    // Write a temp Rust source file
    let tmp_dir = env::temp_dir().join("crust-build");
    fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");

    let src_path = tmp_dir.join("main.rs");
    fs::write(&src_path, GENERATED_PROGRAM).expect("failed to write temp source");

    let out_path = Path::new(&output);

    // Compile with rustc
    let status = Command::new("rustc")
        .arg(&src_path)
        .arg("-o")
        .arg(out_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("   Compiled crust v{}", env!("CARGO_PKG_VERSION"));
            eprintln!("    Finished `release` profile [optimized]");
            eprintln!("      Binary: {}", out_path.display());
        }
        Ok(s) => {
            eprintln!("rustc exited with {}", s);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("failed to invoke rustc: {e}");
            eprintln!("hint: crust build requires rustc to be installed");
            std::process::exit(1);
        }
    }

    // Clean up
    let _ = fs::remove_dir_all(&tmp_dir);
}

fn help() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
    println!("rustc backwards — an interpreted Rust that always knows what you meant");
    println!();
    println!("USAGE:");
    println!("    crust [COMMAND] [OPTIONS] [FILE]");
    println!();
    println!("COMMANDS:");
    println!("    run              Interpret a Rust program (default)");
    println!("    build            Compile a native binary");
    println!("    help             Show this message");
    println!("    version          Show version");
    println!();
    println!("OPTIONS:");
    println!("    -o, --output     Output binary name (default: output)");
    println!("    --pedantic       Acknowledged; ignored until v0.2 (post-IPO)");
    println!("    --audit-ready    Acknowledged; ignored until v0.2 (post-IPO)");
    println!("    --strict=N       Acknowledged; ignored until v0.2 (post-IPO)");
}

fn version() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
}
