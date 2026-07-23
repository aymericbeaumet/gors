#![allow(clippy::panic, clippy::unwrap_used)]

use super::*;
use crate::rustc::ExecutableProduct;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

#[test]
fn default_name_uses_go_stem_or_canonical_directory_basename() {
    assert_eq!(
        default_executable_path(Path::new("cmd/server.go")).unwrap(),
        PathBuf::from(format!("server{}", std::env::consts::EXE_SUFFIX))
    );

    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("worker");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        default_executable_path(&directory).unwrap(),
        PathBuf::from(format!("worker{}", std::env::consts::EXE_SUFFIX))
    );
}

#[test]
fn default_name_rejects_a_filesystem_root() {
    assert!(default_executable_path(Path::new("/")).is_err());
}

#[test]
fn publication_copies_exact_bytes_size_and_executable_mode() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("cached");
    let destination = temporary.path().join("program");
    std::fs::write(&source_path, b"new executable bytes").unwrap();
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o751)).unwrap();
    std::fs::write(&destination, b"old bytes").unwrap();
    let source = ExecutableProduct::admit(&source_path).unwrap();

    let published = publish_executable(&source, &destination).unwrap();

    assert_eq!(published.path(), destination);
    assert_eq!(published.content_hash(), source.content_hash());
    assert_eq!(published.size_bytes(), source.size_bytes());
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"new executable bytes"
    );
    assert_eq!(
        std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o751
    );
}

#[test]
fn exact_republication_preserves_the_existing_inode_and_mtime() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("cached");
    let destination = temporary.path().join("program");
    std::fs::write(&source_path, b"stable executable bytes").unwrap();
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let source = ExecutableProduct::admit(&source_path).unwrap();
    publish_executable(&source, &destination).unwrap();
    let before = std::fs::metadata(&destination).unwrap();

    publish_executable(&source, &destination).unwrap();

    let after = std::fs::metadata(&destination).unwrap();
    assert_eq!(after.ino(), before.ino());
    assert_eq!(after.mtime(), before.mtime());
    assert_eq!(after.mtime_nsec(), before.mtime_nsec());
}

#[test]
fn publication_replaces_a_symlink_without_touching_its_outside_target() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("cached");
    let outside = temporary.path().join("outside");
    let destination = temporary.path().join("program");
    std::fs::write(&source_path, b"new executable").unwrap();
    std::fs::write(&outside, b"outside stays unchanged").unwrap();
    symlink(&outside, &destination).unwrap();
    let source = ExecutableProduct::admit(&source_path).unwrap();

    publish_executable(&source, &destination).unwrap();

    assert_eq!(std::fs::read(&outside).unwrap(), b"outside stays unchanged");
    assert_eq!(std::fs::read(&destination).unwrap(), b"new executable");
    assert!(
        std::fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_file()
    );
}

#[test]
fn publication_rejects_a_symlink_lock_without_touching_its_target() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("cached");
    let outside = temporary.path().join("outside-lock-target");
    let destination = temporary.path().join("program");
    let lock = temporary.path().join(".program.gors-build.lock");
    std::fs::write(&source_path, b"new executable").unwrap();
    std::fs::write(&outside, b"outside lock stays unchanged").unwrap();
    symlink(&outside, &lock).unwrap();
    let source = ExecutableProduct::admit(&source_path).unwrap();

    assert!(publish_executable(&source, &destination).is_err());
    assert_eq!(
        std::fs::read(&outside).unwrap(),
        b"outside lock stays unchanged"
    );
    assert!(!destination.exists());
}

#[test]
fn publication_failure_does_not_predelete_the_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("cached");
    let destination = temporary.path().join("program");
    std::fs::write(&source_path, b"new executable").unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("sentinel"), b"keep").unwrap();
    let source = ExecutableProduct::admit(&source_path).unwrap();

    assert!(publish_executable(&source, &destination).is_err());
    assert_eq!(
        std::fs::read(destination.join("sentinel")).unwrap(),
        b"keep"
    );
}
