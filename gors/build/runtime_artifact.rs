//! Build-time production of the one native runtime provider.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use gors_runtime_abi::{
    DataWidth, Endianness, ImplementationHash, NATIVE_RUNTIME_RUST_TOOLCHAIN,
    RUST_RUNTIME_CODEGEN_UNITS, RUST_RUNTIME_CRATE_NAME, RUST_RUNTIME_EDITION,
    RUST_RUNTIME_EMBED_BITCODE, RUST_RUNTIME_METADATA, RUST_RUNTIME_OPT_LEVEL,
    RUST_RUNTIME_PANIC_STRATEGY, RUST_RUNTIME_REMAP_ROOT, RuntimeAbiManifest,
    RuntimeArtifactFormat, RuntimeArtifactManifest, RustRlibCompatibility, RustRlibProducer,
    TargetCapabilities, TargetCapability, TargetModel, canonical_target_libdir_record,
};
use sha2::{Digest, Sha256};

type BuildResult<T> = Result<T, Box<dyn std::error::Error>>;

const BUILD_KEY_DOMAIN: &[u8] = b"gors-native-runtime-provider-v1\0";
const GENERATED_METADATA: &str = "gors_runtime_artifact.rs";
const PROVIDER_DIRECTORY: &str = "gors_runtime_provider";
const PROVIDER_FILENAME: &str = "lib__gors_runtime.rlib";
const PROVIDER_STAMP: &str = "provider.stamp";
const RUNTIME_SOURCE_DIRECTORY: &str = "gors-runtime/src";
const RUNTIME_SOURCE_ENTRY: &str = "gors-runtime/src/lib.rs";

/// Build and describe the native runtime before stdlib preload can return.
pub fn build_native_provider(out_dir: &Path) -> BuildResult<()> {
    if target_is_wasm()? {
        return Ok(());
    }

    let workspace_root = workspace_root()?;
    let target = target_model()?;
    let rustc = native_runtime_rustc()?;
    let rustc_verbose_version = rustc_verbose_version(&rustc)?;
    let target_libdir = rustc_target_libdir(&rustc, &target)?;
    let target_libdir_record = canonical_target_libdir_record(&target_libdir)?;
    let producer = RustRlibProducer::new(
        &rustc_verbose_version,
        target_libdir_record.clone(),
        target.clone(),
    )?;
    let compatibility =
        RustRlibCompatibility::new(&rustc_verbose_version, target_libdir_record, target.clone())?;
    let provider_dir = out_dir.join(PROVIDER_DIRECTORY);
    std::fs::create_dir_all(&provider_dir)?;
    let payload_path = provider_dir.join(PROVIDER_FILENAME);
    let stamp_path = provider_dir.join(PROVIDER_STAMP);
    let build_key = provider_build_key(&workspace_root, &producer)?;

    if !cached_provider_matches(&payload_path, &stamp_path, &build_key)? {
        compile_provider(&rustc, &workspace_root, &target, &payload_path)?;
    }

    let payload = std::fs::read(&payload_path)?;
    let implementation = ImplementationHash::sha256(&payload);
    let manifest = RuntimeArtifactManifest::new(
        RuntimeAbiManifest::current().identity(),
        target,
        TargetCapabilities::new([TargetCapability::StandardIo]),
        RuntimeArtifactFormat::RustRlibV1,
        compatibility.identity(),
        implementation,
    );
    write_generated_metadata(
        out_dir,
        &manifest,
        &producer,
        &compatibility,
        &rustc_verbose_version,
    )?;
    std::fs::write(stamp_path, format!("{build_key}\n{implementation}\n"))?;
    Ok(())
}

fn target_is_wasm() -> BuildResult<bool> {
    let families = std::env::var("CARGO_CFG_TARGET_FAMILY")?;
    Ok(families.split(',').any(|family| family == "wasm"))
}

fn workspace_root() -> BuildResult<PathBuf> {
    let manifest_dir = required_environment_path("CARGO_MANIFEST_DIR")?;
    manifest_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
        std::io::Error::other(format!(
            "gors manifest directory has no workspace parent: {}",
            manifest_dir.display()
        ))
        .into()
    })
}

fn required_environment_path(name: &str) -> BuildResult<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).ok_or_else(|| {
        std::io::Error::other(format!(
            "required Cargo environment variable {name} is unset"
        ))
        .into()
    })
}

fn native_runtime_rustc() -> BuildResult<PathBuf> {
    let output = Command::new("rustup")
        .args([
            "which",
            "--toolchain",
            NATIVE_RUNTIME_RUST_TOOLCHAIN,
            "rustc",
        ])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "rustup could not resolve native runtime toolchain {NATIVE_RUNTIME_RUST_TOOLCHAIN}: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let value = std::str::from_utf8(&output.stdout)?.trim();
    if value.is_empty() {
        return Err(std::io::Error::other(format!(
            "rustup returned no rustc path for native runtime toolchain {NATIVE_RUNTIME_RUST_TOOLCHAIN}"
        ))
        .into());
    }
    let rustc = PathBuf::from(value);
    if !rustc.is_file() {
        return Err(std::io::Error::other(format!(
            "rustup selected missing rustc {} for native runtime toolchain {NATIVE_RUNTIME_RUST_TOOLCHAIN}",
            rustc.display()
        ))
        .into());
    }
    Ok(rustc)
}

fn target_model() -> BuildResult<TargetModel> {
    let triple = std::env::var("TARGET")?;
    let pointer_width = match std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH")?.as_str() {
        "32" => DataWidth::Bits32,
        "64" => DataWidth::Bits64,
        width => {
            return Err(std::io::Error::other(format!(
                "runtime artifacts do not support a {width}-bit Rust pointer width"
            ))
            .into());
        }
    };
    let endianness = match std::env::var("CARGO_CFG_TARGET_ENDIAN")?.as_str() {
        "little" => Endianness::Little,
        "big" => Endianness::Big,
        endian => {
            return Err(std::io::Error::other(format!(
                "runtime artifacts do not support Rust target endianness {endian:?}"
            ))
            .into());
        }
    };
    Ok(TargetModel::new(triple, pointer_width, endianness)?)
}

fn rustc_verbose_version(rustc: &Path) -> BuildResult<Vec<u8>> {
    let output = Command::new(rustc).arg("-vV").output()?;
    if !output.status.success() {
        return Err(command_error(rustc, "-vV", &output).into());
    }
    if output.stdout.is_empty() {
        return Err(std::io::Error::other(format!(
            "{} -vV returned an empty producer record",
            rustc.display()
        ))
        .into());
    }
    Ok(output.stdout)
}

fn rustc_target_libdir(rustc: &Path, target: &TargetModel) -> BuildResult<PathBuf> {
    let output = Command::new(rustc)
        .args(["--target", target.triple(), "--print", "target-libdir"])
        .output()?;
    if !output.status.success() {
        return Err(command_error(rustc, "target-libdir query", &output).into());
    }
    let value = std::str::from_utf8(&output.stdout)?.trim();
    if value.is_empty() {
        return Err(std::io::Error::other(format!(
            "{} returned an empty target-libdir for {}",
            rustc.display(),
            target.triple()
        ))
        .into());
    }
    Ok(PathBuf::from(value))
}

fn provider_build_key(workspace_root: &Path, producer: &RustRlibProducer) -> BuildResult<String> {
    let source_root = workspace_root.join(RUNTIME_SOURCE_DIRECTORY);
    let mut sources = Vec::new();
    collect_rust_sources(&source_root, &mut sources)?;
    sources.sort();

    let mut hasher = Sha256::new();
    hasher.update(BUILD_KEY_DOMAIN);
    hasher.update(producer.canonical_bytes());
    for source in sources {
        let relative = source.strip_prefix(workspace_root).map_err(|error| {
            std::io::Error::other(format!(
                "runtime source {} is outside workspace {}: {error}",
                source.display(),
                workspace_root.display()
            ))
        })?;
        hasher.update(relative.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(std::fs::read(&source)?);
        hasher.update(b"\0");
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn collect_rust_sources(root: &Path, sources: &mut Vec<PathBuf>) -> BuildResult<()> {
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_sources(&path, sources)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    Ok(())
}

fn cached_provider_matches(
    payload_path: &Path,
    stamp_path: &Path,
    build_key: &str,
) -> BuildResult<bool> {
    let Ok(stamp) = std::fs::read_to_string(stamp_path) else {
        return Ok(false);
    };
    let mut lines = stamp.lines();
    if lines.next() != Some(build_key) {
        return Ok(false);
    }
    let Some(expected_implementation) = lines.next() else {
        return Ok(false);
    };
    let Ok(payload) = std::fs::read(payload_path) else {
        return Ok(false);
    };
    Ok(ImplementationHash::sha256(&payload).to_string() == expected_implementation)
}

fn compile_provider(
    rustc: &Path,
    workspace_root: &Path,
    target: &TargetModel,
    payload_path: &Path,
) -> BuildResult<()> {
    let temporary_path = payload_path.with_extension(format!("rlib.tmp-{}", std::process::id()));
    if temporary_path.exists() {
        std::fs::remove_file(&temporary_path)?;
    }

    let mut remap = OsString::from("--remap-path-prefix=");
    remap.push(workspace_root.as_os_str());
    remap.push("=");
    remap.push(RUST_RUNTIME_REMAP_ROOT);

    let output = Command::new(rustc)
        .current_dir(workspace_root)
        .arg(RUNTIME_SOURCE_ENTRY)
        .arg("--crate-name")
        .arg(RUST_RUNTIME_CRATE_NAME)
        .arg("--crate-type")
        .arg("rlib")
        .arg("--edition")
        .arg(RUST_RUNTIME_EDITION)
        .arg("--target")
        .arg(target.triple())
        .arg("--emit")
        .arg("link")
        .arg("-C")
        .arg(format!("metadata={RUST_RUNTIME_METADATA}"))
        .arg("-C")
        .arg(format!("opt-level={RUST_RUNTIME_OPT_LEVEL}"))
        .arg("-C")
        .arg(format!("codegen-units={RUST_RUNTIME_CODEGEN_UNITS}"))
        .arg("-C")
        .arg(format!("panic={RUST_RUNTIME_PANIC_STRATEGY}"))
        .arg("-C")
        .arg(format!("embed-bitcode={RUST_RUNTIME_EMBED_BITCODE}"))
        .arg(remap)
        .arg("-o")
        .arg(&temporary_path)
        .output()?;
    if !output.status.success() {
        drop(std::fs::remove_file(&temporary_path));
        return Err(command_error(rustc, "runtime provider build", &output).into());
    }
    if !temporary_path.is_file() {
        return Err(std::io::Error::other(format!(
            "{} did not produce runtime artifact {}",
            rustc.display(),
            temporary_path.display()
        ))
        .into());
    }
    if payload_path.exists() {
        std::fs::remove_file(payload_path)?;
    }
    std::fs::rename(&temporary_path, payload_path)?;
    Ok(())
}

fn command_error(rustc: &Path, action: &str, output: &std::process::Output) -> std::io::Error {
    std::io::Error::other(format!(
        "{} {action} failed with {}\nstdout:\n{}\nstderr:\n{}",
        rustc.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

fn write_generated_metadata(
    out_dir: &Path,
    manifest: &RuntimeArtifactManifest,
    producer: &RustRlibProducer,
    compatibility: &RustRlibCompatibility,
    rustc_verbose_version: &[u8],
) -> BuildResult<()> {
    let target = manifest.target();
    let target_endian = match target.endianness() {
        Endianness::Little => "little",
        Endianness::Big => "big",
    };
    let metadata = format!(
        "\
pub const ARTIFACT_SCHEMA: u32 = {schema};\n\
pub const CONTRACT_IDENTITY: [u8; 32] = {contract:?};\n\
pub const TARGET_TRIPLE: &str = {triple:?};\n\
pub const TARGET_POINTER_WIDTH: u16 = {pointer_width};\n\
pub const TARGET_ENDIAN: &str = {target_endian:?};\n\
pub const PRODUCER_IDENTITY: [u8; 32] = {producer_identity:?};\n\
pub const COMPATIBILITY_IDENTITY: [u8; 32] = {compatibility_identity:?};\n\
pub const IMPLEMENTATION_HASH: [u8; 32] = {implementation:?};\n\
pub const ARTIFACT_IDENTITY: [u8; 32] = {artifact:?};\n\
pub const RUSTC_VERBOSE_VERSION: &[u8] = &{rustc_verbose_version:?};\n\
pub const TARGET_LIBDIR_RECORD: &[u8] = &{target_libdir_record:?};\n\
pub const PRODUCER_RECORD: &[u8] = &{producer_record:?};\n\
pub const COMPATIBILITY_RECORD: &[u8] = &{compatibility_record:?};\n\
pub const PAYLOAD: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{PROVIDER_DIRECTORY}/{PROVIDER_FILENAME}\"));\n",
        schema = manifest.schema().get(),
        contract = manifest.contract().as_bytes(),
        triple = target.triple(),
        pointer_width = target.pointer_width().bits(),
        producer_identity = producer.identity().as_bytes(),
        compatibility_identity = manifest.compatibility().as_bytes(),
        implementation = manifest.implementation().as_bytes(),
        artifact = manifest.identity().as_bytes(),
        target_libdir_record = compatibility.target_libdir_record(),
        producer_record = producer.canonical_bytes(),
        compatibility_record = compatibility.canonical_bytes(),
    );
    std::fs::write(out_dir.join(GENERATED_METADATA), metadata)?;
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
