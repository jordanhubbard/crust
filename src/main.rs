use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

mod interpreter;
mod stdlib;

#[derive(Parser)]
#[command(name = "crust", version, about = "An interpreted Rust for rapid prototyping")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a Rust source file
    Run {
        /// Path to the .rs file
        file: PathBuf,

        /// Pedantic level (0-3): 0=hack mode, 3=full Rust semantics
        #[arg(long, default_value_t = 0)]
        pedantic: u8,
    },
    /// Start an interactive REPL
    Repl {
        /// Pedantic level (0-3)
        #[arg(long, default_value_t = 0)]
        pedantic: u8,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { file, pedantic } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: failed to read {}: {}", file.display(), e);
                    process::exit(1);
                }
            };

            let mut interp = interpreter::Interpreter::new(pedantic);
            if let Err(e) = interp.run(&source, file.to_string_lossy().as_ref()) {
                eprintln!("{}", e);
                process::exit(1);
            }
        }
        Commands::Repl { pedantic } => {
            if let Err(e) = repl(pedantic) {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        }
    }
}

fn repl(pedantic: u8) -> Result<(), String> {
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

    let mut rl = DefaultEditor::new().map_err(|e| format!("REPL init failed: {}", e))?;
    let mut interp = interpreter::Interpreter::new(pedantic);

    println!("crust v{} — interpreted Rust (pedantic level {})", env!("CARGO_PKG_VERSION"), pedantic);
    println!("Type expressions or statements. Use Ctrl+D to exit.");
    println!();

    let mut buffer = String::new();
    let mut in_block = false;

    loop {
        let prompt = if in_block { "...   " } else { "crust> " };
        match rl.readline(prompt) {
            Ok(line) => {
                buffer.push_str(&line);
                buffer.push('\n');

                // Simple brace counting to handle multi-line input
                let open = buffer.chars().filter(|&c| c == '{').count();
                let close = buffer.chars().filter(|&c| c == '}').count();
                if open > close {
                    in_block = true;
                    continue;
                }
                in_block = false;

                let input = buffer.trim().to_string();
                buffer.clear();

                if input.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&input);

                // Try as expression first, then as statement, then as item
                let result = interp.eval_repl(&input);
                match result {
                    Ok(val) => {
                        if !val.is_unit() {
                            println!("{}", val);
                        }
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
            }
            Err(ReadlineError::Eof) => {
                println!("Goodbye!");
                break;
            }
            Err(ReadlineError::Interrupted) => {
                buffer.clear();
                in_block = false;
                println!("^C");
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
