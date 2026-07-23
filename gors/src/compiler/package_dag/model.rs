use std::fmt;
use std::sync::Arc;

use crate::compiler::db::DirectImport;
use crate::compiler::ids::PackageId;
use crate::compiler::input::PackageKey;
use crate::compiler::provenance::FileRange;
use crate::import_path::CanonicalImportPath;

/// One package admitted under its exact compiler input key.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageDagNode {
    package: PackageId,
    key: PackageKey,
}

impl PackageDagNode {
    #[must_use]
    pub fn new(package: PackageId, key: PackageKey) -> Self {
        Self { package, key }
    }

    #[must_use]
    pub const fn package(&self) -> PackageId {
        self.package
    }

    #[must_use]
    pub const fn key(&self) -> &PackageKey {
        &self.key
    }

    /// Canonical identity for importable packages.
    ///
    /// A command-line entry deliberately has no synthetic import identity.
    #[must_use]
    pub const fn import_path(&self) -> Option<&CanonicalImportPath> {
        self.key.canonical_import_path()
    }
}

/// One resolved import occurrence supplied to the pure DAG builder.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageDagImport {
    importer: PackageId,
    dependency: PackageId,
    occurrence: DirectImport,
}

impl PackageDagImport {
    #[must_use]
    pub fn new(importer: PackageId, dependency: PackageId, occurrence: DirectImport) -> Self {
        Self {
            importer,
            dependency,
            occurrence,
        }
    }

    #[must_use]
    pub const fn importer(&self) -> PackageId {
        self.importer
    }

    #[must_use]
    pub const fn dependency(&self) -> PackageId {
        self.dependency
    }

    #[must_use]
    pub const fn occurrence(&self) -> &DirectImport {
        &self.occurrence
    }
}

/// One semantic dependency edge with every source occurrence retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDagEdge {
    importer: PackageId,
    dependency: PackageId,
    occurrences: Arc<[DirectImport]>,
}

impl PackageDagEdge {
    pub(super) fn new(
        importer: PackageId,
        dependency: PackageId,
        occurrences: Arc<[DirectImport]>,
    ) -> Self {
        Self {
            importer,
            dependency,
            occurrences,
        }
    }

    #[must_use]
    pub const fn importer(&self) -> PackageId {
        self.importer
    }

    #[must_use]
    pub const fn dependency(&self) -> PackageId {
        self.dependency
    }

    /// Import occurrences in stable path, binding, and source order.
    #[must_use]
    pub fn occurrences(&self) -> &[DirectImport] {
        &self.occurrences
    }
}

/// Packages that have no dependencies outside earlier layers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDagLayer {
    packages: Arc<[PackageId]>,
}

impl PackageDagLayer {
    pub(super) fn new(packages: Arc<[PackageId]>) -> Self {
        Self { packages }
    }

    /// Independently ready packages in stable identity order.
    #[must_use]
    pub fn packages(&self) -> &[PackageId] {
        &self.packages
    }
}

/// Immutable, acyclic, canonical package dependency graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDag {
    entry: PackageId,
    nodes: Arc<[PackageDagNode]>,
    edges: Arc<[PackageDagEdge]>,
    layers: Arc<[PackageDagLayer]>,
    topological_order: Arc<[PackageId]>,
}

impl PackageDag {
    pub(super) fn new(
        entry: PackageId,
        nodes: Arc<[PackageDagNode]>,
        edges: Arc<[PackageDagEdge]>,
        layers: Arc<[PackageDagLayer]>,
        topological_order: Arc<[PackageId]>,
    ) -> Self {
        Self {
            entry,
            nodes,
            edges,
            layers,
            topological_order,
        }
    }

    #[must_use]
    pub const fn entry(&self) -> PackageId {
        self.entry
    }

    /// Nodes in stable package-identity order.
    #[must_use]
    pub fn nodes(&self) -> &[PackageDagNode] {
        &self.nodes
    }

    /// Edges in stable `(importer, dependency)` order.
    #[must_use]
    pub fn edges(&self) -> &[PackageDagEdge] {
        &self.edges
    }

    /// Maximal deterministic dependency-ready frontiers.
    #[must_use]
    pub fn layers(&self) -> &[PackageDagLayer] {
        &self.layers
    }

    /// Flattened dependency-before-importer order.
    #[must_use]
    pub fn topological_order(&self) -> &[PackageId] {
        &self.topological_order
    }
}

/// One source-anchored hop in a deterministic closed import cycle.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageCycleHop {
    importer: PackageId,
    dependency: PackageId,
    import_path: CanonicalImportPath,
    source: FileRange,
}

impl PackageCycleHop {
    pub(super) fn from_import(import: &PackageDagImport) -> Self {
        Self {
            importer: import.importer,
            dependency: import.dependency,
            import_path: import.occurrence.canonical_path().clone(),
            source: import.occurrence.source(),
        }
    }

    #[must_use]
    pub const fn importer(&self) -> PackageId {
        self.importer
    }

    #[must_use]
    pub const fn dependency(&self) -> PackageId {
        self.dependency
    }

    #[must_use]
    pub const fn import_path(&self) -> &CanonicalImportPath {
        &self.import_path
    }

    #[must_use]
    pub const fn source(&self) -> FileRange {
        self.source
    }
}

/// One canonical closed path selected from an illegal strongly connected set.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageCycle {
    hops: Arc<[PackageCycleHop]>,
}

impl PackageCycle {
    pub(super) fn new(hops: Arc<[PackageCycleHop]>) -> Self {
        Self { hops }
    }

    /// Closed path hops; the final dependency equals the first importer.
    #[must_use]
    pub fn hops(&self) -> &[PackageCycleHop] {
        &self.hops
    }
}

/// Invalid resolved package-graph input or illegal import cycles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageDagError {
    UnknownEntry {
        entry: PackageId,
    },
    ConflictingPackageIdentity {
        package: PackageId,
        first: PackageKey,
        second: PackageKey,
    },
    ConflictingImportPath {
        import_path: CanonicalImportPath,
        first: PackageId,
        second: PackageId,
    },
    UnknownImporter {
        importer: PackageId,
        source: FileRange,
    },
    UnknownDependency {
        dependency: PackageId,
        source: FileRange,
    },
    CommandLineDependency {
        dependency: PackageId,
        source: FileRange,
    },
    DependencyPathMismatch {
        dependency: PackageId,
        expected: CanonicalImportPath,
        found: CanonicalImportPath,
        source: FileRange,
    },
    Cycles {
        cycles: Arc<[PackageCycle]>,
    },
    InconsistentTopology,
}

impl PackageDagError {
    #[must_use]
    pub fn cycles(&self) -> Option<&[PackageCycle]> {
        match self {
            Self::Cycles { cycles } => Some(cycles),
            _ => None,
        }
    }
}

impl fmt::Display for PackageDagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEntry { entry } => {
                write!(
                    formatter,
                    "entry package {entry:?} is absent from the package DAG"
                )
            }
            Self::ConflictingPackageIdentity {
                package,
                first,
                second,
            } => write!(
                formatter,
                "package {package:?} has conflicting import identities {first:?} and {second:?}"
            ),
            Self::ConflictingImportPath {
                import_path,
                first,
                second,
            } => write!(
                formatter,
                "canonical import path {import_path:?} identifies both {first:?} and {second:?}"
            ),
            Self::UnknownImporter { importer, .. } => {
                write!(
                    formatter,
                    "importer package {importer:?} is absent from the package DAG"
                )
            }
            Self::UnknownDependency { dependency, .. } => write!(
                formatter,
                "dependency package {dependency:?} is absent from the package DAG"
            ),
            Self::CommandLineDependency { dependency, .. } => write!(
                formatter,
                "dependency {dependency:?} has a command-line key and cannot be imported"
            ),
            Self::DependencyPathMismatch {
                dependency,
                expected,
                found,
                ..
            } => write!(
                formatter,
                "dependency {dependency:?} is indexed as {expected:?} but imported as {found:?}"
            ),
            Self::Cycles { cycles } => {
                write!(
                    formatter,
                    "package graph contains {} import cycle(s)",
                    cycles.len()
                )
            }
            Self::InconsistentTopology => {
                formatter.write_str("package DAG topology is internally inconsistent")
            }
        }
    }
}

impl std::error::Error for PackageDagError {}
