use super::value::Value;
use miette::Diagnostic;

/// All errors in the Crust interpreter.
/// Return, Break, Continue are control flow signals, not real errors.
#[derive(Debug, Diagnostic)]
pub enum CrustError {
    /// Parse error (from syn)
    Parse(String),

    /// Type mismatch
    Type(String),

    /// Runtime error (index out of bounds, division by zero, etc.)
    Runtime(String),

    /// Control flow: return from function
    Return(Value),

    /// Control flow: break from loop
    Break(Value),

    /// Control flow: continue in loop
    Continue,
}

impl std::fmt::Display for CrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrustError::Parse(msg) => write!(f, "parse error: {}", msg),
            CrustError::Type(msg) => write!(f, "type error: {}", msg),
            CrustError::Runtime(msg) => write!(f, "runtime error: {}", msg),
            CrustError::Return(_) => write!(f, "return outside function"),
            CrustError::Break(_) => write!(f, "break outside loop"),
            CrustError::Continue => write!(f, "continue outside loop"),
        }
    }
}

impl std::error::Error for CrustError {}
