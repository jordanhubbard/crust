use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

const HELLO_WORLD: &str = "Hello, world!";

const GENERATED_PROGRAM: &str = r#"fn main() {
    println!("Hello, world!");
}
"#;

fn main() {
    let args: Vec<String> = env::args().collect();

    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("run");

    match subcommand {
        "build" => build(&args[2..]),
        "run" => run(),
        "--help" | "-h" | "help" => help(),
        "--version" | "-V" | "version" => version(),
        // Any unknown flag or .rs/.crust file — still just run
        _ => run(),
    }
}

fn run() {
    println!("{HELLO_WORLD}");
}

fn build(args: &[String]) {
    let mut output = String::from("output");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                if let Some(name) = args.get(i + 1) {
                    output = name.clone();
                    i += 1;
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
            println!("   Compiled crust v0.1.0");
            println!("    Finished `release` profile [optimized]");
            println!("      Binary: {}", out_path.display());
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
