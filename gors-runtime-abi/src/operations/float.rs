//! Exact floating-point widths and directly emitted numeric operations.

/// Exact Go floating-point width represented by one canonical `f64` carrier
/// in generated Rust.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FloatKind {
    F32,
    F64,
}

/// Floating-point operations emitted directly by the compiler.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FloatPrimitive {
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Min,
    Max,
}

impl FloatKind {
    pub const ALL: &'static [Self] = &[Self::F32, Self::F64];

    /// Source-language width of this floating-point kind.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::F32 => 32,
            Self::F64 => 64,
        }
    }

    /// Stable semantic spelling used by operation names.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::F32 => "float32",
            Self::F64 => "float64",
        }
    }

    pub(super) const fn ordinal(self) -> u16 {
        match self {
            Self::F32 => 0,
            Self::F64 => 1,
        }
    }
}

impl FloatPrimitive {
    pub const ALL: &'static [Self] = &[
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Neg,
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::Min,
        Self::Max,
    ];

    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::Neg => 1,
            _ => 2,
        }
    }

    #[must_use]
    pub const fn returns_bool(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
        )
    }

    pub(super) const fn ordinal(self) -> u16 {
        match self {
            Self::Add => 0,
            Self::Sub => 1,
            Self::Mul => 2,
            Self::Div => 3,
            Self::Neg => 4,
            Self::Equal => 5,
            Self::NotEqual => 6,
            Self::Less => 7,
            Self::LessEqual => 8,
            Self::Greater => 9,
            Self::GreaterEqual => 10,
            Self::Min => 11,
            Self::Max => 12,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Neg => "neg",
            Self::Equal => "equal",
            Self::NotEqual => "not-equal",
            Self::Less => "less",
            Self::LessEqual => "less-equal",
            Self::Greater => "greater",
            Self::GreaterEqual => "greater-equal",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}
