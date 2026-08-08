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

/// Integer operations implemented by the versioned runtime ABI.
///
/// Division and remainder carry their exact operand kind. Shift operations
/// additionally encode the signedness of the independently typed shift count;
/// the left operand kind remains the operation's concrete [`IntegerKind`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IntegerRuntimeOp {
    Div,
    Rem,
    ShlSigned,
    ShrSigned,
    ShlUnsigned,
    ShrUnsigned,
}

impl IntegerRuntimeOp {
    pub const ALL: &'static [Self] = &[
        Self::Div,
        Self::Rem,
        Self::ShlSigned,
        Self::ShrSigned,
        Self::ShlUnsigned,
        Self::ShrUnsigned,
    ];

    #[must_use]
    pub const fn is_shift(self) -> bool {
        matches!(
            self,
            Self::ShlSigned | Self::ShrSigned | Self::ShlUnsigned | Self::ShrUnsigned
        )
    }

    #[must_use]
    pub const fn count_constraint(self) -> IntegerKindConstraint {
        match self {
            Self::ShlSigned | Self::ShrSigned => IntegerKindConstraint::Signed,
            Self::ShlUnsigned | Self::ShrUnsigned => IntegerKindConstraint::Unsigned,
            Self::Div | Self::Rem => IntegerKindConstraint::Any,
        }
    }

    pub(super) const fn runtime_id(self, kind: IntegerKind) -> u16 {
        match (self, kind) {
            (Self::Div, IntegerKind::I64) => 8,
            (Self::Rem, IntegerKind::I64) => 9,
            (Self::ShlSigned, IntegerKind::I64) => 10,
            (Self::ShrSigned, IntegerKind::I64) => 11,
            (Self::Div, IntegerKind::I8) => 172,
            (Self::Div, IntegerKind::I16) => 173,
            (Self::Div, IntegerKind::I32) => 174,
            (Self::Div, IntegerKind::U8) => 175,
            (Self::Div, IntegerKind::U16) => 176,
            (Self::Div, IntegerKind::U32) => 177,
            (Self::Div, IntegerKind::U64) => 178,
            (Self::Rem, IntegerKind::I8) => 179,
            (Self::Rem, IntegerKind::I16) => 180,
            (Self::Rem, IntegerKind::I32) => 181,
            (Self::Rem, IntegerKind::U8) => 182,
            (Self::Rem, IntegerKind::U16) => 183,
            (Self::Rem, IntegerKind::U32) => 184,
            (Self::Rem, IntegerKind::U64) => 185,
            (Self::ShlSigned, IntegerKind::I8) => 186,
            (Self::ShlSigned, IntegerKind::I16) => 187,
            (Self::ShlSigned, IntegerKind::I32) => 188,
            (Self::ShlSigned, IntegerKind::U8) => 189,
            (Self::ShlSigned, IntegerKind::U16) => 190,
            (Self::ShlSigned, IntegerKind::U32) => 191,
            (Self::ShlSigned, IntegerKind::U64) => 192,
            (Self::ShrSigned, IntegerKind::I8) => 193,
            (Self::ShrSigned, IntegerKind::I16) => 194,
            (Self::ShrSigned, IntegerKind::I32) => 195,
            (Self::ShrSigned, IntegerKind::U8) => 196,
            (Self::ShrSigned, IntegerKind::U16) => 197,
            (Self::ShrSigned, IntegerKind::U32) => 198,
            (Self::ShrSigned, IntegerKind::U64) => 199,
            (Self::ShlUnsigned, kind) => 200 + kind.ordinal(),
            (Self::ShrUnsigned, kind) => 208 + kind.ordinal(),
        }
    }

    pub(super) fn from_appended_id(id: u16) -> Option<(Self, IntegerKind)> {
        let (op, ordinal, omits_i64) = match id {
            172..=178 => (Self::Div, id - 172, true),
            179..=185 => (Self::Rem, id - 179, true),
            186..=192 => (Self::ShlSigned, id - 186, true),
            193..=199 => (Self::ShrSigned, id - 193, true),
            200..=207 => (Self::ShlUnsigned, id - 200, false),
            208..=215 => (Self::ShrUnsigned, id - 208, false),
            _ => return None,
        };
        let kind = if omits_i64 {
            match ordinal {
                0 => IntegerKind::I8,
                1 => IntegerKind::I16,
                2 => IntegerKind::I32,
                3 => IntegerKind::U8,
                4 => IntegerKind::U16,
                5 => IntegerKind::U32,
                6 => IntegerKind::U64,
                _ => return None,
            }
        } else {
            match ordinal {
                0 => IntegerKind::I8,
                1 => IntegerKind::I16,
                2 => IntegerKind::I32,
                3 => IntegerKind::I64,
                4 => IntegerKind::U8,
                5 => IntegerKind::U16,
                6 => IntegerKind::U32,
                7 => IntegerKind::U64,
                _ => return None,
            }
        };
        Some((op, kind))
    }

    pub(super) const fn symbol(self, kind: IntegerKind) -> &'static str {
        match (self, kind) {
            (Self::Div, IntegerKind::I8) => "int_div_i8",
            (Self::Div, IntegerKind::I16) => "int_div_i16",
            (Self::Div, IntegerKind::I32) => "int_div_i32",
            (Self::Div, IntegerKind::I64) => "int_div",
            (Self::Div, IntegerKind::U8) => "int_div_u8",
            (Self::Div, IntegerKind::U16) => "int_div_u16",
            (Self::Div, IntegerKind::U32) => "int_div_u32",
            (Self::Div, IntegerKind::U64) => "int_div_u64",
            (Self::Rem, IntegerKind::I8) => "int_rem_i8",
            (Self::Rem, IntegerKind::I16) => "int_rem_i16",
            (Self::Rem, IntegerKind::I32) => "int_rem_i32",
            (Self::Rem, IntegerKind::I64) => "int_rem",
            (Self::Rem, IntegerKind::U8) => "int_rem_u8",
            (Self::Rem, IntegerKind::U16) => "int_rem_u16",
            (Self::Rem, IntegerKind::U32) => "int_rem_u32",
            (Self::Rem, IntegerKind::U64) => "int_rem_u64",
            (Self::ShlSigned, IntegerKind::I8) => "int_shl_signed_i8",
            (Self::ShlSigned, IntegerKind::I16) => "int_shl_signed_i16",
            (Self::ShlSigned, IntegerKind::I32) => "int_shl_signed_i32",
            (Self::ShlSigned, IntegerKind::I64) => "int_shl",
            (Self::ShlSigned, IntegerKind::U8) => "int_shl_signed_u8",
            (Self::ShlSigned, IntegerKind::U16) => "int_shl_signed_u16",
            (Self::ShlSigned, IntegerKind::U32) => "int_shl_signed_u32",
            (Self::ShlSigned, IntegerKind::U64) => "int_shl_signed_u64",
            (Self::ShrSigned, IntegerKind::I8) => "int_shr_signed_i8",
            (Self::ShrSigned, IntegerKind::I16) => "int_shr_signed_i16",
            (Self::ShrSigned, IntegerKind::I32) => "int_shr_signed_i32",
            (Self::ShrSigned, IntegerKind::I64) => "int_shr",
            (Self::ShrSigned, IntegerKind::U8) => "int_shr_signed_u8",
            (Self::ShrSigned, IntegerKind::U16) => "int_shr_signed_u16",
            (Self::ShrSigned, IntegerKind::U32) => "int_shr_signed_u32",
            (Self::ShrSigned, IntegerKind::U64) => "int_shr_signed_u64",
            (Self::ShlUnsigned, IntegerKind::I8) => "int_shl_unsigned_i8",
            (Self::ShlUnsigned, IntegerKind::I16) => "int_shl_unsigned_i16",
            (Self::ShlUnsigned, IntegerKind::I32) => "int_shl_unsigned_i32",
            (Self::ShlUnsigned, IntegerKind::I64) => "int_shl_unsigned_i64",
            (Self::ShlUnsigned, IntegerKind::U8) => "int_shl_unsigned_u8",
            (Self::ShlUnsigned, IntegerKind::U16) => "int_shl_unsigned_u16",
            (Self::ShlUnsigned, IntegerKind::U32) => "int_shl_unsigned_u32",
            (Self::ShlUnsigned, IntegerKind::U64) => "int_shl_unsigned_u64",
            (Self::ShrUnsigned, IntegerKind::I8) => "int_shr_unsigned_i8",
            (Self::ShrUnsigned, IntegerKind::I16) => "int_shr_unsigned_i16",
            (Self::ShrUnsigned, IntegerKind::I32) => "int_shr_unsigned_i32",
            (Self::ShrUnsigned, IntegerKind::I64) => "int_shr_unsigned_i64",
            (Self::ShrUnsigned, IntegerKind::U8) => "int_shr_unsigned_u8",
            (Self::ShrUnsigned, IntegerKind::U16) => "int_shr_unsigned_u16",
            (Self::ShrUnsigned, IntegerKind::U32) => "int_shr_unsigned_u32",
            (Self::ShrUnsigned, IntegerKind::U64) => "int_shr_unsigned_u64",
        }
    }
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
