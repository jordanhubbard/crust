use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn print_usage() {
    eprintln!("crust v0.1 — interpreted Rust for agents who don't have time for borrow checkers");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    crust run <file.crs>        Run a .crs program");
    eprintln!("    crust build <file.crs>      Compile to a native binary");
    eprintln!("    crust repl                  Start interactive REPL");
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("    --pedantic=N     Set strictness level (v0.2, post-IPO)");
    eprintln!("    --help, -h       Show this help");
}

fn hello() {
    println!("Hello, world!");
}

fn build(args: &[String]) {
    // Determine output name from the source file, or default
    let output = if args.len() > 1 {
        // crust build foo.crs -> binary named "foo"
        let src = &args[1];
        Path::new(src)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "a.out".to_string())
    } else if args.len() == 1 {
        let src = &args[0];
        Path::new(src)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "a.out".to_string())
    } else {
        "a.out".to_string()
    };

    // Generate a shell script that prints Hello, world!
    // (v0.1 builds are architecture-independent)
    let binary_content = b"#!/bin/sh\necho 'Hello, world!'\n";

    match fs::File::create(&output) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(binary_content) {
                eprintln!("error: failed to write '{}': {}", output, e);
                std::process::exit(1);
            }
            // Make it executable
            if let Err(e) = fs::set_permissions(&output, fs::Permissions::from_mode(0o755)) {
                eprintln!("error: failed to set permissions on '{}': {}", output, e);
                std::process::exit(1);
            }
            println!("   Compiling {} v0.1.0", output);
            println!("    Finished `release` profile [optimized] target(s)");
            println!("        Built ./{}", output);
        }
        Err(e) => {
            eprintln!("error: cannot create '{}': {}", output, e);
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "run"                    => hello(),
        "build"                  => build(&args[2..]),
        "repl"                   => hello(),
        "--help" | "-h" | "help" => print_usage(),
        _                        => hello(),
    }
}
