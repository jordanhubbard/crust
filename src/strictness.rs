/// Crust strictness levels — how many of Rust's type-system and safety checks are active.
///
/// | Level | Name    | Description                                                    |
/// |-------|---------|----------------------------------------------------------------|
/// | 0     | Explore | No borrow checker, implicit Clone, auto-derive (default)       |
/// | 1     | Develop | Warnings on moves, type hints, shadow detection               |
/// | 2     | Harden  | Borrow checker active, explicit lifetimes required             |
/// | 3     | Ship    | Full rustc parity; output identical to `rustc`                 |
/// | 4     | Prove   | Formal verification mode with contracts, overflow checking,    |
/// |       |         | panic-freedom analysis, and Coq/Lean proof skeleton emission   |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum StrictnessLevel {
    #[default]
    Explore = 0,
    Develop = 1,
    Harden = 2,
    Ship = 3,
    Prove = 4,
}

impl StrictnessLevel {
    pub fn from_u8(n: u8) -> Self {
        match n {
            0 => Self::Explore,
            1 => Self::Develop,
            2 => Self::Harden,
            3 => Self::Ship,
            4 => Self::Prove,
            _ => Self::Explore,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Explore => "Explore",
            Self::Develop => "Develop",
            Self::Harden => "Harden",
            Self::Ship => "Ship",
            Self::Prove => "Prove",
        }
    }

    #[allow(dead_code)]
    pub fn description(self) -> &'static str {
        match self {
            Self::Explore => "no borrow checker, implicit Clone (default)",
            Self::Develop => "warnings on moves, type hints",
            Self::Harden => "borrow checker active, explicit lifetimes",
            Self::Ship => "full rustc parity",
            Self::Prove => "formal verification: contracts, overflow checks, panic-freedom proofs",
        }
    }
}

impl std::fmt::Display for StrictnessLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.as_u8(), self.name())
    }
}
