//! Typed, canonical description of the gors compiler/runtime ABI.
//!
//! The target-neutral [`RuntimeAbiManifest`] is the semantic compiler/runtime
//! contract. A [`RuntimeArtifactManifest`] separately identifies one compiled
//! implementation for a concrete target. This crate implements neither the
//! compiler nor the runtime.

#![forbid(unsafe_code)]

mod artifact;
mod contract;
mod encoding;
mod identity;
mod operations;
mod target;

pub use artifact::{
    ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA, MissingCapability, RuntimeArtifactManifest,
};
pub use contract::{
    CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, GoSemanticModel,
    ManifestSchemaVersion, RuntimeAbiManifest,
};
pub use identity::{ArtifactIdentity, ContractIdentity, ImplementationHash};
pub use operations::{PrimitiveOp, RuntimeOp, RuntimeSignature, RuntimeType};
pub use target::{Endianness, TargetCapabilities, TargetCapability, TargetModel, TargetModelError};
