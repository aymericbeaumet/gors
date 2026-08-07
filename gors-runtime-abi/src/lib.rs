//! Typed, canonical description of the gors compiler/runtime ABI.
//!
//! The target-neutral [`RuntimeAbiManifest`] is the semantic compiler/runtime
//! contract. Every [`PrimitiveOp`] and [`RuntimeOp`] carries an exact typed
//! signature. Runtime operations additionally carry target capability
//! requirements and [`RuntimeEffects`] describing allocation, owned-argument
//! mutation, blocking, host I/O, and Go language panic conditions. A canonical
//! [`RuntimeRequirement`] records the exact runtime operations selected by a
//! compiled unit. A [`RuntimeArtifactManifest`] separately identifies one
//! compiled implementation for a concrete target. [`RuntimeDependency`] makes
//! linking that provider unconditional, while [`RuntimeLinkPlan`] records a
//! validated consumer/provider pairing. This crate implements neither the
//! compiler nor the runtime.

#![forbid(unsafe_code)]

mod artifact;
mod contract;
mod effects;
mod encoding;
mod identity;
mod link;
mod operations;
mod requirement;
mod target;
mod toolchain;

pub use artifact::{
    ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA, RuntimeArtifactFormat, RuntimeArtifactManifest,
};
pub use contract::{
    CURRENT_CONTRACT_VERSION, CURRENT_MANIFEST_SCHEMA, ContractVersion, DataWidth, GoSemanticModel,
    ManifestSchemaVersion, RuntimeAbiManifest,
};
pub use effects::{
    AllocationEffect, ArgumentMutationEffect, BlockingEffect, GoPanicCondition, HostIoEffect,
    RuntimeEffects,
};
pub use identity::{
    ArtifactIdentity, CompatibilityIdentity, ContractIdentity, ImplementationHash,
    LinkPlanIdentity, ProducerIdentity,
};
pub use link::{
    CURRENT_LINK_PLAN_SCHEMA, CURRENT_RUNTIME_DEPENDENCY_SCHEMA, RequirementContractError,
    RuntimeDependency, RuntimeLinkError, RuntimeLinkPlan, RuntimeLinkRequest,
};
pub use operations::{
    IntegerKind, IntegerKindConstraint, IntegerPrimitive, IntegerRuntimeOp, PrimitiveOp,
    PrimitiveOpId, RuntimeOp, RuntimeOpId, RuntimeSignature, RuntimeType, UnknownRuntimeOpId,
};
pub use requirement::RuntimeRequirement;
pub use target::{Endianness, TargetCapabilities, TargetCapability, TargetModel, TargetModelError};
pub use toolchain::{
    CURRENT_RUST_RLIB_COMPATIBILITY_SCHEMA, CURRENT_RUST_RLIB_PRODUCER_SCHEMA,
    CURRENT_RUST_TARGET_LIBDIR_SCHEMA, NATIVE_RUNTIME_RUST_TOOLCHAIN, RUST_RUNTIME_CODEGEN_UNITS,
    RUST_RUNTIME_CRATE_NAME, RUST_RUNTIME_EDITION, RUST_RUNTIME_EMBED_BITCODE,
    RUST_RUNTIME_METADATA, RUST_RUNTIME_OPT_LEVEL, RUST_RUNTIME_PANIC_STRATEGY,
    RUST_RUNTIME_REMAP_ROOT, RustRlibCompatibility, RustRlibProducer, RustRlibRecordError,
    RustTargetLibdirError, canonical_target_libdir_record,
};
