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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_u8_round_trips_for_all_levels() {
        for n in 0..=4u8 {
            let level = StrictnessLevel::from_u8(n);
            assert_eq!(level.as_u8(), n);
        }
    }

    #[test]
    fn from_u8_above_4_falls_back_to_explore() {
        assert_eq!(StrictnessLevel::from_u8(99), StrictnessLevel::Explore);
    }

    #[test]
    fn names_match_dial_labels() {
        assert_eq!(StrictnessLevel::Explore.name(), "Explore");
        assert_eq!(StrictnessLevel::Develop.name(), "Develop");
        assert_eq!(StrictnessLevel::Harden.name(), "Harden");
        assert_eq!(StrictnessLevel::Ship.name(), "Ship");
        assert_eq!(StrictnessLevel::Prove.name(), "Prove");
    }

    #[test]
    fn display_formats_number_and_name() {
        assert_eq!(StrictnessLevel::Explore.to_string(), "0 (Explore)");
        assert_eq!(StrictnessLevel::Prove.to_string(), "4 (Prove)");
    }

    #[test]
    fn ordering_is_monotonic() {
        assert!(StrictnessLevel::Explore < StrictnessLevel::Develop);
        assert!(StrictnessLevel::Develop < StrictnessLevel::Harden);
        assert!(StrictnessLevel::Harden < StrictnessLevel::Ship);
        assert!(StrictnessLevel::Ship < StrictnessLevel::Prove);
    }

    #[test]
    fn description_is_non_empty_for_every_level() {
        for n in 0..=4u8 {
            assert!(!StrictnessLevel::from_u8(n).description().is_empty());
        }
    }

    #[test]
    fn default_is_explore() {
        assert_eq!(StrictnessLevel::default(), StrictnessLevel::Explore);
    }
}
