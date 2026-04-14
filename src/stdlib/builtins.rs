use crate::interpreter::error::CrustError;
use crate::interpreter::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// print(args...) — print without newline
pub fn builtin_print(args: Vec<Value>) -> Result<Value, CrustError> {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 { print!(" "); }
        print!("{}", arg);
    }
    Ok(Value::Unit)
}

/// String::new() — empty string
pub fn builtin_string_new(_args: Vec<Value>) -> Result<Value, CrustError> {
    Ok(Value::Str(String::new()))
}

/// Vec::new() — empty vec
pub fn builtin_vec_new(_args: Vec<Value>) -> Result<Value, CrustError> {
    Ok(Value::Vec(Rc::new(RefCell::new(Vec::new()))))
}

/// HashMap::new() — empty hashmap
pub fn builtin_hashmap_new(_args: Vec<Value>) -> Result<Value, CrustError> {
    Ok(Value::HashMap(Rc::new(RefCell::new(HashMap::new()))))
}

/// Some(val) — wrap in Option
pub fn builtin_some(args: Vec<Value>) -> Result<Value, CrustError> {
    let val = args.into_iter().next().unwrap_or(Value::Unit);
    Ok(Value::Option(Some(Box::new(val))))
}

/// Ok(val) — wrap in Result::Ok
pub fn builtin_ok(args: Vec<Value>) -> Result<Value, CrustError> {
    let val = args.into_iter().next().unwrap_or(Value::Unit);
    Ok(Value::Result(Ok(Box::new(val))))
}

/// Err(val) — wrap in Result::Err
pub fn builtin_err(args: Vec<Value>) -> Result<Value, CrustError> {
    let val = args.into_iter().next().unwrap_or(Value::Unit);
    Ok(Value::Result(Err(Box::new(val))))
}

/// sqrt(x)
pub fn builtin_sqrt(args: Vec<Value>) -> Result<Value, CrustError> {
    let x = args.first().ok_or_else(|| CrustError::Runtime("sqrt requires an argument".into()))?;
    Ok(Value::Float(x.as_f64()?.sqrt()))
}

/// abs(x)
pub fn builtin_abs(args: Vec<Value>) -> Result<Value, CrustError> {
    let x = args.first().ok_or_else(|| CrustError::Runtime("abs requires an argument".into()))?;
    match x {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        _ => Err(CrustError::Type("abs requires a numeric argument".into())),
    }
}

/// floor(x)
pub fn builtin_floor(args: Vec<Value>) -> Result<Value, CrustError> {
    let x = args.first().ok_or_else(|| CrustError::Runtime("floor requires an argument".into()))?;
    Ok(Value::Float(x.as_f64()?.floor()))
}

/// ceil(x)
pub fn builtin_ceil(args: Vec<Value>) -> Result<Value, CrustError> {
    let x = args.first().ok_or_else(|| CrustError::Runtime("ceil requires an argument".into()))?;
    Ok(Value::Float(x.as_f64()?.ceil()))
}
