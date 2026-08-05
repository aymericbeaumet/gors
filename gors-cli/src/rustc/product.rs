use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// One regular, non-empty executable admitted from an opened filesystem node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutableProduct {
    path: PathBuf,
    content_hash: String,
    size_bytes: u64,
    mode: u32,
}

impl ExecutableProduct {
    #[must_use]
    pub(crate) const fn canonical_mode() -> u32 {
        if cfg!(unix) { 0o755 } else { 0 }
    }

    pub(crate) fn admit(path: &Path) -> Result<Self, std::io::Error> {
        let path = absolute_path(path)?;
        let admitted = platform::admit(&path)?;
        Ok(Self::from_admitted(path, admitted))
    }

    fn from_admitted(path: PathBuf, admitted: AdmittedExecutable) -> Self {
        Self {
            path,
            content_hash: admitted.content_hash,
            size_bytes: admitted.size_bytes,
            mode: admitted.mode,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    #[must_use]
    pub const fn mode(&self) -> u32 {
        self.mode
    }
}

pub(super) fn publish_pending_executable(
    pending: &Path,
    output: &Path,
) -> Result<ExecutableProduct, std::io::Error> {
    let pending = absolute_path(pending)?;
    let output = absolute_path(output)?;
    let admitted = platform::publish_pending(&pending, &output)?;
    Ok(ExecutableProduct::from_admitted(output, admitted))
}

struct AdmittedExecutable {
    content_hash: String,
    size_bytes: u64,
    mode: u32,
}

fn absolute_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn hash_reader(mut reader: impl std::io::Read) -> Result<(String, u64), std::io::Error> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size_bytes = 0_u64;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let chunk = buffer
            .get(..count)
            .ok_or_else(|| std::io::Error::other("executable read exceeded the hashing buffer"))?;
        hasher.update(chunk);
        size_bytes = size_bytes
            .checked_add(u64::try_from(count).unwrap_or(u64::MAX))
            .ok_or_else(|| std::io::Error::other("executable size overflow"))?;
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        size_bytes,
    ))
}

#[cfg(unix)]
mod platform {
    use super::{AdmittedExecutable, hash_reader};
    use rustix::fs::{
        AtFlags, FileType, Mode, OFlags, Stat, fchmod, fstat, fsync, open, openat, renameat, statat,
    };
    use std::ffi::OsStr;
    use std::fs::File;
    use std::os::fd::{AsFd, OwnedFd};
    use std::path::Path;

    const CANONICAL_EXECUTABLE_MODE: u32 = super::ExecutableProduct::canonical_mode();
    const CANONICAL_EXECUTABLE_FS_MODE: Mode = Mode::from_raw_mode(0o755);
    const NO_MODE: Mode = Mode::empty();

    pub(super) fn admit(path: &Path) -> Result<AdmittedExecutable, std::io::Error> {
        let linked = regular_stat_at(rustix::fs::CWD, path.as_os_str(), path)?;
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            NO_MODE,
        )
        .map_err(|error| errno("open executable", path, error))?;
        let opened = regular_stat(&descriptor, path)?;
        if !same_node(&linked, &opened) {
            return Err(changed(path, "changed while it was opened"));
        }
        let mut file = File::from(descriptor);
        admit_open_file(&mut file, path, opened).map(|(admitted, _)| admitted)
    }

    pub(super) fn publish_pending(
        pending: &Path,
        output: &Path,
    ) -> Result<AdmittedExecutable, std::io::Error> {
        let Some(parent_path) = pending.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pending executable has no parent directory",
            ));
        };
        if Some(parent_path) != output.parent() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "pending and published executables must share one directory",
            ));
        }
        let pending_name = filename(pending)?;
        let output_name = filename(output)?;
        let parent = open_real_directory(parent_path)?;
        let linked = regular_stat_at(&parent, pending_name, pending)?;
        let descriptor = openat(
            &parent,
            pending_name,
            OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            NO_MODE,
        )
        .map_err(|error| errno("open pending executable", pending, error))?;
        let opened = regular_stat(&descriptor, pending)?;
        if !same_node(&linked, &opened) {
            return Err(changed(pending, "changed while it was opened"));
        }

        fchmod(&descriptor, CANONICAL_EXECUTABLE_FS_MODE)
            .map_err(|error| errno("set pending executable mode", pending, error))?;
        fsync(&descriptor).map_err(|error| errno("sync pending executable", pending, error))?;
        let opened = regular_stat(&descriptor, pending)?;
        let mut file = File::from(descriptor);
        let (admitted, admitted_stat) = admit_open_file(&mut file, pending, opened)?;
        if admitted.mode != CANONICAL_EXECUTABLE_MODE {
            return Err(changed(
                pending,
                "did not retain its canonical executable mode",
            ));
        }

        let linked_before_rename = regular_stat_at(&parent, pending_name, pending)?;
        if !same_snapshot(&admitted_stat, &linked_before_rename) {
            return Err(changed(pending, "changed before atomic publication"));
        }
        renameat(&parent, pending_name, &parent, output_name)
            .map_err(|error| errno("publish executable", output, error))?;
        let published = regular_stat_at(&parent, output_name, output)?;
        // Renaming an inode can legitimately advance ctime on some Unix
        // kernels. The descriptor and destination must still name the same
        // inode with the exact admitted content-bearing facts.
        if !same_content_snapshot(&admitted_stat, &published) {
            return Err(changed(output, "is not the validated pending executable"));
        }
        fsync(&parent).map_err(|error| errno("sync executable directory", parent_path, error))?;
        validate_directory_path(parent_path, &parent)?;
        let still_open =
            fstat(&file).map_err(|error| errno("reinspect published executable", output, error))?;
        if !same_snapshot(&published, &still_open) {
            return Err(changed(output, "changed during atomic publication"));
        }
        Ok(admitted)
    }

    fn admit_open_file(
        file: &mut File,
        path: &Path,
        before: Stat,
    ) -> Result<(AdmittedExecutable, Stat), std::io::Error> {
        let before_size = size(&before).ok_or_else(|| invalid(path, "has a negative size"))?;
        if before_size == 0 {
            return Err(invalid(path, "is empty"));
        }
        let before_mode = mode(&before);
        if before_mode & 0o111 == 0 {
            return Err(invalid(path, "has no executable permission bit"));
        }
        let (content_hash, hashed_size) = hash_reader(&mut *file)?;
        let after = fstat(&*file).map_err(|error| errno("reinspect executable", path, error))?;
        if !same_snapshot(&before, &after) || hashed_size != before_size {
            return Err(changed(path, "changed while it was hashed"));
        }
        Ok((
            AdmittedExecutable {
                content_hash,
                size_bytes: before_size,
                mode: before_mode,
            },
            after,
        ))
    }

    fn open_real_directory(path: &Path) -> Result<OwnedFd, std::io::Error> {
        let linked = statat(rustix::fs::CWD, path, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| errno("inspect executable directory", path, error))?;
        if !FileType::from_raw_mode(linked.st_mode).is_dir() {
            return Err(invalid(path, "is not a real directory"));
        }
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            NO_MODE,
        )
        .map_err(|error| errno("open executable directory", path, error))?;
        let opened = fstat(&descriptor)
            .map_err(|error| errno("inspect open executable directory", path, error))?;
        if !FileType::from_raw_mode(opened.st_mode).is_dir() || !same_node(&linked, &opened) {
            return Err(changed(path, "changed while it was opened"));
        }
        Ok(descriptor)
    }

    fn validate_directory_path(path: &Path, expected: &OwnedFd) -> Result<(), std::io::Error> {
        let current = open_real_directory(path)?;
        let expected = fstat(expected)
            .map_err(|error| errno("reinspect executable directory", path, error))?;
        let current = fstat(current)
            .map_err(|error| errno("inspect current executable directory", path, error))?;
        if same_node(&expected, &current) {
            Ok(())
        } else {
            Err(changed(path, "is no longer the publication directory"))
        }
    }

    fn regular_stat_at<Fd: AsFd>(
        directory: Fd,
        name: &OsStr,
        path: &Path,
    ) -> Result<Stat, std::io::Error> {
        let stat = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| errno("inspect executable", path, error))?;
        if FileType::from_raw_mode(stat.st_mode).is_file() {
            Ok(stat)
        } else {
            Err(invalid(path, "is not a regular file"))
        }
    }

    fn regular_stat(descriptor: &OwnedFd, path: &Path) -> Result<Stat, std::io::Error> {
        let stat =
            fstat(descriptor).map_err(|error| errno("inspect open executable", path, error))?;
        if FileType::from_raw_mode(stat.st_mode).is_file() {
            Ok(stat)
        } else {
            Err(invalid(path, "is not a regular file"))
        }
    }

    fn filename(path: &Path) -> Result<&OsStr, std::io::Error> {
        path.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("executable path has no filename: {}", path.display()),
            )
        })
    }

    fn size(stat: &Stat) -> Option<u64> {
        u64::try_from(stat.st_size).ok()
    }

    // `Stat::st_mode` is narrower than `u32` on Apple targets but is already
    // `u32` on Linux, so the portable widening is target-dependent.
    #[allow(clippy::useless_conversion)]
    fn mode(stat: &Stat) -> u32 {
        u32::from(stat.st_mode & 0o7777)
    }

    fn same_node(left: &Stat, right: &Stat) -> bool {
        left.st_dev == right.st_dev && left.st_ino == right.st_ino
    }

    fn same_snapshot(left: &Stat, right: &Stat) -> bool {
        same_content_snapshot(left, right)
            && left.st_ctime == right.st_ctime
            && left.st_ctime_nsec == right.st_ctime_nsec
    }

    fn same_content_snapshot(left: &Stat, right: &Stat) -> bool {
        same_node(left, right)
            && left.st_size == right.st_size
            && left.st_mode == right.st_mode
            && left.st_mtime == right.st_mtime
            && left.st_mtime_nsec == right.st_mtime_nsec
    }

    fn errno(action: &str, path: &Path, error: rustix::io::Errno) -> std::io::Error {
        let source = std::io::Error::from(error);
        std::io::Error::new(
            source.kind(),
            format!("failed to {action} {}: {source}", path.display()),
        )
    }

    fn invalid(path: &Path, detail: &str) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("terminal compiler output {detail}: {}", path.display()),
        )
    }

    fn changed(path: &Path, detail: &str) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("terminal compiler output {detail}: {}", path.display()),
        )
    }
}

#[cfg(not(unix))]
mod platform {
    use super::{AdmittedExecutable, hash_reader};
    use std::path::Path;

    pub(super) fn admit(path: &Path) -> Result<AdmittedExecutable, std::io::Error> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "terminal compiler output is not a regular non-empty file: {}",
                    path.display()
                ),
            ));
        }
        let (content_hash, size_bytes) = hash_reader(std::fs::File::open(path)?)?;
        if size_bytes != metadata.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "terminal compiler output changed while hashing: {}",
                    path.display()
                ),
            ));
        }
        Ok(AdmittedExecutable {
            content_hash,
            size_bytes,
            mode: 0,
        })
    }

    pub(super) fn publish_pending(
        pending: &Path,
        output: &Path,
    ) -> Result<AdmittedExecutable, std::io::Error> {
        let admitted = admit(pending)?;
        tempfile::TempPath::try_from_path(pending.to_path_buf())?
            .persist(output)
            .map_err(|error| error.error)?;
        Ok(admitted)
    }
}
