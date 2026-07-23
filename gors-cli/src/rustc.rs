use crate::runtime_descriptor::RuntimeLinkDescriptor;
use crate::timings::TimingCollector;
use gors_runtime_abi::RUST_RUNTIME_CRATE_NAME;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub const RUST_EDITION: &str = gors_runtime_abi::RUST_RUNTIME_EDITION;

const RUSTC_ACTION_SCHEMA: &[u8] = b"gors-cli-rustc-action-v2";
const GENERATED_SOURCE_FILENAME: &str = "main.rs";
const PENDING_BINARY_FILENAME: &str = ".main.pending";
const TARGET_CPU: &str = "generic";
const REQUESTED_TARGET_FEATURES: &[&str] = &[];

/// One immutable, completely ordered invocation of the terminal Rust compiler.
///
/// Construction consumes the already-admitted hash of every generated Rust
/// file and every semantic input that is not already spelled in `argv`.
/// `program` is the exact absolute rustc path admitted by the compatibility
/// probe, never a rustup selector.
/// The same owned `program`, `cwd`, and `argv` values are used for action-key
/// construction and process execution, so cache admission cannot describe a
/// different command from the one that is run.
#[derive(Clone)]
pub struct RustcAction {
    program: OsString,
    rustc_snapshot_identity: String,
    cwd: PathBuf,
    argv: Vec<OsString>,
    generated_sources: Vec<GeneratedSource>,
    runtime_artifact_path: PathBuf,
    runtime_implementation_hash: [u8; 32],
    runtime_link_plan_identity: String,
    runtime_compatibility_identity: String,
    output_path: PathBuf,
    pending_path: PathBuf,
    profile: RustcProfile,
    target: String,
    target_cpu: String,
    requested_target_features: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct AdmittedRustc<'a> {
    path: &'a Path,
    snapshot_identity: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustcProfile {
    Debug,
    Release,
}

#[derive(Clone)]
struct GeneratedSource {
    filename: String,
    content_hash: [u8; 32],
}

/// Canonical SHA-256 identity of one [`RustcAction`].
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct RustcActionIdentity([u8; 32]);

#[derive(Debug)]
pub enum RustcActionError {
    Io(std::io::Error),
    RustcPathNotAbsolute {
        path: PathBuf,
    },
    RustcSnapshotInspection {
        path: PathBuf,
        detail: String,
    },
    RustcSnapshotChanged {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    InvalidGeneratedSource {
        filename: String,
        detail: &'static str,
    },
    MissingGeneratedSource {
        filename: &'static str,
    },
    InvalidRuntimeImplementationHash {
        value: String,
    },
    GeneratedSourceInspection {
        path: PathBuf,
        source: std::io::Error,
    },
    GeneratedSourceChanged {
        path: PathBuf,
    },
    RuntimeArtifactInspection {
        path: PathBuf,
        source: std::io::Error,
    },
    RuntimeArtifactChanged {
        path: PathBuf,
    },
    CompilerFailed {
        status: ExitStatus,
    },
}

impl RustcAction {
    /// Build the exact terminal action for the generated program in
    /// `output_directory`.
    ///
    /// The provider's target is always passed explicitly. CPU selection is
    /// deliberately portable (`generic`) and the requested target-feature set
    /// is explicitly empty; host-native feature discovery is never part of the
    /// command or its cache identity. Construction deliberately does not inspect
    /// `rustc_path`, allowing a verified warm executable to be admitted after
    /// its historical compiler has been removed. Execution revalidates it.
    pub fn for_generated_binary(
        output_directory: &Path,
        output_path: &Path,
        runtime_artifact_path: &Path,
        runtime: &RuntimeLinkDescriptor,
        rustc: AdmittedRustc<'_>,
        generated_file_hashes: &BTreeMap<String, String>,
        release: bool,
    ) -> Result<Self, RustcActionError> {
        if !rustc.path.is_absolute() {
            return Err(RustcActionError::RustcPathNotAbsolute {
                path: rustc.path.to_path_buf(),
            });
        }
        let cwd = absolute_path(output_directory)?;
        let output_path = absolute_path(output_path)?;
        let runtime_artifact_path = absolute_path(runtime_artifact_path)?;
        let pending_path = cwd.join(PENDING_BINARY_FILENAME);
        let generated_sources = admitted_generated_sources(generated_file_hashes)?;
        let runtime_implementation_hash =
            decode_sha256(runtime.implementation_hash()).ok_or_else(|| {
                RustcActionError::InvalidRuntimeImplementationHash {
                    value: runtime.implementation_hash().to_string(),
                }
            })?;
        let profile = if release {
            RustcProfile::Release
        } else {
            RustcProfile::Debug
        };
        let target = runtime.target_triple().to_string();
        let target_cpu = TARGET_CPU.to_string();
        let requested_target_features = REQUESTED_TARGET_FEATURES
            .iter()
            .map(|feature| (*feature).to_string())
            .collect::<Vec<_>>();
        let argv = rustc_argv(
            &runtime_artifact_path,
            profile,
            &target,
            &target_cpu,
            &requested_target_features,
        );

        Ok(Self {
            program: rustc.path.as_os_str().to_owned(),
            rustc_snapshot_identity: rustc.snapshot_identity.to_string(),
            cwd,
            argv,
            generated_sources,
            runtime_artifact_path,
            runtime_implementation_hash,
            runtime_link_plan_identity: runtime.link_plan_identity().to_string(),
            runtime_compatibility_identity: runtime.compatibility_identity().to_string(),
            output_path,
            pending_path,
            profile,
            target,
            target_cpu,
            requested_target_features,
        })
    }

    #[must_use]
    pub fn identity(&self) -> RustcActionIdentity {
        self.calculate_identity()
    }

    #[cfg(test)]
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[cfg(test)]
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    #[cfg(test)]
    #[must_use]
    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }

    #[cfg(test)]
    #[must_use]
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    #[cfg(test)]
    #[must_use]
    pub fn pending_path(&self) -> &Path {
        &self.pending_path
    }

    /// Execute this exact action and publish its stable pending output path to
    /// the requested final path.
    ///
    /// The caller must hold the output-directory lock for the action's entire
    /// lifetime. That lock makes the fixed `.main.pending` name exclusive and
    /// prevents generated input mutation between identity construction and
    /// execution.
    pub fn execute(&self) -> Result<(), RustcActionError> {
        for source in &self.generated_sources {
            let path = self.cwd.join(&source.filename);
            let actual = sha256_file(&path).map_err(|source| {
                RustcActionError::GeneratedSourceInspection {
                    path: path.clone(),
                    source,
                }
            })?;
            if actual != source.content_hash {
                return Err(RustcActionError::GeneratedSourceChanged { path });
            }
        }
        let actual_runtime_hash = sha256_file(&self.runtime_artifact_path).map_err(|source| {
            RustcActionError::RuntimeArtifactInspection {
                path: self.runtime_artifact_path.clone(),
                source,
            }
        })?;
        if actual_runtime_hash != self.runtime_implementation_hash {
            return Err(RustcActionError::RuntimeArtifactChanged {
                path: self.runtime_artifact_path.clone(),
            });
        }
        let rustc_path = Path::new(&self.program);
        let actual_rustc_snapshot = crate::runtime_link::rustc_snapshot_identity(rustc_path)
            .map_err(|detail| RustcActionError::RustcSnapshotInspection {
                path: rustc_path.to_path_buf(),
                detail,
            })?;
        if actual_rustc_snapshot != self.rustc_snapshot_identity {
            return Err(RustcActionError::RustcSnapshotChanged {
                path: rustc_path.to_path_buf(),
                expected: self.rustc_snapshot_identity.clone(),
                actual: actual_rustc_snapshot,
            });
        }

        remove_file_if_present(&self.pending_path)?;
        let status = Command::new(&self.program)
            .current_dir(&self.cwd)
            .args(&self.argv)
            .status()?;
        if !status.success() {
            remove_file_if_present(&self.pending_path)?;
            return Err(RustcActionError::CompilerFailed { status });
        }

        remove_file_if_present(&self.output_path)?;
        std::fs::rename(&self.pending_path, &self.output_path)?;
        Ok(())
    }

    fn calculate_identity(&self) -> RustcActionIdentity {
        let mut hasher = Sha256::new();
        hash_bytes(&mut hasher, b"schema", RUSTC_ACTION_SCHEMA);
        hash_os_str(&mut hasher, b"program", &self.program);
        hash_bytes(
            &mut hasher,
            b"rustc-snapshot-identity",
            self.rustc_snapshot_identity.as_bytes(),
        );
        hash_os_str(&mut hasher, b"cwd", self.cwd.as_os_str());
        hash_count(
            &mut hasher,
            b"generated-source-count",
            self.generated_sources.len(),
        );
        for source in &self.generated_sources {
            hash_bytes(
                &mut hasher,
                b"generated-source-filename",
                source.filename.as_bytes(),
            );
            hash_bytes(
                &mut hasher,
                b"generated-source-content-sha256",
                &source.content_hash,
            );
        }
        hash_os_str(
            &mut hasher,
            b"runtime-artifact-path",
            self.runtime_artifact_path.as_os_str(),
        );
        hash_bytes(
            &mut hasher,
            b"runtime-implementation-sha256",
            &self.runtime_implementation_hash,
        );
        hash_bytes(
            &mut hasher,
            b"runtime-link-plan-identity",
            self.runtime_link_plan_identity.as_bytes(),
        );
        hash_bytes(
            &mut hasher,
            b"runtime-compatibility-identity",
            self.runtime_compatibility_identity.as_bytes(),
        );
        hash_bytes(&mut hasher, b"output-profile", self.profile.label());
        hash_bytes(&mut hasher, b"target", self.target.as_bytes());
        hash_bytes(&mut hasher, b"target-cpu", self.target_cpu.as_bytes());
        hash_count(
            &mut hasher,
            b"requested-target-feature-count",
            self.requested_target_features.len(),
        );
        for feature in &self.requested_target_features {
            hash_bytes(&mut hasher, b"requested-target-feature", feature.as_bytes());
        }
        hash_os_str(
            &mut hasher,
            b"published-output-path",
            self.output_path.as_os_str(),
        );
        hash_os_str(
            &mut hasher,
            b"pending-output-path",
            self.pending_path.as_os_str(),
        );
        hash_count(&mut hasher, b"argv-count", self.argv.len());
        for argument in &self.argv {
            hash_os_str(&mut hasher, b"argv", argument);
        }
        RustcActionIdentity(hasher.finalize().into())
    }
}

impl<'a> AdmittedRustc<'a> {
    #[must_use]
    pub const fn new(path: &'a Path, snapshot_identity: &'a str) -> Self {
        Self {
            path,
            snapshot_identity,
        }
    }
}

impl RustcProfile {
    const fn label(self) -> &'static [u8] {
        match self {
            Self::Debug => b"debug",
            Self::Release => b"release",
        }
    }
}

impl Debug for RustcActionIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, formatter)
    }
}

impl Display for RustcActionIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Display for RustcActionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::RustcPathNotAbsolute { path } => write!(
                formatter,
                "terminal rustc path is not absolute: {}",
                path.display()
            ),
            Self::RustcSnapshotInspection { path, detail } => write!(
                formatter,
                "failed to revalidate terminal rustc {}: {detail}",
                path.display()
            ),
            Self::RustcSnapshotChanged {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "terminal rustc {} changed after compatibility selection: expected snapshot {expected}, found {actual}",
                path.display()
            ),
            Self::InvalidGeneratedSource { filename, detail } => write!(
                formatter,
                "generated Rust input {filename:?} is invalid: {detail}"
            ),
            Self::MissingGeneratedSource { filename } => write!(
                formatter,
                "terminal rustc action is missing generated Rust input {filename}"
            ),
            Self::InvalidRuntimeImplementationHash { value } => write!(
                formatter,
                "runtime implementation hash is not canonical SHA-256: {value}"
            ),
            Self::GeneratedSourceInspection { path, source } => write!(
                formatter,
                "failed to revalidate generated Rust source {}: {source}",
                path.display()
            ),
            Self::GeneratedSourceChanged { path } => write!(
                formatter,
                "generated Rust source changed after terminal action construction: {}",
                path.display()
            ),
            Self::RuntimeArtifactInspection { path, source } => write!(
                formatter,
                "failed to revalidate runtime artifact {}: {source}",
                path.display()
            ),
            Self::RuntimeArtifactChanged { path } => write!(
                formatter,
                "runtime artifact changed after provider selection: {}",
                path.display()
            ),
            Self::CompilerFailed { status } => {
                write!(formatter, "terminal rustc action failed with {status}")
            }
        }
    }
}

impl std::error::Error for RustcActionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::GeneratedSourceInspection { source, .. }
            | Self::RuntimeArtifactInspection { source, .. } => Some(source),
            Self::RustcPathNotAbsolute { .. }
            | Self::RustcSnapshotInspection { .. }
            | Self::RustcSnapshotChanged { .. }
            | Self::InvalidGeneratedSource { .. }
            | Self::MissingGeneratedSource { .. }
            | Self::InvalidRuntimeImplementationHash { .. }
            | Self::GeneratedSourceChanged { .. }
            | Self::RuntimeArtifactChanged { .. }
            | Self::CompilerFailed { .. } => None,
        }
    }
}

impl From<std::io::Error> for RustcActionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Time and execute one already-admitted terminal action.
pub fn compile_generated_binary(
    action: &RustcAction,
    timings: &TimingCollector,
) -> Result<(), Box<dyn std::error::Error>> {
    let rustc_timer = timings.phase("cli.rustc");
    let result = action.execute();
    drop(rustc_timer);
    match result {
        Err(RustcActionError::CompilerFailed { status }) => {
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(error) => Err(Box::new(error)),
        Ok(()) => Ok(()),
    }
}

fn rustc_argv(
    runtime_artifact_path: &Path,
    profile: RustcProfile,
    target: &str,
    target_cpu: &str,
    requested_target_features: &[String],
) -> Vec<OsString> {
    let mut runtime_extern = OsString::from(RUST_RUNTIME_CRATE_NAME);
    runtime_extern.push("=");
    runtime_extern.push(runtime_artifact_path.as_os_str());
    let target_features = requested_target_features.join(",");
    let mut argv = vec![
        OsString::from(GENERATED_SOURCE_FILENAME),
        OsString::from(format!("--edition={RUST_EDITION}")),
        OsString::from("--target"),
        OsString::from(target),
        OsString::from("--extern"),
        runtime_extern,
        OsString::from("-D"),
        OsString::from("unused_imports"),
        OsString::from("-D"),
        OsString::from("unused_macros"),
        OsString::from("-C"),
        OsString::from("overflow-checks=off"),
        OsString::from(format!("-Ctarget-cpu={target_cpu}")),
        OsString::from(format!("-Ctarget-feature={target_features}")),
        OsString::from("-o"),
        OsString::from(PENDING_BINARY_FILENAME),
    ];
    if profile == RustcProfile::Release {
        argv.extend([
            OsString::from("-Ccodegen-units=1"),
            OsString::from("-Clto=fat"),
            OsString::from("-Copt-level=3"),
        ]);
    }
    argv
}

fn absolute_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn sha256_file(path: &Path) -> Result<[u8; 32], std::io::Error> {
    Ok(Sha256::digest(std::fs::read(path)?).into())
}

fn admitted_generated_sources(
    generated_file_hashes: &BTreeMap<String, String>,
) -> Result<Vec<GeneratedSource>, RustcActionError> {
    if !generated_file_hashes.contains_key(GENERATED_SOURCE_FILENAME) {
        return Err(RustcActionError::MissingGeneratedSource {
            filename: GENERATED_SOURCE_FILENAME,
        });
    }
    generated_file_hashes
        .iter()
        .map(|(filename, content_hash)| {
            if !is_normal_rust_filename(filename) {
                return Err(RustcActionError::InvalidGeneratedSource {
                    filename: filename.clone(),
                    detail: "expected one normal relative .rs filename",
                });
            }
            let content_hash = decode_sha256(content_hash).ok_or_else(|| {
                RustcActionError::InvalidGeneratedSource {
                    filename: filename.clone(),
                    detail: "content hash is not canonical SHA-256",
                }
            })?;
            Ok(GeneratedSource {
                filename: filename.clone(),
                content_hash,
            })
        })
        .collect()
}

fn is_normal_rust_filename(filename: &str) -> bool {
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || Path::new(filename)
            .extension()
            .is_none_or(|extension| extension != "rs")
    {
        return false;
    }
    let mut components = Path::new(filename).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (destination, pair) in decoded.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let [high, low] = pair else {
            return None;
        };
        *destination = (decode_hex_nibble(*high)? << 4) | decode_hex_nibble(*low)?;
    }
    Some(decoded)
}

const fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn hash_count(hasher: &mut Sha256, tag: &[u8], count: usize) {
    hash_bytes(hasher, tag, &usize_as_u128_le_bytes(count));
}

fn hash_os_str(hasher: &mut Sha256, tag: &[u8], value: &OsStr) {
    hash_bytes(
        hasher,
        b"native-os-string-platform",
        std::env::consts::OS.as_bytes(),
    );
    hash_bytes(hasher, tag, value.as_encoded_bytes());
}

fn hash_bytes(hasher: &mut Sha256, tag: &[u8], value: &[u8]) {
    hasher.update(usize_as_u128_le_bytes(tag.len()));
    hasher.update(tag);
    hasher.update(usize_as_u128_le_bytes(value.len()));
    hasher.update(value);
}

fn usize_as_u128_le_bytes(value: usize) -> [u8; 16] {
    let mut encoded = [0_u8; 16];
    for (destination, source) in encoded.iter_mut().zip(value.to_le_bytes()) {
        *destination = source;
    }
    encoded
}

#[cfg(test)]
mod tests;
