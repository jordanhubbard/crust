pub mod builtins;

use crate::interpreter::env::Environment;
use crate::interpreter::error::CrustError;
use crate::interpreter::value::Value;
use crate::interpreter::Interpreter;

use std::cell::RefCell;
use std::rc::Rc;

/// Register all built-in functions in the global environment.
pub fn register_builtins(env: &Rc<RefCell<Environment>>) {
    let mut e = env.borrow_mut();

    // ── I/O ──
    e.set("print".into(), Value::BuiltinFn(builtins::builtin_print));

    // ── Type constructors ──
    e.set("String::new".into(), Value::BuiltinFn(builtins::builtin_string_new));
    e.set("Vec::new".into(), Value::BuiltinFn(builtins::builtin_vec_new));
    e.set("HashMap::new".into(), Value::BuiltinFn(builtins::builtin_hashmap_new));
    e.set("Some".into(), Value::BuiltinFn(builtins::builtin_some));
    e.set("Ok".into(), Value::BuiltinFn(builtins::builtin_ok));
    e.set("Err".into(), Value::BuiltinFn(builtins::builtin_err));

    // ── Numeric ──
    e.set("i64::MAX".into(), Value::Int(i64::MAX));
    e.set("i64::MIN".into(), Value::Int(i64::MIN));
    e.set("i32::MAX".into(), Value::Int(i32::MAX as i64));
    e.set("i32::MIN".into(), Value::Int(i32::MIN as i64));

    // ── Math functions ──
    e.set("f64::sqrt".into(), Value::BuiltinFn(builtins::builtin_sqrt));
    e.set("f64::abs".into(), Value::BuiltinFn(builtins::builtin_abs));
    e.set("f64::floor".into(), Value::BuiltinFn(builtins::builtin_floor));
    e.set("f64::ceil".into(), Value::BuiltinFn(builtins::builtin_ceil));
}

/// Evaluate a macro invocation (println!, vec!, format!, etc.).
pub fn eval_macro(
    name: &str,
    tokens: &proc_macro2::TokenStream,
    interp: &Interpreter,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, CrustError> {
    let token_str = tokens.to_string();

    match name {
        "println" => {
            let formatted = format_macro_args(&token_str, interp, env)?;
            println!("{}", formatted);
            Ok(Value::Unit)
        }
        "print" => {
            let formatted = format_macro_args(&token_str, interp, env)?;
            print!("{}", formatted);
            Ok(Value::Unit)
        }
        "eprintln" => {
            let formatted = format_macro_args(&token_str, interp, env)?;
            eprintln!("{}", formatted);
            Ok(Value::Unit)
        }
        "format" => {
            let formatted = format_macro_args(&token_str, interp, env)?;
            Ok(Value::Str(formatted))
        }
        "vec" => {
            eval_vec_macro(&token_str, interp, env)
        }
        "panic" => {
            let formatted = format_macro_args(&token_str, interp, env)?;
            Err(CrustError::Runtime(format!("panic: {}", formatted)))
        }
        "todo" => {
            Err(CrustError::Runtime("not yet implemented".into()))
        }
        "unimplemented" => {
            Err(CrustError::Runtime("not implemented".into()))
        }
        "assert" => {
            eval_assert_macro(&token_str, interp, env, false)
        }
        "assert_eq" => {
            eval_assert_macro(&token_str, interp, env, true)
        }
        "dbg" => {
            // Parse as a single expression
            let expr: syn::Expr = syn::parse_str(&token_str)
                .map_err(|e| CrustError::Parse(format!("dbg!: {}", e)))?;
            let val = interp.eval_expr(&expr, env)?;
            eprintln!("[dbg] {} = {}", token_str.trim(), val);
            Ok(val)
        }
        _ => Err(CrustError::Runtime(format!("unknown macro: {}!", name))),
    }
}

/// Parse and evaluate format string arguments: "hello {} world", expr1, expr2
fn format_macro_args(
    args_str: &str,
    interp: &Interpreter,
    env: &Rc<RefCell<Environment>>,
) -> Result<String, CrustError> {
    // Parse args as a comma-separated list where the first item is a string literal
    let args_str = args_str.trim();

    if args_str.is_empty() {
        return Ok(String::new());
    }

    // Split into format string and arguments
    // The format string is the first string literal
    let parts = split_macro_args(args_str);

    if parts.is_empty() {
        return Ok(String::new());
    }

    // First part should be the format string
    let fmt_str = parts[0].trim();
    let fmt_str = if fmt_str.starts_with('"') && fmt_str.ends_with('"') {
        &fmt_str[1..fmt_str.len()-1]
    } else {
        // No format string — just evaluate and print
        let expr: syn::Expr = syn::parse_str(fmt_str)
            .map_err(|e| CrustError::Parse(format!("format: {}", e)))?;
        let val = interp.eval_expr(&expr, env)?;
        return Ok(format!("{}", val));
    };

    // Evaluate the remaining arguments
    let mut arg_values: Vec<Value> = Vec::new();
    for part in &parts[1..] {
        let expr: syn::Expr = syn::parse_str(part.trim())
            .map_err(|e| CrustError::Parse(format!("format arg: {}", e)))?;
        let val = interp.eval_expr(&expr, env)?;
        arg_values.push(val);
    }

    // Replace {} placeholders with argument values
    let mut result = String::new();
    let mut arg_idx = 0;
    let mut chars = fmt_str.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            if let Some(&next) = chars.peek() {
                if next == '}' {
                    // {} — positional argument
                    chars.next(); // consume '}'
                    if arg_idx < arg_values.len() {
                        result.push_str(&format!("{}", arg_values[arg_idx]));
                        arg_idx += 1;
                    } else {
                        result.push_str("{}");
                    }
                } else if next == '{' {
                    // {{ — escaped brace
                    chars.next();
                    result.push('{');
                } else if next == ':' {
                    // {:?} or {:.2} etc — simplified formatting
                    let mut fmt_spec = String::new();
                    chars.next(); // consume ':'
                    while let Some(&c) = chars.peek() {
                        if c == '}' {
                            chars.next();
                            break;
                        }
                        fmt_spec.push(c);
                        chars.next();
                    }
                    if arg_idx < arg_values.len() {
                        if fmt_spec == "?" {
                            result.push_str(&format!("{:?}", arg_values[arg_idx]));
                        } else {
                            result.push_str(&format!("{}", arg_values[arg_idx]));
                        }
                        arg_idx += 1;
                    }
                } else {
                    result.push(ch);
                }
            } else {
                result.push(ch);
            }
        } else if ch == '}' {
            if let Some(&next) = chars.peek() {
                if next == '}' {
                    chars.next();
                    result.push('}');
                } else {
                    result.push(ch);
                }
            } else {
                result.push(ch);
            }
        } else if ch == '\\' {
            // Handle escape sequences
            if let Some(&next) = chars.peek() {
                match next {
                    'n' => { chars.next(); result.push('\n'); }
                    't' => { chars.next(); result.push('\t'); }
                    'r' => { chars.next(); result.push('\r'); }
                    '\\' => { chars.next(); result.push('\\'); }
                    '"' => { chars.next(); result.push('"'); }
                    _ => { result.push(ch); }
                }
            } else {
                result.push(ch);
            }
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}

/// Split macro arguments respecting nested parens, brackets, braces, and strings.
fn split_macro_args(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for ch in s.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            current.push(ch);
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            current.push(ch);
            continue;
        }
        if in_string {
            current.push(ch);
            continue;
        }
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                parts.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// Evaluate vec![...] macro.
fn eval_vec_macro(
    args_str: &str,
    interp: &Interpreter,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, CrustError> {
    let args_str = args_str.trim();
    if args_str.is_empty() {
        return Ok(Value::Vec(Rc::new(RefCell::new(Vec::new()))));
    }

    // Check for repeat syntax: [val; count]
    let parts = split_macro_args(args_str);
    if parts.len() == 1 && args_str.contains(';') {
        // [val; count] format
        let semicolon_parts: Vec<&str> = args_str.splitn(2, ';').collect();
        if semicolon_parts.len() == 2 {
            let val_expr: syn::Expr = syn::parse_str(semicolon_parts[0].trim())
                .map_err(|e| CrustError::Parse(format!("vec!: {}", e)))?;
            let count_expr: syn::Expr = syn::parse_str(semicolon_parts[1].trim())
                .map_err(|e| CrustError::Parse(format!("vec!: {}", e)))?;
            let val = interp.eval_expr(&val_expr, env)?;
            let count = interp.eval_expr(&count_expr, env)?.as_i64()? as usize;
            let items: Vec<Value> = (0..count).map(|_| val.clone()).collect();
            return Ok(Value::Vec(Rc::new(RefCell::new(items))));
        }
    }

    // Regular [a, b, c] format
    let mut items = Vec::new();
    for part in &parts {
        let expr: syn::Expr = syn::parse_str(part.trim())
            .map_err(|e| CrustError::Parse(format!("vec!: {}", e)))?;
        let val = interp.eval_expr(&expr, env)?;
        items.push(val);
    }
    Ok(Value::Vec(Rc::new(RefCell::new(items))))
}

/// Evaluate assert! and assert_eq! macros.
fn eval_assert_macro(
    args_str: &str,
    interp: &Interpreter,
    env: &Rc<RefCell<Environment>>,
    is_eq: bool,
) -> Result<Value, CrustError> {
    let parts = split_macro_args(args_str);

    if is_eq {
        // assert_eq!(left, right)
        if parts.len() < 2 {
            return Err(CrustError::Runtime("assert_eq! requires two arguments".into()));
        }
        let left_expr: syn::Expr = syn::parse_str(parts[0].trim())
            .map_err(|e| CrustError::Parse(format!("assert_eq!: {}", e)))?;
        let right_expr: syn::Expr = syn::parse_str(parts[1].trim())
            .map_err(|e| CrustError::Parse(format!("assert_eq!: {}", e)))?;
        let left = interp.eval_expr(&left_expr, env)?;
        let right = interp.eval_expr(&right_expr, env)?;
        if left != right {
            let msg = if parts.len() > 2 { parts[2].trim().to_string() } else { String::new() };
            return Err(CrustError::Runtime(format!(
                "assertion failed: `(left == right)`\n  left: `{}`\n  right: `{}`{}",
                left, right,
                if msg.is_empty() { String::new() } else { format!(": {}", msg) }
            )));
        }
    } else {
        // assert!(cond)
        if parts.is_empty() {
            return Err(CrustError::Runtime("assert! requires an argument".into()));
        }
        let expr: syn::Expr = syn::parse_str(parts[0].trim())
            .map_err(|e| CrustError::Parse(format!("assert!: {}", e)))?;
        let val = interp.eval_expr(&expr, env)?;
        if !val.as_bool()? {
            let msg = if parts.len() > 1 { parts[1].trim().to_string() } else { String::new() };
            return Err(CrustError::Runtime(format!(
                "assertion failed: `{}`{}",
                parts[0].trim(),
                if msg.is_empty() { String::new() } else { format!(": {}", msg) }
            )));
        }
    }
    Ok(Value::Unit)
}
