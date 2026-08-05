//! Descriptor-relative publication for Unix hosts.
//!
//! All mutable operations below the caller-owned cache root use `*at`
//! syscalls against already-open directory descriptors. `O_NOFOLLOW` protects
//! every opened node, and publication is one atomic `renameat`.

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::fd::{AsFd as _, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, Stat, fstat, fsync, mkdirat, open, openat, renameat, statat,
    unlinkat,
};

use super::super::{EmbeddedRuntimeArtifact, RuntimeArtifactError};
use crate::artifact::RUNTIME_CRATE_NAME;

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const DIRECTORY_MODE: Mode = Mode::from_raw_mode(0o755);
const FILE_MODE: Mode = Mode::from_raw_mode(0o600);
const NO_MODE: Mode = Mode::empty();
const LOCK_NAME: &str = ".materialize.lock";

pub(super) fn verify_materialized(
    artifact: &EmbeddedRuntimeArtifact,
    path: &Path,
) -> Result<bool, RuntimeArtifactError> {
    let expected = regular_stat_at(rustix::fs::CWD, path, path)?
        .ok_or_else(|| missing_node_error("open runtime artifact", path))?;
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|source| io_error("open runtime artifact", path, source))?;
    let opened = require_open_regular(&descriptor, path)?;
    if !same_node(&expected, &opened) {
        return Err(unsafe_node(path, "the same regular file"));
    }
    verify_file_payload(artifact, File::from(descriptor), path)
}

pub(super) fn materialize(
    artifact: &EmbeddedRuntimeArtifact,
    cache_root: &Path,
) -> Result<PathBuf, RuntimeArtifactError> {
    let cache = open_or_create_cache_root(cache_root)?;
    let artifact_name = artifact.manifest().identity().to_string();
    let artifact_path = cache_root.join(&artifact_name);
    let directory = open_or_create_artifact_directory(&cache, &artifact_name, &artifact_path)?;

    let lock_path = artifact_path.join(LOCK_NAME);
    let lock_file = open_or_create_regular(&directory, LOCK_NAME, &lock_path)?;
    lock_file.lock().map_err(|source| {
        RuntimeArtifactError::io("lock runtime artifact directory", &lock_path, source)
    })?;

    let destination_name = format!("lib{RUNTIME_CRATE_NAME}.rlib");
    let destination = artifact_path.join(&destination_name);
    if verify_optional_at(artifact, &directory, &destination_name, &destination)? == Some(true) {
        validate_directory_links(
            cache_root,
            &cache,
            &artifact_name,
            &directory,
            &artifact_path,
        )?;
        return Ok(destination);
    }

    let mut temporary = create_temporary_file(&directory, &artifact_path)?;
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
    validate_temporary_path(&directory, &temporary)?;

    // Another well-behaved process cannot pass the lock while this process is
    // publishing, but the re-check makes recovery safe if a prior lock owner
    // completed publication before this descriptor acquired the lock.
    if verify_optional_at(artifact, &directory, &destination_name, &destination)? == Some(true) {
        validate_directory_links(
            cache_root,
            &cache,
            &artifact_name,
            &directory,
            &artifact_path,
        )?;
        return Ok(destination);
    }

    renameat(
        &directory,
        temporary.name.as_str(),
        &directory,
        destination_name.as_str(),
    )
    .map_err(|source| io_error("publish runtime artifact", &destination, source))?;
    temporary.cleanup.disarm();

    let published = regular_stat_at(&directory, &destination_name, &destination)?
        .ok_or_else(|| missing_node_error("verify published runtime artifact", &destination))?;
    if !same_node(&temporary.identity, &published) {
        return Err(unsafe_node(
            &destination,
            "the atomically published regular file",
        ));
    }
    if verify_optional_at(artifact, &directory, &destination_name, &destination)? != Some(true) {
        return Err(RuntimeArtifactError::PublishedPayloadMismatch(destination));
    }
    fsync(&directory)
        .map_err(|source| io_error("sync runtime artifact directory", &artifact_path, source))?;
    validate_directory_links(
        cache_root,
        &cache,
        &artifact_name,
        &directory,
        &artifact_path,
    )?;
    Ok(destination)
}

fn open_or_create_cache_root(cache_root: &Path) -> Result<OwnedFd, RuntimeArtifactError> {
    std::fs::create_dir_all(cache_root).map_err(|source| {
        RuntimeArtifactError::io("create runtime cache root", cache_root, source)
    })?;
    require_path_directory(cache_root)?;
    let descriptor = open(
        cache_root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|source| io_error("open runtime cache root", cache_root, source))?;
    require_open_directory(&descriptor, cache_root)?;
    Ok(descriptor)
}

fn open_or_create_artifact_directory(
    cache: &OwnedFd,
    name: &str,
    path: &Path,
) -> Result<OwnedFd, RuntimeArtifactError> {
    match mkdirat(cache, name, DIRECTORY_MODE) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(source) => {
            return Err(io_error("create runtime artifact directory", path, source));
        }
    }
    let expected = directory_stat_at(cache, name, path)?;
    let descriptor = openat(
        cache,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|source| io_error("open runtime artifact directory", path, source))?;
    let opened = require_open_directory(&descriptor, path)?;
    if !same_node(&expected, &opened) {
        return Err(unsafe_node(path, "the same real directory"));
    }
    Ok(descriptor)
}

fn open_or_create_regular(
    directory: &OwnedFd,
    name: &str,
    path: &Path,
) -> Result<File, RuntimeArtifactError> {
    for _ in 0..8 {
        match regular_stat_at(directory, name, path)? {
            Some(expected) => {
                let descriptor = openat(
                    directory,
                    name,
                    OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    NO_MODE,
                )
                .map_err(|source| io_error("open runtime artifact lock", path, source))?;
                let opened = require_open_regular(&descriptor, path)?;
                if !same_node(&expected, &opened) {
                    return Err(unsafe_node(path, "the same regular lock file"));
                }
                return Ok(File::from(descriptor));
            }
            None => match openat(
                directory,
                name,
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::EXCL
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                FILE_MODE,
            ) {
                Ok(descriptor) => {
                    require_open_regular(&descriptor, path)?;
                    return Ok(File::from(descriptor));
                }
                Err(rustix::io::Errno::EXIST) => {}
                Err(source) => {
                    return Err(io_error("create runtime artifact lock", path, source));
                }
            },
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

fn verify_optional_at(
    artifact: &EmbeddedRuntimeArtifact,
    directory: &OwnedFd,
    name: &str,
    path: &Path,
) -> Result<Option<bool>, RuntimeArtifactError> {
    let Some(expected) = regular_stat_at(directory, name, path)? else {
        return Ok(None);
    };
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|source| io_error("open runtime artifact", path, source))?;
    let opened = require_open_regular(&descriptor, path)?;
    if !same_node(&expected, &opened) {
        return Err(unsafe_node(path, "the same regular runtime artifact"));
    }
    verify_file_payload(artifact, File::from(descriptor), path).map(Some)
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

struct TemporaryArtifact<'a> {
    name: String,
    path: PathBuf,
    file: File,
    identity: Stat,
    cleanup: TemporaryCleanup<'a>,
}

struct TemporaryCleanup<'a> {
    directory: BorrowedFd<'a>,
    name: String,
    armed: bool,
}

impl TemporaryCleanup<'_> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

fn create_temporary_file<'a>(
    directory: &'a OwnedFd,
    display_directory: &Path,
) -> Result<TemporaryArtifact<'a>, RuntimeArtifactError> {
    for _ in 0..64 {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            ".lib{RUNTIME_CRATE_NAME}.rlib.tmp-{}-{sequence}",
            std::process::id()
        );
        let path = display_directory.join(&name);
        match openat(
            directory,
            name.as_str(),
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            FILE_MODE,
        ) {
            Ok(descriptor) => {
                let identity = require_open_regular(&descriptor, &path)?;
                return Ok(TemporaryArtifact {
                    cleanup: TemporaryCleanup {
                        directory: directory.as_fd(),
                        name: name.clone(),
                        armed: true,
                    },
                    name,
                    path,
                    file: File::from(descriptor),
                    identity,
                });
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(source) => {
                return Err(io_error("create temporary runtime artifact", &path, source));
            }
        }
    }
    let path = display_directory.join(format!(".lib{RUNTIME_CRATE_NAME}.rlib.tmp"));
    Err(RuntimeArtifactError::io(
        "create temporary runtime artifact",
        &path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename sequence exhausted",
        ),
    ))
}

fn validate_temporary_path(
    directory: &OwnedFd,
    temporary: &TemporaryArtifact<'_>,
) -> Result<(), RuntimeArtifactError> {
    let linked = regular_stat_at(directory, temporary.name.as_str(), temporary.path.as_path())?
        .ok_or_else(|| {
            missing_node_error(
                "validate temporary runtime artifact",
                temporary.path.as_path(),
            )
        })?;
    if same_node(&temporary.identity, &linked) {
        Ok(())
    } else {
        Err(unsafe_node(
            &temporary.path,
            "the same regular temporary file",
        ))
    }
}

fn validate_directory_links(
    cache_root: &Path,
    cache: &OwnedFd,
    artifact_name: &str,
    directory: &OwnedFd,
    artifact_path: &Path,
) -> Result<(), RuntimeArtifactError> {
    let current_cache = open(
        cache_root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|source| io_error("revalidate runtime cache root", cache_root, source))?;
    let current_cache = require_open_directory(&current_cache, cache_root)?;
    let opened_cache = require_open_directory(cache, cache_root)?;
    if !same_node(&current_cache, &opened_cache) {
        return Err(unsafe_node(cache_root, "the original real cache directory"));
    }

    let linked = directory_stat_at(cache, artifact_name, artifact_path)?;
    let opened = require_open_directory(directory, artifact_path)?;
    if !same_node(&linked, &opened) {
        return Err(unsafe_node(
            artifact_path,
            "the original real artifact directory",
        ));
    }
    Ok(())
}

fn require_path_directory(path: &Path) -> Result<(), RuntimeArtifactError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|source| RuntimeArtifactError::io("inspect runtime cache root", path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unsafe_node(path, "a real directory"));
    }
    Ok(())
}

fn directory_stat_at<Fd: std::os::fd::AsFd>(
    directory: Fd,
    name: impl rustix::path::Arg,
    path: &Path,
) -> Result<Stat, RuntimeArtifactError> {
    let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|source| io_error("inspect runtime cache directory", path, source))?;
    if FileType::from_raw_mode(stat.st_mode).is_dir() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a real directory"))
    }
}

fn regular_stat_at<Fd: std::os::fd::AsFd>(
    directory: Fd,
    name: impl rustix::path::Arg,
    path: &Path,
) -> Result<Option<Stat>, RuntimeArtifactError> {
    match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode).is_file() => Ok(Some(stat)),
        Ok(_) => Err(unsafe_node(path, "a regular file")),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(source) => Err(io_error("inspect runtime cache file", path, source)),
    }
}

fn require_open_directory(descriptor: &OwnedFd, path: &Path) -> Result<Stat, RuntimeArtifactError> {
    let stat = fstat(descriptor)
        .map_err(|source| io_error("inspect open runtime cache directory", path, source))?;
    if FileType::from_raw_mode(stat.st_mode).is_dir() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a real directory"))
    }
}

fn require_open_regular(descriptor: &OwnedFd, path: &Path) -> Result<Stat, RuntimeArtifactError> {
    let stat = fstat(descriptor)
        .map_err(|source| io_error("inspect open runtime cache file", path, source))?;
    if FileType::from_raw_mode(stat.st_mode).is_file() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a regular file"))
    }
}

fn same_node(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn io_error(action: &'static str, path: &Path, source: rustix::io::Errno) -> RuntimeArtifactError {
    RuntimeArtifactError::io(action, path, std::io::Error::from(source))
}

fn missing_node_error(action: &'static str, path: &Path) -> RuntimeArtifactError {
    RuntimeArtifactError::io(
        action,
        path,
        std::io::Error::new(std::io::ErrorKind::NotFound, "filesystem node disappeared"),
    )
}

fn unsafe_node(path: &Path, expected: &'static str) -> RuntimeArtifactError {
    RuntimeArtifactError::UnsafeFilesystemNode {
        path: path.to_path_buf(),
        expected,
    }
}
