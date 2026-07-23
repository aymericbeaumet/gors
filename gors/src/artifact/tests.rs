use gors_runtime_abi::{
    CURRENT_ARTIFACT_SCHEMA, RuntimeAbiManifest, RuntimeArtifactFormat, TargetCapability,
};

use super::{RUNTIME_CRATE_NAME, RuntimeArtifactError, embedded_runtime_artifact};

#[test]
fn embedded_provider_matches_the_current_contract_and_fixed_recipe()
-> Result<(), Box<dyn std::error::Error>> {
    let artifact = embedded_runtime_artifact();
    artifact.verify()?;

    assert_eq!(artifact.manifest().schema(), CURRENT_ARTIFACT_SCHEMA);
    assert_eq!(
        artifact.manifest().contract(),
        RuntimeAbiManifest::current().identity()
    );
    assert_eq!(
        artifact.manifest().format(),
        RuntimeArtifactFormat::RustRlibV1
    );
    assert_eq!(
        artifact.manifest().provided_capabilities().as_slice(),
        &[TargetCapability::StandardIo]
    );
    assert_eq!(
        artifact.manifest().target(),
        artifact.compatibility().target()
    );
    assert_eq!(
        artifact.manifest().compatibility(),
        artifact.compatibility().identity()
    );
    assert_eq!(
        artifact.producer().canonical_record(),
        artifact.producer().provenance().canonical_bytes()
    );
    assert_ne!(
        artifact.producer().identity().to_string(),
        artifact.compatibility().identity().to_string()
    );
    assert!(!artifact.payload().is_empty());
    Ok(())
}

#[test]
fn materialization_is_content_addressed_and_repairs_corruption()
-> Result<(), Box<dyn std::error::Error>> {
    let cache = tempfile::tempdir()?;
    let artifact = embedded_runtime_artifact();
    let expected = cache
        .path()
        .join(artifact.manifest().identity().to_string())
        .join(format!("lib{RUNTIME_CRATE_NAME}.rlib"));

    let path = artifact.materialize(cache.path())?;
    assert_eq!(path, expected);
    assert!(artifact.verify_materialized(&path)?);

    std::fs::write(&path, b"corrupt")?;
    let repaired = artifact.materialize(cache.path())?;
    assert_eq!(repaired, path);
    assert!(artifact.verify_materialized(&repaired)?);
    Ok(())
}

#[test]
fn concurrent_materialization_publishes_one_verified_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    let cache = tempfile::tempdir()?;
    let cache_root = cache.path().to_path_buf();
    let artifact = embedded_runtime_artifact();
    let expected = cache_root
        .join(artifact.manifest().identity().to_string())
        .join(format!("lib{RUNTIME_CRATE_NAME}.rlib"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));

    let workers = (0..8)
        .map(|_| {
            let cache_root = cache_root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                artifact.materialize(&cache_root)
            })
        })
        .collect::<Vec<_>>();

    for worker in workers {
        let Ok(result) = worker.join() else {
            return Err(std::io::Error::other("materialization worker panicked").into());
        };
        let path = result?;
        assert_eq!(path, expected);
    }
    assert!(artifact.verify_materialized(&expected)?);
    Ok(())
}

#[cfg(unix)]
#[test]
fn exact_payload_destination_symlink_is_rejected_without_touching_target()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let cache = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let artifact = embedded_runtime_artifact();
    let outside_artifact = outside.path().join("outside.rlib");
    std::fs::write(&outside_artifact, artifact.payload())?;
    let artifact_directory = cache
        .path()
        .join(artifact.manifest().identity().to_string());
    std::fs::create_dir(&artifact_directory)?;
    let destination = artifact_directory.join(format!("lib{RUNTIME_CRATE_NAME}.rlib"));
    symlink(&outside_artifact, &destination)?;

    let Err(error) = artifact.materialize(cache.path()) else {
        return Err(std::io::Error::other("an exact-payload symlink was admitted").into());
    };
    assert!(matches!(
        error,
        RuntimeArtifactError::UnsafeFilesystemNode { .. }
    ));
    assert_eq!(std::fs::read(&outside_artifact)?, artifact.payload());
    assert!(
        std::fs::symlink_metadata(&destination)?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn artifact_directory_symlink_is_rejected_without_writing_outside_cache()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let cache = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let artifact = embedded_runtime_artifact();
    let marker = outside.path().join("marker");
    std::fs::write(&marker, b"untouched")?;
    let artifact_directory = cache
        .path()
        .join(artifact.manifest().identity().to_string());
    symlink(outside.path(), &artifact_directory)?;

    let Err(error) = artifact.materialize(cache.path()) else {
        return Err(std::io::Error::other("an artifact-directory symlink was followed").into());
    };
    assert!(matches!(
        error,
        RuntimeArtifactError::UnsafeFilesystemNode { .. }
    ));
    assert_eq!(std::fs::read(&marker)?, b"untouched");
    assert!(!outside.path().join(".materialize.lock").exists());
    assert!(
        !outside
            .path()
            .join(format!("lib{RUNTIME_CRATE_NAME}.rlib"))
            .exists()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn lock_symlink_is_rejected_without_touching_target() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let cache = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let artifact = embedded_runtime_artifact();
    let outside_lock = outside.path().join("outside.lock");
    std::fs::write(&outside_lock, b"untouched")?;
    let artifact_directory = cache
        .path()
        .join(artifact.manifest().identity().to_string());
    std::fs::create_dir(&artifact_directory)?;
    symlink(&outside_lock, artifact_directory.join(".materialize.lock"))?;

    let Err(error) = artifact.materialize(cache.path()) else {
        return Err(std::io::Error::other("a lock symlink was followed").into());
    };
    assert!(matches!(
        error,
        RuntimeArtifactError::UnsafeFilesystemNode { .. }
    ));
    assert_eq!(std::fs::read(&outside_lock)?, b"untouched");
    Ok(())
}
