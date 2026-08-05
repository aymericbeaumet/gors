//! Content-free filesystem snapshots used to admit a compatibility cache hit.

use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RUSTC_SNAPSHOT_IDENTITY_DOMAIN: &[u8] = b"gors.native-rustc-snapshot-v1\0";
const TARGET_LIBDIR_SNAPSHOT_IDENTITY_DOMAIN: &[u8] = b"gors.native-target-libdir-snapshot-v1\0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RustcSnapshot {
    path: String,
    lexical: MetadataFacts,
    resolved_path: String,
    resolved: MetadataFacts,
}

impl RustcSnapshot {
    pub(super) fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub(super) fn identity(&self) -> Result<[u8; 32], serde_json::Error> {
        let encoded = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(RUSTC_SNAPSHOT_IDENTITY_DOMAIN);
        hasher.update(
            u64::try_from(encoded.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(encoded);
        Ok(hasher.finalize().into())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TreeSnapshot {
    root: MetadataFacts,
    entries: Vec<TreeEntry>,
}

impl TreeSnapshot {
    pub(super) fn identity(&self) -> Result<[u8; 32], serde_json::Error> {
        let encoded = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(TARGET_LIBDIR_SNAPSHOT_IDENTITY_DOMAIN);
        hasher.update(
            u64::try_from(encoded.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hasher.update(encoded);
        Ok(hasher.finalize().into())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TreeEntry {
    relative_path: String,
    metadata: MetadataFacts,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MetadataFacts {
    kind: EntryKind,
    size: u64,
    modified: ModifiedTime,
    readonly: bool,
    symlink_target: Option<String>,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EntryKind {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModifiedTime {
    before_unix_epoch: bool,
    seconds: u64,
    nanoseconds: u32,
}

#[derive(Debug)]
pub(in crate::runtime_link) enum SnapshotError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    NonAbsolutePath(PathBuf),
    NonUtf8Path(PathBuf),
    UnsupportedFileType(PathBuf),
    RustcNotFile(PathBuf),
    TargetRootNotDirectory(PathBuf),
    AbsoluteSymlink {
        path: PathBuf,
        target: PathBuf,
    },
    EscapingSymlink {
        path: PathBuf,
        target: PathBuf,
    },
}

impl SnapshotError {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl Display for SnapshotError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "failed to {action} {}: {source}", path.display()),
            Self::NonAbsolutePath(path) => {
                write!(
                    formatter,
                    "toolchain snapshot path is not absolute: {}",
                    path.display()
                )
            }
            Self::NonUtf8Path(path) => write!(
                formatter,
                "toolchain snapshot path is not valid UTF-8: {}",
                path.display()
            ),
            Self::UnsupportedFileType(path) => write!(
                formatter,
                "toolchain snapshot contains unsupported file type: {}",
                path.display()
            ),
            Self::RustcNotFile(path) => write!(
                formatter,
                "resolved rustc is not a regular file: {}",
                path.display()
            ),
            Self::TargetRootNotDirectory(path) => write!(
                formatter,
                "target-libdir root is not a real directory: {}",
                path.display()
            ),
            Self::AbsoluteSymlink { path, target } => write!(
                formatter,
                "target-libdir symlink {} has absolute target {}",
                path.display(),
                target.display()
            ),
            Self::EscapingSymlink { path, target } => write!(
                formatter,
                "target-libdir symlink {} escapes through {}",
                path.display(),
                target.display()
            ),
        }
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub(super) fn rustc_snapshot(path: &Path) -> Result<RustcSnapshot, SnapshotError> {
    require_absolute(path)?;
    let lexical = metadata_facts(path, true)?;
    if !matches!(lexical.kind, EntryKind::File | EntryKind::Symlink) {
        return Err(SnapshotError::RustcNotFile(path.to_path_buf()));
    }
    let resolved_path = std::fs::canonicalize(path)
        .map_err(|source| SnapshotError::io("canonicalize resolved rustc", path, source))?;
    let resolved = followed_file_facts(&resolved_path)?;
    Ok(RustcSnapshot {
        path: utf8_path(path)?,
        lexical,
        resolved_path: utf8_path(&resolved_path)?,
        resolved,
    })
}

pub(super) fn tree_snapshot(root: &Path) -> Result<TreeSnapshot, SnapshotError> {
    require_absolute(root)?;
    let root_metadata = metadata_facts(root, false)?;
    if root_metadata.kind != EntryKind::Directory {
        return Err(SnapshotError::TargetRootNotDirectory(root.to_path_buf()));
    }
    let mut entries = Vec::new();
    collect_tree(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(TreeSnapshot {
        root: root_metadata,
        entries,
    })
}

fn collect_tree(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<TreeEntry>,
) -> Result<(), SnapshotError> {
    let children = std::fs::read_dir(directory)
        .map_err(|source| SnapshotError::io("read target-libdir", directory, source))?;
    for child in children {
        let child = child
            .map_err(|source| SnapshotError::io("read target-libdir entry", directory, source))?;
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SnapshotError::NonAbsolutePath(path.clone()))?;
        let relative_path = canonical_relative_path(relative)?;
        let metadata = metadata_facts(&path, true)?;
        if metadata.kind == EntryKind::Symlink {
            let target = std::fs::read_link(&path)
                .map_err(|source| SnapshotError::io("read target-libdir symlink", &path, source))?;
            validate_symlink(relative, &path, &target)?;
        }
        let is_directory = metadata.kind == EntryKind::Directory;
        entries.push(TreeEntry {
            relative_path,
            metadata,
        });
        if is_directory {
            collect_tree(root, &path, entries)?;
        }
    }
    Ok(())
}

fn metadata_facts(path: &Path, allow_symlink: bool) -> Result<MetadataFacts, SnapshotError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|source| SnapshotError::io("inspect toolchain path", path, source))?;
    let file_type = metadata.file_type();
    let (kind, symlink_target) = if file_type.is_dir() {
        (EntryKind::Directory, None)
    } else if file_type.is_file() {
        (EntryKind::File, None)
    } else if file_type.is_symlink() && allow_symlink {
        let target = std::fs::read_link(path)
            .map_err(|source| SnapshotError::io("read toolchain symlink", path, source))?;
        (EntryKind::Symlink, Some(utf8_path(&target)?))
    } else if file_type.is_symlink() {
        return Err(SnapshotError::TargetRootNotDirectory(path.to_path_buf()));
    } else {
        return Err(SnapshotError::UnsupportedFileType(path.to_path_buf()));
    };
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt as _;
    Ok(MetadataFacts {
        kind,
        size: metadata.len(),
        modified: modified_time(metadata.modified().map_err(|source| {
            SnapshotError::io("read toolchain modification time", path, source)
        })?),
        readonly: metadata.permissions().readonly(),
        symlink_target,
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

fn followed_file_facts(path: &Path) -> Result<MetadataFacts, SnapshotError> {
    let metadata = std::fs::metadata(path)
        .map_err(|source| SnapshotError::io("inspect resolved rustc file", path, source))?;
    if !metadata.file_type().is_file() {
        return Err(SnapshotError::RustcNotFile(path.to_path_buf()));
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt as _;
    Ok(MetadataFacts {
        kind: EntryKind::File,
        size: metadata.len(),
        modified: modified_time(
            metadata.modified().map_err(|source| {
                SnapshotError::io("read rustc modification time", path, source)
            })?,
        ),
        readonly: metadata.permissions().readonly(),
        symlink_target: None,
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

fn modified_time(time: SystemTime) -> ModifiedTime {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => ModifiedTime {
            before_unix_epoch: false,
            seconds: duration.as_secs(),
            nanoseconds: duration.subsec_nanos(),
        },
        Err(error) => {
            let duration = error.duration();
            ModifiedTime {
                before_unix_epoch: true,
                seconds: duration.as_secs(),
                nanoseconds: duration.subsec_nanos(),
            }
        }
    }
}

fn require_absolute(path: &Path) -> Result<(), SnapshotError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(SnapshotError::NonAbsolutePath(path.to_path_buf()))
    }
}

fn utf8_path(path: &Path) -> Result<String, SnapshotError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| SnapshotError::NonUtf8Path(path.to_path_buf()))
}

fn canonical_relative_path(path: &Path) -> Result<String, SnapshotError> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(
                value
                    .to_str()
                    .ok_or_else(|| SnapshotError::NonUtf8Path(path.to_path_buf()))?,
            ),
            _ => return Err(SnapshotError::NonAbsolutePath(path.to_path_buf())),
        }
    }
    Ok(components.join("/"))
}

fn validate_symlink(
    link_relative: &Path,
    link_path: &Path,
    target: &Path,
) -> Result<(), SnapshotError> {
    let mut depth = link_relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| SnapshotError::EscapingSymlink {
                        path: link_path.to_path_buf(),
                        target: target.to_path_buf(),
                    })?;
            }
            Component::Normal(value) => {
                value
                    .to_str()
                    .ok_or_else(|| SnapshotError::NonUtf8Path(target.to_path_buf()))?;
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| SnapshotError::EscapingSymlink {
                        path: link_path.to_path_buf(),
                        target: target.to_path_buf(),
                    })?;
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(SnapshotError::AbsoluteSymlink {
                    path: link_path.to_path_buf(),
                    target: target.to_path_buf(),
                });
            }
        }
    }
    Ok(())
}
