//! Target-specific runtime artifact provider manifests.

use crate::encoding::CanonicalEncoder;
use crate::identity::{
    ArtifactIdentity, CompatibilityIdentity, ContractIdentity, ImplementationHash,
};
use crate::target::{TargetCapabilities, TargetModel};

/// Current schema for target-specific runtime artifact identities.
///
/// Schema 2 separates target facts from capabilities actually provided by an
/// artifact and records the exact Rust link format and compatibility identity.
pub const CURRENT_ARTIFACT_SCHEMA: ArtifactSchemaVersion = ArtifactSchemaVersion::new(2);

/// Version of the canonical target-specific artifact encoding.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactSchemaVersion(u32);

impl ArtifactSchemaVersion {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Link format of a precompiled runtime provider.
///
/// Generated Rust currently shares runtime-defined Rust value types, so the
/// only supported provider is an rlib built by an exactly compatible rustc.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeArtifactFormat {
    RustRlibV1,
}

impl RuntimeArtifactFormat {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::RustRlibV1 => 1,
        }
    }
}

/// Target-specific runtime implementation reusable by compatible programs.
///
/// The provider records capabilities it can actually supply. It is not
/// rejected merely because another operation in the contract needs a missing
/// capability; compatibility is checked later against one program's selected
/// operation requirement.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeArtifactManifest {
    schema: ArtifactSchemaVersion,
    contract: ContractIdentity,
    target: TargetModel,
    provided_capabilities: TargetCapabilities,
    format: RuntimeArtifactFormat,
    compatibility: CompatibilityIdentity,
    implementation: ImplementationHash,
}

impl RuntimeArtifactManifest {
    /// Construct a provider using the current artifact schema.
    #[must_use]
    pub fn new(
        contract: ContractIdentity,
        target: TargetModel,
        provided_capabilities: TargetCapabilities,
        format: RuntimeArtifactFormat,
        compatibility: CompatibilityIdentity,
        implementation: ImplementationHash,
    ) -> Self {
        Self::from_parts(
            CURRENT_ARTIFACT_SCHEMA,
            contract,
            target,
            provided_capabilities,
            format,
            compatibility,
            implementation,
        )
    }

    /// Construct a decoded provider before compatibility validation.
    ///
    /// Callers admitting a sidecar manifest must pass the result through
    /// [`Self::select`]. That operation rejects every schema except the current
    /// one. Keeping the schema explicit also makes stale-cache tests possible
    /// without unsafe mutation or a legacy decoding path.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        schema: ArtifactSchemaVersion,
        contract: ContractIdentity,
        target: TargetModel,
        provided_capabilities: TargetCapabilities,
        format: RuntimeArtifactFormat,
        compatibility: CompatibilityIdentity,
        implementation: ImplementationHash,
    ) -> Self {
        Self {
            schema,
            contract,
            target,
            provided_capabilities,
            format,
            compatibility,
            implementation,
        }
    }

    #[must_use]
    pub const fn schema(&self) -> ArtifactSchemaVersion {
        self.schema
    }

    #[must_use]
    pub const fn contract(&self) -> ContractIdentity {
        self.contract
    }

    #[must_use]
    pub const fn target(&self) -> &TargetModel {
        &self.target
    }

    #[must_use]
    pub const fn provided_capabilities(&self) -> &TargetCapabilities {
        &self.provided_capabilities
    }

    #[must_use]
    pub const fn format(&self) -> RuntimeArtifactFormat {
        self.format
    }

    #[must_use]
    pub const fn compatibility(&self) -> CompatibilityIdentity {
        self.compatibility
    }

    #[must_use]
    pub const fn implementation(&self) -> ImplementationHash {
        self.implementation
    }

    /// Whether the provider payload still has its recorded exact content.
    #[must_use]
    pub fn verifies_payload(&self, bytes: &[u8]) -> bool {
        self.implementation.matches(bytes)
    }

    /// Encode exact artifact-selection facts using a separate stable domain.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::artifact();
        encoder.u32(self.schema.get());
        encoder.fixed_bytes(self.contract.as_bytes());
        self.target.encode(&mut encoder);
        self.provided_capabilities.encode(&mut encoder);
        encoder.u8(self.format.canonical_tag());
        encoder.fixed_bytes(self.compatibility.as_bytes());
        encoder.fixed_bytes(self.implementation.as_bytes());
        encoder.finish()
    }

    /// Hash the exact provider contract, compatibility, and payload facts.
    #[must_use]
    pub fn identity(&self) -> ArtifactIdentity {
        ArtifactIdentity::sha256(&self.canonical_bytes())
    }
}
