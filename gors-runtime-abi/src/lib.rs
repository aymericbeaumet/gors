//! Typed, canonical description of the gors compiler/runtime ABI.
//!
//! The target-neutral [`RuntimeAbiManifest`] is the semantic compiler/runtime
//! contract. Every [`PrimitiveOp`] and [`RuntimeOp`] carries an exact typed
//! signature. Runtime operations additionally carry target capability
//! requirements and [`RuntimeEffects`] describing allocation, owned-argument
//! mutation, host I/O, and Go language panic conditions. A canonical
//! [`RuntimeRequirement`] records the exact runtime operations selected by a
//! compiled unit. A [`RuntimeArtifactManifest`] separately identifies one
//! compiled implementation for a concrete target. This crate implements
//! neither the compiler nor the runtime.

#![forbid(unsafe_code)]

mod artifact;
mod contract;
mod effects;
mod encoding;
mod identity;
mod operations;
mod requirement;
mod target;

pub use artifact::{
    ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA, MissingCapability, RuntimeArtifactManifest,
};
pub use contract::{
    CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, GoSemanticModel,
    ManifestSchemaVersion, RuntimeAbiManifest,
};
pub use effects::{
    AllocationEffect, ArgumentMutationEffect, GoPanicCondition, HostIoEffect, RuntimeEffects,
};
pub use identity::{ArtifactIdentity, ContractIdentity, ImplementationHash};
pub use operations::{
    PrimitiveOp, PrimitiveOpId, RuntimeOp, RuntimeOpId, RuntimeSignature, RuntimeType,
};
pub use requirement::RuntimeRequirement;
pub use target::{Endianness, TargetCapabilities, TargetCapability, TargetModel, TargetModelError};
