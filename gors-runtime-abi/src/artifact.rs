//! Target-specific runtime artifact selection and validation.

use std::fmt::{Display, Formatter};

use crate::contract::RuntimeAbiManifest;
use crate::encoding::CanonicalEncoder;
use crate::identity::{ArtifactIdentity, ContractIdentity, ImplementationHash};
use crate::operations::RuntimeOp;
use crate::target::{TargetCapability, TargetModel};

/// Current schema for target-specific runtime artifact identities.
pub const CURRENT_ARTIFACT_SCHEMA: ArtifactSchemaVersion = ArtifactSchemaVersion::new(1);

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

/// A contract requirement that the selected artifact target cannot provide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MissingCapability {
    operation: RuntimeOp,
    capability: TargetCapability,
}

impl MissingCapability {
    #[must_use]
    pub const fn operation(self) -> RuntimeOp {
        self.operation
    }

    #[must_use]
    pub const fn capability(self) -> TargetCapability {
        self.capability
    }
}

impl Display for MissingCapability {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "runtime operation {:?} requires missing target capability {:?}",
            self.operation, self.capability
        )
    }
}

impl std::error::Error for MissingCapability {}

/// Target-specific runtime implementation selected for one contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeArtifactManifest {
    schema: ArtifactSchemaVersion,
    contract: ContractIdentity,
    target: TargetModel,
    implementation: ImplementationHash,
}

impl RuntimeArtifactManifest {
    pub fn new(
        contract: &RuntimeAbiManifest,
        target: TargetModel,
        implementation: ImplementationHash,
    ) -> Result<Self, MissingCapability> {
        for operation in contract.runtime_ops() {
            for capability in operation.required_capabilities() {
                if !target.capabilities().contains(*capability) {
                    return Err(MissingCapability {
                        operation: *operation,
                        capability: *capability,
                    });
                }
            }
        }
        Ok(Self {
            schema: CURRENT_ARTIFACT_SCHEMA,
            contract: contract.identity(),
            target,
            implementation,
        })
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
    pub const fn implementation(&self) -> ImplementationHash {
        self.implementation
    }

    /// Encode exact artifact-selection facts using a separate stable domain.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::artifact();
        encoder.u32(self.schema.get());
        encoder.fixed_bytes(self.contract.as_bytes());
        self.target.encode(&mut encoder);
        encoder.fixed_bytes(self.implementation.as_bytes());
        encoder.finish()
    }

    /// Hash the exact contract, target, capability, and implementation tuple.
    #[must_use]
    pub fn identity(&self) -> ArtifactIdentity {
        ArtifactIdentity::sha256(&self.canonical_bytes())
    }
}
