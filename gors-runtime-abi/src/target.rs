//! Target-specific runtime representation and host capability facts.

use std::fmt::{Display, Formatter};

use crate::contract::DataWidth;
use crate::encoding::CanonicalEncoder;

/// Byte order selected for one compiled runtime artifact.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Endianness {
    Little,
    Big,
}

impl Endianness {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Little => 1,
            Self::Big => 2,
        }
    }
}

/// Host facility required by an operation or provided by an artifact target.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TargetCapability {
    Threads,
    Atomics32,
    Atomics64,
    StandardIo,
    FileSystem,
    Environment,
    Process,
    MonotonicClock,
    WallClock,
    Entropy,
    Network,
}

impl TargetCapability {
    pub(crate) const fn canonical_tag(self) -> u16 {
        match self {
            Self::Threads => 1,
            Self::Atomics32 => 2,
            Self::Atomics64 => 3,
            Self::StandardIo => 4,
            Self::FileSystem => 5,
            Self::Environment => 6,
            Self::Process => 7,
            Self::MonotonicClock => 8,
            Self::WallClock => 9,
            Self::Entropy => 10,
            Self::Network => 11,
        }
    }
}

/// Sorted, duplicate-free target capability set.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TargetCapabilities {
    values: Box<[TargetCapability]>,
}

impl TargetCapabilities {
    #[must_use]
    pub fn new(values: impl IntoIterator<Item = TargetCapability>) -> Self {
        let mut values: Vec<_> = values.into_iter().collect();
        values.sort_unstable_by_key(|value| value.canonical_tag());
        values.dedup_by_key(|value| value.canonical_tag());
        Self {
            values: values.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn contains(&self, capability: TargetCapability) -> bool {
        self.values
            .binary_search_by_key(&capability.canonical_tag(), |value| value.canonical_tag())
            .is_ok()
    }

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = TargetCapability> + ExactSizeIterator + '_ {
        self.values.iter().copied()
    }

    #[must_use]
    pub const fn as_slice(&self) -> &[TargetCapability] {
        &self.values
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.count(self.values.len());
        for capability in &self.values {
            encoder.u16(capability.canonical_tag());
        }
    }
}

/// Invalid target description rejected before it can become an artifact key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetModelError {
    EmptyTriple,
    InvalidTripleCharacter,
}

impl Display for TargetModelError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTriple => formatter.write_str("target triple must not be empty"),
            Self::InvalidTripleCharacter => {
                formatter.write_str("target triple must contain only printable ASCII characters")
            }
        }
    }
}

impl std::error::Error for TargetModelError {}

/// Target-specific representation and host facts for one runtime artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TargetModel {
    triple: Box<str>,
    pointer_width: DataWidth,
    endianness: Endianness,
}

impl TargetModel {
    pub fn new(
        triple: impl Into<Box<str>>,
        pointer_width: DataWidth,
        endianness: Endianness,
    ) -> Result<Self, TargetModelError> {
        let triple = triple.into();
        if triple.is_empty() {
            return Err(TargetModelError::EmptyTriple);
        }
        if !triple.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(TargetModelError::InvalidTripleCharacter);
        }
        Ok(Self {
            triple,
            pointer_width,
            endianness,
        })
    }

    #[must_use]
    pub fn triple(&self) -> &str {
        &self.triple
    }

    #[must_use]
    pub const fn pointer_width(&self) -> DataWidth {
        self.pointer_width
    }

    #[must_use]
    pub const fn endianness(&self) -> Endianness {
        self.endianness
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.text(&self.triple);
        encoder.u8(self.pointer_width.canonical_tag());
        encoder.u8(self.endianness.canonical_tag());
    }
}
