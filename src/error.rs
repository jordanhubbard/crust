use thiserror::Error;

#[derive(Debug, Error)]
pub enum CrustError {
    #[error("parse error at line {line}: {msg}")]
    Parse { msg: String, line: usize },

    #[error("runtime error: {msg}")]
    Runtime { msg: String },

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CrustError>;

impl CrustError {
    pub fn parse(msg: impl Into<String>, line: usize) -> Self {
        CrustError::Parse { msg: msg.into(), line }
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        CrustError::Runtime { msg: msg.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_formats() {
        let e = CrustError::parse("unexpected token", 5);
        assert_eq!(e.to_string(), "parse error at line 5: unexpected token");
    }

    #[test]
    fn runtime_error_formats() {
        let e = CrustError::runtime("undefined variable: x");
        assert_eq!(e.to_string(), "runtime error: undefined variable: x");
    }
}
