use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::{Block, Param, Ty};

#[derive(Debug, Clone)]
pub struct CrustFn {
    pub params: Vec<Param>,
    pub ret_ty: Option<Ty>,
    pub body: Block,
    pub captured: Option<Rc<RefCell<crate::env::Env>>>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Char(char),
    Unit,
    Vec(Vec<Value>),
    HashMap(HashMap<String, Value>),
    Struct {
        type_name: String,
        fields: HashMap<String, Value>,
    },
    Enum {
        type_name: String,
        variant: String,
        inner: Option<Box<Value>>,
    },
    Fn(CrustFn),
    Tuple(Vec<Value>),
    Range(i64, i64, bool), // start, end_exclusive_or_inclusive, inclusive
    Option_(Option<Box<Value>>),
    Result_(std::result::Result<Box<Value>, Box<Value>>),
    EntryRef {
        map_name: String,
        key: String,
    },
    /// Sorted, deduped collection — backs `BTreeSet`. Invariant: items sorted
    /// in ascending order with no duplicates. Iteration honours this order,
    /// matching rustc's `BTreeSet` semantics where Crust's Vec-backed
    /// `HashSet` would emit insertion order. crust-4ri.
    SortedSet(Vec<Value>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Str(s) => write!(f, "{}", s),
            Value::Char(c) => write!(f, "{}", c),
            Value::Unit => write!(f, "()"),
            Value::Vec(v) => {
                write!(f, "[")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, "]")
            }
            Value::Tuple(v) => {
                write!(f, "(")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, ")")
            }
            Value::Struct { type_name, fields } => {
                write!(f, "{} {{", type_name)?;
                let mut pairs: Vec<_> = fields.iter().collect();
                pairs.sort_by_key(|(k, _)| (*k).clone());
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, " {}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Enum {
                type_name,
                variant,
                inner,
            } => {
                write!(f, "{}::{}", type_name, variant)?;
                if let Some(v) = inner {
                    match v.as_ref() {
                        Value::Tuple(items) => {
                            write!(f, "(")?;
                            for (i, item) in items.iter().enumerate() {
                                if i > 0 {
                                    write!(f, ", ")?;
                                }
                                write!(f, "{}", item)?;
                            }
                            write!(f, ")")?;
                        }
                        other => {
                            write!(f, "({})", other)?;
                        }
                    }
                }
                Ok(())
            }
            Value::Fn(_) => write!(f, "<fn>"),
            Value::HashMap(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Range(a, b, inc) => {
                if *inc {
                    write!(f, "{}..={}", a, b)
                } else {
                    write!(f, "{}..{}", a, b)
                }
            }
            Value::Option_(Some(v)) => write!(f, "Some({})", v),
            Value::Option_(None) => write!(f, "None"),
            Value::Result_(Ok(v)) => write!(f, "Ok({})", v),
            Value::Result_(Err(e)) => write!(f, "Err({})", e),
            Value::EntryRef { .. } => write!(f, "<entry-ref>"),
            Value::SortedSet(v) => {
                write!(f, "{{")?;
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", x)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "i64",
            Value::Float(_) => "f64",
            Value::Bool(_) => "bool",
            Value::Str(_) => "String",
            Value::Char(_) => "char",
            Value::Unit => "()",
            Value::Vec(_) => "Vec",
            Value::HashMap(_) => "HashMap",
            Value::Struct { .. } => "struct",
            Value::Enum { .. } => "enum",
            Value::Fn(_) => "fn",
            Value::Tuple(_) => "tuple",
            Value::Range(..) => "Range",
            Value::Option_(_) => "Option",
            Value::Result_(_) => "Result",
            Value::EntryRef { .. } => "EntryRef",
            Value::SortedSet(_) => "BTreeSet",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Unit => false,
            _ => true,
        }
    }

    pub fn debug_repr(&self) -> String {
        match self {
            Value::Str(s) => format!("{:?}", s),
            Value::Char(c) => format!("{:?}", c),
            Value::Float(f) => format!("{:?}", f), // shows 58.0 instead of 58
            Value::Vec(v) => {
                let items: Vec<String> = v.iter().map(|x| x.debug_repr()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::SortedSet(v) => {
                let items: Vec<String> = v.iter().map(|x| x.debug_repr()).collect();
                format!("{{{}}}", items.join(", "))
            }
            Value::Tuple(v) => {
                let items: Vec<String> = v.iter().map(|x| x.debug_repr()).collect();
                format!("({})", items.join(", "))
            }
            Value::Option_(Some(v)) => format!("Some({})", v.debug_repr()),
            Value::Option_(None) => "None".to_string(),
            Value::Result_(Ok(v)) => format!("Ok({})", v.debug_repr()),
            Value::Result_(Err(e)) => format!("Err({})", e.debug_repr()),
            Value::Enum {
                type_name,
                variant,
                inner,
            } => {
                let prefix = format!("{}::{}", type_name, variant);
                match inner {
                    None => prefix,
                    Some(v) => match v.as_ref() {
                        Value::Tuple(items) => {
                            let parts: Vec<String> = items.iter().map(|x| x.debug_repr()).collect();
                            format!("{}({})", prefix, parts.join(", "))
                        }
                        other => format!("{}({})", prefix, other.debug_repr()),
                    },
                }
            }
            Value::Struct { type_name, fields } => {
                let mut pairs: Vec<_> = fields.iter().collect();
                pairs.sort_by_key(|(k, _)| k.parse::<usize>().unwrap_or(usize::MAX));
                // Tuple struct: all fields are numeric keys → TypeName(v0, v1, ...)
                let is_tuple =
                    !pairs.is_empty() && pairs.iter().all(|(k, _)| k.parse::<usize>().is_ok());
                if is_tuple {
                    let parts: Vec<String> = pairs.iter().map(|(_, v)| v.debug_repr()).collect();
                    format!("{}({})", type_name, parts.join(", "))
                } else {
                    let parts: Vec<String> = pairs
                        .iter()
                        .map(|(k, v)| format!("{}: {}", k, v.debug_repr()))
                        .collect();
                    format!("{} {{ {} }}", type_name, parts.join(", "))
                }
            }
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_int() {
        assert_eq!(Value::Int(42).to_string(), "42");
    }

    #[test]
    fn display_str() {
        assert_eq!(Value::Str("hello".into()).to_string(), "hello");
    }

    #[test]
    fn display_vec() {
        let v = Value::Vec(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(v.to_string(), "[1, 2]");
    }

    #[test]
    fn truthy() {
        assert!(Value::Bool(true).is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        assert!(Value::Int(1).is_truthy());
        assert!(!Value::Int(0).is_truthy());
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn type_name_for_each_variant() {
        assert_eq!(Value::Int(1).type_name(), "i64");
        assert_eq!(Value::Float(1.0).type_name(), "f64");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Str("a".into()).type_name(), "String");
        assert_eq!(Value::Char('c').type_name(), "char");
        assert_eq!(Value::Unit.type_name(), "()");
        assert_eq!(Value::Vec(vec![]).type_name(), "Vec");
        assert_eq!(Value::HashMap(HashMap::new()).type_name(), "HashMap");
        assert_eq!(Value::Tuple(vec![]).type_name(), "tuple");
        assert_eq!(Value::Range(0, 0, false).type_name(), "Range");
        assert_eq!(Value::Option_(None).type_name(), "Option");
        assert_eq!(
            Value::Result_(Ok(Box::new(Value::Unit))).type_name(),
            "Result"
        );
    }

    #[test]
    fn debug_repr_quotes_strings_and_chars() {
        assert_eq!(Value::Str("hi".into()).debug_repr(), "\"hi\"");
        assert_eq!(Value::Char('a').debug_repr(), "'a'");
    }

    #[test]
    fn debug_repr_floats_show_decimal() {
        // 58.0 should debug-print as "58.0", not "58"
        let s = Value::Float(58.0).debug_repr();
        assert!(s.contains('.'));
    }

    #[test]
    fn debug_repr_recurses_into_compound() {
        let v = Value::Vec(vec![Value::Int(1), Value::Str("two".into())]);
        assert_eq!(v.debug_repr(), "[1, \"two\"]");
    }

    #[test]
    fn debug_repr_handles_some_and_none() {
        assert_eq!(
            Value::Option_(Some(Box::new(Value::Int(5)))).debug_repr(),
            "Some(5)"
        );
        assert_eq!(Value::Option_(None).debug_repr(), "None");
    }

    #[test]
    fn debug_repr_handles_ok_and_err() {
        assert_eq!(
            Value::Result_(Ok(Box::new(Value::Int(5)))).debug_repr(),
            "Ok(5)"
        );
        assert_eq!(
            Value::Result_(Err(Box::new(Value::Str("e".into())))).debug_repr(),
            "Err(\"e\")"
        );
    }

    #[test]
    fn debug_repr_tuple_struct_versus_named() {
        let mut fields = HashMap::new();
        fields.insert("0".to_string(), Value::Int(1));
        fields.insert("1".to_string(), Value::Int(2));
        let tup_struct = Value::Struct {
            type_name: "Pair".to_string(),
            fields,
        };
        // Tuple struct (numeric field keys) prints as Pair(1, 2)
        let s = tup_struct.debug_repr();
        assert!(s.starts_with("Pair("));
    }

    #[test]
    fn display_for_enum_with_no_inner() {
        let v = Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
            inner: None,
        };
        assert_eq!(v.to_string(), "Color::Red");
    }

    #[test]
    fn display_for_range() {
        assert_eq!(Value::Range(0, 5, false).to_string(), "0..5");
        assert_eq!(Value::Range(0, 5, true).to_string(), "0..=5");
    }

    #[test]
    fn truthy_for_int_zero_is_false() {
        assert!(!Value::Int(0).is_truthy());
        assert!(Value::Int(-1).is_truthy());
    }

    #[test]
    fn truthy_for_unit_is_false() {
        assert!(!Value::Unit.is_truthy());
    }

    #[test]
    fn truthy_for_other_compound_is_true() {
        assert!(Value::Vec(vec![]).is_truthy());
        assert!(Value::Str("".into()).is_truthy());
    }
}
