use std::path::{Path, PathBuf};
use std::process::Command;

use gors_runtime_abi::{
    NATIVE_RUNTIME_RUST_TOOLCHAIN, RuntimeArtifactFormat, RuntimeDependency, RuntimeLinkRequest,
    RustRlibCompatibility, canonical_target_libdir_record,
};

use crate::runtime_descriptor::{RuntimeLinkDescriptor, RuntimeLinkOutput};

/// Fully validated and materialized runtime sidecar for one generated program.
pub struct ResolvedRuntime {
    descriptor: RuntimeLinkDescriptor,
    artifact_path: PathBuf,
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
    pub fn output_descriptor(&self) -> RuntimeLinkOutput {
        RuntimeLinkOutput::new(self.descriptor.clone(), &self.artifact_path)
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
    let provider = gors::artifact::embedded_runtime_artifact();

    let rustc_verbose_version = live_rustc_verbose_version()?;
    let target_libdir = live_target_libdir(provider.manifest().target().triple())?;
    let target_libdir_record = canonical_target_libdir_record(&target_libdir)?;
    let live_compatibility = RustRlibCompatibility::new(
        rustc_verbose_version,
        target_libdir_record,
        provider.manifest().target().clone(),
    )?;
    if live_compatibility.canonical_bytes() != provider.compatibility().canonical_bytes()
        || live_compatibility.identity() != provider.manifest().compatibility()
    {
        return Err(std::io::Error::other(format!(
            "rustup toolchain {NATIVE_RUNTIME_RUST_TOOLCHAIN} is incompatible with embedded runtime provider: live identity {}, provider identity {}",
            live_compatibility.identity(),
            provider.manifest().compatibility()
        ))
        .into());
    }

    let request = RuntimeLinkRequest::new(
        dependency,
        provider.manifest().target().clone(),
        RuntimeArtifactFormat::RustRlibV1,
        live_compatibility.identity(),
    );
    let plan = provider.manifest().select(request)?;
    let runtime_cache = if cache_base.is_absolute() {
        cache_base.join("runtime")
    } else {
        std::env::current_dir()?.join(cache_base).join("runtime")
    };
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

    Ok(ResolvedRuntime {
        descriptor: RuntimeLinkDescriptor::from_plan(&plan, provider.producer().identity()),
        artifact_path,
    })
}

fn live_rustc_verbose_version() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let output = Command::new("rustup")
        .args(["run", NATIVE_RUNTIME_RUST_TOOLCHAIN, "rustc", "-vV"])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "rustup run {NATIVE_RUNTIME_RUST_TOOLCHAIN} rustc -vV failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    if output.stdout.is_empty() {
        return Err(std::io::Error::other(format!(
            "rustup run {NATIVE_RUNTIME_RUST_TOOLCHAIN} rustc -vV returned an empty compatibility record"
        ))
        .into());
    }
    Ok(output.stdout)
}

fn live_target_libdir(target: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let output = Command::new("rustup")
        .args([
            "run",
            NATIVE_RUNTIME_RUST_TOOLCHAIN,
            "rustc",
            "--target",
            target,
            "--print",
            "target-libdir",
        ])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "rustup run {NATIVE_RUNTIME_RUST_TOOLCHAIN} rustc target-libdir query failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let path = std::str::from_utf8(&output.stdout)?.trim();
    if path.is_empty() {
        return Err(std::io::Error::other(format!(
            "rustup run {NATIVE_RUNTIME_RUST_TOOLCHAIN} rustc returned an empty target-libdir for {target}"
        ))
        .into());
    }
    Ok(PathBuf::from(path))
}
