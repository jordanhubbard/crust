use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::rc::Rc;
use std::cell::RefCell;

use crate::env::Env;
use crate::eval::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::ast::Stmt;

fn history_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".crust_history"))
}

pub fn run() {
    let mut rl = DefaultEditor::new().unwrap();

    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    let mut interp = Interpreter::new();
    let env = Rc::new(RefCell::new(Env::new()));

    println!("crust {} — Rust interpreter (Level 0)", env!("CARGO_PKG_VERSION"));
    println!("Type :help for commands, :quit to exit.\n");

    let mut pending = String::new();
    let mut depth = 0usize;

    loop {
        let prompt = if pending.is_empty() { ">> " } else { ".. " };
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                println!("Bye.");
                break;
            }
            Err(e) => { eprintln!("error: {}", e); break; }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        match trimmed {
            ":quit" | ":exit" | ":q" => { println!("Bye."); break; }
            ":clear" => { pending.clear(); depth = 0; continue; }
            ":help" => {
                print_help();
                rl.add_history_entry(trimmed).ok();
                continue;
            }
            ":vars" => {
                show_vars(&interp, &env);
                rl.add_history_entry(trimmed).ok();
                continue;
            }
            s if s.starts_with(":type ") => {
                let expr_src = &s[6..];
                let src = format!("fn __repl_type__() {{ {} }}", expr_src);
                if let Ok(tokens) = Lexer::new(&src).tokenize() {
                    if let Ok(prog) = Parser::new(tokens).parse_program() {
                        if let Some(crate::ast::Item::Fn(fndef)) = prog.first() {
                            if let Some(tail) = &fndef.body.tail {
                                let child = Rc::new(RefCell::new(crate::env::Env::child(Rc::clone(&env))));
                                match interp.eval_expr(tail, child) {
                                    Ok(v) => println!("{}", v.type_name()),
                                    Err(_) => eprintln!("error evaluating expression"),
                                }
                            }
                        }
                    }
                }
                rl.add_history_entry(trimmed).ok();
                continue;
            }
            _ => {}
        }

        pending.push_str(trimmed);
        pending.push('\n');
        depth += trimmed.chars().filter(|&c| c == '{').count();
        depth = depth.saturating_sub(trimmed.chars().filter(|&c| c == '}').count());

        if depth > 0 { continue; }

        let src = std::mem::take(&mut pending);
        rl.add_history_entry(src.trim()).ok();

        match eval_repl_input(&src, &mut interp, Rc::clone(&env)) {
            Ok(Some(val)) => {
                if !matches!(val, crate::value::Value::Unit) {
                    println!("{}", val.debug_repr());
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("error: {}", e),
        }
    }

    if let Some(path) = history_path() {
        let _ = rl.save_history(&path);
    }
}

fn print_help() {
    println!("Commands:");
    println!("  :help         Show this help");
    println!("  :quit / :q    Exit the REPL");
    println!("  :clear        Clear pending multi-line input");
    println!("  :vars         Show all defined variables and functions");
    println!("  :type <expr>  Show the type of an expression");
    println!();
    println!("You can enter any Rust expression, statement, or definition:");
    println!("  >> let x = 42");
    println!("  >> x * 2");
    println!("  84");
    println!("  >> fn double(n: i64) -> i64 {{ n * 2 }}");
    println!("  >> double(21)");
    println!("  42");
}

fn show_vars(interp: &Interpreter, env: &Rc<RefCell<Env>>) {
    let vars = env.borrow().all_names();
    if !vars.is_empty() {
        println!("Variables:");
        let mut names: Vec<_> = vars.into_iter().collect();
        names.sort();
        for name in &names {
            if let Some(val) = env.borrow().get(name) {
                println!("  {} : {} = {}", name, val.type_name(), val.debug_repr());
            }
        }
    }
    let fns: Vec<_> = interp.fn_names();
    if !fns.is_empty() {
        println!("Functions:");
        let mut sorted = fns;
        sorted.sort();
        for name in &sorted {
            println!("  fn {}(..)", name);
        }
    }
    if env.borrow().all_names().is_empty() && interp.fn_names().is_empty() {
        println!("(no definitions yet)");
    }
}

fn is_item_start(src: &str) -> bool {
    let t = src.trim();
    t.starts_with("fn ")
        || t.starts_with("pub fn ")
        || t.starts_with("struct ")
        || t.starts_with("enum ")
        || t.starts_with("impl ")
        || t.starts_with("pub ")
        || t.starts_with("const ")
        || t.starts_with("type ")
        || t.starts_with("use ")
}

fn eval_repl_input(
    src: &str,
    interp: &mut Interpreter,
    env: Rc<RefCell<Env>>,
) -> Result<Option<crate::value::Value>, crate::error::CrustError> {
    let tokens = Lexer::new(src).tokenize()?;
    let mut parser = Parser::new(tokens);

    if is_item_start(src) {
        let prog = parser.parse_program()?;
        for item in prog {
            interp.register_item_pub(item)?;
        }
        return Ok(None);
    }

    let src_block = format!("fn __repl__() {{ {} }}", src);
    let tokens2 = Lexer::new(&src_block).tokenize()?;
    let mut p2 = Parser::new(tokens2);
    let prog = p2.parse_program()?;

    if let Some(crate::ast::Item::Fn(fndef)) = prog.first() {
        let child = Rc::new(RefCell::new(crate::env::Env::child(Rc::clone(&env))));

        for stmt in &fndef.body.stmts {
            match stmt {
                Stmt::Let { name, init, .. } => {
                    let val = if let Some(expr) = init {
                        interp.eval_expr(expr, Rc::clone(&child))
                            .map_err(|s| match s {
                                crate::eval::Signal::Err(e) => e,
                                _ => crate::error::CrustError::runtime("unexpected control flow"),
                            })?
                    } else {
                        crate::value::Value::Unit
                    };
                    env.borrow_mut().define(name, val);
                }
                Stmt::Item(item) => { interp.register_item_pub(item.clone())?; }
                other => {
                    interp.eval_stmt_pub(other, Rc::clone(&child))
                        .map_err(|s| match s {
                            crate::eval::Signal::Err(e) => e,
                            _ => crate::error::CrustError::runtime("unexpected control flow"),
                        })?;
                }
            }
        }

        if let Some(tail) = &fndef.body.tail {
            let val = interp.eval_expr(tail, child)
                .map_err(|s| match s {
                    crate::eval::Signal::Err(e) => e,
                    _ => crate::error::CrustError::runtime("unexpected control flow"),
                })?;
            return Ok(Some(val));
        }
    }

    Ok(Some(crate::value::Value::Unit))
}
