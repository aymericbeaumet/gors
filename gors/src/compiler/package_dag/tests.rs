#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::compiler::db::CompilerDatabase;
use crate::compiler::ids::{FileId, PackageId};
use crate::compiler::input::{PackageKey, SourceSnapshot, WorkspaceKey};
use crate::import_path::CanonicalImportPath;

struct PackageFixture {
    package: PackageId,
    file: FileId,
    path: CanonicalImportPath,
    key: PackageKey,
}

fn insert_package(
    database: &mut CompilerDatabase,
    workspace: &WorkspaceKey,
    path: &str,
    imports: &[&str],
) -> PackageFixture {
    let package = PackageKey::import_path(path).unwrap();
    let import_block = imports
        .iter()
        .map(|import| format!("\t\"{import}\""))
        .collect::<Vec<_>>()
        .join("\n");
    let source = if import_block.is_empty() {
        format!("package {}\n", path.rsplit('/').next().unwrap())
    } else {
        format!(
            "package {}\nimport (\n{import_block}\n)\n",
            path.rsplit('/').next().unwrap()
        )
    };
    let file = database
        .set_source(
            workspace,
            &package,
            "package.go",
            Arc::new(
                SourceSnapshot::from_source(format!("/fixture/{path}/package.go"), source).unwrap(),
            ),
        )
        .unwrap()
        .file();
    PackageFixture {
        package: database.package_for_file(file).unwrap(),
        file,
        path: CanonicalImportPath::new(path).unwrap(),
        key: package,
    }
}

fn graph_inputs(
    database: &CompilerDatabase,
    packages: &[PackageFixture],
) -> (Vec<PackageDagNode>, Vec<PackageDagImport>) {
    let by_path = packages
        .iter()
        .map(|package| (package.path.as_str(), package.package))
        .collect::<BTreeMap<_, _>>();
    let nodes = packages
        .iter()
        .map(|package| PackageDagNode::new(package.package, package.key.clone()))
        .collect::<Vec<_>>();
    let mut imports = Vec::new();
    for package in packages {
        for occurrence in database.file_imports(package.file).unwrap().direct() {
            imports.push(PackageDagImport::new(
                package.package,
                *by_path.get(occurrence.path()).unwrap(),
                occurrence.clone(),
            ));
        }
    }
    (nodes, imports)
}

#[test]
fn diamond_topology_is_dependency_first_and_order_independent() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let entry = insert_package(
        &mut database,
        &workspace,
        "example.test/a",
        &["example.test/b", "example.test/c"],
    );
    let left = insert_package(
        &mut database,
        &workspace,
        "example.test/b",
        &["example.test/d"],
    );
    let right = insert_package(
        &mut database,
        &workspace,
        "example.test/c",
        &["example.test/d"],
    );
    let leaf = insert_package(&mut database, &workspace, "example.test/d", &[]);
    let packages = [entry, left, right, leaf];
    let (nodes, imports) = graph_inputs(&database, &packages);

    let forward = build_package_dag(packages[0].package, nodes.clone(), imports.clone()).unwrap();
    let reverse = build_package_dag(
        packages[0].package,
        nodes.into_iter().rev(),
        imports.into_iter().rev(),
    )
    .unwrap();
    assert_eq!(forward, reverse);

    let leaf_id = packages[3].package;
    assert_eq!(forward.layers().first().unwrap().packages(), &[leaf_id]);
    let middle = forward.layers().get(1).unwrap().packages();
    assert_eq!(middle.len(), 2);
    assert!(middle.contains(&packages[1].package));
    assert!(middle.contains(&packages[2].package));
    assert_eq!(
        forward.layers().get(2).unwrap().packages(),
        &[packages[0].package]
    );
    for edge in forward.edges() {
        let dependency = forward
            .topological_order()
            .iter()
            .position(|package| *package == edge.dependency())
            .unwrap();
        let importer = forward
            .topological_order()
            .iter()
            .position(|package| *package == edge.importer())
            .unwrap();
        assert!(dependency < importer);
    }
}

#[test]
fn command_line_entry_can_import_canonical_packages() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let dependency = insert_package(&mut database, &workspace, "example.test/dependency", &[]);
    let command_line = PackageKey::command_line();
    let entry_file = database
        .set_source(
            &workspace,
            &command_line,
            "main.go",
            Arc::new(
                SourceSnapshot::from_source(
                    "/fixture/main.go",
                    "package main\nimport \"example.test/dependency\"\n",
                )
                .unwrap(),
            ),
        )
        .unwrap()
        .file();
    let entry = database.package_for_file(entry_file).unwrap();
    let occurrence = database
        .file_imports(entry_file)
        .unwrap()
        .direct()
        .first()
        .unwrap()
        .clone();

    let graph = build_package_dag(
        entry,
        [
            PackageDagNode::new(entry, command_line),
            PackageDagNode::new(dependency.package, dependency.key),
        ],
        [PackageDagImport::new(entry, dependency.package, occurrence)],
    )
    .unwrap();

    assert_eq!(graph.entry(), entry);
    assert_eq!(graph.topological_order(), &[dependency.package, entry]);
    assert!(graph.nodes().iter().any(|node| {
        node.package() == entry && node.key().is_command_line() && node.import_path().is_none()
    }));
}

#[test]
fn duplicate_occurrences_share_one_edge_without_losing_sources() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let entry = insert_package(
        &mut database,
        &workspace,
        "example.test/a",
        &["example.test/b", "example.test/b"],
    );
    let dependency = insert_package(&mut database, &workspace, "example.test/b", &[]);
    let packages = [entry, dependency];
    let (nodes, imports) = graph_inputs(&database, &packages);

    let graph = build_package_dag(packages[0].package, nodes, imports).unwrap();
    assert_eq!(graph.edges().len(), 1);
    let occurrences = graph.edges().first().unwrap().occurrences();
    assert_eq!(occurrences.len(), 2);
    assert_ne!(
        occurrences.first().unwrap().source(),
        occurrences.last().unwrap().source()
    );
}

#[test]
fn cycle_selection_and_source_evidence_are_deterministic() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let first = insert_package(
        &mut database,
        &workspace,
        "example.test/a",
        &["example.test/b"],
    );
    let second = insert_package(
        &mut database,
        &workspace,
        "example.test/b",
        &["example.test/c"],
    );
    let third = insert_package(
        &mut database,
        &workspace,
        "example.test/c",
        &["example.test/a"],
    );
    let packages = [first, second, third];
    let (nodes, imports) = graph_inputs(&database, &packages);

    let forward =
        build_package_dag(packages[0].package, nodes.clone(), imports.clone()).unwrap_err();
    let reverse = build_package_dag(
        packages[0].package,
        nodes.into_iter().rev(),
        imports.into_iter().rev(),
    )
    .unwrap_err();
    assert_eq!(forward, reverse);

    let cycles = forward.cycles().unwrap();
    assert_eq!(cycles.len(), 1);
    let hops = cycles.first().unwrap().hops();
    assert_eq!(hops.len(), 3);
    assert_eq!(
        hops.last().unwrap().dependency(),
        hops.first().unwrap().importer()
    );
    let files_by_package = packages
        .iter()
        .map(|package| (package.package, package.file))
        .collect::<BTreeMap<_, _>>();
    for hop in hops {
        assert_eq!(
            Some(&hop.source().file()),
            files_by_package.get(&hop.importer())
        );
    }
}

#[test]
fn self_import_is_a_source_anchored_cycle() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let package = insert_package(
        &mut database,
        &workspace,
        "example.test/self",
        &["example.test/self"],
    );
    let (nodes, imports) = graph_inputs(&database, std::slice::from_ref(&package));

    let error = build_package_dag(package.package, nodes, imports).unwrap_err();
    let cycle = error.cycles().unwrap().first().unwrap();
    assert_eq!(cycle.hops().len(), 1);
    let hop = cycle.hops().first().unwrap();
    assert_eq!(hop.importer(), package.package);
    assert_eq!(hop.dependency(), package.package);
    assert_eq!(hop.source().file(), package.file);
}

#[test]
fn missing_dependency_is_rejected_before_topology() {
    let workspace = WorkspaceKey::module("example.test").unwrap();
    let mut database = CompilerDatabase::default();
    let entry = insert_package(
        &mut database,
        &workspace,
        "example.test/a",
        &["example.test/missing"],
    );
    let missing = insert_package(&mut database, &workspace, "example.test/missing", &[]);
    let entry_id = entry.package;
    let missing_id = missing.package;
    let packages = [entry, missing];
    let (mut nodes, imports) = graph_inputs(&database, &packages);
    nodes.retain(|node| node.package() != missing_id);

    let error = build_package_dag(entry_id, nodes, imports).unwrap_err();
    assert!(matches!(
        error,
        PackageDagError::UnknownDependency { dependency, .. } if dependency == missing_id
    ));
}
