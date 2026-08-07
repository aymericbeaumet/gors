//! Target-neutral compiler/runtime contract manifest.

use crate::encoding::CanonicalEncoder;
use crate::identity::ContractIdentity;
use crate::operations::{PrimitiveOp, RuntimeOp};

/// Schema used by the canonical contract and artifact encodings.
pub const CURRENT_MANIFEST_SCHEMA: ManifestSchemaVersion = ManifestSchemaVersion::new(2);

/// Current semantic compiler/runtime operation contract.
pub const CURRENT_CONTRACT_VERSION: ContractVersion = ContractVersion::new(2, 22, 0);

/// Version of the canonical manifest encoding itself.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ManifestSchemaVersion(u32);

impl ManifestSchemaVersion {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Semantic version of the compiler/runtime operation contract.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContractVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ContractVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    #[must_use]
    pub const fn patch(self) -> u16 {
        self.patch
    }
}

/// Fixed-width scalar dimension in a Go or Rust data model.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DataWidth {
    Bits32,
    Bits64,
}

impl DataWidth {
    #[must_use]
    pub const fn bits(self) -> u16 {
        match self {
            Self::Bits32 => 32,
            Self::Bits64 => 64,
        }
    }

    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::Bits32 => 1,
            Self::Bits64 => 2,
        }
    }
}

/// Target-neutral Go language data model shared by compiler and runtime.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GoSemanticModel {
    int_width: DataWidth,
}

impl GoSemanticModel {
    #[must_use]
    pub const fn new(int_width: DataWidth) -> Self {
        Self { int_width }
    }

    #[must_use]
    pub const fn int_width(self) -> DataWidth {
        self.int_width
    }

    fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u8(self.int_width.canonical_tag());
    }
}

/// Immutable target-neutral compiler/runtime operation contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAbiManifest {
    schema: ManifestSchemaVersion,
    contract: ContractVersion,
    semantics: GoSemanticModel,
    primitive_ops: Box<[PrimitiveOp]>,
    runtime_ops: Box<[RuntimeOp]>,
}

impl RuntimeAbiManifest {
    /// Construct the current target-neutral compiler/runtime contract.
    #[must_use]
    pub fn current() -> Self {
        Self::new(
            CURRENT_MANIFEST_SCHEMA,
            CURRENT_CONTRACT_VERSION,
            GoSemanticModel::new(DataWidth::Bits64),
            PrimitiveOp::ALL.iter().copied(),
            RuntimeOp::ALL.iter().copied(),
        )
    }

    #[must_use]
    pub fn new(
        schema: ManifestSchemaVersion,
        contract: ContractVersion,
        semantics: GoSemanticModel,
        primitive_ops: impl IntoIterator<Item = PrimitiveOp>,
        runtime_ops: impl IntoIterator<Item = RuntimeOp>,
    ) -> Self {
        Self {
            schema,
            contract,
            semantics,
            primitive_ops: sorted_unique_by_key(primitive_ops, |op| op.id()),
            runtime_ops: sorted_unique_by_key(runtime_ops, |op| op.id()),
        }
    }

    #[must_use]
    pub const fn schema(&self) -> ManifestSchemaVersion {
        self.schema
    }

    #[must_use]
    pub const fn contract(&self) -> ContractVersion {
        self.contract
    }

    #[must_use]
    pub const fn semantics(&self) -> GoSemanticModel {
        self.semantics
    }

    #[must_use]
    pub const fn primitive_ops(&self) -> &[PrimitiveOp] {
        &self.primitive_ops
    }

    #[must_use]
    pub const fn runtime_ops(&self) -> &[RuntimeOp] {
        &self.runtime_ops
    }

    /// Encode all target-neutral contract facts using a stable binary format.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::contract();
        encoder.u32(self.schema.get());
        encoder.u16(self.contract.major());
        encoder.u16(self.contract.minor());
        encoder.u16(self.contract.patch());
        self.semantics.encode(&mut encoder);
        encoder.count(self.primitive_ops.len());
        for op in &self.primitive_ops {
            op.encode(&mut encoder);
        }
        encoder.count(self.runtime_ops.len());
        for op in &self.runtime_ops {
            op.encode(&mut encoder);
        }
        encoder.finish()
    }

    /// Hash the canonical target-neutral contract for semantic cache keys.
    #[must_use]
    pub fn identity(&self) -> ContractIdentity {
        ContractIdentity::sha256(&self.canonical_bytes())
    }
}

fn sorted_unique_by_key<T, K>(
    values: impl IntoIterator<Item = T>,
    mut key: impl FnMut(&T) -> K,
) -> Box<[T]>
where
    K: Ord,
{
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_unstable_by_key(&mut key);
    values.dedup_by(|left, right| key(left) == key(right));
    values.into_boxed_slice()
}
