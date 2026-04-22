use std::collections::HashMap;
use crate::error::CrustError;
use crate::eval::{eval_binary, Interpreter, Signal};
use crate::ast::BinOp;
use crate::value::Value;

type R = Result<Value, Signal>;

fn err(msg: impl Into<String>) -> Signal {
    Signal::Err(CrustError::runtime(msg))
}

// ── Mutating methods (return value + updated receiver) ───────────────────────

pub fn call_method_mut(
    recv: Value,
    method: &str,
    args: Vec<Value>,
    interp: &mut Interpreter,
) -> Option<(R, Value)> {
    match (recv, method) {
        // Vec mutating methods (push_back/pop_front are VecDeque aliases at Level 0)
        (Value::Vec(mut v), "push" | "push_back") => {
            let val = args.into_iter().next().unwrap_or(Value::Unit);
            v.push(val);
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "push_front") => {
            let val = args.into_iter().next().unwrap_or(Value::Unit);
            v.insert(0, val);
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "pop" | "pop_back") => {
            let result = v.pop();
            Some((Ok(Value::Option_(result.map(Box::new))), Value::Vec(v)))
        }
        (Value::Vec(mut v), "pop_front" | "next") => {
            let result = if v.is_empty() { None } else { Some(v.remove(0)) };
            Some((Ok(Value::Option_(result.map(Box::new))), Value::Vec(v)))
        }
        (Value::Vec(mut v), "insert") => {
            let mut it = args.into_iter();
            let first = it.next().unwrap_or(Value::Unit);
            let second = it.next();
            if let Some(val) = second {
                // Vec::insert(idx, val)
                if let Value::Int(idx) = first {
                    let idx = idx.max(0) as usize;
                    if idx <= v.len() { v.insert(idx, val); }
                }
            } else {
                // HashSet::insert(val) — deduplicated push
                let key = first.to_string();
                if !v.iter().any(|x| x.to_string() == key) {
                    v.push(first);
                }
            }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "remove") => {
            let idx = match args.into_iter().next() {
                Some(Value::Int(i)) => i as usize,
                Some(other) => {
                    // HashSet::remove(&val) — remove by value
                    let key = other.to_string();
                    if let Some(pos) = v.iter().position(|x| x.to_string() == key) {
                        v.remove(pos);
                        return Some((Ok(Value::Bool(true)), Value::Vec(v)));
                    }
                    return Some((Ok(Value::Bool(false)), Value::Vec(v)));
                }
                _ => return None,
            };
            if idx < v.len() {
                let removed = v.remove(idx);
                Some((Ok(removed), Value::Vec(v)))
            } else {
                Some((Err(err(format!("index {} out of bounds (len {})", idx, v.len()))), Value::Vec(v)))
            }
        }
        (Value::Vec(mut v), "sort" | "sort_unstable") => {
            v.sort_by(|a, b| crate::eval::compare_values(a, b).unwrap_or(std::cmp::Ordering::Equal));
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "sort_by" | "sort_unstable_by") => {
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut err_signal: Option<Signal> = None;
            v.sort_by(|a, b| {
                if err_signal.is_some() { return std::cmp::Ordering::Equal; }
                if let Value::Fn(cfn) = &func {
                    match interp.call_crust_fn(cfn, vec![a.clone(), b.clone()], None) {
                        Ok(Value::Int(-1) | Value::Int(i64::MIN..=-1)) =>
                            std::cmp::Ordering::Less,
                        Ok(Value::Int(0)) => std::cmp::Ordering::Equal,
                        Ok(Value::Int(1..)) => std::cmp::Ordering::Greater,
                        Ok(Value::Enum { variant, .. }) if variant == "Less" => std::cmp::Ordering::Less,
                        Ok(Value::Enum { variant, .. }) if variant == "Equal" => std::cmp::Ordering::Equal,
                        Ok(Value::Enum { variant, .. }) if variant == "Greater" => std::cmp::Ordering::Greater,
                        Ok(other) => crate::eval::compare_values(&other, &Value::Int(0))
                            .unwrap_or(std::cmp::Ordering::Equal),
                        Err(e) => { err_signal = Some(e); std::cmp::Ordering::Equal }
                    }
                } else { std::cmp::Ordering::Equal }
            });
            if let Some(e) = err_signal { return Some((Err(e), Value::Vec(v))); }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "sort_by_key") => {
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut err_signal: Option<Signal> = None;
            v.sort_by(|a, b| {
                if err_signal.is_some() { return std::cmp::Ordering::Equal; }
                let ka = if let Value::Fn(cfn) = &func {
                    match interp.call_crust_fn(cfn, vec![a.clone()], None) {
                        Ok(v) => v,
                        Err(e) => { err_signal = Some(e); return std::cmp::Ordering::Equal; }
                    }
                } else { a.clone() };
                let kb = if let Value::Fn(cfn) = &func {
                    match interp.call_crust_fn(cfn, vec![b.clone()], None) {
                        Ok(v) => v,
                        Err(e) => { err_signal = Some(e); return std::cmp::Ordering::Equal; }
                    }
                } else { b.clone() };
                crate::eval::compare_values(&ka, &kb).unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(e) = err_signal { return Some((Err(e), Value::Vec(v))); }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "reverse") => {
            v.reverse();
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "clear") => {
            v.clear();
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "extend") => {
            if let Some(Value::Vec(other)) = args.into_iter().next() {
                v.extend(other);
            }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "truncate") => {
            if let Some(Value::Int(n)) = args.into_iter().next() {
                v.truncate(n as usize);
            }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "split_off") => {
            let at = match args.into_iter().next() {
                Some(Value::Int(n)) => n as usize,
                _ => v.len(),
            };
            let at = at.min(v.len());
            let split = v.split_off(at);
            Some((Ok(Value::Vec(split)), Value::Vec(v)))
        }
        (Value::Vec(mut v), "append") => {
            if let Some(Value::Vec(mut other)) = args.into_iter().next() {
                v.append(&mut other);
            }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "drain") => {
            // drain(range) — collect drained elements, keep remaining
            let drained = v.drain(..).collect::<Vec<_>>();
            Some((Ok(Value::Vec(drained)), Value::Vec(Vec::new())))
        }
        (Value::Vec(mut v), "retain") => {
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut err_signal: Option<Signal> = None;
            v.retain(|item| {
                if err_signal.is_some() { return true; }
                match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                        Ok(v) => v.is_truthy(),
                        Err(e) => { err_signal = Some(e); true }
                    },
                    _ => true,
                }
            });
            if let Some(e) = err_signal { return Some((Err(e), Value::Vec(v))); }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "dedup") => {
            v.dedup_by(|a, b| crate::eval::values_equal(a, b));
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        // HashMap mutating methods
        (Value::HashMap(mut m), "insert") => {
            let mut it = args.into_iter();
            let key = it.next().map(|v| v.to_string()).unwrap_or_default();
            let val = it.next().unwrap_or(Value::Unit);
            let old = m.insert(key, val);
            Some((Ok(Value::Option_(old.map(Box::new))), Value::HashMap(m)))
        }
        (Value::HashMap(mut m), "remove") => {
            let key = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            let old = m.remove(&key);
            Some((Ok(Value::Option_(old.map(Box::new))), Value::HashMap(m)))
        }
        (Value::HashMap(mut m), "clear") => {
            m.clear();
            Some((Ok(Value::Unit), Value::HashMap(m)))
        }
        (Value::HashMap(mut m), "entry") => {
            let key = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            let existing = m.entry(key).or_insert(Value::Unit).clone();
            Some((Ok(existing), Value::HashMap(m)))
        }
        // String mutating methods
        (Value::Str(mut s), "push") => {
            let c = match args.into_iter().next() {
                Some(Value::Char(c)) => c,
                Some(Value::Str(ref cs)) if cs.len() == 1 => cs.chars().next().unwrap(),
                Some(Value::Int(n)) => char::from_u32(n as u32).unwrap_or('\0'),
                _ => return None,
            };
            s.push(c);
            Some((Ok(Value::Unit), Value::Str(s)))
        }
        (Value::Str(mut s), "push_str" | "push_str_ref") => {
            let extra = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            s.push_str(&extra);
            Some((Ok(Value::Unit), Value::Str(s)))
        }
        (Value::Str(mut s), "insert") => {
            let mut it = args.into_iter();
            if let (Some(Value::Int(idx)), Some(Value::Char(c))) = (it.next(), it.next()) {
                let byte_idx = s.char_indices().nth(idx as usize).map(|(i, _)| i).unwrap_or(s.len());
                s.insert(byte_idx, c);
            }
            Some((Ok(Value::Unit), Value::Str(s)))
        }
        (Value::Str(mut s), "clear") => {
            s.clear();
            Some((Ok(Value::Unit), Value::Str(s)))
        }
        _ => None,
    }
}

// ── Free built-in functions ───────────────────────────────────────────────────

pub fn call_builtin(name: &str, args: Vec<Value>, interp: &mut Interpreter) -> Option<R> {
    match name {
        // Type constructors
        "Some"  => Some(Ok(Value::Option_(Some(Box::new(args.into_iter().next().unwrap_or(Value::Unit)))))),
        "None"  => Some(Ok(Value::Option_(None))),
        "Ok"    => Some(Ok(Value::Result_(Ok(Box::new(args.into_iter().next().unwrap_or(Value::Unit)))))),
        "Err"   => Some(Ok(Value::Result_(Err(Box::new(args.into_iter().next().unwrap_or(Value::Unit)))))),
        // Box::new is identity at Level 0 (no heap allocation needed)
        "Box::new" => Some(Ok(args.into_iter().next().unwrap_or(Value::Unit))),
        "Rc::new" | "Arc::new" | "Cell::new" | "RefCell::new" => {
            Some(Ok(args.into_iter().next().unwrap_or(Value::Unit)))
        }

        // String conversions
        "String::from" | "String::new" => {
            let s = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(s)))
        }
        "str::to_string" => {
            let s = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(s)))
        }

        // Vec / VecDeque / HashSet constructors (all backed by Vec at Level 0)
        "Vec::new" | "VecDeque::new" | "HashSet::new" | "BTreeSet::new" => Some(Ok(Value::Vec(Vec::new()))),
        "Vec::with_capacity" | "VecDeque::with_capacity" | "HashSet::with_capacity" => Some(Ok(Value::Vec(Vec::new()))),

        // HashMap constructors
        "HashMap::new" => Some(Ok(Value::HashMap(HashMap::new()))),

        // char constructors
        "char::from" | "char::from_u32_unchecked" => {
            let v = args.into_iter().next().unwrap_or(Value::Int(0));
            let n = match v { Value::Int(n) => n as u32, Value::Char(c) => c as u32, _ => 0 };
            Some(Ok(Value::Char(char::from_u32(n).unwrap_or('\0'))))
        }
        "char::from_u32" => {
            let v = args.into_iter().next().unwrap_or(Value::Int(0));
            let n = match v { Value::Int(n) => n as u32, _ => 0 };
            Some(Ok(Value::Option_(char::from_u32(n).map(|c| Box::new(Value::Char(c))))))
        }

        // Numeric
        "std::i64::MIN" | "i64::MIN" => Some(Ok(Value::Int(i64::MIN))),
        "std::i64::MAX" | "i64::MAX" => Some(Ok(Value::Int(i64::MAX))),

        // Printing (handled as macros, but support function-style too)
        "print"    | "println" => {
            let s = args.into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
            println!("{}", s);
            interp.output.push(s);
            Some(Ok(Value::Unit))
        }

        // std::cmp free functions
        "cmp::min" | "std::cmp::min" | "min" => {
            let mut it = args.into_iter();
            let a = it.next().unwrap_or(Value::Int(0));
            let b = it.next().unwrap_or(Value::Int(0));
            Some(Ok(match crate::eval::compare_values(&a, &b) {
                Some(std::cmp::Ordering::Greater) => b,
                _ => a,
            }))
        }
        "cmp::max" | "std::cmp::max" | "max" => {
            let mut it = args.into_iter();
            let a = it.next().unwrap_or(Value::Int(0));
            let b = it.next().unwrap_or(Value::Int(0));
            Some(Ok(match crate::eval::compare_values(&a, &b) {
                Some(std::cmp::Ordering::Less) => b,
                _ => a,
            }))
        }

        // Misc
        "drop" => Some(Ok(Value::Unit)),
        "clone" => Some(Ok(args.into_iter().next().unwrap_or(Value::Unit))),
        "String::with_capacity" => Some(Ok(Value::Str(String::new()))),

        // f64 constants
        "f64::INFINITY" | "std::f64::INFINITY" => Some(Ok(Value::Float(f64::INFINITY))),
        "f64::NEG_INFINITY" | "std::f64::NEG_INFINITY" => Some(Ok(Value::Float(f64::NEG_INFINITY))),
        "f64::NAN" | "std::f64::NAN" => Some(Ok(Value::Float(f64::NAN))),
        "f64::MAX" | "std::f64::MAX" => Some(Ok(Value::Float(f64::MAX))),
        "f64::MIN" | "std::f64::MIN" => Some(Ok(Value::Float(f64::MIN))),
        "f64::MIN_POSITIVE" => Some(Ok(Value::Float(f64::MIN_POSITIVE))),
        "f64::EPSILON" => Some(Ok(Value::Float(f64::EPSILON))),
        "f64::PI" | "std::f64::consts::PI" => Some(Ok(Value::Float(std::f64::consts::PI))),
        "f64::E" | "std::f64::consts::E" => Some(Ok(Value::Float(std::f64::consts::E))),

        // usize/u32 constants
        "usize::MAX" => Some(Ok(Value::Int(usize::MAX as i64))),
        "u32::MAX" => Some(Ok(Value::Int(u32::MAX as i64))),
        "i32::MAX" => Some(Ok(Value::Int(i32::MAX as i64))),
        "i32::MIN" => Some(Ok(Value::Int(i32::MIN as i64))),

        _ => None,
    }
}

// ── Method calls by type ──────────────────────────────────────────────────────

pub fn call_method(
    _type_name: &str,
    method: &str,
    self_val: Option<Value>,
    args: Vec<Value>,
    interp: &mut Interpreter,
) -> Option<R> {
    let recv = self_val?;

    match (&recv, method) {
        // ── Vec methods ───────────────────────────────────────────────────────
        (Value::Vec(_), "len") => {
            if let Value::Vec(v) = recv { Some(Ok(Value::Int(v.len() as i64))) } else { None }
        }
        (Value::Vec(_), "push") => {
            // Vec::push takes &mut self — return new vec (Level 0 semantics)
            // The caller is responsible for reassigning; we return Unit here
            Some(Ok(Value::Unit))
        }
        (Value::Vec(_), "pop") => {
            if let Value::Vec(mut v) = recv {
                let last = v.pop();
                Some(Ok(Value::Option_(last.map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "is_empty") => {
            if let Value::Vec(v) = recv { Some(Ok(Value::Bool(v.is_empty()))) } else { None }
        }
        (Value::Vec(_), "contains") => {
            if let Value::Vec(v) = recv {
                let needle = args.into_iter().next().unwrap_or(Value::Unit);
                let found = v.iter().any(|x| crate::eval::values_equal(x, &needle));
                Some(Ok(Value::Bool(found)))
            } else { None }
        }
        // HashSet set operations (all backed by Vec at Level 0)
        (Value::Vec(_), "union") => {
            if let Value::Vec(mut a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    for item in b {
                        let key = item.to_string();
                        if !a.iter().any(|x| x.to_string() == key) { a.push(item); }
                    }
                }
                Some(Ok(Value::Vec(a)))
            } else { None }
        }
        (Value::Vec(_), "intersection") => {
            if let Value::Vec(a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    let result: Vec<Value> = a.into_iter()
                        .filter(|x| b.iter().any(|y| y.to_string() == x.to_string()))
                        .collect();
                    Some(Ok(Value::Vec(result)))
                } else { Some(Ok(Value::Vec(vec![]))) }
            } else { None }
        }
        (Value::Vec(_), "difference") => {
            if let Value::Vec(a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    let result: Vec<Value> = a.into_iter()
                        .filter(|x| !b.iter().any(|y| y.to_string() == x.to_string()))
                        .collect();
                    Some(Ok(Value::Vec(result)))
                } else { Some(Ok(Value::Vec(a))) }
            } else { None }
        }
        (Value::Vec(_), "symmetric_difference") => {
            if let Value::Vec(a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    let mut result: Vec<Value> = a.iter()
                        .filter(|x| !b.iter().any(|y| y.to_string() == x.to_string()))
                        .cloned().collect();
                    for item in &b {
                        if !a.iter().any(|x| x.to_string() == item.to_string()) {
                            result.push(item.clone());
                        }
                    }
                    Some(Ok(Value::Vec(result)))
                } else { Some(Ok(Value::Vec(a))) }
            } else { None }
        }
        (Value::Vec(_), "is_subset") => {
            if let Value::Vec(a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    let ok = a.iter().all(|x| b.iter().any(|y| y.to_string() == x.to_string()));
                    Some(Ok(Value::Bool(ok)))
                } else { Some(Ok(Value::Bool(a.is_empty()))) }
            } else { None }
        }
        (Value::Vec(_), "is_superset") => {
            if let Value::Vec(a) = recv {
                if let Some(Value::Vec(b)) = args.into_iter().next() {
                    let ok = b.iter().all(|x| a.iter().any(|y| y.to_string() == x.to_string()));
                    Some(Ok(Value::Bool(ok)))
                } else { Some(Ok(Value::Bool(true))) }
            } else { None }
        }
        (Value::Vec(_), "first") => {
            if let Value::Vec(v) = recv {
                Some(Ok(Value::Option_(v.into_iter().next().map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "peek") => {
            // peekable iterator: return Option(first) without consuming
            if let Value::Vec(ref v) = recv {
                Some(Ok(Value::Option_(v.first().cloned().map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "last") => {
            if let Value::Vec(v) = recv {
                Some(Ok(Value::Option_(v.into_iter().last().map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "get") => {
            if let Value::Vec(v) = recv {
                let idx = match args.into_iter().next() {
                    Some(Value::Int(i)) => i as usize,
                    _ => return Some(Ok(Value::Option_(None))),
                };
                Some(Ok(Value::Option_(v.into_iter().nth(idx).map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "nth") => {
            if let Value::Vec(v) = recv {
                let idx = match args.into_iter().next() {
                    Some(Value::Int(i)) => i as usize,
                    _ => return Some(Ok(Value::Option_(None))),
                };
                Some(Ok(Value::Option_(v.into_iter().nth(idx).map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "iter" | "into_iter" | "iter_mut") => {
            // At Level 0, iterators are just the vec itself
            Some(Ok(recv))
        }
        (Value::Vec(_), "clone") => Some(Ok(recv)),
        (Value::Vec(_), "sort" | "sort_unstable") => {
            if let Value::Vec(mut v) = recv {
                v.sort_by(|a, b| {
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x.cmp(y),
                        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                        (Value::Str(x), Value::Str(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                Some(Ok(Value::Vec(v)))
            } else { None }
        }
        (Value::Vec(_), "binary_search") => {
            if let Value::Vec(v) = recv {
                let target = args.into_iter().next().unwrap_or(Value::Unit);
                let result = v.iter().enumerate().find(|(_, x)| crate::eval::values_equal(x, &target))
                    .map(|(i, _)| i);
                match result {
                    Some(i) => Some(Ok(Value::Result_(Ok(Box::new(Value::Int(i as i64)))))),
                    None => {
                        // return Err with insertion point
                        let pos = v.iter().take_while(|x| crate::eval::compare_values(x, &target)
                            .map_or(false, |o| o == std::cmp::Ordering::Less)).count();
                        Some(Ok(Value::Result_(Err(Box::new(Value::Int(pos as i64))))))
                    }
                }
            } else { None }
        }
        (Value::Vec(_), "reverse") => {
            if let Value::Vec(mut v) = recv { v.reverse(); Some(Ok(Value::Vec(v))) } else { None }
        }
        (Value::Vec(_), "join") => {
            if let Value::Vec(v) = recv {
                let sep = match args.into_iter().next() {
                    Some(Value::Str(s)) => s,
                    _ => "".to_string(),
                };
                let s = v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(&sep);
                Some(Ok(Value::Str(s)))
            } else { None }
        }
        (Value::Vec(_), "map") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let result: Result<Vec<Value>, Signal> = v.into_iter().map(|item| {
                    match &func {
                        Value::Fn(cfn) => interp.call_crust_fn(cfn, vec![item], None),
                        _ => Ok(item),
                    }
                }).collect();
                Some(result.map(Value::Vec))
            } else { None }
        }
        (Value::Vec(_), "filter") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut result = Vec::new();
                for item in v {
                    let keep = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => true,
                    };
                    if keep { result.push(item); }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "filter_map") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut result = Vec::new();
                for item in v {
                    let mapped = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => item,
                    };
                    match mapped {
                        Value::Option_(Some(inner)) => result.push(*inner),
                        Value::Option_(None) => {}
                        other if !matches!(other, Value::Unit) => result.push(other),
                        _ => {}
                    }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "fold" | "reduce") => {
            if let Value::Vec(v) = recv {
                let mut arg_iter = args.into_iter();
                let mut acc = arg_iter.next().unwrap_or(Value::Int(0));
                let func = arg_iter.next().unwrap_or(Value::Unit);
                for item in v {
                    acc = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![acc, item], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => item,
                    };
                }
                Some(Ok(acc))
            } else { None }
        }
        (Value::Vec(_), "enumerate") => {
            if let Value::Vec(v) = recv {
                let pairs: Vec<Value> = v.into_iter().enumerate()
                    .map(|(i, x)| Value::Tuple(vec![Value::Int(i as i64), x]))
                    .collect();
                Some(Ok(Value::Vec(pairs)))
            } else { None }
        }
        (Value::Vec(_), "collect") => {
            // collect::<Result<Vec<T>, E>>() — first Err short-circuits
            if let Value::Vec(ref v) = recv {
                if !v.is_empty() && v.iter().all(|x| matches!(x, Value::Result_(_))) {
                    if let Value::Vec(v) = recv {
                        let mut items = Vec::new();
                        for item in v {
                            match item {
                                Value::Result_(Err(e)) => return Some(Ok(Value::Result_(Err(e)))),
                                Value::Result_(Ok(inner)) => items.push(*inner),
                                other => items.push(other),
                            }
                        }
                        return Some(Ok(Value::Result_(Ok(Box::new(Value::Vec(items))))));
                    }
                }
            }
            Some(Ok(recv))
        }
        (Value::Vec(_), "collect_string") => {
            // collect::<String>() — join chars/strings into a String
            if let Value::Vec(v) = recv {
                let s: String = v.iter().map(|x| match x {
                    Value::Char(c) => c.to_string(),
                    other => other.to_string(),
                }).collect();
                Some(Ok(Value::Str(s)))
            } else { None }
        }
        (Value::Vec(_), "count") => {
            if let Value::Vec(v) = recv { Some(Ok(Value::Int(v.len() as i64))) } else { None }
        }
        (Value::Vec(_), "sum") => {
            if let Value::Vec(v) = recv {
                let mut acc = Value::Int(0);
                for item in v {
                    acc = eval_binary(&BinOp::Add, acc, item).unwrap_or(Value::Int(0));
                }
                Some(Ok(acc))
            } else { None }
        }
        (Value::Vec(_), "max" | "min") => {
            if let Value::Vec(v) = recv {
                let is_max = method == "max";
                let mut best: Option<Value> = None;
                for item in v {
                    best = Some(match best {
                        None => item,
                        Some(b) => {
                            let cmp = eval_binary(if is_max { &BinOp::Gt } else { &BinOp::Lt }, item.clone(), b.clone()).unwrap_or(Value::Bool(false));
                            if cmp.is_truthy() { item } else { b }
                        }
                    });
                }
                Some(Ok(Value::Option_(best.map(Box::new))))
            } else { None }
        }

        (Value::Vec(_), "zip") => {
            if let Value::Vec(v) = recv {
                let other = match args.into_iter().next() {
                    Some(Value::Vec(o)) => o,
                    _ => return Some(Ok(Value::Vec(vec![]))),
                };
                let pairs = v.into_iter().zip(other.into_iter())
                    .map(|(a, b)| Value::Tuple(vec![a, b]))
                    .collect();
                Some(Ok(Value::Vec(pairs)))
            } else { None }
        }
        (Value::Vec(_), "chain") => {
            if let Value::Vec(mut v) = recv {
                if let Some(Value::Vec(other)) = args.into_iter().next() {
                    v.extend(other);
                }
                Some(Ok(Value::Vec(v)))
            } else { None }
        }
        (Value::Vec(_), "take") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n as usize, _ => 0 };
                Some(Ok(Value::Vec(v.into_iter().take(n).collect())))
            } else { None }
        }
        (Value::Vec(_), "skip") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n as usize, _ => 0 };
                Some(Ok(Value::Vec(v.into_iter().skip(n).collect())))
            } else { None }
        }
        (Value::Vec(_), "take_while") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut result = Vec::new();
                for item in v {
                    let keep = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => false,
                    };
                    if keep { result.push(item); } else { break; }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "skip_while") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut skipping = true;
                let mut result = Vec::new();
                for item in v {
                    if skipping {
                        let skip = match &func {
                            Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                                Ok(v) => v.is_truthy(),
                                Err(e) => return Some(Err(e)),
                            },
                            _ => false,
                        };
                        if !skip { skipping = false; result.push(item); }
                    } else {
                        result.push(item);
                    }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "scan") => {
            if let Value::Vec(v) = recv {
                let mut it = args.into_iter();
                let mut state = it.next().unwrap_or(Value::Int(0));
                let func = it.next().unwrap_or(Value::Unit);
                let mut result = Vec::new();
                if let Value::Fn(cfn) = func {
                    for item in v {
                        let out = match interp.call_crust_fn(&cfn, vec![state.clone(), item], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                        // scan closure returns Option<B> or just B; unwrap Option
                        match out {
                            Value::Option_(None) => break,
                            Value::Option_(Some(v)) => { state = *v.clone(); result.push(*v); }
                            other => { state = other.clone(); result.push(other); }
                        }
                    }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "any") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                for item in v {
                    let keep = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => false,
                    };
                    if keep { return Some(Ok(Value::Bool(true))); }
                }
                Some(Ok(Value::Bool(false)))
            } else { None }
        }
        (Value::Vec(_), "all") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                for item in v {
                    let ok = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => true,
                    };
                    if !ok { return Some(Ok(Value::Bool(false))); }
                }
                Some(Ok(Value::Bool(true)))
            } else { None }
        }
        (Value::Vec(_), "find") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                for item in v {
                    let found = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => false,
                    };
                    if found { return Some(Ok(Value::Option_(Some(Box::new(item))))); }
                }
                Some(Ok(Value::Option_(None)))
            } else { None }
        }
        (Value::Vec(_), "position" | "find_index") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                for (i, item) in v.into_iter().enumerate() {
                    let found = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item], None) {
                            Ok(v) => v.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => false,
                    };
                    if found { return Some(Ok(Value::Option_(Some(Box::new(Value::Int(i as i64)))))); }
                }
                Some(Ok(Value::Option_(None)))
            } else { None }
        }
        (Value::Vec(_), "flat_map" | "and_then") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut result = Vec::new();
                for item in v {
                    let mapped = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => continue,
                    };
                    match mapped {
                        Value::Vec(inner) => result.extend(inner),
                        other => result.push(other),
                    }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "product") => {
            if let Value::Vec(v) = recv {
                let mut acc = Value::Int(1);
                for item in v {
                    acc = eval_binary(&BinOp::Mul, acc, item).unwrap_or(Value::Int(0));
                }
                Some(Ok(acc))
            } else { None }
        }
        (Value::Vec(_), "rev" | "into_iter_rev") => {
            if let Value::Vec(mut v) = recv { v.reverse(); Some(Ok(Value::Vec(v))) } else { None }
        }
        (Value::Vec(_), "peekable") => Some(Ok(recv)),
        (Value::Vec(_), "cloned" | "copied" | "to_vec" | "to_owned") => Some(Ok(recv)),
        (Value::Vec(_), "by_ref") => Some(Ok(recv)),
        (Value::Vec(_), "cycle") => Some(Ok(recv)), // simplified: returns self (not infinite)
        (Value::Vec(_), "step_by") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n.max(1) as usize, _ => 1 };
                Some(Ok(Value::Vec(v.into_iter().step_by(n).collect())))
            } else { None }
        }
        (Value::Vec(_), "min_by_key") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut best: Option<(Value, Value)> = None; // (key, item)
                if let Value::Fn(cfn) = &func {
                    for item in v {
                        let k = match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                        let replace = match &best {
                            None => true,
                            Some((bk, _)) => crate::eval::compare_values(&k, bk) == Some(std::cmp::Ordering::Less),
                        };
                        if replace { best = Some((k, item)); }
                    }
                }
                Some(Ok(Value::Option_(best.map(|(_, v)| Box::new(v)))))
            } else { None }
        }
        (Value::Vec(_), "max_by") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut best: Option<Value> = None;
                if let Value::Fn(cfn) = &func {
                    for item in v {
                        let replace = match &best {
                            None => true,
                            Some(b) => {
                                match interp.call_crust_fn(cfn, vec![item.clone(), b.clone()], None) {
                                    Ok(Value::Int(n)) if n > 0 => true,
                                    Ok(_) => false,
                                    Err(e) => return Some(Err(e)),
                                }
                            }
                        };
                        if replace { best = Some(item); }
                    }
                }
                Some(Ok(Value::Option_(best.map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "min_by") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut best: Option<Value> = None;
                if let Value::Fn(cfn) = &func {
                    for item in v {
                        let replace = match &best {
                            None => true,
                            Some(b) => {
                                match interp.call_crust_fn(cfn, vec![item.clone(), b.clone()], None) {
                                    Ok(Value::Int(n)) if n < 0 => true,
                                    Ok(_) => false,
                                    Err(e) => return Some(Err(e)),
                                }
                            }
                        };
                        if replace { best = Some(item); }
                    }
                }
                Some(Ok(Value::Option_(best.map(Box::new))))
            } else { None }
        }
        (Value::Vec(_), "max_by_key") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut best: Option<(Value, Value)> = None;
                if let Value::Fn(cfn) = &func {
                    for item in v {
                        let k = match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        };
                        let replace = match &best {
                            None => true,
                            Some((bk, _)) => crate::eval::compare_values(&k, bk) == Some(std::cmp::Ordering::Greater),
                        };
                        if replace { best = Some((k, item)); }
                    }
                }
                Some(Ok(Value::Option_(best.map(|(_, v)| Box::new(v)))))
            } else { None }
        }
        (Value::Vec(_), "unzip") => {
            if let Value::Vec(v) = recv {
                let mut a = Vec::new();
                let mut b = Vec::new();
                for item in v {
                    match item {
                        Value::Tuple(pair) if pair.len() == 2 => {
                            let mut it = pair.into_iter();
                            a.push(it.next().unwrap());
                            b.push(it.next().unwrap());
                        }
                        other => a.push(other),
                    }
                }
                Some(Ok(Value::Tuple(vec![Value::Vec(a), Value::Vec(b)])))
            } else { None }
        }
        (Value::Vec(_), "partition") => {
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut yes = Vec::new();
                let mut no = Vec::new();
                if let Value::Fn(cfn) = &func {
                    for item in v {
                        match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) if v.is_truthy() => yes.push(item),
                            _ => no.push(item),
                        }
                    }
                }
                Some(Ok(Value::Tuple(vec![Value::Vec(yes), Value::Vec(no)])))
            } else { None }
        }
        (Value::Vec(_), "flatten") => {
            if let Value::Vec(v) = recv {
                let mut result = Vec::new();
                for item in v {
                    match item {
                        Value::Vec(inner) => result.extend(inner),
                        Value::Option_(Some(inner)) => result.push(*inner),
                        Value::Option_(None) => {}  // skip None
                        Value::Result_(Ok(inner)) => result.push(*inner),
                        Value::Result_(Err(_)) => {}  // skip Err
                        other => result.push(other),
                    }
                }
                Some(Ok(Value::Vec(result)))
            } else { None }
        }
        (Value::Vec(_), "windows") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n.max(1) as usize, _ => 1 };
                let windows: Vec<Value> = v.windows(n).map(|w| Value::Vec(w.to_vec())).collect();
                Some(Ok(Value::Vec(windows)))
            } else { None }
        }
        (Value::Vec(_), "chunks") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n.max(1) as usize, _ => 1 };
                let chunks: Vec<Value> = v.chunks(n).map(|c| Value::Vec(c.to_vec())).collect();
                Some(Ok(Value::Vec(chunks)))
            } else { None }
        }
        (Value::Vec(_), "split_at") => {
            if let Value::Vec(v) = recv {
                let mid = match args.into_iter().next() { Some(Value::Int(n)) => (n as usize).min(v.len()), _ => 0 };
                let (left, right) = v.split_at(mid);
                Some(Ok(Value::Tuple(vec![Value::Vec(left.to_vec()), Value::Vec(right.to_vec())])))
            } else { None }
        }
        (Value::Vec(_), "split_first") => {
            if let Value::Vec(v) = recv {
                if v.is_empty() { Some(Ok(Value::Option_(None))) }
                else {
                    let mut it = v.into_iter();
                    let first = it.next().unwrap();
                    Some(Ok(Value::Option_(Some(Box::new(Value::Tuple(vec![first, Value::Vec(it.collect())]))))))
                }
            } else { None }
        }
        (Value::Vec(_), "split_last") => {
            if let Value::Vec(mut v) = recv {
                if v.is_empty() { Some(Ok(Value::Option_(None))) }
                else {
                    let last = v.pop().unwrap();
                    Some(Ok(Value::Option_(Some(Box::new(Value::Tuple(vec![last, Value::Vec(v)]))))))
                }
            } else { None }
        }
        (Value::Vec(_), "split" | "splitn") if matches!(recv, Value::Vec(_)) => {
            // Vec::split(|predicate|) — not the same as String::split
            if let Value::Vec(v) = recv {
                let func = args.into_iter().next().unwrap_or(Value::Unit);
                let mut groups: Vec<Value> = Vec::new();
                let mut current: Vec<Value> = Vec::new();
                for item in v {
                    let is_sep = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![item.clone()], None) {
                            Ok(v) => v.is_truthy(),
                            _ => false,
                        },
                        _ => false,
                    };
                    if is_sep {
                        groups.push(Value::Vec(std::mem::take(&mut current)));
                    } else {
                        current.push(item);
                    }
                }
                groups.push(Value::Vec(current));
                Some(Ok(Value::Vec(groups)))
            } else { None }
        }

        // ── String/str methods ────────────────────────────────────────────────
        (Value::Str(_), "len") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Int(s.len() as i64))) } else { None }
        }
        (Value::Str(_), "is_empty") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Bool(s.is_empty()))) } else { None }
        }
        (Value::Str(_), "contains") => {
            if let Value::Str(s) = recv {
                let needle = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Bool(s.contains(&needle[..]))))
            } else { None }
        }
        (Value::Str(_), "starts_with") => {
            if let Value::Str(s) = recv {
                let prefix = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Bool(s.starts_with(&prefix[..]))))
            } else { None }
        }
        (Value::Str(_), "ends_with") => {
            if let Value::Str(s) = recv {
                let suffix = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Bool(s.ends_with(&suffix[..]))))
            } else { None }
        }
        (Value::Str(_), "to_uppercase") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Str(s.to_uppercase()))) } else { None }
        }
        (Value::Str(_), "to_lowercase") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Str(s.to_lowercase()))) } else { None }
        }
        (Value::Str(_), "trim") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Str(s.trim().to_string()))) } else { None }
        }
        (Value::Str(_), "trim_start" | "trim_left") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Str(s.trim_start().to_string()))) } else { None }
        }
        (Value::Str(_), "trim_end" | "trim_right") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Str(s.trim_end().to_string()))) } else { None }
        }
        (Value::Str(_), "split") => {
            if let Value::Str(s) = recv {
                let sep_arg = args.into_iter().next().unwrap_or(Value::Unit);
                let parts: Vec<Value> = match sep_arg {
                    Value::Fn(cfn) => {
                        // Split by closure predicate on chars
                        let mut result = Vec::new();
                        let mut current = String::new();
                        for c in s.chars() {
                            match interp.call_crust_fn(&cfn, vec![Value::Char(c)], None) {
                                Ok(v) if v.is_truthy() => {
                                    result.push(Value::Str(current.clone()));
                                    current.clear();
                                }
                                _ => current.push(c),
                            }
                        }
                        result.push(Value::Str(current));
                        result
                    }
                    Value::Char(c) => s.split(c).map(|p| Value::Str(p.to_string())).collect(),
                    other => {
                        let sep = other.to_string();
                        s.split(&sep[..]).map(|p| Value::Str(p.to_string())).collect()
                    }
                };
                Some(Ok(Value::Vec(parts)))
            } else { None }
        }
        (Value::Str(_), "split_whitespace" | "split_ascii_whitespace") => {
            if let Value::Str(s) = recv {
                let ws: Vec<Value> = s.split_whitespace().map(|w| Value::Str(w.to_string())).collect();
                Some(Ok(Value::Vec(ws)))
            } else { None }
        }
        (Value::Str(_), "lines") => {
            if let Value::Str(s) = recv {
                let ls: Vec<Value> = s.lines().map(|l| Value::Str(l.to_string())).collect();
                Some(Ok(Value::Vec(ls)))
            } else { None }
        }
        (Value::Str(_), "chars") => {
            if let Value::Str(s) = recv {
                let cs: Vec<Value> = s.chars().map(Value::Char).collect();
                Some(Ok(Value::Vec(cs)))
            } else { None }
        }
        (Value::Str(_), "bytes") => {
            if let Value::Str(s) = recv {
                let bs: Vec<Value> = s.bytes().map(|b| Value::Int(b as i64)).collect();
                Some(Ok(Value::Vec(bs)))
            } else { None }
        }
        (Value::Str(_), "replace") => {
            if let Value::Str(s) = recv {
                let from = args.first().map(|v| v.to_string()).unwrap_or_default();
                let to = args.into_iter().nth(1).map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Str(s.replace(&from[..], &to[..]))))
            } else { None }
        }
        (Value::Str(_), "repeat") => {
            if let Value::Str(s) = recv {
                let n = match args.first() { Some(Value::Int(n)) => *n as usize, _ => 0 };
                Some(Ok(Value::Str(s.repeat(n))))
            } else { None }
        }
        (Value::Str(_), "parse") => {
            // parse::<T>() — try int first, then float
            if let Value::Str(s) = recv {
                let s = s.trim();
                if let Ok(n) = s.parse::<i64>() {
                    Some(Ok(Value::Result_(Ok(Box::new(Value::Int(n))))))
                } else if let Ok(f) = s.parse::<f64>() {
                    Some(Ok(Value::Result_(Ok(Box::new(Value::Float(f))))))
                } else {
                    Some(Ok(Value::Result_(Err(Box::new(Value::Str(format!("parse error: {}", s)))))))
                }
            } else { None }
        }
        (Value::Str(_), "cmp" | "partial_cmp") => {
            if let Value::Str(s) = recv {
                let other = match args.into_iter().next() { Some(Value::Str(x)) => x, Some(v) => v.to_string(), _ => return None };
                let ord = s.cmp(&other);
                let v = match ord {
                    std::cmp::Ordering::Less => Value::Int(-1),
                    std::cmp::Ordering::Equal => Value::Int(0),
                    std::cmp::Ordering::Greater => Value::Int(1),
                };
                Some(Ok(v))
            } else { None }
        }
        (Value::Str(_), "to_string" | "clone") => Some(Ok(recv)),
        (Value::Str(_), "find") => {
            if let Value::Str(s) = recv {
                let pat = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Option_(s.find(&*pat).map(|i| Box::new(Value::Int(i as i64))))))
            } else { None }
        }
        (Value::Str(_), "rfind") => {
            if let Value::Str(s) = recv {
                let pat = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Option_(s.rfind(&*pat).map(|i| Box::new(Value::Int(i as i64))))))
            } else { None }
        }
        (Value::Str(_), "get") => {
            if let Value::Str(s) = recv {
                match args.into_iter().next() {
                    Some(Value::Range(a, b, inc)) => {
                        let end = if inc { b + 1 } else { b } as usize;
                        Some(Ok(Value::Option_(s.get(a as usize..end).map(|r| Box::new(Value::Str(r.to_string()))))))
                    }
                    _ => Some(Ok(Value::Option_(None))),
                }
            } else { None }
        }
        (Value::Str(_), "chars_count" | "char_count") => {
            if let Value::Str(s) = recv { Some(Ok(Value::Int(s.chars().count() as i64))) } else { None }
        }
        (Value::Str(_), "splitn") => {
            if let Value::Str(s) = recv {
                let mut it = args.into_iter();
                let n = match it.next() { Some(Value::Int(n)) => n as usize, _ => 2 };
                let pat = it.next().map(|v| v.to_string()).unwrap_or_default();
                let parts: Vec<Value> = s.splitn(n, &*pat).map(|p| Value::Str(p.to_string())).collect();
                Some(Ok(Value::Vec(parts)))
            } else { None }
        }
        (Value::Str(_), "trim_matches") => {
            if let Value::Str(s) = recv {
                let pat = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                let ch: char = pat.chars().next().unwrap_or(' ');
                Some(Ok(Value::Str(s.trim_matches(ch).to_string())))
            } else { None }
        }
        (Value::Str(_), "trim_start_matches") => {
            if let Value::Str(s) = recv {
                let pat = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Str(s.trim_start_matches(&*pat).to_string())))
            } else { None }
        }
        (Value::Str(_), "trim_end_matches") => {
            if let Value::Str(s) = recv {
                let pat = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Str(s.trim_end_matches(&*pat).to_string())))
            } else { None }
        }
        (Value::Str(_), "replacen") => {
            if let Value::Str(s) = recv {
                let mut it = args.into_iter();
                let from = it.next().map(|v| v.to_string()).unwrap_or_default();
                let to = it.next().map(|v| v.to_string()).unwrap_or_default();
                let n = match it.next() { Some(Value::Int(n)) => n as usize, _ => 1 };
                Some(Ok(Value::Str(s.replacen(&*from, &to, n))))
            } else { None }
        }
        (Value::Str(_), "char_indices") => {
            if let Value::Str(s) = recv {
                let pairs: Vec<Value> = s.char_indices()
                    .map(|(i, c)| Value::Tuple(vec![Value::Int(i as i64), Value::Char(c)]))
                    .collect();
                Some(Ok(Value::Vec(pairs)))
            } else { None }
        }
        (Value::Str(_), "as_bytes" | "as_str") => Some(Ok(recv)),
        // Vec<char>::as_str() — collect chars back into a String (Chars iterator pattern)
        (Value::Vec(_), "as_str") => {
            if let Value::Vec(v) = recv {
                let s: String = v.iter().filter_map(|c| if let Value::Char(ch) = c { Some(*ch) } else { None }).collect();
                Some(Ok(Value::Str(s)))
            } else { None }
        }
        (Value::Str(_), "push_str") => {
            // In Rust this mutates; at Level 0 we return a concatenated string
            if let Value::Str(mut s) = recv {
                let extra = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                s.push_str(&extra);
                Some(Ok(Value::Str(s)))
            } else { None }
        }

        // ── f64 methods ───────────────────────────────────────────────────────
        (Value::Float(_), "sqrt") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.sqrt()))) } else { None }
        }
        (Value::Float(_), "powi") => {
            if let Value::Float(f) = recv {
                let exp = match args.into_iter().next() {
                    Some(Value::Int(n)) => n as i32,
                    Some(Value::Float(n)) => n as i32,
                    _ => 0,
                };
                Some(Ok(Value::Float(f.powi(exp))))
            } else { None }
        }
        (Value::Float(_), "powf") => {
            if let Value::Float(f) = recv {
                let exp = match args.into_iter().next() {
                    Some(Value::Float(n)) => n,
                    Some(Value::Int(n)) => n as f64,
                    _ => 1.0,
                };
                Some(Ok(Value::Float(f.powf(exp))))
            } else { None }
        }
        (Value::Float(_), "abs") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.abs()))) } else { None }
        }
        (Value::Float(_), "floor") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.floor()))) } else { None }
        }
        (Value::Float(_), "ceil") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.ceil()))) } else { None }
        }
        (Value::Float(_), "round") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.round()))) } else { None }
        }
        (Value::Float(_), "sin") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.sin()))) } else { None }
        }
        (Value::Float(_), "cos") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.cos()))) } else { None }
        }
        (Value::Float(_), "tan") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.tan()))) } else { None }
        }
        (Value::Float(_), "ln") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.ln()))) } else { None }
        }
        (Value::Float(_), "log2") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.log2()))) } else { None }
        }
        (Value::Float(_), "log10") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Float(f.log10()))) } else { None }
        }
        (Value::Float(_), "is_nan") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Bool(f.is_nan()))) } else { None }
        }
        (Value::Float(_), "is_finite") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Bool(f.is_finite()))) } else { None }
        }
        (Value::Float(_), "to_string") => {
            if let Value::Float(f) = recv { Some(Ok(Value::Str(f.to_string()))) } else { None }
        }
        (Value::Float(_), "clone") => Some(Ok(recv)),
        (Value::Float(_), "max") => {
            if let Value::Float(f) = recv {
                let other = match args.into_iter().next() {
                    Some(Value::Float(x)) => x,
                    Some(Value::Int(x)) => x as f64,
                    _ => f,
                };
                Some(Ok(Value::Float(f.max(other))))
            } else { None }
        }
        (Value::Float(_), "min") => {
            if let Value::Float(f) = recv {
                let other = match args.into_iter().next() {
                    Some(Value::Float(x)) => x,
                    Some(Value::Int(x)) => x as f64,
                    _ => f,
                };
                Some(Ok(Value::Float(f.min(other))))
            } else { None }
        }

        (Value::Float(_), "partial_cmp" | "total_cmp") => {
            if let Value::Float(f) = recv {
                let other = match args.into_iter().next() {
                    Some(Value::Float(x)) => x,
                    Some(Value::Int(x)) => x as f64,
                    _ => return None,
                };
                let v = match f.partial_cmp(&other) {
                    Some(std::cmp::Ordering::Less) => Value::Int(-1),
                    Some(std::cmp::Ordering::Equal) => Value::Int(0),
                    _ => Value::Int(1),
                };
                Some(Ok(Value::Option_(Some(Box::new(v)))))
            } else { None }
        }

        // ── Integer methods ───────────────────────────────────────────────────
        (Value::Int(_), "abs") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.abs()))) } else { None }
        }
        (Value::Int(_), "pow") => {
            if let Value::Int(n) = recv {
                let exp = match args.into_iter().next() {
                    Some(Value::Int(e)) => e as u32,
                    _ => 0,
                };
                Some(Ok(Value::Int(n.pow(exp))))
            } else { None }
        }
        (Value::Int(_), "min") => {
            if let Value::Int(n) = recv {
                let other = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => n };
                Some(Ok(Value::Int(n.min(other))))
            } else { None }
        }
        (Value::Int(_), "max") => {
            if let Value::Int(n) = recv {
                let other = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => n };
                Some(Ok(Value::Int(n.max(other))))
            } else { None }
        }
        (Value::Int(_), "clamp") => {
            if let Value::Int(n) = recv {
                let lo = match args.first() { Some(Value::Int(x)) => *x, _ => n };
                let hi = match args.get(1) { Some(Value::Int(x)) => *x, _ => n };
                Some(Ok(Value::Int(n.clamp(lo, hi))))
            } else { None }
        }
        (Value::Int(_), "cmp") => {
            if let Value::Int(n) = recv {
                let other = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => return None };
                let v = match n.cmp(&other) {
                    std::cmp::Ordering::Less => Value::Int(-1),
                    std::cmp::Ordering::Equal => Value::Int(0),
                    std::cmp::Ordering::Greater => Value::Int(1),
                };
                Some(Ok(v))
            } else { None }
        }
        // Ordering::then / then_with — self if non-zero (non-Equal), else other
        (Value::Int(_), "then") => {
            if let Value::Int(n) = recv {
                if n != 0 {
                    Some(Ok(Value::Int(n)))
                } else {
                    Some(Ok(args.into_iter().next().unwrap_or(Value::Int(0))))
                }
            } else { None }
        }
        (Value::Int(_), "then_with") => {
            if let Value::Int(n) = recv {
                if n != 0 {
                    Some(Ok(Value::Int(n)))
                } else {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        Some(interp.call_crust_fn(&cfn, vec![], None))
                    } else {
                        Some(Ok(func))
                    }
                }
            } else { None }
        }
        (Value::Int(_), "partial_cmp") => {
            if let Value::Int(n) = recv {
                let other = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => return None };
                let v = match n.cmp(&other) {
                    std::cmp::Ordering::Less => Value::Int(-1),
                    std::cmp::Ordering::Equal => Value::Int(0),
                    std::cmp::Ordering::Greater => Value::Int(1),
                };
                Some(Ok(Value::Option_(Some(Box::new(v)))))
            } else { None }
        }
        (Value::Int(_), "rem_euclid") => {
            if let Value::Int(n) = recv {
                let rhs = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => return None };
                Some(Ok(Value::Int(n.rem_euclid(rhs))))
            } else { None }
        }
        (Value::Int(_), "checked_add") => {
            if let Value::Int(n) = recv {
                let rhs = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => return None };
                Some(Ok(Value::Option_(n.checked_add(rhs).map(|v| Box::new(Value::Int(v))))))
            } else { None }
        }
        (Value::Int(_), "wrapping_add") => {
            if let Value::Int(n) = recv {
                let rhs = match args.into_iter().next() { Some(Value::Int(x)) => x, _ => return None };
                Some(Ok(Value::Int(n.wrapping_add(rhs))))
            } else { None }
        }
        (Value::Int(_), "to_string") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Str(n.to_string()))) } else { None }
        }
        (Value::Int(_), "clone") => Some(Ok(recv)),
        (Value::Int(_), "count_ones") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.count_ones() as i64))) } else { None }
        }
        (Value::Int(_), "leading_zeros") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.leading_zeros() as i64))) } else { None }
        }
        (Value::Int(_), "trailing_zeros") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.trailing_zeros() as i64))) } else { None }
        }

        // ── Char methods ──────────────────────────────────────────────────────
        (Value::Char(_), "to_uppercase") => {
            if let Value::Char(c) = recv {
                // Return a String (most common use is .to_string() after)
                Some(Ok(Value::Str(c.to_uppercase().collect())))
            } else { None }
        }
        (Value::Char(_), "to_lowercase") => {
            if let Value::Char(c) = recv {
                Some(Ok(Value::Str(c.to_lowercase().collect())))
            } else { None }
        }
        (Value::Char(_), "is_alphabetic") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_alphabetic()))) } else { None }
        }
        (Value::Char(_), "is_alphanumeric") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_alphanumeric()))) } else { None }
        }
        (Value::Char(_), "is_numeric" | "is_ascii_digit") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_digit()))) } else { None }
        }
        (Value::Char(_), "is_whitespace" | "is_ascii_whitespace") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_whitespace()))) } else { None }
        }
        (Value::Char(_), "is_uppercase" | "is_ascii_uppercase") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_uppercase()))) } else { None }
        }
        (Value::Char(_), "is_lowercase" | "is_ascii_lowercase") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_lowercase()))) } else { None }
        }
        (Value::Char(_), "is_ascii") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii()))) } else { None }
        }
        (Value::Char(_), "is_ascii_alphabetic") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_alphabetic()))) } else { None }
        }
        (Value::Char(_), "is_ascii_alphanumeric") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_alphanumeric()))) } else { None }
        }
        (Value::Char(_), "is_ascii_punctuation") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Bool(c.is_ascii_punctuation()))) } else { None }
        }
        (Value::Char(_), "to_string") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Str(c.to_string()))) } else { None }
        }
        (Value::Char(_), "to_digit") => {
            if let Value::Char(c) = recv {
                let base = match args.into_iter().next() { Some(Value::Int(b)) => b as u32, _ => 10 };
                Some(Ok(Value::Option_(c.to_digit(base).map(|d| Box::new(Value::Int(d as i64))))))
            } else { None }
        }
        (Value::Char(_), "len_utf8") => {
            if let Value::Char(c) = recv { Some(Ok(Value::Int(c.len_utf8() as i64))) } else { None }
        }

        // ── Option methods ────────────────────────────────────────────────────
        (Value::Option_(_), "unwrap") => {
            match recv {
                Value::Option_(Some(v)) => Some(Ok(*v)),
                Value::Option_(None) => Some(Err(err("called unwrap() on None"))),
                _ => None,
            }
        }
        (Value::Option_(_), "unwrap_or") => {
            let default = args.into_iter().next().unwrap_or(Value::Unit);
            match recv {
                Value::Option_(Some(v)) => Some(Ok(*v)),
                Value::Option_(None) => Some(Ok(default)),
                _ => None,
            }
        }
        (Value::Option_(_), "is_some") => {
            Some(Ok(Value::Bool(matches!(recv, Value::Option_(Some(_))))))
        }
        (Value::Option_(_), "is_none") => {
            Some(Ok(Value::Bool(matches!(recv, Value::Option_(None)))))
        }
        (Value::Option_(_), "expect") => {
            let msg = args.into_iter().next().map(|v| v.to_string()).unwrap_or_else(|| "expect failed".into());
            match recv {
                Value::Option_(Some(v)) => Some(Ok(*v)),
                Value::Option_(None) => Some(Err(err(msg))),
                _ => None,
            }
        }
        (Value::Option_(_), "map") => {
            match recv {
                Value::Option_(None) => Some(Ok(Value::Option_(None))),
                Value::Option_(Some(v)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let result = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![*v], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => *v,
                    };
                    Some(Ok(Value::Option_(Some(Box::new(result)))))
                }
                _ => None,
            }
        }
        (Value::Option_(_), "and_then" | "flat_map") => {
            match recv {
                Value::Option_(None) => Some(Ok(Value::Option_(None))),
                Value::Option_(Some(v)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let result = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![*v], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => *v,
                    };
                    Some(Ok(result))
                }
                _ => None,
            }
        }
        (Value::Option_(_), "filter") => {
            match recv {
                Value::Option_(None) => Some(Ok(Value::Option_(None))),
                Value::Option_(Some(v)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let keep = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![*v.clone()], None) {
                            Ok(r) => r.is_truthy(),
                            Err(e) => return Some(Err(e)),
                        },
                        _ => true,
                    };
                    if keep { Some(Ok(Value::Option_(Some(v)))) }
                    else { Some(Ok(Value::Option_(None))) }
                }
                _ => None,
            }
        }
        (Value::Option_(_), "flatten") => {
            match recv {
                Value::Option_(None) => Some(Ok(Value::Option_(None))),
                Value::Option_(Some(inner)) => Some(Ok(*inner)),
                other => Some(Ok(other)),
            }
        }
        (Value::Option_(_), "unwrap_or_default") => {
            match recv {
                Value::Option_(Some(v)) => Some(Ok(*v)),
                Value::Option_(None) => Some(Ok(Value::Int(0))),
                other => Some(Ok(other)),
            }
        }
        (Value::Option_(_), "or") => {
            match recv {
                Value::Option_(Some(_)) => Some(Ok(recv)),
                Value::Option_(None) => Some(Ok(args.into_iter().next().unwrap_or(Value::Option_(None)))),
                other => Some(Ok(other)),
            }
        }
        (Value::Option_(_), "or_else") => {
            match recv {
                Value::Option_(Some(_)) => Some(Ok(recv)),
                Value::Option_(None) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        match interp.call_crust_fn(&cfn, vec![], None) {
                            Ok(v) => Some(Ok(v)),
                            Err(e) => Some(Err(e)),
                        }
                    } else { Some(Ok(Value::Option_(None))) }
                }
                other => Some(Ok(other)),
            }
        }
        (Value::Option_(_), "take") => {
            Some(Ok(recv)) // Level 0: returns the current value (mut semantics not tracked)
        }
        (Value::Option_(_), "replace") => {
            // replace(new) -> old value; at Level 0 returns old, mutation not tracked
            Some(Ok(recv))
        }
        (Value::Option_(_), "as_ref" | "as_deref") => {
            Some(Ok(recv)) // at Level 0, ref = value
        }
        (Value::Option_(_), "copied" | "cloned") => {
            Some(Ok(recv)) // at Level 0, everything is cloned
        }
        (Value::Option_(_), "zip") => {
            match recv {
                Value::Option_(None) => Some(Ok(Value::Option_(None))),
                Value::Option_(Some(a)) => {
                    let other = args.into_iter().next().unwrap_or(Value::Option_(None));
                    match other {
                        Value::Option_(None) => Some(Ok(Value::Option_(None))),
                        Value::Option_(Some(b)) => Some(Ok(Value::Option_(Some(Box::new(Value::Tuple(vec![*a, *b])))))),
                        b => Some(Ok(Value::Option_(Some(Box::new(Value::Tuple(vec![*a, b])))))),
                    }
                }
                _ => None,
            }
        }

        // ── Result methods ────────────────────────────────────────────────────
        (Value::Result_(_), "unwrap") => {
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(*v)),
                Value::Result_(Err(e)) => Some(Err(err(format!("called unwrap() on Err: {}", e)))),
                _ => None,
            }
        }
        (Value::Result_(_), "expect") => {
            let msg = args.into_iter().next().map(|v| v.to_string()).unwrap_or_else(|| "expect failed".into());
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(*v)),
                Value::Result_(Err(e)) => Some(Err(err(format!("{}: {}", msg, e)))),
                _ => None,
            }
        }
        (Value::Result_(_), "is_ok") => {
            Some(Ok(Value::Bool(matches!(recv, Value::Result_(Ok(_))))))
        }
        (Value::Result_(_), "map") => {
            match recv {
                Value::Result_(Ok(v)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let result = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![*v], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => *v,
                    };
                    Some(Ok(Value::Result_(Ok(Box::new(result)))))
                }
                Value::Result_(Err(e)) => Some(Ok(Value::Result_(Err(e)))),
                other => Some(Ok(other)),
            }
        }
        (Value::Result_(_), "and_then") => {
            match recv {
                Value::Result_(Ok(v)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        match interp.call_crust_fn(&cfn, vec![*v], None) {
                            Ok(v) => Some(Ok(v)),
                            Err(e) => Some(Err(e)),
                        }
                    } else { Some(Ok(Value::Result_(Ok(v)))) }
                }
                Value::Result_(Err(e)) => Some(Ok(Value::Result_(Err(e)))),
                other => Some(Ok(other)),
            }
        }
        (Value::Result_(_), "is_err") => {
            Some(Ok(Value::Bool(matches!(recv, Value::Result_(Err(_))))))
        }
        (Value::Result_(_), "unwrap_or") => {
            let default = args.into_iter().next().unwrap_or(Value::Unit);
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(*v)),
                Value::Result_(Err(_)) => Some(Ok(default)),
                _ => None,
            }
        }
        (Value::Result_(_), "ok") => {
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(Value::Option_(Some(v)))),
                Value::Result_(Err(_)) => Some(Ok(Value::Option_(None))),
                _ => None,
            }
        }
        (Value::Result_(_), "err") => {
            match recv {
                Value::Result_(Err(e)) => Some(Ok(Value::Option_(Some(e)))),
                Value::Result_(Ok(_)) => Some(Ok(Value::Option_(None))),
                _ => None,
            }
        }
        (Value::Result_(_), "map_err") => {
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(Value::Result_(Ok(v)))),
                Value::Result_(Err(e)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let new_err = match &func {
                        Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![*e], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        },
                        _ => *e,
                    };
                    Some(Ok(Value::Result_(Err(Box::new(new_err)))))
                }
                _ => None,
            }
        }
        (Value::Result_(_), "unwrap_or_default") => {
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(*v)),
                Value::Result_(Err(_)) => Some(Ok(Value::Int(0))),
                _ => None,
            }
        }
        (Value::Result_(_), "or") => {
            match recv {
                Value::Result_(Ok(_)) => Some(Ok(recv)),
                Value::Result_(Err(_)) => Some(Ok(args.into_iter().next().unwrap_or(recv))),
                other => Some(Ok(other)),
            }
        }
        (Value::Result_(_), "or_else") => {
            match recv {
                Value::Result_(Ok(_)) => Some(Ok(recv)),
                Value::Result_(Err(e)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        match interp.call_crust_fn(&cfn, vec![*e], None) {
                            Ok(v) => Some(Ok(v)),
                            Err(e) => Some(Err(e)),
                        }
                    } else { Some(Ok(Value::Result_(Err(e)))) }
                }
                other => Some(Ok(other)),
            }
        }
        (Value::Result_(_), "as_ref" | "as_deref" | "as_mut") => {
            Some(Ok(recv)) // at Level 0, ref = value
        }

        // ── HashMap methods ───────────────────────────────────────────────────
        (Value::HashMap(_), "insert") => {
            Some(Ok(Value::Unit)) // mutation handled at call site if needed
        }
        (Value::HashMap(_), "get") => {
            if let Value::HashMap(m) = recv {
                let key = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Option_(m.get(&key).cloned().map(Box::new))))
            } else { None }
        }
        (Value::HashMap(_), "contains_key") => {
            if let Value::HashMap(m) = recv {
                let key = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                Some(Ok(Value::Bool(m.contains_key(&key))))
            } else { None }
        }
        (Value::HashMap(_), "len") => {
            if let Value::HashMap(m) = recv { Some(Ok(Value::Int(m.len() as i64))) } else { None }
        }
        (Value::HashMap(_), "is_empty") => {
            if let Value::HashMap(m) = recv { Some(Ok(Value::Bool(m.is_empty()))) } else { None }
        }
        (Value::HashMap(_), "keys") => {
            if let Value::HashMap(m) = recv {
                let keys: Vec<Value> = m.keys().map(|k| Value::Str(k.clone())).collect();
                Some(Ok(Value::Vec(keys)))
            } else { None }
        }
        (Value::HashMap(_), "values") => {
            if let Value::HashMap(m) = recv {
                let vals: Vec<Value> = m.values().cloned().collect();
                Some(Ok(Value::Vec(vals)))
            } else { None }
        }
        (Value::HashMap(_), "remove") => {
            Some(Ok(Value::Option_(None))) // simplified
        }
        (Value::HashMap(_), "iter" | "into_iter") => {
            if let Value::HashMap(m) = recv {
                let mut pairs: Vec<Value> = m.into_iter()
                    .map(|(k, v)| Value::Tuple(vec![Value::Str(k), v]))
                    .collect();
                pairs.sort_by_key(|p| if let Value::Tuple(t) = p { t[0].to_string() } else { String::new() });
                Some(Ok(Value::Vec(pairs)))
            } else { None }
        }
        (Value::HashMap(_), "clone") => Some(Ok(recv)),

        // ── Range ─────────────────────────────────────────────────────────────
        (Value::Range(start, end, inclusive), "sum") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let sum: i64 = (s..end).sum();
            Some(Ok(Value::Int(sum)))
        }
        (Value::Range(start, end, inclusive), "count" | "len") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            Some(Ok(Value::Int((end - s).max(0))))
        }
        (Value::Range(start, end, inclusive), "min") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            if s >= end { Some(Ok(Value::Option_(None))) }
            else { Some(Ok(Value::Option_(Some(Box::new(Value::Int(s)))))) }
        }
        (Value::Range(start, end, inclusive), "max") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            if s >= end { Some(Ok(Value::Option_(None))) }
            else { Some(Ok(Value::Option_(Some(Box::new(Value::Int(end - 1)))))) }
        }
        (Value::Range(start, end, inclusive), "collect") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            Some(Ok(Value::Vec((s..end).map(Value::Int).collect())))
        }
        (Value::Range(start, end, inclusive), "rev") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            Some(Ok(Value::Vec((s..end).rev().map(Value::Int).collect())))
        }
        (Value::Range(start, end, inclusive), "map") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut result = Vec::new();
            for i in s..end_val {
                let v = match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    },
                    _ => Value::Int(i),
                };
                result.push(v);
            }
            Some(Ok(Value::Vec(result)))
        }
        (Value::Range(start, end, inclusive), "filter") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut result = Vec::new();
            for i in s..end_val {
                let keep = match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(v) => v.is_truthy(),
                        Err(e) => return Some(Err(e)),
                    },
                    _ => true,
                };
                if keep { result.push(Value::Int(i)); }
            }
            Some(Ok(Value::Vec(result)))
        }
        (Value::Range(start, end, inclusive), "filter_map") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut result = Vec::new();
            for i in s..end_val {
                let mapped = match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    },
                    _ => Value::Int(i),
                };
                match mapped {
                    Value::Option_(Some(inner)) => result.push(*inner),
                    Value::Option_(None) => {}
                    other if !matches!(other, Value::Unit) => result.push(other),
                    _ => {}
                }
            }
            Some(Ok(Value::Vec(result)))
        }
        (Value::Range(start, end, inclusive), "product") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let product: i64 = (s..end).product();
            Some(Ok(Value::Int(product)))
        }
        (Value::Range(start, end, inclusive), "find") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            for i in s..end_val {
                let found = match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(v) => v.is_truthy(),
                        Err(e) => return Some(Err(e)),
                    },
                    _ => false,
                };
                if found { return Some(Ok(Value::Option_(Some(Box::new(Value::Int(i)))))); }
            }
            Some(Ok(Value::Option_(None)))
        }
        (Value::Range(start, end, inclusive), "position" | "find_map") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            for (idx, i) in (s..end_val).enumerate() {
                let v = match &func {
                    Value::Fn(cfn) => match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(v) => v,
                        Err(e) => return Some(Err(e)),
                    },
                    _ => Value::Bool(false),
                };
                if v.is_truthy() { return Some(Ok(Value::Option_(Some(Box::new(Value::Int(idx as i64)))))); }
            }
            Some(Ok(Value::Option_(None)))
        }
        (Value::Range(start, end, inclusive), "zip") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let other = match args.into_iter().next() {
                Some(Value::Vec(v)) => v,
                _ => return Some(Ok(Value::Vec(vec![]))),
            };
            let pairs: Vec<Value> = (s..end_val).zip(other.into_iter())
                .map(|(i, v)| Value::Tuple(vec![Value::Int(i), v]))
                .collect();
            Some(Ok(Value::Vec(pairs)))
        }
        (Value::Range(start, end, inclusive), "chain") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let mut result: Vec<Value> = (s..end_val).map(Value::Int).collect();
            match args.into_iter().next() {
                Some(Value::Vec(v)) => result.extend(v),
                Some(Value::Range(s2, e2, inc2)) => {
                    let end2 = if inc2 { e2 + 1 } else { e2 };
                    result.extend((s2..end2).map(Value::Int));
                }
                _ => {}
            }
            Some(Ok(Value::Vec(result)))
        }
        (Value::Range(start, end, inclusive), "for_each") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end_val = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            for i in s..end_val {
                if let Value::Fn(cfn) = &func {
                    match interp.call_crust_fn(cfn, vec![Value::Int(i)], None) {
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Unit))
        }
        (Value::Range(start, end, inclusive), "contains") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let v = args.into_iter().next().unwrap_or(Value::Unit);
            if let Value::Int(n) = v {
                let inside = if inc { n >= s && n <= e } else { n >= s && n < e };
                Some(Ok(Value::Bool(inside)))
            } else {
                Some(Ok(Value::Bool(false)))
            }
        }
        (Value::Range(start, end, inclusive), "fold" | "fold_first") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let mut it = args.into_iter();
            let mut acc = it.next().unwrap_or(Value::Unit);
            let func = it.next().unwrap_or(Value::Unit);
            if let Value::Fn(cfn) = func {
                for n in s..end {
                    match interp.call_crust_fn(&cfn, vec![acc, Value::Int(n)], None) {
                        Ok(v) => acc = v,
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(acc))
        }
        (Value::Range(start, end, inclusive), "scan") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let mut it = args.into_iter();
            let mut acc = it.next().unwrap_or(Value::Int(0));
            let func = it.next().unwrap_or(Value::Unit);
            let mut output = Vec::new();
            if let Value::Fn(cfn) = func {
                for n in s..end {
                    match interp.call_crust_fn(&cfn, vec![acc.clone(), Value::Int(n)], None) {
                        Ok(Value::Option_(Some(inner))) => { acc = *inner.clone(); output.push(*inner); }
                        Ok(Value::Option_(None)) => break,
                        Ok(other) => { acc = other.clone(); output.push(other); }
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Vec(output)))
        }
        (Value::Range(start, end, inclusive), "enumerate") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let pairs: Vec<Value> = (s..end).enumerate()
                .map(|(i, n)| Value::Tuple(vec![Value::Int(i as i64), Value::Int(n)]))
                .collect();
            Some(Ok(Value::Vec(pairs)))
        }
        (Value::Range(start, end, inclusive), "flat_map") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut result = Vec::new();
            if let Value::Fn(cfn) = func {
                for n in s..end {
                    match interp.call_crust_fn(&cfn, vec![Value::Int(n)], None) {
                        Ok(Value::Vec(v)) => result.extend(v),
                        Ok(v) => result.push(v),
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Vec(result)))
        }
        (Value::Range(start, end, inclusive), "skip") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let n = match args.into_iter().next() { Some(Value::Int(n)) => n, _ => 0 };
            Some(Ok(Value::Range(s + n, e, inc)))
        }
        (Value::Range(start, end, inclusive), "take") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let n = match args.into_iter().next() { Some(Value::Int(n)) => n, _ => 0 };
            let new_end = (s + n).min(if inc { e + 1 } else { e });
            Some(Ok(Value::Range(s, new_end, false)))
        }
        (Value::Range(start, end, inclusive), "step_by") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let step = match args.into_iter().next() { Some(Value::Int(n)) => n, _ => 1 };
            let end = if inc { e + 1 } else { e };
            let v: Vec<Value> = (s..end).step_by(step as usize).map(Value::Int).collect();
            Some(Ok(Value::Vec(v)))
        }
        (Value::Range(start, end, inclusive), "any") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            if let Value::Fn(cfn) = func {
                for n in s..end {
                    match interp.call_crust_fn(&cfn, vec![Value::Int(n)], None) {
                        Ok(v) if v.is_truthy() => return Some(Ok(Value::Bool(true))),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Bool(false)))
        }
        (Value::Range(start, end, inclusive), "all") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            if let Value::Fn(cfn) = func {
                for n in s..end {
                    match interp.call_crust_fn(&cfn, vec![Value::Int(n)], None) {
                        Ok(v) if !v.is_truthy() => return Some(Ok(Value::Bool(false))),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Bool(true)))
        }
        (Value::Range(start, end, inclusive), "partition") => {
            let (s, e, inc) = (*start, *end, *inclusive);
            let end = if inc { e + 1 } else { e };
            let func = args.into_iter().next().unwrap_or(Value::Unit);
            let mut yes = Vec::new();
            let mut no = Vec::new();
            if let Value::Fn(cfn) = &func {
                for n in s..end {
                    match interp.call_crust_fn(cfn, vec![Value::Int(n)], None) {
                        Ok(v) if v.is_truthy() => yes.push(Value::Int(n)),
                        Ok(_) => no.push(Value::Int(n)),
                        Err(e) => return Some(Err(e)),
                    }
                }
            }
            Some(Ok(Value::Tuple(vec![Value::Vec(yes), Value::Vec(no)])))
        }
        (Value::Range(..), "clone") => Some(Ok(recv)),

        // ── Universal ─────────────────────────────────────────────────────────
        (_, "copied" | "cloned") => Some(Ok(recv)),  // identity in Level 0
        (_, "to_string") => Some(Ok(Value::Str(recv.to_string()))),
        (_, "clone") => Some(Ok(recv)),
        (_, "type_name") => Some(Ok(Value::Str(recv.type_name().to_string()))),
        // Reference coercions — identity in Level 0
        (_, "as_str" | "as_ref" | "as_mut" | "as_slice" | "borrow" | "borrow_mut"
           | "deref" | "deref_mut" | "into" | "as_deref" | "as_deref_mut") => Some(Ok(recv)),
        // Discard (used to suppress must-use)
        (_, "ok" | "err" | "is_err" | "is_ok") if matches!(&recv, Value::Result_(_)) => {
            match (method, &recv) {
                ("is_ok",  Value::Result_(r)) => Some(Ok(Value::Bool(r.is_ok()))),
                ("is_err", Value::Result_(r)) => Some(Ok(Value::Bool(r.is_err()))),
                ("ok", Value::Result_(Ok(v))) => Some(Ok(Value::Option_(Some(v.clone())))),
                ("ok", Value::Result_(_))     => Some(Ok(Value::Option_(None))),
                ("err", Value::Result_(Err(e))) => Some(Ok(Value::Option_(Some(Box::new(Value::Str(e.to_string())))))),
                ("err", Value::Result_(_))      => Some(Ok(Value::Option_(None))),
                _ => None,
            }
        }
        (_, "unwrap_or_else") => {
            match recv {
                Value::Option_(Some(v)) => Some(Ok(*v)),
                Value::Option_(None) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        Some(interp.call_crust_fn(&cfn, vec![], None))
                    } else { Some(Ok(Value::Unit)) }
                }
                Value::Result_(Ok(v)) => Some(Ok(*v)),
                Value::Result_(Err(_)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    if let Value::Fn(cfn) = func {
                        Some(interp.call_crust_fn(&cfn, vec![], None))
                    } else { Some(Ok(Value::Unit)) }
                }
                other => Some(Ok(other)),
            }
        }
        (_, "ok_or" | "ok_or_else") => {
            match recv {
                Value::Option_(Some(v)) => Some(Ok(Value::Result_(Ok(v)))),
                Value::Option_(None) => {
                    let arg = args.into_iter().next().unwrap_or(Value::Str("None".to_string()));
                    let err_val = if let Value::Fn(cfn) = arg {
                        match interp.call_crust_fn(&cfn, vec![], None) {
                            Ok(v) => v,
                            Err(e) => return Some(Err(e)),
                        }
                    } else { arg };
                    Some(Ok(Value::Result_(Err(Box::new(err_val)))))
                }
                other => Some(Ok(other)),
            }
        }
        (_, "map_err") => {
            match recv {
                Value::Result_(Ok(v)) => Some(Ok(Value::Result_(Ok(v)))),
                Value::Result_(Err(e)) => {
                    let func = args.into_iter().next().unwrap_or(Value::Unit);
                    let new_e = if let Value::Fn(cfn) = func {
                        match interp.call_crust_fn(&cfn, vec![*e], None) {
                            Ok(v) => Box::new(v),
                            Err(sig) => return Some(Err(sig)),
                        }
                    } else { e };
                    Some(Ok(Value::Result_(Err(new_e))))
                }
                other => Some(Ok(other)),
            }
        }
        (_, "unwrap_err") => {
            match recv {
                Value::Result_(Err(e)) => Some(Ok(*e)),
                Value::Result_(Ok(v))  => Some(Err(err(format!("called unwrap_err on Ok({:?})", v)))),
                other => Some(Ok(other)),
            }
        }

        _ => None,
    }
}

// ── Format string interpolation ───────────────────────────────────────────────

pub fn format_string(fmt: &str, args: &[Value]) -> Result<String, CrustError> {
    let mut result = String::new();
    let mut arg_idx = 0;
    let mut chars = fmt.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                result.push('{');
                continue;
            }
            // Consume until }
            let mut spec = String::new();
            for ch in &mut chars {
                if ch == '}' { break; }
                spec.push(ch);
            }
            if arg_idx < args.len() {
                let val = &args[arg_idx];
                arg_idx += 1;
                if spec.contains(':') {
                    let fmt_spec = spec.split(':').nth(1).unwrap_or("");
                    if fmt_spec == "?" || fmt_spec == "#?" {
                        result.push_str(&val.debug_repr());
                    } else if let Some(align_pos) = fmt_spec.find(['>', '<', '^']) {
                        // fill+align: e.g. ">10", "0>5", "<10", "^10", ">8.2"
                        let fill = if align_pos > 0 { fmt_spec.chars().next().unwrap_or(' ') } else { ' ' };
                        let align = fmt_spec.chars().nth(align_pos).unwrap_or('>');
                        let after_align = &fmt_spec[align_pos+1..];
                        // Check for width.precision after align char
                        let s = if let Some(dot) = after_align.find('.') {
                            let w: usize = after_align[..dot].parse().unwrap_or(0);
                            let prec: usize = after_align[dot+1..].trim_end_matches(|c: char| !c.is_numeric()).parse().unwrap_or(6);
                            let base = match val {
                                Value::Float(f) => format!("{:.prec$}", f, prec = prec),
                                Value::Int(n) => format!("{:.prec$}", *n as f64, prec = prec),
                                other => other.to_string(),
                            };
                            let len = base.chars().count();
                            let pad = w.saturating_sub(len);
                            let fill_str: String = std::iter::repeat(fill).take(pad).collect();
                            match align {
                                '>' => format!("{}{}", fill_str, base),
                                '<' => format!("{}{}", base, fill_str),
                                _ => base,
                            }
                        } else {
                            let w: usize = after_align.parse().unwrap_or(0);
                            let s = val.to_string();
                            let len = s.chars().count();
                            let pad = w.saturating_sub(len);
                            let fill_str: String = std::iter::repeat(fill).take(pad).collect();
                            match align {
                                '>' => format!("{}{}", fill_str, s),
                                '<' => format!("{}{}", s, fill_str),
                                '^' => {
                                    let left = pad / 2;
                                    let right = pad - left;
                                    let fl: String = std::iter::repeat(fill).take(left).collect();
                                    let fr: String = std::iter::repeat(fill).take(right).collect();
                                    format!("{}{}{}", fl, s, fr)
                                }
                                _ => s,
                            }
                        };
                        result.push_str(&s);
                    } else if fmt_spec.contains('.') && !fmt_spec.starts_with('.') {
                        // width.precision: {:10.3} or {:10.3} — parse both
                        let dot = fmt_spec.find('.').unwrap();
                        let width: usize = fmt_spec[..dot].parse().unwrap_or(0);
                        let prec: usize = fmt_spec[dot+1..].parse().unwrap_or(6);
                        let s = match val {
                            Value::Float(f) => format!("{:.prec$}", f, prec = prec),
                            Value::Int(n) => format!("{:.prec$}", *n as f64, prec = prec),
                            other => other.to_string(),
                        };
                        if width > s.len() {
                            result.push_str(&format!("{:>width$}", s, width = width));
                        } else {
                            result.push_str(&s);
                        }
                    } else if fmt_spec.starts_with('.') {
                        // precision: {:.2} or {:.3}
                        let prec: usize = fmt_spec[1..].parse().unwrap_or(6);
                        match val {
                            Value::Float(f) => result.push_str(&format!("{:.prec$}", f, prec = prec)),
                            Value::Int(n) => result.push_str(&format!("{:.prec$}", *n as f64, prec = prec)),
                            other => result.push_str(&other.to_string()),
                        }
                    } else if fmt_spec == "+" {
                        match val {
                            Value::Int(n) => result.push_str(&format!("{:+}", n)),
                            Value::Float(f) => result.push_str(&format!("{:+}", f)),
                            other => result.push_str(&other.to_string()),
                        }
                    } else if fmt_spec.ends_with('b') || fmt_spec.ends_with('x') || fmt_spec.ends_with('X') || fmt_spec.ends_with('o') {
                        // Possibly zero-padded with # prefix: {:08b}, {:04x}, {:#x}
                        let suffix = fmt_spec.chars().last().unwrap();
                        let mut rest = &fmt_spec[..fmt_spec.len()-1];
                        let has_prefix = rest.starts_with('#');
                        if has_prefix { rest = &rest[1..]; }
                        let (zero_pad, width) = if rest.starts_with('0') {
                            (true, rest[1..].parse::<usize>().unwrap_or(0))
                        } else {
                            (false, rest.parse::<usize>().unwrap_or(0))
                        };
                        if let Value::Int(n) = val {
                            let prefix = if has_prefix { match suffix { 'b' => "0b", 'x' => "0x", 'X' => "0x", 'o' => "0o", _ => "" } } else { "" };
                            let base_str = match suffix {
                                'b' => format!("{}{:b}", prefix, n),
                                'x' => format!("{}{:x}", prefix, n),
                                'X' => format!("{}{:X}", prefix, n),
                                'o' => format!("{}{:o}", prefix, n),
                                _ => n.to_string(),
                            };
                            if width > 0 {
                                if zero_pad {
                                    result.push_str(&format!("{:0>width$}", base_str, width = width));
                                } else {
                                    result.push_str(&format!("{:width$}", base_str, width = width));
                                }
                            } else {
                                result.push_str(&base_str);
                            }
                        } else { result.push_str(&val.to_string()); }
                    } else if fmt_spec.starts_with('0') {
                        // zero-padded decimal: {:05}
                        let rest = &fmt_spec[1..];
                        if let Ok(w) = rest.parse::<usize>() {
                            match val {
                                Value::Int(n) => result.push_str(&format!("{:0>width$}", n, width = w)),
                                other => result.push_str(&format!("{:0>width$}", other.to_string(), width = w)),
                            }
                        } else {
                            result.push_str(&val.to_string());
                        }
                    } else if fmt_spec == "e" || fmt_spec == "E" {
                        if let Value::Float(f) = val { result.push_str(&format!("{:e}", f)); }
                        else { result.push_str(&val.to_string()); }
                    } else if let Ok(width) = fmt_spec.parse::<usize>() {
                        let s = val.to_string();
                        result.push_str(&format!("{:width$}", s, width = width));
                    } else {
                        result.push_str(&val.to_string());
                    }
                } else {
                    result.push_str(&val.to_string());
                }
            } else {
                result.push_str("{}");
            }
        } else if c == '}' && chars.peek() == Some(&'}') {
            chars.next();
            result.push('}');
        } else {
            result.push(c);
        }
    }

    Ok(result)
}
