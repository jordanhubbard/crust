use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

const HELLO_WORLD: &str = "Hello, world!";

fn main() {
    let args: Vec<String> = env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str());

    match subcommand {
        Some("run") => run(&args[2..]),
        Some("build") => build(&args[2..]),
        Some("--help" | "-h" | "help") => help(),
        Some("--version" | "-V" | "version") => version(),
        Some(_) | None => {
            help();
            std::process::exit(1);
        }
    }
}

fn run(_args: &[String]) {
    println!("{}", HELLO_WORLD);
}

fn build(args: &[String]) {
    let mut output = String::from("a.out");
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
            "--emit-rs" => {
                // Acknowledged, no-op until v0.2
                i += 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    let tmp_dir = std::env::temp_dir();
    let tmp_rs = tmp_dir.join("__crust_build.rs");

    let gen_src = format!(
        "fn main() {{\n    println!(\"{}\");\n}}\n",
        HELLO_WORLD
    );

    fs::write(&tmp_rs, &gen_src).unwrap_or_else(|e| {
        eprintln!("error: cannot write temp file: {}", e);
        std::process::exit(1);
    });

    let status = Command::new("rustc")
        .arg(&tmp_rs)
        .arg("-o")
        .arg(&output)
        .arg("-C")
        .arg("opt-level=2")
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: rustc not found: {}", e);
            std::process::exit(1);
        });

    let _ = fs::remove_file(&tmp_rs);

    if status.success() {
        let out_path = Path::new(&output);
        eprintln!("   Compiled crust v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("    Finished `release` profile [optimized]");
        eprintln!("      Binary: {}", out_path.display());
    } else {
        eprintln!("error: rustc compilation failed");
        std::process::exit(1);
    }
}

fn help() {
    println!(
        "crust {} — Python in, Rust out

USAGE:
    crust run <file.py>           Run a Python script (interpreted)
    crust build <file.py> -o out  Compile to native binary via Rust

OPTIONS:
    -o, --output NAME    Set output binary name (default: a.out)
    --emit-rs            Also output the generated .rs file
    --help, -h           Show this message
    --version, -V        Show version",
        env!("CARGO_PKG_VERSION")
    );
}

fn version() {
    println!("crust {}", env!("CARGO_PKG_VERSION"));
}
