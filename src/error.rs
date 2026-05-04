use thiserror::Error;

/// Structured error type spanning Crust's pipeline stages.
///
/// `Analysis`/`TypeCheck`/`Verify` carry the count of underlying diagnostics
/// (which are streamed to stderr as they're produced) and a hint string so
/// the user-facing one-liner stays useful while machine-readable callers
/// (IDE integration, `crust verify` JSON, the eventual structured-report
/// surface in crust-ob9 / crust-wtp) can branch on the kind.
#[derive(Debug, Error)]
pub enum CrustError {
    #[error("parse error at line {line}: {msg}")]
    Parse { msg: String, line: usize },

    #[error("runtime error: {msg}")]
    Runtime { msg: String },

    #[error("{count} analysis error(s); {hint}")]
    Analysis { count: usize, hint: &'static str },

    #[error("{count} type error(s) at --strict=4")]
    TypeCheck { count: usize },

    #[error("rustc compilation failed")]
    Rustc,

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CrustError>;

impl CrustError {
    pub fn parse(msg: impl Into<String>, line: usize) -> Self {
        CrustError::Parse {
            msg: msg.into(),
            line,
        }
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        CrustError::Runtime { msg: msg.into() }
    }

    /// True if this is a "summary" error whose underlying details have
    /// already been streamed to stderr (used by main.rs to avoid printing
    /// the summary line again as `error: <summary>`).
    #[allow(dead_code)]
    pub fn is_summary(&self) -> bool {
        matches!(
            self,
            CrustError::Analysis { .. } | CrustError::TypeCheck { .. } | CrustError::Rustc
        )
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

    #[test]
    fn analysis_error_formats_with_count_and_hint() {
        let e = CrustError::Analysis {
            count: 3,
            hint: "fix or relax --strict",
        };
        assert_eq!(e.to_string(), "3 analysis error(s); fix or relax --strict");
        assert!(e.is_summary());
    }

    #[test]
    fn typecheck_error_formats() {
        let e = CrustError::TypeCheck { count: 2 };
        assert_eq!(e.to_string(), "2 type error(s) at --strict=4");
        assert!(e.is_summary());
    }

    #[test]
    fn rustc_error_formats() {
        let e = CrustError::Rustc;
        assert_eq!(e.to_string(), "rustc compilation failed");
        assert!(e.is_summary());
    }

    #[test]
    fn runtime_is_not_a_summary_error() {
        let e = CrustError::runtime("oops");
        assert!(!e.is_summary());
    }
}
