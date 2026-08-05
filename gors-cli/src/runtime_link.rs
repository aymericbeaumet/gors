use std::path::{Path, PathBuf};

use gors_runtime_abi::{
    NATIVE_RUNTIME_RUST_TOOLCHAIN, RuntimeArtifactFormat, RuntimeDependency, RuntimeLinkRequest,
};

use crate::runtime_descriptor::RuntimeLinkDescriptor;
use crate::rustc::TerminalToolchain;

mod compatibility_cache;

#[cfg(test)]
thread_local! {
    static RUNTIME_RESOLUTION_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Fully validated and materialized runtime sidecar for one generated program.
pub struct ResolvedRuntime {
    descriptor: RuntimeLinkDescriptor,
    artifact_path: PathBuf,
    terminal_toolchain: TerminalToolchain,
}

impl ResolvedRuntime {
    #[must_use]
    pub const fn descriptor(&self) -> &RuntimeLinkDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }

    #[must_use]
    pub const fn terminal_toolchain(&self) -> &TerminalToolchain {
        &self.terminal_toolchain
    }
}

/// Select the repository's single embedded provider and make its exact bytes
/// available in the global content-addressed runtime cache.
///
/// No runtime source is read and no provider is compiled on demand. The live
/// rustup toolchain must reproduce the provider's consumer compatibility
/// before its manifest can satisfy the generated program's dependency.
pub fn resolve_runtime(
    cache_base: &Path,
    dependency: RuntimeDependency,
) -> Result<ResolvedRuntime, Box<dyn std::error::Error>> {
    #[cfg(test)]
    RUNTIME_RESOLUTION_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let provider = gors::artifact::embedded_runtime_artifact();
    let absolute_cache_base = if cache_base.is_absolute() {
        cache_base.to_path_buf()
    } else {
        std::env::current_dir()?.join(cache_base)
    };
    let resolved_rustc = compatibility_cache::resolve(
        &absolute_cache_base.join("toolchains"),
        NATIVE_RUNTIME_RUST_TOOLCHAIN,
        provider.manifest().target(),
        provider.compatibility(),
    )?;

    let request = RuntimeLinkRequest::new(
        dependency,
        provider.manifest().target().clone(),
        RuntimeArtifactFormat::RustRlibV1,
        resolved_rustc.compatibility().identity(),
    );
    let plan = provider.manifest().select(request)?;
    let runtime_cache = absolute_cache_base.join("runtime");
    let artifact_path = provider.materialize(&runtime_cache)?;

    let expected_path = runtime_cache
        .join(plan.artifact().to_string())
        .join(format!(
            "lib{}.rlib",
            gors_runtime_abi::RUST_RUNTIME_CRATE_NAME
        ));
    if artifact_path != expected_path {
        return Err(std::io::Error::other(format!(
            "runtime provider materialized at {}, expected content-addressed path {}",
            artifact_path.display(),
            expected_path.display()
        ))
        .into());
    }

    let terminal_toolchain = TerminalToolchain::admit_live(
        resolved_rustc.rustc_path(),
        resolved_rustc.rustc_snapshot_identity(),
        resolved_rustc.target_libdir(),
        resolved_rustc.target_libdir_snapshot_identity(),
        provider.manifest().target().triple(),
    )?;
    Ok(ResolvedRuntime {
        descriptor: RuntimeLinkDescriptor::from_plan(&plan, provider.producer().identity()),
        artifact_path,
        terminal_toolchain,
    })
}

#[cfg(test)]
pub fn resolution_count() -> u64 {
    RUNTIME_RESOLUTION_COUNT.with(std::cell::Cell::get)
}

pub fn rustc_snapshot_identity(path: &Path) -> Result<String, String> {
    compatibility_cache::current_rustc_snapshot_identity(path).map_err(|error| error.to_string())
}

pub fn target_libdir_snapshot_identity(path: &Path) -> Result<String, String> {
    compatibility_cache::current_target_libdir_snapshot_identity(path)
        .map_err(|error| error.to_string())
}
