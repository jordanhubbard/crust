/// Runtime values for the Crust interpreter.
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Vec(Vec<Value>),
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    Fn {
        name: String,
        params: Vec<(String, String)>,
        body: Vec<crate::ast::Stmt>,
    },
    Unit,
}

impl Value {
    /// Deep clone — all values are implicitly cloned in Level 0
    pub fn crust_clone(&self) -> Value {
        self.clone()
    }

    /// Display format (for {} in println!)
    pub fn display_fmt(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => {
                if *f == (*f as i64) as f64 {
                    format!("{}", *f as i64)
                } else {
                    format!("{}", f)
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::String(s) => s.clone(),
            Value::Vec(v) => {
                let items: Vec<String> = v.iter().map(|i| i.debug_fmt()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Struct { name, fields } => {
                let f: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.debug_fmt()))
                    .collect();
                format!("{} {{ {} }}", name, f.join(", "))
            }
            Value::Fn { name, .. } => format!("<fn {}>", name),
            Value::Unit => "()".to_string(),
        }
    }

    /// Debug format (for {:?} in println!)
    pub fn debug_fmt(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => format!("{:?}", f),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{}\"", s),
            Value::Vec(v) => {
                let items: Vec<String> = v.iter().map(|i| i.debug_fmt()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Struct { name, fields } => {
                let f: Vec<String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.debug_fmt()))
                    .collect();
                format!("{} {{ {} }}", name, f.join(", "))
            }
            Value::Fn { name, .. } => format!("<fn {}>", name),
            Value::Unit => "()".to_string(),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Unit => false,
            _ => true,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_fmt())
    }
}
