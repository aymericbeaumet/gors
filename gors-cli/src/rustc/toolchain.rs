use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Read as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

mod environment;
#[cfg(test)]
mod tests;

use environment::{deterministic_environment, is_canonical_environment};

const TOOLCHAIN_SCHEMA: u32 = 2;
const TOOLCHAIN_IDENTITY_DOMAIN: &[u8] = b"gors.terminal-toolchain-v2\0";

/// Immutable native tools and process environment used by one terminal action.
///
/// The descriptor is safe to reconstruct from a terminal manifest without
/// touching the tool executables. Before a cache miss executes, the recorded
/// tool revisions and target-libdir snapshot are revalidated. The remaining
/// path-to-spawn race and dynamically loaded compiler closure are documented
/// non-hermetic boundaries, not properties hidden by this descriptor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalToolchain {
    schema_version: u32,
    target: String,
    rustc: ToolArtifact,
    linker: ToolArtifact,
    target_libdir: TargetLibdirArtifact,
    environment: BTreeMap<String, String>,
    platform_link_contract: Option<PlatformLinkContract>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolArtifact {
    path: String,
    content_hash: String,
    size_bytes: u64,
    revision: ToolRevision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolRevision {
    readonly: bool,
    modified_before_epoch: bool,
    modified_seconds: u64,
    modified_nanoseconds: u32,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PlatformLinkContract {
    root: String,
    settings_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TargetLibdirArtifact {
    path: String,
    snapshot_identity: String,
}

#[derive(Serialize)]
struct ToolchainIdentity<'descriptor> {
    schema_version: u32,
    target: &'descriptor str,
    rustc: ToolIdentity<'descriptor>,
    linker: ToolIdentity<'descriptor>,
    environment: &'descriptor BTreeMap<String, String>,
    platform_link_contract: Option<&'descriptor PlatformLinkContract>,
}

#[derive(Serialize)]
struct ToolIdentity<'artifact> {
    path: &'artifact str,
    content_hash: &'artifact str,
    size_bytes: u64,
}

#[derive(Debug)]
pub enum TerminalToolchainError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidTool {
        path: PathBuf,
        detail: &'static str,
    },
    ToolChanged {
        role: &'static str,
        path: PathBuf,
    },
    RustcSnapshotChanged {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    TargetLibdirSnapshotChanged {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    LinkerResolution(String),
    InvalidProbeOutput {
        command: String,
        output: String,
    },
    NonUtf8Path(PathBuf),
    InvalidDescriptor(&'static str),
}

impl TerminalToolchain {
    pub fn admit_live(
        rustc_path: &Path,
        rustc_snapshot_identity: &str,
        target_libdir: &Path,
        target_libdir_snapshot_identity: &str,
        target: &str,
    ) -> Result<Self, TerminalToolchainError> {
        if target.is_empty() {
            return Err(TerminalToolchainError::InvalidDescriptor(
                "target triple is empty",
            ));
        }
        verify_rustc_snapshot(rustc_path, rustc_snapshot_identity)?;
        let target_libdir =
            TargetLibdirArtifact::admit(target_libdir, target_libdir_snapshot_identity)?;
        let rustc = ToolArtifact::admit(rustc_path)?;
        verify_rustc_snapshot(rustc_path, rustc_snapshot_identity)?;

        let linker_path = resolve_linker(target)?;
        let linker = ToolArtifact::admit(&linker_path)?;
        let mut environment = deterministic_environment(&rustc, &linker)?;
        let platform_link_contract = if is_apple_darwin_target(target) {
            let contract = apple_link_contract()?;
            environment.insert("SDKROOT".to_string(), contract.root.clone());
            if let Some((name, value)) = deployment_target(&rustc, target)? {
                if name != "MACOSX_DEPLOYMENT_TARGET" {
                    return Err(TerminalToolchainError::InvalidProbeOutput {
                        command: format!(
                            "{} --target {target} --print deployment-target",
                            rustc.path
                        ),
                        output: format!("{name}={value}"),
                    });
                }
                environment.insert(name, value);
            }
            Some(contract)
        } else {
            None
        };

        let descriptor = Self {
            schema_version: TOOLCHAIN_SCHEMA,
            target: target.to_string(),
            rustc,
            linker,
            target_libdir,
            environment,
            platform_link_contract,
        };
        if !descriptor.is_canonical() {
            return Err(TerminalToolchainError::InvalidDescriptor(
                "constructed terminal toolchain is not canonical",
            ));
        }
        descriptor.target_libdir.verify()?;
        Ok(descriptor)
    }

    #[cfg(test)]
    pub fn for_test(
        rustc_path: &Path,
        linker_path: &Path,
        target_libdir: &Path,
        target: &str,
    ) -> Result<Self, TerminalToolchainError> {
        let rustc = ToolArtifact::admit(rustc_path)?;
        let linker = ToolArtifact::admit(linker_path)?;
        let target_libdir = TargetLibdirArtifact::admit_current(target_libdir)?;
        Ok(Self {
            schema_version: TOOLCHAIN_SCHEMA,
            target: target.to_string(),
            environment: deterministic_environment(&rustc, &linker)?,
            rustc,
            linker,
            target_libdir,
            platform_link_contract: None,
        })
    }

    #[must_use]
    pub fn is_canonical(&self) -> bool {
        let platform_is_canonical = match &self.platform_link_contract {
            Some(contract) => {
                is_apple_darwin_target(&self.target)
                    && contract.is_canonical()
                    && self.environment.get("SDKROOT") == Some(&contract.root)
            }
            None => !self.target.contains("-apple-") && !self.environment.contains_key("SDKROOT"),
        };
        self.schema_version == TOOLCHAIN_SCHEMA
            && !self.target.is_empty()
            && self.rustc.is_canonical()
            && self.linker.is_canonical()
            && self.target_libdir.is_canonical()
            && is_canonical_environment(
                &self.environment,
                &self.target,
                self.platform_link_contract.as_ref(),
            )
            && platform_is_canonical
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[must_use]
    pub fn rustc_path(&self) -> &Path {
        Path::new(&self.rustc.path)
    }

    #[must_use]
    pub fn linker_path(&self) -> &Path {
        Path::new(&self.linker.path)
    }

    #[cfg(test)]
    #[must_use]
    pub fn target_libdir_path(&self) -> &Path {
        Path::new(&self.target_libdir.path)
    }

    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn identity(&self) -> Result<String, serde_json::Error> {
        // Revision metadata and the target-libdir snapshot guard are excluded:
        // RustcAction separately owns the content-bearing runtime compatibility
        // identity, while these facts only reject drift before execution.
        let encoded = serde_json::to_vec(&ToolchainIdentity {
            schema_version: self.schema_version,
            target: &self.target,
            rustc: self.rustc.identity_input(),
            linker: self.linker.identity_input(),
            environment: &self.environment,
            platform_link_contract: self.platform_link_contract.as_ref(),
        })?;
        let mut hasher = Sha256::new();
        hasher.update(TOOLCHAIN_IDENTITY_DOMAIN);
        hasher.update(
            u64::try_from(encoded.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(encoded);
        Ok(hex(hasher.finalize().into()))
    }

    pub fn verify_live(&self) -> Result<(), TerminalToolchainError> {
        if !self.is_canonical() {
            return Err(TerminalToolchainError::InvalidDescriptor(
                "terminal toolchain descriptor is not canonical",
            ));
        }
        self.rustc.verify("rustc")?;
        self.linker.verify("linker")?;
        self.target_libdir.verify()?;
        if let Some(contract) = &self.platform_link_contract {
            let current = PlatformLinkContract::admit(Path::new(&contract.root))?;
            if current != *contract {
                return Err(TerminalToolchainError::ToolChanged {
                    role: "platform link contract",
                    path: PathBuf::from(&contract.root),
                });
            }
        }
        Ok(())
    }
}

impl TargetLibdirArtifact {
    fn admit(path: &Path, expected: &str) -> Result<Self, TerminalToolchainError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| {
            TerminalToolchainError::io("canonicalize target-libdir", path, source)
        })?;
        if !canonical.is_absolute() || !canonical.is_dir() || !is_sha256(expected) {
            return Err(TerminalToolchainError::InvalidTool {
                path: canonical,
                detail: "expected an absolute directory and canonical snapshot identity",
            });
        }
        let actual = target_libdir_snapshot_identity(&canonical)?;
        if actual != expected {
            return Err(TerminalToolchainError::TargetLibdirSnapshotChanged {
                path: canonical,
                expected: expected.to_string(),
                actual,
            });
        }
        Ok(Self {
            path: utf8_path(&canonical)?,
            snapshot_identity: expected.to_string(),
        })
    }

    #[cfg(test)]
    fn admit_current(path: &Path) -> Result<Self, TerminalToolchainError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| {
            TerminalToolchainError::io("canonicalize target-libdir", path, source)
        })?;
        let identity = target_libdir_snapshot_identity(&canonical)?;
        Self::admit(&canonical, &identity)
    }

    fn is_canonical(&self) -> bool {
        Path::new(&self.path).is_absolute() && is_sha256(&self.snapshot_identity)
    }

    fn verify(&self) -> Result<(), TerminalToolchainError> {
        let actual = target_libdir_snapshot_identity(Path::new(&self.path))?;
        if actual == self.snapshot_identity {
            Ok(())
        } else {
            Err(TerminalToolchainError::TargetLibdirSnapshotChanged {
                path: PathBuf::from(&self.path),
                expected: self.snapshot_identity.clone(),
                actual,
            })
        }
    }
}

impl ToolArtifact {
    fn identity_input(&self) -> ToolIdentity<'_> {
        ToolIdentity {
            path: &self.path,
            content_hash: &self.content_hash,
            size_bytes: self.size_bytes,
        }
    }

    fn admit(path: &Path) -> Result<Self, TerminalToolchainError> {
        let canonical = std::fs::canonicalize(path).map_err(|source| {
            TerminalToolchainError::io("canonicalize terminal tool", path, source)
        })?;
        if !canonical.is_absolute() {
            return Err(TerminalToolchainError::InvalidTool {
                path: canonical,
                detail: "canonical path is not absolute",
            });
        }
        let file = open_tool_file(&canonical, "terminal tool")?;
        let before = file.metadata().map_err(|source| {
            TerminalToolchainError::io("inspect terminal tool", &canonical, source)
        })?;
        if !before.is_file() || before.len() == 0 {
            return Err(TerminalToolchainError::InvalidTool {
                path: canonical,
                detail: "expected a non-empty regular file",
            });
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if before.permissions().mode() & 0o111 == 0 {
                return Err(TerminalToolchainError::InvalidTool {
                    path: canonical,
                    detail: "tool has no executable permission bit",
                });
            }
        }
        let before_revision = ToolRevision::from_metadata(&before, &canonical)?;
        let mut reader = BufReader::new(&file);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer).map_err(|source| {
                TerminalToolchainError::io("hash terminal tool", &canonical, source)
            })?;
            if read == 0 {
                break;
            }
            let chunk = buffer
                .get(..read)
                .ok_or_else(|| TerminalToolchainError::InvalidTool {
                    path: canonical.clone(),
                    detail: "tool reader returned an impossible byte count",
                })?;
            hasher.update(chunk);
        }
        let after = file.metadata().map_err(|source| {
            TerminalToolchainError::io("reinspect terminal tool", &canonical, source)
        })?;
        let after_revision = ToolRevision::from_metadata(&after, &canonical)?;
        if before.len() != after.len() || before_revision != after_revision {
            return Err(TerminalToolchainError::ToolChanged {
                role: "terminal tool",
                path: canonical,
            });
        }
        verify_open_tool_path(
            &file,
            &canonical,
            "terminal tool",
            after.len(),
            &after_revision,
        )?;
        Ok(Self {
            path: utf8_path(&canonical)?,
            content_hash: hex(hasher.finalize().into()),
            size_bytes: after.len(),
            revision: after_revision,
        })
    }

    fn is_canonical(&self) -> bool {
        Path::new(&self.path).is_absolute()
            && self.size_bytes > 0
            && is_sha256(&self.content_hash)
            && self.revision.is_canonical()
    }

    fn verify(&self, role: &'static str) -> Result<(), TerminalToolchainError> {
        #[cfg(not(unix))]
        {
            let current = Self::admit(Path::new(&self.path))?;
            return if current == *self {
                Ok(())
            } else {
                Err(TerminalToolchainError::ToolChanged {
                    role,
                    path: PathBuf::from(&self.path),
                })
            };
        }
        #[cfg(unix)]
        {
            let path = Path::new(&self.path);
            let canonical = std::fs::canonicalize(path).map_err(|source| {
                TerminalToolchainError::io("canonicalize admitted terminal tool", path, source)
            })?;
            let file = open_tool_file(&canonical, role)?;
            let metadata = file.metadata().map_err(|source| {
                TerminalToolchainError::io("inspect admitted terminal tool", &canonical, source)
            })?;
            let revision = ToolRevision::from_metadata(&metadata, &canonical)?;
            if canonical != path || metadata.len() != self.size_bytes || revision != self.revision {
                Err(TerminalToolchainError::ToolChanged {
                    role,
                    path: PathBuf::from(&self.path),
                })
            } else {
                verify_open_tool_path(&file, &canonical, role, metadata.len(), &revision)
            }
        }
    }
}

fn open_tool_file(path: &Path, role: &'static str) -> Result<File, TerminalToolchainError> {
    let linked = std::fs::symlink_metadata(path)
        .map_err(|source| TerminalToolchainError::io("inspect terminal tool path", path, source))?;
    if !linked.file_type().is_file() {
        return Err(TerminalToolchainError::InvalidTool {
            path: path.to_path_buf(),
            detail: "expected a non-symlink regular file",
        });
    }
    let linked_revision = ToolRevision::from_metadata(&linked, path)?;

    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags, open};
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            TerminalToolchainError::io("open terminal tool", path, std::io::Error::from(error))
        })?;
        File::from(descriptor)
    };
    #[cfg(not(unix))]
    let file = File::open(path)
        .map_err(|source| TerminalToolchainError::io("open terminal tool", path, source))?;

    let opened = file
        .metadata()
        .map_err(|source| TerminalToolchainError::io("inspect open terminal tool", path, source))?;
    let opened_revision = ToolRevision::from_metadata(&opened, path)?;
    if !opened.file_type().is_file()
        || opened.len() != linked.len()
        || opened_revision != linked_revision
    {
        return Err(TerminalToolchainError::ToolChanged {
            role,
            path: path.to_path_buf(),
        });
    }
    Ok(file)
}

fn verify_open_tool_path(
    file: &File,
    path: &Path,
    role: &'static str,
    expected_size: u64,
    expected_revision: &ToolRevision,
) -> Result<(), TerminalToolchainError> {
    let linked = std::fs::symlink_metadata(path).map_err(|source| {
        TerminalToolchainError::io("reinspect terminal tool path", path, source)
    })?;
    let opened = file.metadata().map_err(|source| {
        TerminalToolchainError::io("reinspect open terminal tool", path, source)
    })?;
    let linked_revision = ToolRevision::from_metadata(&linked, path)?;
    let opened_revision = ToolRevision::from_metadata(&opened, path)?;
    if !linked.file_type().is_file()
        || !opened.file_type().is_file()
        || linked.len() != expected_size
        || opened.len() != expected_size
        || &linked_revision != expected_revision
        || &opened_revision != expected_revision
    {
        Err(TerminalToolchainError::ToolChanged {
            role,
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

impl ToolRevision {
    fn from_metadata(
        metadata: &std::fs::Metadata,
        path: &Path,
    ) -> Result<Self, TerminalToolchainError> {
        let modified = metadata.modified().map_err(|source| {
            TerminalToolchainError::io("read terminal tool modification time", path, source)
        })?;
        let (modified_before_epoch, modified_seconds, modified_nanoseconds) =
            match modified.duration_since(std::time::UNIX_EPOCH) {
                Ok(duration) => (false, duration.as_secs(), duration.subsec_nanos()),
                Err(error) => {
                    let duration = error.duration();
                    (true, duration.as_secs(), duration.subsec_nanos())
                }
            };
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;
        Ok(Self {
            readonly: metadata.permissions().readonly(),
            modified_before_epoch,
            modified_seconds,
            modified_nanoseconds,
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }

    fn is_canonical(&self) -> bool {
        self.modified_nanoseconds < 1_000_000_000 && {
            #[cfg(unix)]
            {
                self.changed_nanoseconds >= 0 && self.changed_nanoseconds < 1_000_000_000
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
    }
}

impl PlatformLinkContract {
    fn admit(root: &Path) -> Result<Self, TerminalToolchainError> {
        let root = std::fs::canonicalize(root).map_err(|source| {
            TerminalToolchainError::io("canonicalize platform SDK", root, source)
        })?;
        let mut hasher = Sha256::new();
        for relative in ["SDKSettings.json", "SDKSettings.plist"] {
            let path = root.join(relative);
            let bytes = std::fs::read(&path).map_err(|source| {
                TerminalToolchainError::io("hash platform SDK settings", &path, source)
            })?;
            hasher.update(
                u64::try_from(relative.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            hasher.update(relative.as_bytes());
            hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
            hasher.update(bytes);
        }
        Ok(Self {
            root: utf8_path(&root)?,
            settings_hash: hex(hasher.finalize().into()),
        })
    }

    fn is_canonical(&self) -> bool {
        Path::new(&self.root).is_absolute() && is_sha256(&self.settings_hash)
    }
}

impl TerminalToolchainError {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl Display for TerminalToolchainError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "failed to {action} {}: {source}", path.display()),
            Self::InvalidTool { path, detail } => {
                write!(
                    formatter,
                    "invalid terminal tool {}: {detail}",
                    path.display()
                )
            }
            Self::ToolChanged { role, path } => {
                write!(
                    formatter,
                    "{role} changed after admission: {}",
                    path.display()
                )
            }
            Self::RustcSnapshotChanged {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "terminal rustc {} changed during selection: expected snapshot {expected}, found {actual}",
                path.display()
            ),
            Self::TargetLibdirSnapshotChanged {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "terminal target-libdir {} changed after compatibility admission: expected snapshot {expected}, found {actual}",
                path.display()
            ),
            Self::LinkerResolution(detail) => formatter.write_str(detail),
            Self::InvalidProbeOutput { command, output } => {
                write!(formatter, "{command} returned invalid output {output:?}")
            }
            Self::NonUtf8Path(path) => {
                write!(
                    formatter,
                    "terminal path is not valid UTF-8: {}",
                    path.display()
                )
            }
            Self::InvalidDescriptor(detail) => {
                write!(formatter, "invalid terminal toolchain descriptor: {detail}")
            }
        }
    }
}

impl std::error::Error for TerminalToolchainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn verify_rustc_snapshot(rustc_path: &Path, expected: &str) -> Result<(), TerminalToolchainError> {
    let actual = crate::runtime_link::rustc_snapshot_identity(rustc_path).map_err(|detail| {
        TerminalToolchainError::LinkerResolution(format!(
            "failed to inspect selected rustc {}: {detail}",
            rustc_path.display()
        ))
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(TerminalToolchainError::RustcSnapshotChanged {
            path: rustc_path.to_path_buf(),
            expected: expected.to_string(),
            actual,
        })
    }
}

fn target_libdir_snapshot_identity(path: &Path) -> Result<String, TerminalToolchainError> {
    crate::runtime_link::target_libdir_snapshot_identity(path).map_err(|detail| {
        TerminalToolchainError::LinkerResolution(format!(
            "failed to inspect target-libdir {}: {detail}",
            path.display()
        ))
    })
}

fn resolve_linker(target: &str) -> Result<PathBuf, TerminalToolchainError> {
    if target.contains("-apple-") && !is_apple_darwin_target(target) {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "unsupported Apple terminal target {target}; only *-apple-darwin is admitted"
        )));
    }
    if is_apple_darwin_target(target) {
        #[cfg(not(target_os = "macos"))]
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "cannot select a Darwin linker for {target} on this host"
        )));
        #[cfg(target_os = "macos")]
        return command_path(
            Path::new("/usr/bin/xcrun"),
            &["--find", "clang"],
            "/usr/bin/xcrun --find clang",
        );
    }
    if target.contains("-windows-") && !is_windows_msvc_target(target) {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "unsupported Windows terminal target {target}; only *-pc-windows-msvc is admitted"
        )));
    }
    if is_windows_msvc_target(target) {
        #[cfg(not(windows))]
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "cannot select an MSVC linker for {target} on this host"
        )));
    } else if !is_supported_cc_target(target) {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "unsupported native terminal target {target}"
        )));
    }
    #[cfg(windows)]
    if !is_windows_msvc_target(target) {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "cannot select a native Windows linker for terminal target {target}"
        )));
    }
    #[cfg(windows)]
    let name = "link.exe";
    #[cfg(not(windows))]
    let name = "cc";
    let search_path = std::env::var_os("PATH").ok_or_else(|| {
        TerminalToolchainError::LinkerResolution(format!(
            "cannot resolve terminal linker {name}: PATH is unset"
        ))
    })?;
    for directory in std::env::split_paths(&search_path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(TerminalToolchainError::LinkerResolution(format!(
        "cannot resolve terminal linker {name} from PATH"
    )))
}

fn is_apple_darwin_target(target: &str) -> bool {
    target.ends_with("-apple-darwin")
}

fn is_windows_msvc_target(target: &str) -> bool {
    target.ends_with("-pc-windows-msvc")
}

fn is_supported_cc_target(target: &str) -> bool {
    [
        "-unknown-linux-gnu",
        "-unknown-linux-musl",
        "-unknown-freebsd",
        "-unknown-netbsd",
        "-unknown-openbsd",
        "-unknown-dragonfly",
    ]
    .iter()
    .any(|suffix| target.ends_with(suffix))
}

fn apple_link_contract() -> Result<PlatformLinkContract, TerminalToolchainError> {
    let root = command_path(
        Path::new("/usr/bin/xcrun"),
        &["--sdk", "macosx", "--show-sdk-path"],
        "/usr/bin/xcrun --sdk macosx --show-sdk-path",
    )?;
    PlatformLinkContract::admit(&root)
}

fn deployment_target(
    rustc: &ToolArtifact,
    target: &str,
) -> Result<Option<(String, String)>, TerminalToolchainError> {
    let output = Command::new(&rustc.path)
        .env_clear()
        .args(["--target", target, "--print", "deployment-target"])
        .output()
        .map_err(|source| {
            TerminalToolchainError::io(
                "query rustc deployment target",
                Path::new(&rustc.path),
                source,
            )
        })?;
    if !output.status.success() {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "{} --target {target} --print deployment-target failed with {}: {}",
            rustc.path,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let value = String::from_utf8(output.stdout).map_err(|error| {
        TerminalToolchainError::InvalidProbeOutput {
            command: format!("{} --target {target} --print deployment-target", rustc.path),
            output: String::from_utf8_lossy(error.as_bytes()).into_owned(),
        }
    })?;
    let value = value.strip_suffix('\n').unwrap_or(&value);
    if value.is_empty() {
        return Ok(None);
    }
    if value.contains(['\n', '\r', '\0']) {
        return Err(TerminalToolchainError::InvalidProbeOutput {
            command: format!("{} --target {target} --print deployment-target", rustc.path),
            output: value.to_string(),
        });
    }
    let Some((name, value)) = value.split_once('=') else {
        return Err(TerminalToolchainError::InvalidProbeOutput {
            command: format!("{} --target {target} --print deployment-target", rustc.path),
            output: value.to_string(),
        });
    };
    if name.is_empty()
        || value.is_empty()
        || !canonical_environment_name(name)
        || !canonical_deployment_target_value(value)
    {
        return Err(TerminalToolchainError::InvalidProbeOutput {
            command: format!("{} --target {target} --print deployment-target", rustc.path),
            output: format!("{name}={value}"),
        });
    }
    Ok(Some((name.to_string(), value.to_string())))
}

fn canonical_deployment_target_value(value: &str) -> bool {
    let components = value.split('.').collect::<Vec<_>>();
    (2..=3).contains(&components.len())
        && components.iter().all(|component| {
            !component.is_empty()
                && component.len() <= 3
                && component.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn command_path(
    program: &Path,
    arguments: &[&str],
    description: &str,
) -> Result<PathBuf, TerminalToolchainError> {
    let output = Command::new(program)
        .env_clear()
        .args(arguments)
        .output()
        .map_err(|source| TerminalToolchainError::io("run terminal tool probe", program, source))?;
    if !output.status.success() {
        return Err(TerminalToolchainError::LinkerResolution(format!(
            "{description} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let value = String::from_utf8(output.stdout).map_err(|error| {
        TerminalToolchainError::InvalidProbeOutput {
            command: description.to_string(),
            output: String::from_utf8_lossy(error.as_bytes()).into_owned(),
        }
    })?;
    let value = value.trim();
    let path = PathBuf::from(value);
    if value.is_empty() || value.contains(['\n', '\r', '\0']) || !path.is_absolute() {
        return Err(TerminalToolchainError::InvalidProbeOutput {
            command: description.to_string(),
            output: value.to_string(),
        });
    }
    Ok(path)
}

fn canonical_environment_name(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn utf8_path(path: &Path) -> Result<String, TerminalToolchainError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| TerminalToolchainError::NonUtf8Path(path.to_path_buf()))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
