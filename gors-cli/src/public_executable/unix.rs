//! Descriptor-relative public executable publication for Unix.

use super::{PublicExecutableError, PublishedExecutable, absolute_path};
use crate::rustc::ExecutableProduct;
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, Stat, fstat, fsync, open, openat, renameat, statat, unlinkat,
};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const NO_MODE: Mode = Mode::empty();
const PRIVATE_FILE_MODE: Mode = Mode::from_raw_mode(0o600);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn publish(
    source: &ExecutableProduct,
    destination: &Path,
) -> Result<PublishedExecutable, PublicExecutableError> {
    let destination = absolute_path(destination)?;
    let parent_path = destination.parent().ok_or_else(|| {
        PublicExecutableError::message(format!(
            "public executable has no parent directory: {}",
            destination.display()
        ))
    })?;
    let destination_name = destination.file_name().ok_or_else(|| {
        PublicExecutableError::message(format!(
            "public executable has no filename: {}",
            destination.display()
        ))
    })?;
    std::fs::create_dir_all(parent_path).map_err(|source_error| {
        PublicExecutableError::io(
            format!(
                "cannot create public executable directory {}",
                parent_path.display()
            ),
            source_error,
        )
    })?;
    let parent = open_real_directory(parent_path)?;
    let lock_name = sibling_lock_name(destination_name);
    let lock_path = parent_path.join(&lock_name);
    let lock = open_or_create_regular(&parent, &lock_name, &lock_path)?;
    lock.lock().map_err(|source_error| {
        PublicExecutableError::io(
            format!("cannot lock public executable {}", destination.display()),
            source_error,
        )
    })?;

    let source_fd = open(
        source.path(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|error| errno("open cached executable", source.path(), error))?;
    let source_stat = require_regular(&source_fd, source.path())?;
    if source_stat.st_size < 0
        || u64::try_from(source_stat.st_size).ok() != Some(source.size_bytes())
    {
        return Err(PublicExecutableError::message(format!(
            "cached executable size changed before publication: {}",
            source.path().display()
        )));
    }
    let expected_mode = source.mode();
    if u32::from(source_stat.st_mode & 0o7777) != expected_mode {
        return Err(PublicExecutableError::message(format!(
            "cached executable mode changed before publication: {}",
            source.path().display()
        )));
    }
    if admit_existing(
        &parent,
        destination_name,
        &destination,
        source.content_hash(),
        source.size_bytes(),
        expected_mode,
    )? {
        validate_directory_path(parent_path, &parent)?;
        return Ok(PublishedExecutable {
            path: destination,
            content_hash: source.content_hash().to_string(),
            size_bytes: source.size_bytes(),
        });
    }

    let mut source_file = File::from(source_fd);
    let mut temporary = create_temporary(&parent, parent_path, destination_name)?;
    let (copied_hash, copied_size) = copy_and_hash(&mut source_file, &mut temporary.file)?;
    let source_after_copy = require_regular_file(&source_file, source.path())?;
    if !same_snapshot(&source_stat, &source_after_copy) {
        return Err(PublicExecutableError::message(format!(
            "cached executable changed while copying: {}",
            source.path().display()
        )));
    }
    if copied_hash != source.content_hash() || copied_size != source.size_bytes() {
        return Err(PublicExecutableError::message(format!(
            "cached executable changed while copying: {}",
            source.path().display()
        )));
    }
    temporary
        .file
        .set_permissions(std::fs::Permissions::from_mode(expected_mode))
        .map_err(|source_error| {
            PublicExecutableError::io("cannot apply executable mode to temporary", source_error)
        })?;
    temporary.file.flush().map_err(|source_error| {
        PublicExecutableError::io("cannot flush temporary executable", source_error)
    })?;
    temporary.file.sync_all().map_err(|source_error| {
        PublicExecutableError::io("cannot sync temporary executable", source_error)
    })?;
    let temporary_stat = require_regular_file(&temporary.file, &temporary.path)?;
    let linked_temporary = regular_stat_at(&parent, &temporary.name, &temporary.path)?;
    if !same_node(&temporary_stat, &linked_temporary)
        || u32::from(temporary_stat.st_mode & 0o7777) != expected_mode
    {
        return Err(unsafe_node(
            &temporary.path,
            "the original regular temporary executable",
        ));
    }

    renameat(&parent, &temporary.name, &parent, destination_name)
        .map_err(|error| errno("publish executable", &destination, error))?;
    temporary.cleanup.disarm();
    let published_stat = regular_stat_at(&parent, destination_name, &destination)?;
    if !same_node(&temporary_stat, &published_stat)
        || u32::from(published_stat.st_mode & 0o7777) != expected_mode
    {
        return Err(unsafe_node(
            &destination,
            "the atomically published executable",
        ));
    }
    fsync(&parent).map_err(|error| errno("sync publication directory", parent_path, error))?;
    validate_directory_path(parent_path, &parent)?;

    Ok(PublishedExecutable {
        path: destination,
        content_hash: copied_hash,
        size_bytes: source.size_bytes(),
    })
}

fn admit_existing(
    directory: &OwnedFd,
    name: &OsStr,
    path: &Path,
    expected_hash: &str,
    expected_size: u64,
    expected_mode: u32,
) -> Result<bool, PublicExecutableError> {
    let linked = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode).is_file() => stat,
        Ok(_) | Err(rustix::io::Errno::NOENT) => return Ok(false),
        Err(error) => return Err(errno("inspect existing executable", path, error)),
    };
    if linked.st_size < 0
        || u64::try_from(linked.st_size).ok() != Some(expected_size)
        || u32::from(linked.st_mode & 0o7777) != expected_mode
    {
        return Ok(false);
    }
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|error| errno("open existing executable", path, error))?;
    let opened = require_regular(&descriptor, path)?;
    if !same_node(&linked, &opened) {
        return Err(unsafe_node(path, "the same existing regular executable"));
    }
    let mut file = File::from(descriptor);
    let actual_hash = hash_file(&mut file)?;
    let after = require_regular_file(&file, path)?;
    if !same_snapshot(&opened, &after) {
        return Err(unsafe_node(path, "one stable existing regular executable"));
    }
    Ok(actual_hash == expected_hash)
}

fn open_real_directory(path: &Path) -> Result<OwnedFd, PublicExecutableError> {
    let expected = statat(rustix::fs::CWD, path, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| errno("inspect publication directory", path, error))?;
    if !FileType::from_raw_mode(expected.st_mode).is_dir() {
        return Err(unsafe_node(path, "a real directory"));
    }
    let directory = open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        NO_MODE,
    )
    .map_err(|error| errno("open publication directory", path, error))?;
    let opened = require_directory(&directory, path)?;
    if !same_node(&expected, &opened) {
        return Err(unsafe_node(path, "the same real directory"));
    }
    Ok(directory)
}

fn validate_directory_path(path: &Path, directory: &OwnedFd) -> Result<(), PublicExecutableError> {
    let current = open_real_directory(path)?;
    let expected = require_directory(directory, path)?;
    let current = require_directory(&current, path)?;
    if !same_node(&expected, &current) {
        return Err(unsafe_node(path, "the original publication directory"));
    }
    Ok(())
}

fn open_or_create_regular(
    directory: &OwnedFd,
    name: &OsStr,
    path: &Path,
) -> Result<File, PublicExecutableError> {
    for _ in 0..8 {
        match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(expected) if FileType::from_raw_mode(expected.st_mode).is_file() => {
                let descriptor = openat(
                    directory,
                    name,
                    OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    NO_MODE,
                )
                .map_err(|error| errno("open public executable lock", path, error))?;
                let opened = require_regular(&descriptor, path)?;
                if !same_node(&expected, &opened) {
                    return Err(unsafe_node(path, "the same regular lock file"));
                }
                return Ok(File::from(descriptor));
            }
            Ok(_) => return Err(unsafe_node(path, "a regular lock file")),
            Err(rustix::io::Errno::NOENT) => match openat(
                directory,
                name,
                OFlags::RDWR
                    | OFlags::CREATE
                    | OFlags::EXCL
                    | OFlags::NOFOLLOW
                    | OFlags::NONBLOCK
                    | OFlags::CLOEXEC,
                PRIVATE_FILE_MODE,
            ) {
                Ok(descriptor) => {
                    require_regular(&descriptor, path)?;
                    return Ok(File::from(descriptor));
                }
                Err(rustix::io::Errno::EXIST) => {}
                Err(error) => return Err(errno("create public executable lock", path, error)),
            },
            Err(error) => return Err(errno("inspect public executable lock", path, error)),
        }
    }
    Err(PublicExecutableError::message(format!(
        "public executable lock changed repeatedly: {}",
        path.display()
    )))
}

struct Temporary<'a> {
    name: OsString,
    path: PathBuf,
    file: File,
    cleanup: TemporaryCleanup<'a>,
}

struct TemporaryCleanup<'a> {
    directory: BorrowedFd<'a>,
    name: OsString,
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
            let _ = unlinkat(self.directory, &self.name, AtFlags::empty());
        }
    }
}

fn create_temporary<'a>(
    directory: &'a OwnedFd,
    parent: &Path,
    destination: &OsStr,
) -> Result<Temporary<'a>, PublicExecutableError> {
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(destination);
        name.push(format!(".tmp-{}-{sequence}", std::process::id()));
        let path = parent.join(&name);
        match openat(
            directory,
            &name,
            OFlags::WRONLY
                | OFlags::CREATE
                | OFlags::EXCL
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            PRIVATE_FILE_MODE,
        ) {
            Ok(descriptor) => {
                require_regular(&descriptor, &path)?;
                return Ok(Temporary {
                    cleanup: TemporaryCleanup {
                        directory: directory.as_fd(),
                        name: name.clone(),
                        armed: true,
                    },
                    name,
                    path,
                    file: File::from(descriptor),
                });
            }
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(errno("create temporary executable", &path, error)),
        }
    }
    Err(PublicExecutableError::message(
        "temporary executable filename sequence exhausted",
    ))
}

fn copy_and_hash(
    source: &mut File,
    temporary: &mut File,
) -> Result<(String, u64), PublicExecutableError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        let count = source.read(&mut buffer).map_err(|source_error| {
            PublicExecutableError::io("cannot read cached executable", source_error)
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            PublicExecutableError::message("cached executable read exceeded its buffer")
        })?;
        temporary.write_all(chunk).map_err(|source_error| {
            PublicExecutableError::io("cannot copy cached executable", source_error)
        })?;
        hasher.update(chunk);
        bytes = bytes.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
    }
    temporary.flush().map_err(|source_error| {
        PublicExecutableError::io("cannot flush copied executable", source_error)
    })?;
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    ))
}

fn hash_file(file: &mut File) -> Result<String, PublicExecutableError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|source_error| {
            PublicExecutableError::io("cannot read existing executable", source_error)
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            PublicExecutableError::message("existing executable read exceeded its buffer")
        })?;
        hasher.update(chunk);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn regular_stat_at<Fd: AsFd>(
    directory: Fd,
    name: &OsStr,
    path: &Path,
) -> Result<Stat, PublicExecutableError> {
    let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| errno("inspect executable", path, error))?;
    if FileType::from_raw_mode(stat.st_mode).is_file() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a regular executable"))
    }
}

fn require_regular(descriptor: &OwnedFd, path: &Path) -> Result<Stat, PublicExecutableError> {
    let stat = fstat(descriptor).map_err(|error| errno("inspect open file", path, error))?;
    if FileType::from_raw_mode(stat.st_mode).is_file() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a regular file"))
    }
}

fn require_regular_file(file: &File, path: &Path) -> Result<Stat, PublicExecutableError> {
    let stat = fstat(file).map_err(|error| errno("inspect open file", path, error))?;
    if FileType::from_raw_mode(stat.st_mode).is_file() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a regular file"))
    }
}

fn require_directory(descriptor: &OwnedFd, path: &Path) -> Result<Stat, PublicExecutableError> {
    let stat = fstat(descriptor).map_err(|error| errno("inspect open directory", path, error))?;
    if FileType::from_raw_mode(stat.st_mode).is_dir() {
        Ok(stat)
    } else {
        Err(unsafe_node(path, "a real directory"))
    }
}

fn sibling_lock_name(destination: &OsStr) -> OsString {
    let mut name = OsString::from(".");
    name.push(destination);
    name.push(".gors-build.lock");
    name
}

fn same_node(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn same_snapshot(left: &Stat, right: &Stat) -> bool {
    same_node(left, right)
        && left.st_size == right.st_size
        && left.st_mode == right.st_mode
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}

fn errno(action: &str, path: &Path, error: rustix::io::Errno) -> PublicExecutableError {
    PublicExecutableError::io(
        format!("{action} {}", path.display()),
        std::io::Error::from(error),
    )
}

fn unsafe_node(path: &Path, expected: &str) -> PublicExecutableError {
    PublicExecutableError::message(format!(
        "unsafe filesystem node at {}; expected {expected}",
        path.display()
    ))
}
