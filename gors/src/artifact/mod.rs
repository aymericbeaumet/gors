//! Target-specific runtime artifact packaging.
//!
//! The semantic compiler depends only on the target-neutral runtime contract.
//! Native distributions additionally carry one exact, precompiled runtime
//! provider selected at the terminal Rust link boundary. There is no bundled
//! source or alternate provider path.

/// Crate name used by verified Rust IR emission and the terminal `--extern`
/// link argument.
pub use gors_runtime_abi::RUST_RUNTIME_CRATE_NAME as RUNTIME_CRATE_NAME;

#[cfg(not(target_family = "wasm"))]
mod embedded;

#[cfg(not(target_family = "wasm"))]
pub use embedded::{
    EmbeddedRuntimeArtifact, RuntimeArtifactError, RuntimeArtifactProducer,
    embedded_runtime_artifact,
};

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
