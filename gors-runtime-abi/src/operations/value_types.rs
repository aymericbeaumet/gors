//! Canonical encoding and accessors for runtime value and operation types.

use std::fmt::{Display, Formatter};

use super::{RuntimeOpId, RuntimeSignature, RuntimeType, UnknownRuntimeOpId};
use crate::encoding::CanonicalEncoder;

impl RuntimeType {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Unit => 1,
            Self::Bool => 2,
            Self::I64 => 3,
            Self::GoString => 4,
            Self::ByteSlice => 5,
            Self::StaticByteSlice => 6,
            Self::F64 => 7,
            Self::Complex128 => 8,
            Self::GoSliceI64 => 9,
            Self::StaticI64Slice => 10,
            Self::GoSliceU8 => 11,
            Self::GoMapStringI64 => 12,
            Self::GoPointerI64 => 13,
            Self::GoChannelI64 => 14,
            Self::I64BoolTuple => 15,
            Self::I64I64Tuple => 16,
            Self::GoPointerStructI64 => 17,
            Self::GoInterface => 18,
        }
    }

    pub(super) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u8(self.canonical_tag());
    }
}

impl RuntimeSignature {
    pub(super) const fn new(parameters: &'static [RuntimeType], result: RuntimeType) -> Self {
        Self { parameters, result }
    }

    #[must_use]
    pub const fn parameters(self) -> &'static [RuntimeType] {
        self.parameters
    }

    #[must_use]
    pub const fn result(self) -> RuntimeType {
        self.result
    }

    pub(super) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.count(self.parameters.len());
        for parameter in self.parameters {
            parameter.encode(encoder);
        }
        self.result.encode(encoder);
    }
}

impl RuntimeOpId {
    /// Canonical numeric value used by fingerprints and manifest encodings.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl UnknownRuntimeOpId {
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Display for UnknownRuntimeOpId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "unknown runtime operation ID {}", self.0)
    }
}

impl std::error::Error for UnknownRuntimeOpId {}
