use std::error::Error;

use gors_runtime_abi::{
    DataWidth, Endianness, RustRlibCompatibility, TargetModel, TargetModelError,
};

fn target_model(triple: &str) -> Result<TargetModel, TargetModelError> {
    TargetModel::new(triple, DataWidth::Bits32, Endianness::Little)
}

#[test]
fn invalid_target_triples_are_rejected() {
    assert!(target_model("wasm32-unknown-unknown").is_ok());
    assert_eq!(target_model(""), Err(TargetModelError::EmptyTriple));
    assert_eq!(
        target_model("x86_64 unknown linux gnu"),
        Err(TargetModelError::InvalidTripleCharacter)
    );
}

#[test]
fn rust_rlib_compatibility_identity_is_path_free_and_covers_exact_producer_facts()
-> Result<(), Box<dyn Error>> {
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let rustc_version = b"rustc 1.96.0\nhost: x86_64-unknown-linux-gnu\n";
    let target_libdir = b"canonical-target-libdir-v1";
    let baseline =
        RustRlibCompatibility::new(rustc_version, target_libdir.as_slice(), target.clone())?;
    let same = RustRlibCompatibility::new(
        b"rustc 1.96.0\nhost: aarch64-unknown-linux-gnu\n",
        target_libdir.as_slice(),
        target,
    )?;
    let other_rustc = RustRlibCompatibility::new(
        b"rustc 1.96.1\nhost: x86_64-unknown-linux-gnu\n",
        target_libdir.as_slice(),
        target_model("x86_64-unknown-linux-gnu")?,
    )?;
    let other_target = RustRlibCompatibility::new(
        rustc_version,
        target_libdir.as_slice(),
        target_model("i686-unknown-linux-musl")?,
    )?;
    let other_sysroot = RustRlibCompatibility::new(
        rustc_version,
        b"different-target-libdir".as_slice(),
        target_model("x86_64-unknown-linux-gnu")?,
    )?;

    assert_eq!(baseline.canonical_bytes(), same.canonical_bytes());
    assert_eq!(baseline.identity(), same.identity());
    assert_ne!(baseline.identity(), other_rustc.identity());
    assert_ne!(baseline.identity(), other_target.identity());
    assert_ne!(baseline.identity(), other_sysroot.identity());
    assert!(
        !baseline
            .canonical_bytes()
            .windows(7)
            .any(|bytes| bytes == b"/Users/")
    );
    assert_eq!(
        RustRlibCompatibility::new(
            [],
            target_libdir.as_slice(),
            target_model("x86_64-unknown-linux-gnu")?
        ),
        Err(gors_runtime_abi::RustRlibRecordError::EmptyRustcVerboseVersion)
    );
    Ok(())
}
