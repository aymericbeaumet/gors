use std::error::Error;

use gors_runtime_abi::{
    DataWidth, Endianness, RustRlibCompatibility, RustRlibProducer, TargetModel, TargetModelError,
    canonical_target_libdir_record,
};

fn target() -> Result<TargetModel, TargetModelError> {
    TargetModel::new(
        "aarch64-unknown-linux-gnu",
        DataWidth::Bits64,
        Endianness::Little,
    )
}

#[test]
fn producer_host_is_not_a_target_rlib_compatibility_fact() -> Result<(), Box<dyn Error>> {
    let inventory = b"target-sysroot-inventory";
    let cross_compatibility = RustRlibCompatibility::new(
        b"rustc 1.96.0\ncommit-hash: abc\nhost: x86_64-unknown-linux-gnu\n",
        inventory.as_slice(),
        target()?,
    )?;
    let native_compatibility = RustRlibCompatibility::new(
        b"rustc 1.96.0\ncommit-hash: abc\nhost: aarch64-unknown-linux-gnu\n",
        inventory.as_slice(),
        target()?,
    )?;
    let other_release = RustRlibCompatibility::new(
        b"rustc 1.96.0\ncommit-hash: def\nhost: aarch64-unknown-linux-gnu\n",
        inventory.as_slice(),
        target()?,
    )?;

    let cross_producer = RustRlibProducer::new(
        b"rustc 1.96.0\ncommit-hash: abc\nhost: x86_64-unknown-linux-gnu\n",
        inventory.as_slice(),
        target()?,
    )?;
    let native_producer = RustRlibProducer::new(
        b"rustc 1.96.0\ncommit-hash: abc\nhost: aarch64-unknown-linux-gnu\n",
        inventory.as_slice(),
        target()?,
    )?;

    assert_eq!(
        cross_compatibility.identity(),
        native_compatibility.identity()
    );
    assert_ne!(cross_compatibility.identity(), other_release.identity());
    assert_ne!(cross_producer.identity(), native_producer.identity());
    assert!(
        !cross_compatibility
            .rustc_release_record()
            .windows(5)
            .any(|part| part == b"host:")
    );
    Ok(())
}

#[test]
fn recursive_inventory_tracks_crate_metadata_and_unhashed_native_payloads()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let nested = directory.path().join("self-contained");
    std::fs::create_dir(&nested)?;
    std::fs::write(
        directory.path().join("libcore-0123456789abcdef.rmeta"),
        b"metadata",
    )?;
    let native = nested.join("crt1.o");
    std::fs::write(&native, b"native-a")?;

    let baseline = canonical_target_libdir_record(directory.path())?;
    let same = canonical_target_libdir_record(directory.path())?;
    assert_eq!(baseline, same);

    std::fs::write(&native, b"native-b")?;
    let changed_native = canonical_target_libdir_record(directory.path())?;
    assert_ne!(baseline, changed_native);

    std::fs::write(
        directory.path().join("libcore-0123456789abcdef.rmeta"),
        b"metadata-longer",
    )?;
    let changed_metadata = canonical_target_libdir_record(directory.path())?;
    assert_ne!(changed_native, changed_metadata);
    Ok(())
}

#[cfg(unix)]
#[test]
fn inventory_tracks_relative_symlink_targets() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    std::fs::write(directory.path().join("first.o"), b"first")?;
    std::fs::write(directory.path().join("second.o"), b"second")?;
    let alias = directory.path().join("current.o");
    symlink("first.o", &alias)?;
    let first = canonical_target_libdir_record(directory.path())?;

    std::fs::remove_file(&alias)?;
    symlink("second.o", &alias)?;
    let second = canonical_target_libdir_record(directory.path())?;

    assert_ne!(first, second);
    Ok(())
}

#[cfg(unix)]
#[test]
fn inventory_rejects_relative_symlinks_that_escape_the_root() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let nested = directory.path().join("nested");
    std::fs::create_dir(&nested)?;
    symlink("../../outside.rlib", nested.join("escape.rlib"))?;

    assert!(matches!(
        canonical_target_libdir_record(directory.path()),
        Err(gors_runtime_abi::RustTargetLibdirError::EscapingSymlink { .. })
    ));
    Ok(())
}
