use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn forced_collision(_: &[u8]) -> StableFingerprint {
    StableFingerprint([0x5a; 32])
}

#[test]
fn identical_full_keys_reuse_an_identity() -> TestResult {
    let mut interner = IdentityInterner::default();
    let key = WorkspaceKey::ad_hoc("workspace")?;
    let first = interner.workspace(&key)?;
    let second = interner.workspace(&key)?;
    assert_eq!(first, second);
    Ok(())
}

fn package(
    interner: &mut IdentityInterner,
    import_path: &str,
) -> Result<PackageId, Box<dyn std::error::Error>> {
    let workspace_key = WorkspaceKey::ad_hoc("workspace")?;
    let workspace = interner.workspace(&workspace_key)?;
    let package_key = PackageKey::import_path(import_path)?;
    Ok(interner.package(workspace, &package_key)?)
}

#[test]
fn structured_key_variants_have_distinct_identities() -> TestResult {
    let mut interner = IdentityInterner::default();
    let module = interner.workspace(&WorkspaceKey::module("example/workspace")?)?;
    let ad_hoc = interner.workspace(&WorkspaceKey::ad_hoc("example/workspace")?)?;
    assert_ne!(module, ad_hoc);

    let import = interner.package(module, &PackageKey::import_path("command-line")?)?;
    let command_line = interner.package(module, &PackageKey::command_line())?;
    assert_ne!(import, command_line);
    Ok(())
}

#[test]
fn named_definitions_are_owned_by_the_package_not_the_file() -> TestResult {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project")?;
    let first_file = interner.file(package, "first.go")?;
    let second_file = interner.file(package, "second.go")?;
    assert_ne!(first_file, second_file);

    let key = DefinitionKey::package_named(package, DefinitionKind::Function, "Serve");
    let first = interner.definition(key.clone())?;
    let moved = interner.definition(key)?;
    assert_eq!(first, moved);
    Ok(())
}

#[test]
fn identical_package_clauses_in_distinct_import_paths_do_not_alias() -> TestResult {
    let mut interner = IdentityInterner::default();
    let first_package = package(&mut interner, "example/one")?;
    let second_package = package(&mut interner, "example/two")?;
    let first = interner.definition(DefinitionKey::package_named(
        first_package,
        DefinitionKind::Function,
        "Run",
    ))?;
    let second = interner.definition(DefinitionKey::package_named(
        second_package,
        DefinitionKind::Function,
        "Run",
    ))?;
    assert_ne!(first, second);
    Ok(())
}

#[test]
fn method_receiver_identity_is_part_of_the_definition_key() -> TestResult {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project")?;
    let first = interner.definition(DefinitionKey::method(
        ReceiverIdentity::named(package, "First"),
        "Read",
    ))?;
    let second = interner.definition(DefinitionKey::method(
        ReceiverIdentity::named(package, "Second"),
        "Read",
    ))?;
    assert_ne!(first, second);
    Ok(())
}

#[test]
fn repeated_init_definitions_require_stable_semantic_disambiguators() -> TestResult {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project")?;
    let first = interner.definition(DefinitionKey::disambiguated_package_definition(
        package,
        DefinitionKind::Function,
        "init",
        "syntax-node:first",
    ))?;
    let second = interner.definition(DefinitionKey::disambiguated_package_definition(
        package,
        DefinitionKind::Function,
        "init",
        "syntax-node:second",
    ))?;
    assert_ne!(first, second);
    Ok(())
}

#[test]
fn digest_collisions_compare_and_report_both_full_keys() -> TestResult {
    let mut interner = IdentityInterner::with_fingerprint(forced_collision);
    interner.workspace(&WorkspaceKey::ad_hoc("first")?)?;
    let collision = match interner.workspace(&WorkspaceKey::ad_hoc("second")?) {
        Ok(_) => return Err("expected a workspace identity collision".into()),
        Err(collision) => collision,
    };
    assert!(collision.to_string().contains("existing full key"));
    assert!(collision.to_string().contains("requested full key"));
    assert_ne!(collision.existing, collision.requested);
    Ok(())
}

#[test]
fn definition_collisions_report_both_canonical_full_keys() -> TestResult {
    let mut interner = IdentityInterner::with_fingerprint(forced_collision);
    let package = package(&mut interner, "example/project")?;
    interner.definition(DefinitionKey::package_named(
        package,
        DefinitionKind::Function,
        "First",
    ))?;
    let collision = match interner.definition(DefinitionKey::package_named(
        package,
        DefinitionKind::Function,
        "Second",
    )) {
        Ok(_) => return Err("expected a definition identity collision".into()),
        Err(collision) => collision,
    };
    let message = collision.to_string();
    assert!(message.contains("First"));
    assert!(message.contains("Second"));
    assert!(message.contains("existing full key"));
    assert!(message.contains("requested full key"));
    Ok(())
}
