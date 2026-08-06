//! Stable decoding for persisted runtime operation identifiers.

use super::{RuntimeOp, UnknownRuntimeOpId};

impl TryFrom<u16> for RuntimeOp {
    type Error = UnknownRuntimeOpId;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::GoStringFromBytes),
            2 => Ok(Self::GoStringFromStatic),
            3 => Ok(Self::ConcatGoStrings),
            8 => Ok(Self::IntDiv),
            9 => Ok(Self::IntRem),
            10 => Ok(Self::IntShl),
            11 => Ok(Self::IntShr),
            13 => Ok(Self::PrintBool),
            14 => Ok(Self::PrintI64),
            15 => Ok(Self::PrintSpace),
            16 => Ok(Self::PrintNewline),
            17 => Ok(Self::PrintGoString),
            18 => Ok(Self::PanicBool),
            19 => Ok(Self::PanicI64),
            20 => Ok(Self::PanicGoString),
            21 => Ok(Self::GoSliceI64FromStatic),
            22 => Ok(Self::GoSliceI64Index),
            23 => Ok(Self::GoSliceI64Range),
            24 => Ok(Self::GoSliceI64Set),
            25 => Ok(Self::GoSliceI64Make),
            26 => Ok(Self::GoSliceI64Len),
            27 => Ok(Self::GoSliceI64Cap),
            28 => Ok(Self::GoSliceI64Append),
            29 => Ok(Self::GoSliceU8FromStatic),
            30 => Ok(Self::GoSliceU8AppendSlice),
            31 => Ok(Self::GoSliceU8AppendString),
            32 => Ok(Self::GoSliceU8CopyString),
            33 => Ok(Self::GoSliceI64Clear),
            34 => Ok(Self::GoStringFromSliceU8),
            35 => Ok(Self::GoSliceI64Copy),
            36 => Ok(Self::GoMapStringI64Nil),
            37 => Ok(Self::GoMapStringI64Make),
            38 => Ok(Self::GoMapStringI64Len),
            39 => Ok(Self::GoMapStringI64Get),
            40 => Ok(Self::GoMapStringI64Contains),
            41 => Ok(Self::GoMapStringI64Set),
            42 => Ok(Self::GoMapStringI64Delete),
            43 => Ok(Self::GoMapStringI64Clear),
            44 => Ok(Self::GoMapStringI64IsNil),
            45 => Ok(Self::GoMapStringI64KeyAt),
            46 => Ok(Self::GoPointerI64Nil),
            47 => Ok(Self::GoPointerI64New),
            48 => Ok(Self::GoPointerI64Get),
            49 => Ok(Self::GoPointerI64Set),
            50 => Ok(Self::GoPointerI64IsNil),
            unknown => Err(UnknownRuntimeOpId(unknown)),
        }
    }
}
