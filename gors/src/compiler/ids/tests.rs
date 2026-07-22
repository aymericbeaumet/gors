use super::*;

fn forced_collision(_: &[u8]) -> StableFingerprint {
    StableFingerprint([0x5a; 32])
}

#[test]
fn identical_full_keys_reuse_an_identity() {
    let mut interner = IdentityInterner::default();
    let first = interner.workspace("workspace").unwrap();
    let second = interner.workspace("workspace").unwrap();
    assert_eq!(first, second);
}

fn package(interner: &mut IdentityInterner, import_path: &str) -> PackageId {
    let workspace = interner.workspace("workspace").unwrap();
    interner.package(workspace, import_path).unwrap()
}

#[test]
fn named_definitions_are_owned_by_the_package_not_the_file() {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project");
    let first_file = interner.file(package, "first.go").unwrap();
    let second_file = interner.file(package, "second.go").unwrap();
    assert_ne!(first_file, second_file);

    let key = DefinitionKey::package_named(package, DefinitionKind::Function, "Serve");
    let first = interner.definition(key.clone()).unwrap();
    let moved = interner.definition(key).unwrap();
    assert_eq!(first, moved);
}

#[test]
fn identical_package_clauses_in_distinct_import_paths_do_not_alias() {
    let mut interner = IdentityInterner::default();
    let first_package = package(&mut interner, "example/one");
    let second_package = package(&mut interner, "example/two");
    let first = interner
        .definition(DefinitionKey::package_named(
            first_package,
            DefinitionKind::Function,
            "Run",
        ))
        .unwrap();
    let second = interner
        .definition(DefinitionKey::package_named(
            second_package,
            DefinitionKind::Function,
            "Run",
        ))
        .unwrap();
    assert_ne!(first, second);
}

#[test]
fn method_receiver_identity_is_part_of_the_definition_key() {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project");
    let first = interner
        .definition(DefinitionKey::method(
            ReceiverIdentity::named(package, "First"),
            "Read",
        ))
        .unwrap();
    let second = interner
        .definition(DefinitionKey::method(
            ReceiverIdentity::named(package, "Second"),
            "Read",
        ))
        .unwrap();
    assert_ne!(first, second);
}

#[test]
fn repeated_init_definitions_require_stable_semantic_disambiguators() {
    let mut interner = IdentityInterner::default();
    let package = package(&mut interner, "example/project");
    let first = interner
        .definition(DefinitionKey::disambiguated_package_definition(
            package,
            DefinitionKind::Function,
            "init",
            "syntax-node:first",
        ))
        .unwrap();
    let second = interner
        .definition(DefinitionKey::disambiguated_package_definition(
            package,
            DefinitionKind::Function,
            "init",
            "syntax-node:second",
        ))
        .unwrap();
    assert_ne!(first, second);
}

#[test]
fn digest_collisions_compare_and_report_both_full_keys() {
    let mut interner = IdentityInterner::with_fingerprint(forced_collision);
    interner.workspace("first").unwrap();
    let collision = interner.workspace("second").unwrap_err();
    assert!(collision.to_string().contains("existing full key"));
    assert!(collision.to_string().contains("requested full key"));
    assert_ne!(collision.existing, collision.requested);
}

#[test]
fn definition_collisions_report_both_canonical_full_keys() {
    let mut interner = IdentityInterner::with_fingerprint(forced_collision);
    let package = package(&mut interner, "example/project");
    interner
        .definition(DefinitionKey::package_named(
            package,
            DefinitionKind::Function,
            "First",
        ))
        .unwrap();
    let collision = interner
        .definition(DefinitionKey::package_named(
            package,
            DefinitionKind::Function,
            "Second",
        ))
        .unwrap_err();
    let message = collision.to_string();
    assert!(message.contains("First"));
    assert!(message.contains("Second"));
    assert!(message.contains("existing full key"));
    assert!(message.contains("requested full key"));
}
