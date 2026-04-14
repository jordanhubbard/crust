use super::error::CrustError;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// Runtime value in the Crust interpreter.
/// In hack mode, everything is cheaply cloneable via Rc.
#[derive(Clone)]
pub enum Value {
    /// 64-bit integer (covers i32, i64, usize in hack mode)
    Int(i64),
    /// 64-bit float
    Float(f64),
    /// Boolean
    Bool(bool),
    /// Character
    Char(char),
    /// Heap-allocated string (always String, never &str in hack mode)
    Str(String),
    /// Vector — Rc<RefCell<>> for shared mutability in hack mode
    Vec(Rc<RefCell<Vec<Value>>>),
    /// Tuple
    Tuple(Vec<Value>),
    /// HashMap
    HashMap(Rc<RefCell<HashMap<String, Value>>>),
    /// Struct instance
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    /// Struct definition (constructor)
    StructDef {
        fields: Vec<(String, String)>,
    },
    /// Function (user-defined)
    Function {
        params: Vec<(String, String)>,
        body: Vec<syn::Stmt>,
        closure_env: Rc<RefCell<super::env::Environment>>,
        return_type: Option<String>,
    },
    /// Built-in function
    BuiltinFn(fn(Vec<Value>) -> Result<Value, CrustError>),
    /// Option<Value>
    Option(Option<Box<Value>>),
    /// Result<Value, Value>
    Result(Result<Box<Value>, Box<Value>>),
    /// Unit type ()
    Unit,
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "Int({})", n),
            Value::Float(n) => write!(f, "Float({})", n),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::Char(c) => write!(f, "Char('{}')", c),
            Value::Str(s) => write!(f, "Str(\"{}\")", s),
            Value::Vec(v) => write!(f, "Vec({:?})", v.borrow()),
            Value::Tuple(v) => write!(f, "Tuple({:?})", v),
            Value::HashMap(m) => write!(f, "HashMap({:?})", m.borrow()),
            Value::Struct { name, fields } => write!(f, "Struct({} {{ {:?} }})", name, fields),
            Value::StructDef { fields } => write!(f, "StructDef({:?})", fields),
            Value::Function { params, .. } => write!(f, "Function({:?})", params),
            Value::BuiltinFn(_) => write!(f, "BuiltinFn"),
            Value::Option(v) => write!(f, "Option({:?})", v),
            Value::Result(v) => write!(f, "Result({:?})", v),
            Value::Unit => write!(f, "()"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if *n == n.floor() && !n.is_infinite() {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Char(c) => write!(f, "{}", c),
            Value::Str(s) => write!(f, "{}", s),
            Value::Vec(v) => {
                let items = v.borrow();
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    // Strings in debug format for vec display
                    match item {
                        Value::Str(s) => write!(f, "\"{}\"", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, "]")
            }
            Value::Tuple(v) => {
                write!(f, "(")?;
                for (i, item) in v.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                if v.len() == 1 { write!(f, ",")?; }
                write!(f, ")")
            }
            Value::HashMap(m) => {
                let map = m.borrow();
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", k, v)?;
                }
                write!(f, "}}")
            }
            Value::Struct { name, fields } => {
                write!(f, "{} {{ ", name)?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, " }}")
            }
            Value::StructDef { .. } => write!(f, "<struct>"),
            Value::Function { .. } => write!(f, "<function>"),
            Value::BuiltinFn(_) => write!(f, "<builtin>"),
            Value::Option(Some(v)) => write!(f, "Some({})", v),
            Value::Option(None) => write!(f, "None"),
            Value::Result(Ok(v)) => write!(f, "Ok({})", v),
            Value::Result(Err(v)) => write!(f, "Err({})", v),
            Value::Unit => write!(f, "()"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            (Value::Option(a), Value::Option(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn is_unit(&self) -> bool {
        matches!(self, Value::Unit)
    }

    // ── Type conversions ───────────────────────────────────────────

    pub fn as_bool(&self) -> Result<bool, CrustError> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Int(n) => Ok(*n != 0),
            _ => Err(CrustError::Type(format!("expected bool, got {}", self.type_name()))),
        }
    }

    pub fn as_i64(&self) -> Result<i64, CrustError> {
        match self {
            Value::Int(n) => Ok(*n),
            Value::Float(f) => Ok(*f as i64),
            _ => Err(CrustError::Type(format!("expected integer, got {}", self.type_name()))),
        }
    }

    pub fn as_f64(&self) -> Result<f64, CrustError> {
        match self {
            Value::Float(f) => Ok(*f),
            Value::Int(n) => Ok(*n as f64),
            _ => Err(CrustError::Type(format!("expected float, got {}", self.type_name()))),
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "i64",
            Value::Float(_) => "f64",
            Value::Bool(_) => "bool",
            Value::Char(_) => "char",
            Value::Str(_) => "String",
            Value::Vec(_) => "Vec",
            Value::Tuple(_) => "tuple",
            Value::HashMap(_) => "HashMap",
            Value::Struct { name, .. } => name.as_str(),
            Value::StructDef { .. } => "struct",
            Value::Function { .. } => "fn",
            Value::BuiltinFn(_) => "fn",
            Value::Option(_) => "Option",
            Value::Result(_) => "Result",
            Value::Unit => "()",
        }
    }

    // ── Arithmetic ─────────────────────────────────────────────────

    pub fn add(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
            _ => Err(CrustError::Type(format!("cannot add {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn sub(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            _ => Err(CrustError::Type(format!("cannot subtract {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn mul(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * *b as f64)),
            _ => Err(CrustError::Type(format!("cannot multiply {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn div(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(_, ), Value::Int(0)) => Err(CrustError::Runtime("division by zero".into())),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
            _ => Err(CrustError::Type(format!("cannot divide {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn rem(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
            _ => Err(CrustError::Type(format!("cannot modulo {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn negate(&self) -> Result<Value, CrustError> {
        match self {
            Value::Int(n) => Ok(Value::Int(-n)),
            Value::Float(n) => Ok(Value::Float(-n)),
            _ => Err(CrustError::Type(format!("cannot negate {}", self.type_name()))),
        }
    }

    pub fn not(&self) -> Result<Value, CrustError> {
        match self {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            Value::Int(n) => Ok(Value::Int(!n)),
            _ => Err(CrustError::Type(format!("cannot apply ! to {}", self.type_name()))),
        }
    }

    // ── Bitwise ────────────────────────────────────────────────────

    pub fn bitand(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            _ => Err(CrustError::Type("bitwise AND requires integers".into())),
        }
    }

    pub fn bitor(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            _ => Err(CrustError::Type("bitwise OR requires integers".into())),
        }
    }

    pub fn bitxor(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
            _ => Err(CrustError::Type("bitwise XOR requires integers".into())),
        }
    }

    pub fn shl(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a << b)),
            _ => Err(CrustError::Type("shift requires integers".into())),
        }
    }

    pub fn shr(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a >> b)),
            _ => Err(CrustError::Type("shift requires integers".into())),
        }
    }

    // ── Comparison ─────────────────────────────────────────────────

    pub fn lt_val(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) < *b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a < (*b as f64))),
            (Value::Str(a), Value::Str(b)) => Ok(Value::Bool(a < b)),
            _ => Err(CrustError::Type(format!("cannot compare {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn le_val(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) <= *b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a <= (*b as f64))),
            _ => Err(CrustError::Type(format!("cannot compare {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn gt_val(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) > *b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a > (*b as f64))),
            _ => Err(CrustError::Type(format!("cannot compare {} and {}", self.type_name(), other.type_name()))),
        }
    }

    pub fn ge_val(&self, other: &Value) -> Result<Value, CrustError> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Bool((*a as f64) >= *b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Bool(*a >= (*b as f64))),
            _ => Err(CrustError::Type(format!("cannot compare {} and {}", self.type_name(), other.type_name()))),
        }
    }

    // ── Collection operations ──────────────────────────────────────

    pub fn get_field(&self, name: &str) -> Result<Value, CrustError> {
        match self {
            Value::Struct { fields, .. } => {
                fields.get(name).cloned()
                    .ok_or_else(|| CrustError::Runtime(format!("no field `{}` on struct", name)))
            }
            Value::Tuple(vals) => {
                let idx: usize = name.parse()
                    .map_err(|_| CrustError::Runtime(format!("invalid tuple index: {}", name)))?;
                vals.get(idx).cloned()
                    .ok_or_else(|| CrustError::Runtime(format!("tuple index {} out of bounds", idx)))
            }
            _ => Err(CrustError::Runtime(format!("cannot access field on {}", self.type_name()))),
        }
    }

    pub fn index(&self, idx: &Value) -> Result<Value, CrustError> {
        match self {
            Value::Vec(v) => {
                let i = idx.as_i64()? as usize;
                let vec = v.borrow();
                vec.get(i).cloned()
                    .ok_or_else(|| CrustError::Runtime(format!("index {} out of bounds (len {})", i, vec.len())))
            }
            Value::Str(s) => {
                let i = idx.as_i64()? as usize;
                s.chars().nth(i).map(Value::Char)
                    .ok_or_else(|| CrustError::Runtime(format!("string index {} out of bounds", i)))
            }
            Value::HashMap(m) => {
                if let Value::Str(key) = idx {
                    let map = m.borrow();
                    map.get(key).cloned()
                        .ok_or_else(|| CrustError::Runtime(format!("key \"{}\" not found", key)))
                } else {
                    Err(CrustError::Runtime("HashMap key must be a string".into()))
                }
            }
            _ => Err(CrustError::Runtime(format!("cannot index {}", self.type_name()))),
        }
    }

    pub fn into_iter(self) -> Result<Vec<Value>, CrustError> {
        match self {
            Value::Vec(v) => Ok(v.borrow().clone()),
            Value::Str(s) => Ok(s.chars().map(Value::Char).collect()),
            _ => Err(CrustError::Runtime(format!("cannot iterate over {}", self.type_name()))),
        }
    }

    // ── Type casting ───────────────────────────────────────────────

    pub fn cast_to(&self, target: &str) -> Result<Value, CrustError> {
        match target {
            "i32" | "i64" | "isize" => Ok(Value::Int(self.as_i64()?)),
            "f32" | "f64" => Ok(Value::Float(self.as_f64()?)),
            "bool" => Ok(Value::Bool(self.as_bool()?)),
            "String" => Ok(Value::Str(format!("{}", self))),
            "usize" => Ok(Value::Int(self.as_i64()?)),
            _ => Err(CrustError::Type(format!("cannot cast {} to {}", self.type_name(), target))),
        }
    }

    // ── Method dispatch ────────────────────────────────────────────

    pub fn call_method(
        &self,
        name: &str,
        args: Vec<Value>,
        interp: &super::Interpreter,
        env: &Rc<RefCell<super::env::Environment>>,
    ) -> Result<Value, CrustError> {
        use std::rc::Rc;

        match (self, name) {
            // ── String methods ──
            (Value::Str(s), "len") => Ok(Value::Int(s.len() as i64)),
            (Value::Str(s), "is_empty") => Ok(Value::Bool(s.is_empty())),
            (Value::Str(s), "contains") => {
                let needle = args.first().ok_or_else(|| CrustError::Runtime("contains requires an argument".into()))?;
                if let Value::Str(n) = needle {
                    Ok(Value::Bool(s.contains(n.as_str())))
                } else {
                    Err(CrustError::Type("contains argument must be a string".into()))
                }
            }
            (Value::Str(s), "to_uppercase") => Ok(Value::Str(s.to_uppercase())),
            (Value::Str(s), "to_lowercase") => Ok(Value::Str(s.to_lowercase())),
            (Value::Str(s), "trim") => Ok(Value::Str(s.trim().to_string())),
            (Value::Str(s), "starts_with") => {
                let prefix = args.first().and_then(|a| if let Value::Str(s) = a { Some(s.as_str()) } else { None })
                    .ok_or_else(|| CrustError::Runtime("starts_with requires a string argument".into()))?;
                Ok(Value::Bool(s.starts_with(prefix)))
            }
            (Value::Str(s), "ends_with") => {
                let suffix = args.first().and_then(|a| if let Value::Str(s) = a { Some(s.as_str()) } else { None })
                    .ok_or_else(|| CrustError::Runtime("ends_with requires a string argument".into()))?;
                Ok(Value::Bool(s.ends_with(suffix)))
            }
            (Value::Str(s), "replace") => {
                let from = args.first().and_then(|a| if let Value::Str(s) = a { Some(s.clone()) } else { None })
                    .ok_or_else(|| CrustError::Runtime("replace requires string arguments".into()))?;
                let to = args.get(1).and_then(|a| if let Value::Str(s) = a { Some(s.clone()) } else { None })
                    .ok_or_else(|| CrustError::Runtime("replace requires two string arguments".into()))?;
                Ok(Value::Str(s.replace(&from, &to)))
            }
            (Value::Str(s), "split") => {
                let delim = args.first().and_then(|a| if let Value::Str(s) = a { Some(s.clone()) } else { None })
                    .ok_or_else(|| CrustError::Runtime("split requires a string argument".into()))?;
                let parts: Vec<Value> = s.split(&delim).map(|p| Value::Str(p.to_string())).collect();
                Ok(Value::Vec(Rc::new(RefCell::new(parts))))
            }
            (Value::Str(s), "chars") => {
                let chars: Vec<Value> = s.chars().map(Value::Char).collect();
                Ok(Value::Vec(Rc::new(RefCell::new(chars))))
            }
            (Value::Str(s), "parse") => {
                // Try parsing as various types
                if let Ok(n) = s.parse::<i64>() {
                    Ok(Value::Result(Ok(Box::new(Value::Int(n)))))
                } else if let Ok(f) = s.parse::<f64>() {
                    Ok(Value::Result(Ok(Box::new(Value::Float(f)))))
                } else if let Ok(b) = s.parse::<bool>() {
                    Ok(Value::Result(Ok(Box::new(Value::Bool(b)))))
                } else {
                    Ok(Value::Result(Err(Box::new(Value::Str(format!("failed to parse: {}", s))))))
                }
            }
            (Value::Str(s), "to_string") => Ok(Value::Str(s.clone())),

            // ── Vec methods ──
            (Value::Vec(v), "len") => Ok(Value::Int(v.borrow().len() as i64)),
            (Value::Vec(v), "is_empty") => Ok(Value::Bool(v.borrow().is_empty())),
            (Value::Vec(v), "push") => {
                let val = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("push requires an argument".into()))?;
                v.borrow_mut().push(val);
                Ok(Value::Unit)
            }
            (Value::Vec(v), "pop") => {
                Ok(v.borrow_mut().pop()
                    .map(|val| Value::Option(Some(Box::new(val))))
                    .unwrap_or(Value::Option(None)))
            }
            (Value::Vec(v), "first") => {
                Ok(v.borrow().first().cloned()
                    .map(|val| Value::Option(Some(Box::new(val))))
                    .unwrap_or(Value::Option(None)))
            }
            (Value::Vec(v), "last") => {
                Ok(v.borrow().last().cloned()
                    .map(|val| Value::Option(Some(Box::new(val))))
                    .unwrap_or(Value::Option(None)))
            }
            (Value::Vec(v), "contains") => {
                let needle = args.first()
                    .ok_or_else(|| CrustError::Runtime("contains requires an argument".into()))?;
                Ok(Value::Bool(v.borrow().contains(needle)))
            }
            (Value::Vec(v), "reverse") => {
                v.borrow_mut().reverse();
                Ok(Value::Unit)
            }
            (Value::Vec(v), "iter") => {
                // In hack mode, .iter() just returns the vec itself for chaining
                Ok(Value::Vec(v.clone()))
            }
            (Value::Vec(v), "into_iter") => {
                Ok(Value::Vec(v.clone()))
            }
            (Value::Vec(v), "map") => {
                let func = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("map requires a closure argument".into()))?;
                let items = v.borrow().clone();
                let mut result = Vec::new();
                for item in items {
                    let val = self.apply_fn(&func, vec![item], interp, env)?;
                    result.push(val);
                }
                Ok(Value::Vec(Rc::new(RefCell::new(result))))
            }
            (Value::Vec(v), "filter") => {
                let func = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("filter requires a closure argument".into()))?;
                let items = v.borrow().clone();
                let mut result = Vec::new();
                for item in items {
                    let keep = self.apply_fn(&func, vec![item.clone()], interp, env)?;
                    if keep.as_bool()? {
                        result.push(item);
                    }
                }
                Ok(Value::Vec(Rc::new(RefCell::new(result))))
            }
            (Value::Vec(v), "fold") => {
                let mut args_iter = args.into_iter();
                let init = args_iter.next()
                    .ok_or_else(|| CrustError::Runtime("fold requires initial value".into()))?;
                let func = args_iter.next()
                    .ok_or_else(|| CrustError::Runtime("fold requires a closure argument".into()))?;
                let items = v.borrow().clone();
                let mut acc = init;
                for item in items {
                    acc = self.apply_fn(&func, vec![acc, item], interp, env)?;
                }
                Ok(acc)
            }
            (Value::Vec(v), "enumerate") => {
                let items = v.borrow().clone();
                let enumerated: Vec<Value> = items.into_iter().enumerate()
                    .map(|(i, val)| Value::Tuple(vec![Value::Int(i as i64), val]))
                    .collect();
                Ok(Value::Vec(Rc::new(RefCell::new(enumerated))))
            }
            (Value::Vec(v), "collect") => {
                // .collect() on a Vec just returns itself
                Ok(Value::Vec(v.clone()))
            }
            (Value::Vec(v), "sum") => {
                let items = v.borrow();
                let mut total: i64 = 0;
                let mut is_float = false;
                let mut ftotal: f64 = 0.0;
                for item in items.iter() {
                    match item {
                        Value::Int(n) => {
                            total += n;
                            ftotal += *n as f64;
                        }
                        Value::Float(f) => {
                            is_float = true;
                            ftotal += f;
                        }
                        _ => return Err(CrustError::Type("sum requires numeric values".into())),
                    }
                }
                if is_float {
                    Ok(Value::Float(ftotal))
                } else {
                    Ok(Value::Int(total))
                }
            }
            (Value::Vec(v), "join") => {
                let sep = args.first().and_then(|a| if let Value::Str(s) = a { Some(s.clone()) } else { None })
                    .unwrap_or_default();
                let items = v.borrow();
                let s: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                Ok(Value::Str(s.join(&sep)))
            }
            (Value::Vec(v), "sort") => {
                let mut items = v.borrow_mut();
                items.sort_by(|a, b| {
                    match (a, b) {
                        (Value::Int(a), Value::Int(b)) => a.cmp(b),
                        (Value::Str(a), Value::Str(b)) => a.cmp(b),
                        _ => std::cmp::Ordering::Equal,
                    }
                });
                Ok(Value::Unit)
            }
            (Value::Vec(v), "for_each") => {
                let func = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("for_each requires a closure argument".into()))?;
                let items = v.borrow().clone();
                for item in items {
                    self.apply_fn(&func, vec![item], interp, env)?;
                }
                Ok(Value::Unit)
            }
            (Value::Vec(v), "any") => {
                let func = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("any requires a closure argument".into()))?;
                let items = v.borrow().clone();
                for item in items {
                    let result = self.apply_fn(&func, vec![item], interp, env)?;
                    if result.as_bool()? {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            (Value::Vec(v), "all") => {
                let func = args.into_iter().next()
                    .ok_or_else(|| CrustError::Runtime("all requires a closure argument".into()))?;
                let items = v.borrow().clone();
                for item in items {
                    let result = self.apply_fn(&func, vec![item], interp, env)?;
                    if !result.as_bool()? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }

            // ── Option methods ──
            (Value::Option(opt), "unwrap") => {
                opt.as_ref().map(|v| *v.clone())
                    .ok_or_else(|| CrustError::Runtime("called unwrap() on None".into()))
            }
            (Value::Option(opt), "unwrap_or") => {
                let default = args.into_iter().next().unwrap_or(Value::Unit);
                Ok(opt.as_ref().map(|v| *v.clone()).unwrap_or(default))
            }
            (Value::Option(opt), "is_some") => Ok(Value::Bool(opt.is_some())),
            (Value::Option(opt), "is_none") => Ok(Value::Bool(opt.is_none())),

            // ── Result methods ──
            (Value::Result(res), "unwrap") => {
                match res {
                    Ok(v) => Ok(*v.clone()),
                    Err(e) => Err(CrustError::Runtime(format!("called unwrap() on Err({})", e))),
                }
            }
            (Value::Result(res), "is_ok") => Ok(Value::Bool(res.is_ok())),
            (Value::Result(res), "is_err") => Ok(Value::Bool(res.is_err())),

            // ── Generic methods ──
            (_, "to_string") => Ok(Value::Str(format!("{}", self))),
            (_, "clone") => Ok(self.clone()),

            _ => Err(CrustError::Runtime(format!("no method `{}` on type `{}`", name, self.type_name()))),
        }
    }

    /// Apply a function/closure value to arguments.
    fn apply_fn(
        &self,
        func: &Value,
        args: Vec<Value>,
        interp: &super::Interpreter,
        env: &Rc<RefCell<super::env::Environment>>,
    ) -> Result<Value, CrustError> {
        match func {
            Value::Function { params, body, closure_env, .. } => {
                let fn_env = Rc::new(RefCell::new(super::env::Environment::with_parent(closure_env.clone())));
                for (i, (name, _)) in params.iter().enumerate() {
                    let arg = args.get(i).cloned().unwrap_or(Value::Unit);
                    fn_env.borrow_mut().set(name.clone(), arg);
                }
                match interp.exec_block(body, &fn_env) {
                    Ok(val) => Ok(val),
                    Err(CrustError::Return(val)) => Ok(val),
                    Err(e) => Err(e),
                }
            }
            Value::BuiltinFn(f) => f(args),
            _ => Err(CrustError::Runtime("not a function".into())),
        }
    }
}
