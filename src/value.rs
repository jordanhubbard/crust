use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::cell::RefCell;

use crate::ast::{Block, Param};

#[derive(Debug, Clone)]
pub struct CrustFn {
    pub params: Vec<Param>,
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
    Struct { type_name: String, fields: HashMap<String, Value> },
    Enum { type_name: String, variant: String, inner: Option<Box<Value>> },
    Fn(CrustFn),
    Tuple(Vec<Value>),
    Range(i64, i64, bool),   // start, end_exclusive_or_inclusive, inclusive
    Option_(Option<Box<Value>>),
    Result_(std::result::Result<Box<Value>, Box<Value>>),
    EntryRef { map_name: String, key: String },
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n)   => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Bool(b)  => write!(f, "{}", b),
            Value::Str(s)   => write!(f, "{}", s),
            Value::Char(c)  => write!(f, "{}", c),
            Value::Unit     => write!(f, "()"),
            Value::Vec(v)   => {
                write!(f, "[")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", val)?;
                }
                write!(f, "]")
            }
            Value::Tuple(v) => {
                write!(f, "(")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", val)?;
                }
                write!(f, ")")
            }
            Value::Struct { type_name, fields } => {
                write!(f, "{} {{", type_name)?;
                let mut pairs: Vec<_> = fields.iter().collect();
                pairs.sort_by_key(|(k, _)| k.clone());
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, " {}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::Enum { type_name, variant, inner } => {
                write!(f, "{}::{}", type_name, variant)?;
                if let Some(v) = inner { write!(f, "({})", v)?; }
                Ok(())
            }
            Value::Fn(_) => write!(f, "<fn>"),
            Value::HashMap(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Range(a, b, inc) => {
                if *inc { write!(f, "{}..={}", a, b) }
                else    { write!(f, "{}..{}", a, b) }
            }
            Value::Option_(Some(v)) => write!(f, "Some({})", v),
            Value::Option_(None)    => write!(f, "None"),
            Value::Result_(Ok(v))   => write!(f, "Ok({})", v),
            Value::Result_(Err(e))  => write!(f, "Err({})", e),
            Value::EntryRef { .. }  => write!(f, "<entry-ref>"),
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_)     => "i64",
            Value::Float(_)   => "f64",
            Value::Bool(_)    => "bool",
            Value::Str(_)     => "String",
            Value::Char(_)    => "char",
            Value::Unit       => "()",
            Value::Vec(_)     => "Vec",
            Value::HashMap(_) => "HashMap",
            Value::Struct { .. } => "struct",
            Value::Enum { .. }   => "enum",
            Value::Fn(_)         => "fn",
            Value::Tuple(_)      => "tuple",
            Value::Range(..)     => "Range",
            Value::Option_(_)    => "Option",
            Value::Result_(_)    => "Result",
            Value::EntryRef { .. } => "EntryRef",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n)  => *n != 0,
            Value::Unit    => false,
            _              => true,
        }
    }

    pub fn debug_repr(&self) -> String {
        match self {
            Value::Str(s) => format!("{:?}", s),
            Value::Char(c) => format!("{:?}", c),
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
