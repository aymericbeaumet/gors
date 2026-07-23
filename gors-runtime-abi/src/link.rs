//! Runtime consumer requirements and validated provider link plans.

use std::fmt::{Display, Formatter};

use crate::artifact::{
    ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA, RuntimeArtifactFormat, RuntimeArtifactManifest,
};
use crate::contract::RuntimeAbiManifest;
use crate::encoding::CanonicalEncoder;
use crate::identity::{
    ArtifactIdentity, CompatibilityIdentity, ContractIdentity, ImplementationHash, LinkPlanIdentity,
};
use crate::operations::RuntimeOp;
use crate::requirement::RuntimeRequirement;
use crate::target::{TargetCapability, TargetModel};

/// Current canonical encoding schema for validated runtime link plans.
pub const CURRENT_LINK_PLAN_SCHEMA: u32 = 1;

/// Current wire schema for a target-neutral compiled-program dependency.
///
/// Consumers must reject any other schema rather than guessing how stable
/// operation IDs from another protocol should be interpreted.
pub const CURRENT_RUNTIME_DEPENDENCY_SCHEMA: u32 = 1;

/// A selected operation that is absent from the named runtime contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequirementContractError {
    operation: RuntimeOp,
}

impl RequirementContractError {
    #[must_use]
    pub const fn operation(self) -> RuntimeOp {
        self.operation
    }
}

impl Display for RequirementContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "runtime operation {:?} is absent from the selected runtime contract",
            self.operation
        )
    }
}

impl std::error::Error for RequirementContractError {}

/// Unconditional runtime dependency of one compiled program.
///
/// This is deliberately not optional. An empty operation set still requires a
/// runtime provider because runtime-backed value representations such as
/// `GoString` are not yet separately represented as sliceable requirements.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeDependency {
    contract: ContractIdentity,
    requirement: RuntimeRequirement,
}

impl RuntimeDependency {
    /// Validate selected operations and bind them to an exact contract.
    pub fn new(
        contract: &RuntimeAbiManifest,
        requirement: RuntimeRequirement,
    ) -> Result<Self, RequirementContractError> {
        for operation in requirement.iter() {
            if !contract.runtime_ops().contains(&operation) {
                return Err(RequirementContractError { operation });
            }
        }
        Ok(Self {
            contract: contract.identity(),
            requirement,
        })
    }

    #[must_use]
    pub const fn contract(&self) -> ContractIdentity {
        self.contract
    }

    #[must_use]
    pub const fn requirement(&self) -> &RuntimeRequirement {
        &self.requirement
    }

    fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.fixed_bytes(self.contract.as_bytes());
        self.requirement.encode(encoder);
    }
}

/// Exact consumer compatibility request presented to an artifact provider.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeLinkRequest {
    dependency: RuntimeDependency,
    target: TargetModel,
    format: RuntimeArtifactFormat,
    compatibility: CompatibilityIdentity,
}

impl RuntimeLinkRequest {
    #[must_use]
    pub const fn new(
        dependency: RuntimeDependency,
        target: TargetModel,
        format: RuntimeArtifactFormat,
        compatibility: CompatibilityIdentity,
    ) -> Self {
        Self {
            dependency,
            target,
            format,
            compatibility,
        }
    }

    #[must_use]
    pub const fn dependency(&self) -> &RuntimeDependency {
        &self.dependency
    }

    #[must_use]
    pub const fn target(&self) -> &TargetModel {
        &self.target
    }

    #[must_use]
    pub const fn format(&self) -> RuntimeArtifactFormat {
        self.format
    }

    #[must_use]
    pub const fn compatibility(&self) -> CompatibilityIdentity {
        self.compatibility
    }

    fn encode(&self, encoder: &mut CanonicalEncoder) {
        self.dependency.encode(encoder);
        self.target.encode(encoder);
        encoder.u8(self.format.canonical_tag());
        encoder.fixed_bytes(self.compatibility.as_bytes());
    }
}

/// Deterministic incompatibility between one consumer and one provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeLinkError {
    UnsupportedSchema {
        found: ArtifactSchemaVersion,
        expected: ArtifactSchemaVersion,
    },
    ContractMismatch {
        required: ContractIdentity,
        provided: ContractIdentity,
    },
    TargetMismatch {
        required: TargetModel,
        provided: TargetModel,
    },
    FormatMismatch {
        required: RuntimeArtifactFormat,
        provided: RuntimeArtifactFormat,
    },
    CompatibilityMismatch {
        required: CompatibilityIdentity,
        provided: CompatibilityIdentity,
    },
    MissingCapability {
        operation: RuntimeOp,
        capability: TargetCapability,
    },
}

impl Display for RuntimeLinkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { found, expected } => write!(
                formatter,
                "runtime artifact schema {} is unsupported; expected {}",
                found.get(),
                expected.get()
            ),
            Self::ContractMismatch { required, provided } => write!(
                formatter,
                "runtime artifact contract {provided} does not match required contract {required}"
            ),
            Self::TargetMismatch { required, provided } => write!(
                formatter,
                "runtime artifact target {provided:?} does not match required target {required:?}"
            ),
            Self::FormatMismatch { required, provided } => write!(
                formatter,
                "runtime artifact format {provided:?} does not match required format {required:?}"
            ),
            Self::CompatibilityMismatch { required, provided } => write!(
                formatter,
                "runtime artifact compatibility {provided} does not match required compatibility {required}"
            ),
            Self::MissingCapability {
                operation,
                capability,
            } => write!(
                formatter,
                "runtime operation {operation:?} requires missing provider capability {capability:?}"
            ),
        }
    }
}

impl std::error::Error for RuntimeLinkError {}

/// One validated, path-independent runtime link plan.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeLinkPlan {
    request: RuntimeLinkRequest,
    artifact: ArtifactIdentity,
    implementation: ImplementationHash,
}

impl RuntimeLinkPlan {
    fn validated(request: RuntimeLinkRequest, artifact: &RuntimeArtifactManifest) -> Self {
        Self {
            request,
            artifact: artifact.identity(),
            implementation: artifact.implementation(),
        }
    }

    #[must_use]
    pub const fn request(&self) -> &RuntimeLinkRequest {
        &self.request
    }

    #[must_use]
    pub const fn artifact(&self) -> ArtifactIdentity {
        self.artifact
    }

    #[must_use]
    pub const fn implementation(&self) -> ImplementationHash {
        self.implementation
    }

    /// Canonical cache input for the validated consumer/provider pair.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::link_plan();
        encoder.u32(CURRENT_LINK_PLAN_SCHEMA);
        self.request.encode(&mut encoder);
        encoder.fixed_bytes(self.artifact.as_bytes());
        encoder.fixed_bytes(self.implementation.as_bytes());
        encoder.finish()
    }

    #[must_use]
    pub fn identity(&self) -> LinkPlanIdentity {
        LinkPlanIdentity::sha256(&self.canonical_bytes())
    }
}

impl RuntimeArtifactManifest {
    /// Validate this provider against one program and construct its link plan.
    ///
    /// Checks are ordered deliberately so diagnostics and tests are stable.
    pub fn select(&self, request: RuntimeLinkRequest) -> Result<RuntimeLinkPlan, RuntimeLinkError> {
        if self.schema() != CURRENT_ARTIFACT_SCHEMA {
            return Err(RuntimeLinkError::UnsupportedSchema {
                found: self.schema(),
                expected: CURRENT_ARTIFACT_SCHEMA,
            });
        }
        if self.contract() != request.dependency().contract() {
            return Err(RuntimeLinkError::ContractMismatch {
                required: request.dependency().contract(),
                provided: self.contract(),
            });
        }
        if self.target() != request.target() {
            return Err(RuntimeLinkError::TargetMismatch {
                required: request.target().clone(),
                provided: self.target().clone(),
            });
        }
        if self.format() != request.format() {
            return Err(RuntimeLinkError::FormatMismatch {
                required: request.format(),
                provided: self.format(),
            });
        }
        if self.compatibility() != request.compatibility() {
            return Err(RuntimeLinkError::CompatibilityMismatch {
                required: request.compatibility(),
                provided: self.compatibility(),
            });
        }
        for operation in request.dependency().requirement().iter() {
            for capability in operation.required_capabilities() {
                if !self.provided_capabilities().contains(*capability) {
                    return Err(RuntimeLinkError::MissingCapability {
                        operation,
                        capability: *capability,
                    });
                }
            }
        }
        Ok(RuntimeLinkPlan::validated(request, self))
    }
}
