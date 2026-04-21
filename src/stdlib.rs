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
        // Vec mutating methods
        (Value::Vec(mut v), "push") => {
            let val = args.into_iter().next().unwrap_or(Value::Unit);
            v.push(val);
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "pop") => {
            let result = v.pop();
            Some((Ok(Value::Option_(result.map(Box::new))), Value::Vec(v)))
        }
        (Value::Vec(mut v), "insert") => {
            let mut it = args.into_iter();
            if let (Some(Value::Int(idx)), Some(val)) = (it.next(), it.next()) {
                let idx = idx.max(0) as usize;
                if idx <= v.len() { v.insert(idx, val); }
            }
            Some((Ok(Value::Unit), Value::Vec(v)))
        }
        (Value::Vec(mut v), "remove") => {
            let idx = match args.into_iter().next() {
                Some(Value::Int(i)) => i as usize,
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

        // String conversions
        "String::from" | "String::new" => {
            let s = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(s)))
        }
        "str::to_string" => {
            let s = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(s)))
        }

        // Vec constructors
        "Vec::new" => Some(Ok(Value::Vec(Vec::new()))),
        "Vec::with_capacity" => Some(Ok(Value::Vec(Vec::new()))),

        // HashMap constructors
        "HashMap::new" => Some(Ok(Value::HashMap(HashMap::new()))),

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

        // Misc
        "drop" => Some(Ok(Value::Unit)),
        "clone" => Some(Ok(args.into_iter().next().unwrap_or(Value::Unit))),
        "String::new" => Some(Ok(Value::Str(String::new()))),
        "String::from" | "String::from_str" => {
            let s = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
            Some(Ok(Value::Str(s)))
        }
        "String::with_capacity" => Some(Ok(Value::Str(String::new()))),
        "Vec::new" | "Vec::new()" => Some(Ok(Value::Vec(Vec::new()))),
        "Vec::with_capacity" => Some(Ok(Value::Vec(Vec::new()))),
        "println" | "print" | "eprintln" | "eprint" => {
            let s = args.into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ");
            println!("{}", s);
            interp.output.push(s);
            Some(Ok(Value::Unit))
        }

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
        (Value::Vec(_), "first") => {
            if let Value::Vec(v) = recv {
                Some(Ok(Value::Option_(v.into_iter().next().map(Box::new))))
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
        (Value::Vec(_), "collect") => Some(Ok(recv)),
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
        (Value::Vec(_), "step_by") => {
            if let Value::Vec(v) = recv {
                let n = match args.into_iter().next() { Some(Value::Int(n)) => n.max(1) as usize, _ => 1 };
                Some(Ok(Value::Vec(v.into_iter().step_by(n).collect())))
            } else { None }
        }
        (Value::Vec(_), "flatten") => {
            if let Value::Vec(v) = recv {
                let mut result = Vec::new();
                for item in v {
                    match item {
                        Value::Vec(inner) => result.extend(inner),
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
                let sep = args.into_iter().next().map(|v| v.to_string()).unwrap_or_default();
                let parts: Vec<Value> = s.split(&sep[..]).map(|p| Value::Str(p.to_string())).collect();
                Some(Ok(Value::Vec(parts)))
            } else { None }
        }
        (Value::Str(_), "splitn") => {
            if let Value::Str(s) = recv {
                let n = match args.first() { Some(Value::Int(n)) => *n as usize, _ => 0 };
                let sep = args.into_iter().nth(1).map(|v| v.to_string()).unwrap_or_default();
                let parts: Vec<Value> = s.splitn(n, &sep[..]).map(|p| Value::Str(p.to_string())).collect();
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
        (Value::Str(_), "to_string" | "clone") => Some(Ok(recv)),
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
        (Value::Float(_), "to_string" | "clone") => Some(Ok(recv)),
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
        (Value::Int(_), "to_string" | "clone") => Some(Ok(recv)),
        (Value::Int(_), "count_ones") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.count_ones() as i64))) } else { None }
        }
        (Value::Int(_), "leading_zeros") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.leading_zeros() as i64))) } else { None }
        }
        (Value::Int(_), "trailing_zeros") => {
            if let Value::Int(n) = recv { Some(Ok(Value::Int(n.trailing_zeros() as i64))) } else { None }
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
        (Value::Range(..), "clone") => Some(Ok(recv)),

        // ── Universal ─────────────────────────────────────────────────────────
        (_, "copied" | "cloned") => Some(Ok(recv)),  // identity in Level 0
        (_, "to_string" | "clone") => Some(Ok(recv)),
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
                    let err_val = args.into_iter().next().unwrap_or(Value::Str("None".to_string()));
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
                    } else if let Some(width) = fmt_spec.strip_prefix('>') {
                        let w: usize = width.parse().unwrap_or(0);
                        result.push_str(&format!("{:>width$}", val, width = w));
                    } else if let Some(width) = fmt_spec.strip_prefix('<') {
                        let w: usize = width.parse().unwrap_or(0);
                        result.push_str(&format!("{:<width$}", val, width = w));
                    } else if let Some(width) = fmt_spec.strip_prefix('^') {
                        let w: usize = width.parse().unwrap_or(0);
                        result.push_str(&format!("{:^width$}", val, width = w));
                    } else if fmt_spec.starts_with('0') {
                        // zero-padded
                        result.push_str(&val.to_string());
                    } else if fmt_spec == "b" {
                        if let Value::Int(n) = val { result.push_str(&format!("{:b}", n)); }
                        else { result.push_str(&val.to_string()); }
                    } else if fmt_spec == "x" || fmt_spec == "X" {
                        if let Value::Int(n) = val {
                            if fmt_spec == "x" { result.push_str(&format!("{:x}", n)); }
                            else { result.push_str(&format!("{:X}", n)); }
                        } else { result.push_str(&val.to_string()); }
                    } else if fmt_spec == "o" {
                        if let Value::Int(n) = val { result.push_str(&format!("{:o}", n)); }
                        else { result.push_str(&val.to_string()); }
                    } else if fmt_spec == "e" || fmt_spec == "E" {
                        if let Value::Float(f) = val { result.push_str(&format!("{:e}", f)); }
                        else { result.push_str(&val.to_string()); }
                    } else if let Ok(width) = fmt_spec.parse::<usize>() {
                        result.push_str(&format!("{:width$}", val, width = width));
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
