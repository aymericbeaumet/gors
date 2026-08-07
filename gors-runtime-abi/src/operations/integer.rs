use crate::encoding::CanonicalEncoder;

/// Exact Go integer width and signedness represented by one canonical `i64`
/// carrier in generated Rust.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegerKind {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

/// Accepted semantic Go integer kinds at an `i64` runtime ABI carrier slot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegerKindConstraint {
    Exact(IntegerKind),
    I64OrI32,
    Any,
    Signed,
    Unsigned,
}

impl IntegerKindConstraint {
    #[must_use]
    pub const fn accepts(self, kind: IntegerKind) -> bool {
        match self {
            Self::Exact(expected) => kind as u8 == expected as u8,
            Self::I64OrI32 => matches!(kind, IntegerKind::I64 | IntegerKind::I32),
            Self::Any => true,
            Self::Signed => kind.is_signed(),
            Self::Unsigned => !kind.is_signed(),
        }
    }

    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        match self {
            Self::Exact(kind) => {
                encoder.u8(0);
                encoder.u8(kind.ordinal() as u8);
            }
            Self::I64OrI32 => encoder.u8(1),
            Self::Any => encoder.u8(2),
            Self::Signed => encoder.u8(3),
            Self::Unsigned => encoder.u8(4),
        }
    }
}

impl IntegerKind {
    pub const ALL: &'static [Self] = &[
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
    ];

    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 => 64,
        }
    }

    #[must_use]
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    /// Canonicalize the shared `i64` carrier to this Go integer kind.
    #[must_use]
    pub const fn canonicalize_carrier(self, value: i64) -> i64 {
        match self {
            Self::I8 => (value as i8) as i64,
            Self::I16 => (value as i16) as i64,
            Self::I32 => (value as i32) as i64,
            Self::I64 | Self::U64 => value,
            Self::U8 => (value as u8) as i64,
            Self::U16 => (value as u16) as i64,
            Self::U32 => (value as u32) as i64,
        }
    }

    /// Whether an `i64` carrier is canonical for this Go integer kind.
    #[must_use]
    pub const fn is_canonical_carrier(self, value: i64) -> bool {
        self.canonicalize_carrier(value) == value
    }

    pub(super) const fn ordinal(self) -> u16 {
        match self {
            Self::I8 => 0,
            Self::I16 => 1,
            Self::I32 => 2,
            Self::I64 => 3,
            Self::U8 => 4,
            Self::U16 => 5,
            Self::U32 => 6,
            Self::U64 => 7,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }
}

/// Integer primitive family selected after Go MIR verification.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegerPrimitive {
    BitNot,
    BitAnd,
    BitOr,
    BitXor,
    AndNot,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    WrappingNeg,
    Min,
    Max,
}

impl IntegerPrimitive {
    pub const ALL: &'static [Self] = &[
        Self::BitNot,
        Self::BitAnd,
        Self::BitOr,
        Self::BitXor,
        Self::AndNot,
        Self::Equal,
        Self::NotEqual,
        Self::Less,
        Self::LessEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::WrappingAdd,
        Self::WrappingSub,
        Self::WrappingMul,
        Self::WrappingNeg,
        Self::Min,
        Self::Max,
    ];

    #[must_use]
    pub const fn arity(self) -> usize {
        match self {
            Self::BitNot | Self::WrappingNeg => 1,
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
            Self::BitNot => 0,
            Self::BitAnd => 1,
            Self::BitOr => 2,
            Self::BitXor => 3,
            Self::AndNot => 4,
            Self::Equal => 5,
            Self::NotEqual => 6,
            Self::Less => 7,
            Self::LessEqual => 8,
            Self::Greater => 9,
            Self::GreaterEqual => 10,
            Self::WrappingAdd => 11,
            Self::WrappingSub => 12,
            Self::WrappingMul => 13,
            Self::WrappingNeg => 14,
            Self::Min => 15,
            Self::Max => 16,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::BitNot => "bit-not",
            Self::BitAnd => "bit-and",
            Self::BitOr => "bit-or",
            Self::BitXor => "bit-xor",
            Self::AndNot => "and-not",
            Self::Equal => "equal",
            Self::NotEqual => "not-equal",
            Self::Less => "less",
            Self::LessEqual => "less-equal",
            Self::Greater => "greater",
            Self::GreaterEqual => "greater-equal",
            Self::WrappingAdd => "wrapping-add",
            Self::WrappingSub => "wrapping-sub",
            Self::WrappingMul => "wrapping-mul",
            Self::WrappingNeg => "wrapping-neg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}
