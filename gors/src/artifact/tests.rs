use gors_runtime_abi::{
    CURRENT_ARTIFACT_SCHEMA, RuntimeAbiManifest, RuntimeArtifactFormat, TargetCapability,
};

use super::{RUNTIME_CRATE_NAME, embedded_runtime_artifact};

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
