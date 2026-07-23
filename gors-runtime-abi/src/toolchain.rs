//! Canonical producer recipe and target sysroot ABI for Rust rlib artifacts.

use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

use crate::encoding::CanonicalEncoder;
use crate::encoding::sha256;
use crate::identity::{CompatibilityIdentity, ProducerIdentity};
use crate::target::TargetModel;

/// Current canonical encoding schema for immutable rlib producer provenance.
pub const CURRENT_RUST_RLIB_PRODUCER_SCHEMA: u32 = 1;
/// Current canonical encoding schema for consumer rlib compatibility.
pub const CURRENT_RUST_RLIB_COMPATIBILITY_SCHEMA: u32 = 2;
/// Current canonical encoding schema for a target rustlib inventory.
pub const CURRENT_RUST_TARGET_LIBDIR_SCHEMA: u32 = 1;

/// Fixed external crate name referenced by generated Rust.
pub const RUST_RUNTIME_CRATE_NAME: &str = "__gors_runtime";
/// Exact rustup toolchain used for native provider production and consumption.
pub const NATIVE_RUNTIME_RUST_TOOLCHAIN: &str = "1.96.0";
/// Rust edition used for the runtime provider and generated programs.
pub const RUST_RUNTIME_EDITION: &str = "2024";
/// Fixed crate metadata seed used to avoid physical build-path identities.
pub const RUST_RUNTIME_METADATA: &str = "gors-runtime-v1";
/// Runtime provider optimization level.
pub const RUST_RUNTIME_OPT_LEVEL: &str = "3";
/// Runtime provider codegen-unit count.
pub const RUST_RUNTIME_CODEGEN_UNITS: &str = "1";
/// Runtime provider panic strategy required by generated unwind boundaries.
pub const RUST_RUNTIME_PANIC_STRATEGY: &str = "unwind";
/// Whether producer bitcode is retained for release-mode LTO consumers.
pub const RUST_RUNTIME_EMBED_BITCODE: &str = "yes";
/// Virtual source root embedded in the provider instead of a checkout path.
pub const RUST_RUNTIME_REMAP_ROOT: &str = "/gors-workspace";

/// Invalid exact-rustc producer record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustRlibRecordError {
    EmptyRustcVerboseVersion,
    InvalidRustcVerboseVersionUtf8,
    MissingRustcHost,
    DuplicateRustcHost,
    EmptyTargetLibdirRecord,
}

impl Display for RustRlibRecordError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRustcVerboseVersion => {
                formatter.write_str("rustc verbose version output must not be empty")
            }
            Self::InvalidRustcVerboseVersionUtf8 => {
                formatter.write_str("rustc verbose version output must be valid UTF-8")
            }
            Self::MissingRustcHost => {
                formatter.write_str("rustc verbose version output has no host record")
            }
            Self::DuplicateRustcHost => {
                formatter.write_str("rustc verbose version output has multiple host records")
            }
            Self::EmptyTargetLibdirRecord => {
                formatter.write_str("target rustlib inventory must not be empty")
            }
        }
    }
}

impl std::error::Error for RustRlibRecordError {}

/// Failure to derive a path-independent target rustlib inventory.
#[derive(Debug)]
pub enum RustTargetLibdirError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    NonUtf8Path(PathBuf),
    EscapingPath(PathBuf),
    AbsoluteSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    EscapingSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    UnsupportedFileType(PathBuf),
}

impl RustTargetLibdirError {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl Display for RustTargetLibdirError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "failed to {action} {}: {source}", path.display()),
            Self::NonUtf8Path(path) => write!(
                formatter,
                "target rustlib inventory path is not valid UTF-8: {}",
                path.display()
            ),
            Self::EscapingPath(path) => write!(
                formatter,
                "target rustlib inventory path escapes its root: {}",
                path.display()
            ),
            Self::AbsoluteSymlink { path, target } => write!(
                formatter,
                "target rustlib symlink {} has host-specific absolute target {}",
                path.display(),
                target.display()
            ),
            Self::EscapingSymlink { path, target } => write!(
                formatter,
                "target rustlib symlink {} escapes its inventory root through {}",
                path.display(),
                target.display()
            ),
            Self::UnsupportedFileType(path) => write!(
                formatter,
                "target rustlib inventory contains an unsupported file type: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RustTargetLibdirError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Immutable provenance for the compiler host, target sysroot, and recipe that
/// produced an rlib payload.
///
/// Unlike [`RustRlibCompatibility`], this record retains the `rustc -vV` host
/// and the pre-build target-libdir inventory. It is evidence only: consumers
/// must never reject a cross-produced artifact because this identity differs
/// from their local host.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RustRlibProducer {
    rustc_record: Box<[u8]>,
    target_libdir_record: Box<[u8]>,
    target: TargetModel,
}

impl RustRlibProducer {
    pub fn new(
        rustc_verbose_version: impl AsRef<[u8]>,
        target_libdir_record: impl Into<Box<[u8]>>,
        target: TargetModel,
    ) -> Result<Self, RustRlibRecordError> {
        let rustc_record = canonical_rustc_record(rustc_verbose_version.as_ref(), false)?;
        let target_libdir_record = target_libdir_record.into();
        if target_libdir_record.is_empty() {
            return Err(RustRlibRecordError::EmptyTargetLibdirRecord);
        }
        Ok(Self {
            rustc_record: rustc_record.into_boxed_slice(),
            target_libdir_record,
            target,
        })
    }

    /// Canonical complete `rustc -vV`, including the producer host.
    #[must_use]
    pub const fn rustc_record(&self) -> &[u8] {
        &self.rustc_record
    }

    /// Canonical inventory present when the payload was compiled.
    #[must_use]
    pub const fn target_libdir_record(&self) -> &[u8] {
        &self.target_libdir_record
    }

    #[must_use]
    pub const fn target(&self) -> &TargetModel {
        &self.target
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::rust_rlib_producer();
        encoder.u32(CURRENT_RUST_RLIB_PRODUCER_SCHEMA);
        encoder.bytes(&self.rustc_record);
        encoder.bytes(&self.target_libdir_record);
        self.target.encode(&mut encoder);
        encode_runtime_recipe(&mut encoder);
        encoder.finish()
    }

    #[must_use]
    pub fn identity(&self) -> ProducerIdentity {
        ProducerIdentity::sha256(&self.canonical_bytes())
    }
}

/// Exact compiler release, target sysroot ABI, and fixed recipe for one rlib.
///
/// The producer host from `rustc -vV` is deliberately excluded: an rlib
/// cross-produced on x86_64 must be consumable by the same Rust release on its
/// aarch64 target. Compatibility instead binds the target model and a canonical
/// target-libdir inventory. Hash-bearing rustlib filenames encode Rust crate
/// metadata identities; file sizes additionally detect truncated/replaced
/// distribution payloads without hashing the whole sysroot on every cold run.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RustRlibCompatibility {
    rustc_release_record: Box<[u8]>,
    target_libdir_record: Box<[u8]>,
    target: TargetModel,
}

impl RustRlibCompatibility {
    pub fn new(
        rustc_verbose_version: impl AsRef<[u8]>,
        target_libdir_record: impl Into<Box<[u8]>>,
        target: TargetModel,
    ) -> Result<Self, RustRlibRecordError> {
        let rustc_release_record = canonical_rustc_release_record(rustc_verbose_version.as_ref())?;
        let target_libdir_record = target_libdir_record.into();
        if target_libdir_record.is_empty() {
            return Err(RustRlibRecordError::EmptyTargetLibdirRecord);
        }
        Ok(Self {
            rustc_release_record: rustc_release_record.into_boxed_slice(),
            target_libdir_record,
            target,
        })
    }

    /// Canonical `rustc -vV` facts with the producer host removed.
    #[must_use]
    pub const fn rustc_release_record(&self) -> &[u8] {
        &self.rustc_release_record
    }

    /// Canonical inventory of the target rustlib directory.
    #[must_use]
    pub const fn target_libdir_record(&self) -> &[u8] {
        &self.target_libdir_record
    }

    #[must_use]
    pub const fn target(&self) -> &TargetModel {
        &self.target
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::rust_rlib_compatibility();
        encoder.u32(CURRENT_RUST_RLIB_COMPATIBILITY_SCHEMA);
        encoder.bytes(&self.rustc_release_record);
        encoder.bytes(&self.target_libdir_record);
        self.target.encode(&mut encoder);
        encode_runtime_recipe(&mut encoder);
        encoder.finish()
    }

    #[must_use]
    pub fn identity(&self) -> CompatibilityIdentity {
        CompatibilityIdentity::sha256(&self.canonical_bytes())
    }
}

/// Build the canonical, path-independent ABI inventory for a rustc target
/// library directory returned by `rustc --target ... --print target-libdir`.
pub fn canonical_target_libdir_record(root: &Path) -> Result<Vec<u8>, RustTargetLibdirError> {
    let mut entries = Vec::new();
    collect_target_libdir(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));

    let mut encoder = CanonicalEncoder::rust_target_libdir();
    encoder.u32(CURRENT_RUST_TARGET_LIBDIR_SCHEMA);
    encoder.count(entries.len());
    for entry in entries {
        encoder.text(&entry.relative);
        encoder.u8(entry.kind);
        encoder.u64(entry.size);
        encoder.text(entry.symlink_target.as_deref().unwrap_or(""));
        match entry.content_hash {
            Some(hash) => encoder.bytes(&hash),
            None => encoder.bytes(&[]),
        }
    }
    Ok(encoder.finish())
}

fn canonical_rustc_release_record(
    rustc_verbose_version: &[u8],
) -> Result<Vec<u8>, RustRlibRecordError> {
    canonical_rustc_record(rustc_verbose_version, true)
}

fn canonical_rustc_record(
    rustc_verbose_version: &[u8],
    omit_host: bool,
) -> Result<Vec<u8>, RustRlibRecordError> {
    if rustc_verbose_version.is_empty() {
        return Err(RustRlibRecordError::EmptyRustcVerboseVersion);
    }
    let source = std::str::from_utf8(rustc_verbose_version)
        .map_err(|_| RustRlibRecordError::InvalidRustcVerboseVersionUtf8)?;
    let mut host_count = 0_u8;
    let mut canonical = String::new();
    for line in source.lines() {
        if let Some(host) = line.strip_prefix("host: ") {
            if host.is_empty() {
                return Err(RustRlibRecordError::MissingRustcHost);
            }
            host_count = host_count.saturating_add(1);
            if omit_host {
                continue;
            }
        }
        canonical.push_str(line);
        canonical.push('\n');
    }
    match host_count {
        0 => Err(RustRlibRecordError::MissingRustcHost),
        1 => Ok(canonical.into_bytes()),
        _ => Err(RustRlibRecordError::DuplicateRustcHost),
    }
}

fn encode_runtime_recipe(encoder: &mut CanonicalEncoder) {
    encoder.text(RUST_RUNTIME_CRATE_NAME);
    encoder.text("rlib");
    encoder.text(RUST_RUNTIME_EDITION);
    encoder.text(RUST_RUNTIME_METADATA);
    encoder.text(RUST_RUNTIME_OPT_LEVEL);
    encoder.text(RUST_RUNTIME_CODEGEN_UNITS);
    encoder.text(RUST_RUNTIME_PANIC_STRATEGY);
    encoder.text(RUST_RUNTIME_EMBED_BITCODE);
    encoder.text(RUST_RUNTIME_REMAP_ROOT);
}

struct TargetLibdirEntry {
    relative: String,
    kind: u8,
    size: u64,
    symlink_target: Option<String>,
    content_hash: Option<[u8; 32]>,
}

fn collect_target_libdir(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<TargetLibdirEntry>,
) -> Result<(), RustTargetLibdirError> {
    let children = std::fs::read_dir(directory).map_err(|source| {
        RustTargetLibdirError::io("read target rustlib directory", directory, source)
    })?;
    for child in children {
        let child = child.map_err(|source| {
            RustTargetLibdirError::io("read target rustlib directory entry", directory, source)
        })?;
        let path = child.path();
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_| RustTargetLibdirError::EscapingPath(path.clone()))?;
        let relative = canonical_relative_path(relative_path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|source| {
            RustTargetLibdirError::io("inspect target rustlib entry", &path, source)
        })?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            entries.push(TargetLibdirEntry {
                relative,
                kind: b'd',
                size: 0,
                symlink_target: None,
                content_hash: None,
            });
            collect_target_libdir(root, &path, entries)?;
        } else if file_type.is_file() {
            let content_hash = if has_hash_suffixed_filename(&relative) {
                None
            } else {
                Some(sha256(&std::fs::read(&path).map_err(|source| {
                    RustTargetLibdirError::io("hash target rustlib entry", &path, source)
                })?))
            };
            entries.push(TargetLibdirEntry {
                relative,
                kind: b'f',
                size: metadata.len(),
                symlink_target: None,
                content_hash,
            });
        } else if file_type.is_symlink() {
            let target = std::fs::read_link(&path).map_err(|source| {
                RustTargetLibdirError::io("read target rustlib symlink", &path, source)
            })?;
            if target.is_absolute() {
                return Err(RustTargetLibdirError::AbsoluteSymlink { path, target });
            }
            entries.push(TargetLibdirEntry {
                relative,
                kind: b'l',
                size: 0,
                symlink_target: Some(canonical_symlink_target(relative_path, &path, &target)?),
                content_hash: None,
            });
        } else {
            return Err(RustTargetLibdirError::UnsupportedFileType(path));
        }
    }
    Ok(())
}

fn has_hash_suffixed_filename(relative: &str) -> bool {
    let filename = relative.rsplit('/').next().unwrap_or(relative);
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    stem.rsplit_once('-').is_some_and(|(_, suffix)| {
        suffix.len() == 16
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn canonical_relative_path(path: &Path) -> Result<String, RustTargetLibdirError> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(
                value
                    .to_str()
                    .ok_or_else(|| RustTargetLibdirError::NonUtf8Path(path.to_path_buf()))?,
            ),
            _ => return Err(RustTargetLibdirError::EscapingPath(path.to_path_buf())),
        }
    }
    Ok(components.join("/"))
}

fn canonical_symlink_target(
    link_relative: &Path,
    link_path: &Path,
    target: &Path,
) -> Result<String, RustTargetLibdirError> {
    let mut components = Vec::new();
    let mut depth = link_relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let Some(parent_depth) = depth.checked_sub(1) else {
                    return Err(RustTargetLibdirError::EscapingSymlink {
                        path: link_path.to_path_buf(),
                        target: target.to_path_buf(),
                    });
                };
                depth = parent_depth;
                components.push("..");
            }
            Component::Normal(value) => {
                depth =
                    depth
                        .checked_add(1)
                        .ok_or_else(|| RustTargetLibdirError::EscapingSymlink {
                            path: link_path.to_path_buf(),
                            target: target.to_path_buf(),
                        })?;
                components.push(
                    value
                        .to_str()
                        .ok_or_else(|| RustTargetLibdirError::NonUtf8Path(target.to_path_buf()))?,
                );
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(RustTargetLibdirError::AbsoluteSymlink {
                    path: link_path.to_path_buf(),
                    target: target.to_path_buf(),
                });
            }
        }
    }
    Ok(components.join("/"))
}
