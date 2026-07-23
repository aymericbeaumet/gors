//! Portable no-follow publication for non-Unix native hosts.
//!
//! Windows cannot atomically replace an existing file with `std::fs::rename`,
//! so a corrupt regular destination is removed while the per-artifact lock is
//! held. Every path is revalidated as a non-reparse regular file immediately
//! before use; a suspicious node fails closed.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::{EmbeddedRuntimeArtifact, RuntimeArtifactError};
use crate::artifact::RUNTIME_CRATE_NAME;

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const LOCK_NAME: &str = ".materialize.lock";

pub(super) fn verify_materialized(
    artifact: &EmbeddedRuntimeArtifact,
    path: &Path,
) -> Result<bool, RuntimeArtifactError> {
    require_regular_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    let file = options
        .open(path)
        .map_err(|source| RuntimeArtifactError::io("open runtime artifact", path, source))?;
    validate_open_regular(&file, path)?;
    verify_file_payload(artifact, file, path)
}

pub(super) fn materialize(
    artifact: &EmbeddedRuntimeArtifact,
    cache_root: &Path,
) -> Result<PathBuf, RuntimeArtifactError> {
    std::fs::create_dir_all(cache_root).map_err(|source| {
        RuntimeArtifactError::io("create runtime cache root", cache_root, source)
    })?;
    require_real_directory(cache_root)?;

    let artifact_path = cache_root.join(artifact.manifest().identity().to_string());
    match std::fs::create_dir(&artifact_path) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(RuntimeArtifactError::io(
                "create runtime artifact directory",
                &artifact_path,
                source,
            ));
        }
    }
    require_real_directory(&artifact_path)?;

    let lock_path = artifact_path.join(LOCK_NAME);
    let lock_file = open_or_create_regular(&lock_path)?;
    lock_file.lock().map_err(|source| {
        RuntimeArtifactError::io("lock runtime artifact directory", &lock_path, source)
    })?;

    let destination = artifact_path.join(format!("lib{RUNTIME_CRATE_NAME}.rlib"));
    if verify_optional(artifact, &destination)? == Some(true) {
        validate_directory_paths(cache_root, &artifact_path)?;
        return Ok(destination);
    }

    let mut temporary = create_temporary_file(&artifact_path)?;
    if let Err(source) = temporary.file.write_all(artifact.payload()) {
        return Err(RuntimeArtifactError::io(
            "write temporary runtime artifact",
            &temporary.path,
            source,
        ));
    }
    if let Err(source) = temporary.file.sync_all() {
        return Err(RuntimeArtifactError::io(
            "sync temporary runtime artifact",
            &temporary.path,
            source,
        ));
    }
    require_regular_path(&temporary.path)?;

    if verify_optional(artifact, &destination)? == Some(true) {
        validate_directory_paths(cache_root, &artifact_path)?;
        return Ok(destination);
    }

    if regular_path_state(&destination)?.is_some() {
        std::fs::remove_file(&destination).map_err(|source| {
            RuntimeArtifactError::io("replace corrupt runtime artifact", &destination, source)
        })?;
    }
    if let Err(source) = std::fs::rename(&temporary.path, &destination) {
        if verify_optional(artifact, &destination)? == Some(true) {
            validate_directory_paths(cache_root, &artifact_path)?;
            return Ok(destination);
        }
        return Err(RuntimeArtifactError::io(
            "publish runtime artifact",
            &destination,
            source,
        ));
    }
    temporary.cleanup.disarm();

    if verify_optional(artifact, &destination)? != Some(true) {
        return Err(RuntimeArtifactError::PublishedPayloadMismatch(destination));
    }
    validate_directory_paths(cache_root, &artifact_path)?;
    Ok(destination)
}

fn verify_optional(
    artifact: &EmbeddedRuntimeArtifact,
    path: &Path,
) -> Result<Option<bool>, RuntimeArtifactError> {
    if regular_path_state(path)?.is_none() {
        return Ok(None);
    }
    verify_materialized(artifact, path).map(Some)
}

fn verify_file_payload(
    artifact: &EmbeddedRuntimeArtifact,
    mut file: File,
    path: &Path,
) -> Result<bool, RuntimeArtifactError> {
    let mut payload = Vec::new();
    file.read_to_end(&mut payload)
        .map_err(|source| RuntimeArtifactError::io("read runtime artifact", path, source))?;
    Ok(artifact.manifest().verifies_payload(&payload))
}

fn open_or_create_regular(path: &Path) -> Result<File, RuntimeArtifactError> {
    for _ in 0..8 {
        match regular_path_state(path)? {
            Some(_) => {
                let mut options = OpenOptions::new();
                options.read(true).write(true);
                configure_no_follow(&mut options);
                let file = options.open(path).map_err(|source| {
                    RuntimeArtifactError::io("open runtime artifact lock", path, source)
                })?;
                validate_open_regular(&file, path)?;
                return Ok(file);
            }
            None => {
                let mut options = OpenOptions::new();
                options.create_new(true).read(true).write(true);
                configure_no_follow(&mut options);
                match options.open(path) {
                    Ok(file) => {
                        validate_open_regular(&file, path)?;
                        return Ok(file);
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(source) => {
                        return Err(RuntimeArtifactError::io(
                            "create runtime artifact lock",
                            path,
                            source,
                        ));
                    }
                }
            }
        }
    }
    Err(RuntimeArtifactError::io(
        "create runtime artifact lock",
        path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "runtime artifact lock changed repeatedly during admission",
        ),
    ))
}

struct TemporaryArtifact {
    path: PathBuf,
    file: File,
    cleanup: TemporaryCleanup,
}

struct TemporaryCleanup {
    path: PathBuf,
    armed: bool,
}

impl TemporaryCleanup {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryCleanup {
    fn drop(&mut self) {
        if self.armed {
            drop(std::fs::remove_file(&self.path));
        }
    }
}

fn create_temporary_file(directory: &Path) -> Result<TemporaryArtifact, RuntimeArtifactError> {
    for _ in 0..64 {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".lib{RUNTIME_CRATE_NAME}.rlib.tmp-{}-{sequence}",
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        configure_no_follow(&mut options);
        match options.open(&path) {
            Ok(file) => {
                validate_open_regular(&file, &path)?;
                return Ok(TemporaryArtifact {
                    cleanup: TemporaryCleanup {
                        path: path.clone(),
                        armed: true,
                    },
                    path,
                    file,
                });
            }
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(RuntimeArtifactError::io(
                    "create temporary runtime artifact",
                    &path,
                    source,
                ));
            }
        }
    }
    let path = directory.join(format!(".lib{RUNTIME_CRATE_NAME}.rlib.tmp"));
    Err(RuntimeArtifactError::io(
        "create temporary runtime artifact",
        &path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename sequence exhausted",
        ),
    ))
}

fn validate_directory_paths(
    cache_root: &Path,
    artifact_path: &Path,
) -> Result<(), RuntimeArtifactError> {
    require_real_directory(cache_root)?;
    require_real_directory(artifact_path)
}

fn require_real_directory(path: &Path) -> Result<(), RuntimeArtifactError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| {
        RuntimeArtifactError::io("inspect runtime cache directory", path, source)
    })?;
    if is_link_like(&metadata) || !metadata.is_dir() {
        return Err(unsafe_node(path, "a real directory"));
    }
    Ok(())
}

fn require_regular_path(path: &Path) -> Result<Metadata, RuntimeArtifactError> {
    regular_path_state(path)?.ok_or_else(|| {
        RuntimeArtifactError::io(
            "inspect runtime cache file",
            path,
            std::io::Error::new(std::io::ErrorKind::NotFound, "filesystem node disappeared"),
        )
    })
}

fn regular_path_state(path: &Path) -> Result<Option<Metadata>, RuntimeArtifactError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !is_link_like(&metadata) && metadata.is_file() => Ok(Some(metadata)),
        Ok(_) => Err(unsafe_node(path, "a regular file")),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(RuntimeArtifactError::io(
            "inspect runtime cache file",
            path,
            source,
        )),
    }
}

fn validate_open_regular(file: &File, path: &Path) -> Result<(), RuntimeArtifactError> {
    let metadata = file.metadata().map_err(|source| {
        RuntimeArtifactError::io("inspect open runtime cache file", path, source)
    })?;
    if is_link_like(&metadata) || !metadata.is_file() {
        return Err(unsafe_node(path, "a regular file"));
    }
    require_regular_path(path)?;
    Ok(())
}

#[cfg(windows)]
fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt as _;

    // Win32 FILE_FLAG_OPEN_REPARSE_POINT. This opens a reparse point itself
    // instead of its target, allowing `validate_open_regular` to reject it.
    options.custom_flags(0x0020_0000);
}

#[cfg(not(windows))]
fn configure_no_follow(_options: &mut OpenOptions) {}

fn is_link_like(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;

        // Win32 FILE_ATTRIBUTE_REPARSE_POINT. Reject junctions and every
        // other reparse-backed redirect, not only ordinary symlinks.
        return metadata.file_attributes() & 0x0000_0400 != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn unsafe_node(path: &Path, expected: &'static str) -> RuntimeArtifactError {
    RuntimeArtifactError::UnsafeFilesystemNode {
        path: path.to_path_buf(),
        expected,
    }
}
