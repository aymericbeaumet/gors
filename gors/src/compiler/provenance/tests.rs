use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::*;
use crate::compiler::ids::{DefinitionKey, DefinitionKind, IdentityInterner};
use crate::compiler::input::{PackageKey, WorkspaceKey};

#[derive(Clone, Copy)]
struct Ids {
    file: FileId,
    other_file: FileId,
    owner: DefId,
    other_owner: DefId,
}

fn ids() -> Ids {
    let mut interner = IdentityInterner::default();
    let workspace = interner
        .workspace(&WorkspaceKey::ad_hoc("provenance").unwrap())
        .unwrap();
    let package = interner
        .package(workspace, &PackageKey::command_line())
        .unwrap();
    let file = interner.file(package, "main.go").unwrap();
    let other_file = interner.file(package, "other.go").unwrap();
    let owner = interner
        .definition(DefinitionKey::package_named(
            package,
            DefinitionKind::Function,
            "main",
        ))
        .unwrap();
    let other_owner = interner
        .definition(DefinitionKey::package_named(
            package,
            DefinitionKind::Function,
            "other",
        ))
        .unwrap();
    Ids {
        file,
        other_file,
        owner,
        other_owner,
    }
}

fn range(file: FileId, start: u32, end: u32) -> FileRange {
    FileRange::new(
        file,
        TextRange::new(TextSize::new(start), TextSize::new(end)).unwrap(),
    )
}

fn node(owner: DefId, index: u32) -> NodeId {
    NodeId::owner_local(owner, index)
}

fn complete_mappings(ids: Ids) -> Vec<(SourceRef, FileRange)> {
    vec![
        (SourceRef::definition(ids.owner), range(ids.file, 0, 40)),
        (SourceRef::node(node(ids.owner, 0)), range(ids.file, 4, 8)),
        (SourceRef::node(node(ids.owner, 1)), range(ids.file, 12, 20)),
        (
            SourceRef::local(ids.owner, LocalId(0)),
            range(ids.file, 4, 5),
        ),
        (
            SourceRef::local(ids.owner, LocalId(1)),
            range(ids.file, 12, 13),
        ),
    ]
}

fn table(ids: Ids) -> DefinitionSourceTable {
    DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        complete_mappings(ids),
    )
    .unwrap()
}

#[test]
fn source_ref_constructors_preserve_exact_identity() {
    let ids = ids();
    let node_id = node(ids.owner, 7);

    assert_eq!(SourceRef::definition(ids.owner).owner(), ids.owner);
    assert_eq!(
        SourceRef::definition(ids.owner).kind(),
        SourceRefKind::Definition
    );
    assert_eq!(SourceRef::node(node_id).owner(), ids.owner);
    assert_eq!(
        SourceRef::node(node_id).kind(),
        SourceRefKind::Node(node_id)
    );
    assert_eq!(
        SourceRef::local(ids.owner, LocalId(3)).kind(),
        SourceRefKind::Local(LocalId(3))
    );
}

#[test]
fn resolves_definition_node_and_local_ranges_exactly() {
    let ids = ids();
    let table = table(ids);

    assert_eq!(table.owner(), ids.owner);
    assert_eq!(table.file(), ids.file);
    assert_eq!(table.source_len(), TextSize::new(64));
    assert_eq!(
        table.resolve(SourceRef::definition(ids.owner)).unwrap(),
        range(ids.file, 0, 40)
    );
    assert_eq!(
        table.resolve(SourceRef::node(node(ids.owner, 1))).unwrap(),
        range(ids.file, 12, 20)
    );
    assert_eq!(
        table
            .resolve(SourceRef::local(ids.owner, LocalId(0)))
            .unwrap(),
        range(ids.file, 4, 5)
    );
}

#[test]
fn construction_rejects_mismatched_owner() {
    let ids = ids();
    let offending = SourceRef::node(node(ids.other_owner, 0));
    let error = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [
            (SourceRef::definition(ids.owner), range(ids.file, 0, 40)),
            (offending, range(ids.file, 4, 8)),
        ],
    )
    .unwrap_err();

    assert_eq!(
        error,
        SourceTableBuildError::OwnerMismatch {
            table_owner: ids.owner,
            source_ref: offending,
        }
    );
}

#[test]
fn construction_rejects_mismatched_file() {
    let ids = ids();
    let source_ref = SourceRef::definition(ids.owner);
    let error = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [(source_ref, range(ids.other_file, 0, 40))],
    )
    .unwrap_err();

    assert_eq!(
        error,
        SourceTableBuildError::FileMismatch {
            source_ref,
            expected: ids.file,
            actual: ids.other_file,
        }
    );
}

#[test]
fn construction_rejects_out_of_bounds_ranges() {
    let ids = ids();
    let source_ref = SourceRef::definition(ids.owner);
    let offending = range(ids.file, 0, 65);
    let error = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [(source_ref, offending)],
    )
    .unwrap_err();

    assert_eq!(
        error,
        SourceTableBuildError::RangeOutOfBounds {
            source_ref,
            range: offending.range(),
            source_len: TextSize::new(64),
        }
    );
}

#[test]
fn construction_rejects_missing_definition() {
    let ids = ids();
    let error = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [(SourceRef::node(node(ids.owner, 0)), range(ids.file, 4, 8))],
    )
    .unwrap_err();

    assert_eq!(
        error,
        SourceTableBuildError::MissingDefinition { owner: ids.owner }
    );
}

#[test]
fn construction_rejects_duplicate_references() {
    let ids = ids();
    let source_ref = SourceRef::definition(ids.owner);
    let error = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [
            (source_ref, range(ids.file, 0, 40)),
            (source_ref, range(ids.file, 1, 39)),
        ],
    )
    .unwrap_err();

    assert_eq!(
        error,
        SourceTableBuildError::DuplicateReference { source_ref }
    );
}

#[test]
fn construction_rejects_missing_dense_node_and_local_slots() {
    let ids = ids();
    let missing_node = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [
            (SourceRef::definition(ids.owner), range(ids.file, 0, 40)),
            (SourceRef::node(node(ids.owner, 1)), range(ids.file, 4, 8)),
        ],
    )
    .unwrap_err();
    assert_eq!(
        missing_node,
        SourceTableBuildError::MissingDenseSlot {
            owner: ids.owner,
            kind: SourceTableSlotKind::Node,
            index: 0,
        }
    );

    let missing_local = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(64),
        [
            (SourceRef::definition(ids.owner), range(ids.file, 0, 40)),
            (
                SourceRef::local(ids.owner, LocalId(1)),
                range(ids.file, 4, 8),
            ),
        ],
    )
    .unwrap_err();
    assert_eq!(
        missing_local,
        SourceTableBuildError::MissingDenseSlot {
            owner: ids.owner,
            kind: SourceTableSlotKind::Local,
            index: 0,
        }
    );
}

#[test]
fn lookup_rejects_other_owners_and_missing_slots() {
    let ids = ids();
    let table = table(ids);
    let foreign = SourceRef::definition(ids.other_owner);

    assert_eq!(
        table.resolve(foreign).unwrap_err(),
        SourceLookupError::OwnerMismatch {
            table_owner: ids.owner,
            source_ref: foreign,
        }
    );
    assert_eq!(
        table
            .resolve(SourceRef::node(node(ids.owner, 2)))
            .unwrap_err(),
        SourceLookupError::MissingNode {
            owner: ids.owner,
            index: 2,
        }
    );
    assert_eq!(
        table
            .resolve(SourceRef::local(ids.owner, LocalId(2)))
            .unwrap_err(),
        SourceLookupError::MissingLocal {
            owner: ids.owner,
            local: LocalId(2),
        }
    );
}

#[test]
fn construction_order_does_not_change_equality_or_hash() {
    let ids = ids();
    let forward = table(ids);
    let mut reversed_mappings = complete_mappings(ids);
    reversed_mappings.reverse();
    let reversed =
        DefinitionSourceTable::try_new(ids.owner, ids.file, TextSize::new(64), reversed_mappings)
            .unwrap();

    assert_eq!(forward, reversed);
    assert_eq!(hash(&forward), hash(&reversed));
}

#[test]
fn same_reference_resolves_through_relocated_revision_table() {
    let ids = ids();
    let source_ref = SourceRef::node(node(ids.owner, 0));
    let before = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(32),
        [
            (SourceRef::definition(ids.owner), range(ids.file, 0, 16)),
            (source_ref, range(ids.file, 4, 8)),
        ],
    )
    .unwrap();
    let after_whitespace_insert = DefinitionSourceTable::try_new(
        ids.owner,
        ids.file,
        TextSize::new(36),
        [
            (SourceRef::definition(ids.owner), range(ids.file, 0, 20)),
            (source_ref, range(ids.file, 8, 12)),
        ],
    )
    .unwrap();

    assert_eq!(before.resolve(source_ref).unwrap(), range(ids.file, 4, 8));
    assert_eq!(
        after_whitespace_insert.resolve(source_ref).unwrap(),
        range(ids.file, 8, 12)
    );
}

fn hash(value: &DefinitionSourceTable) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
