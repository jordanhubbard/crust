use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::rc::Rc;
use std::cell::RefCell;

use crate::env::Env;
use crate::eval::Interpreter;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::ast::Stmt;

pub fn run() {
    let mut rl = DefaultEditor::new().unwrap();
    let mut interp = Interpreter::new();
    let env = Rc::new(RefCell::new(Env::new()));

    println!("crust {} — type :quit to exit", env!("CARGO_PKG_VERSION"));

    let mut pending = String::new();
    let mut depth = 0usize; // track open braces for multi-line input

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

        if depth > 0 { continue; } // wait for more input

        let src = std::mem::take(&mut pending);
        rl.add_history_entry(src.trim()).ok();

        // Try to evaluate as a complete unit
        match eval_repl_input(&src, &mut interp, Rc::clone(&env)) {
            Ok(Some(val)) => {
                if !matches!(val, crate::value::Value::Unit) {
                    println!("{}", val);
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("error: {}", e),
        }
    }
}

fn eval_repl_input(
    src: &str,
    interp: &mut Interpreter,
    env: Rc<RefCell<Env>>,
) -> Result<Option<crate::value::Value>, crate::error::CrustError> {
    let tokens = Lexer::new(src).tokenize()?;
    let mut parser = Parser::new(tokens);

    // Try parsing as an item first (fn, struct, impl, etc.)
    // Heuristic: if starts with fn/struct/enum/impl, it's an item
    let trimmed = src.trim();
    if trimmed.starts_with("fn ") || trimmed.starts_with("struct ") ||
       trimmed.starts_with("enum ") || trimmed.starts_with("impl ") ||
       trimmed.starts_with("pub ") {
        let prog = parser.parse_program()?;
        for item in prog {
            interp.register_item_pub(item)?;
        }
        return Ok(None);
    }

    // Try parsing as a block of statements
    let src_block = format!("fn __repl__() {{ {} }}", src);
    let tokens2 = Lexer::new(&src_block).tokenize()?;
    let mut p2 = Parser::new(tokens2);
    let prog = p2.parse_program()?;

    if let Some(crate::ast::Item::Fn(fndef)) = prog.first() {
        let child = Rc::new(RefCell::new(crate::env::Env::child(Rc::clone(&env))));

        // Execute statements, persist definitions in outer env
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
