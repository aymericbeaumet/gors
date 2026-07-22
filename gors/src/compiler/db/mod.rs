//! Demand-driven compiler query database.
//!
//! Salsa is an implementation detail of this module. Public callers exchange
//! compiler-owned stable IDs and immutable products only; no Salsa handle or
//! lifetime crosses this facade.

mod model;
mod products;
mod provenance;
mod queries;
mod telemetry;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use salsa::{Durability, Setter as _};

use crate::parser::SourceSnapshot;

use super::ids::{DefId, FileId, IdentityInterner, PackageId};
use queries::{BuildInput, FileFacts, FunctionProjection, PackageInput, SourceInput};
use telemetry::Telemetry;

pub use super::fingerprint::Fingerprint;
pub use model::{
    BuildConfig, FileAnalysis, FileIssue, FunctionBody, FunctionDescriptor, FunctionSignature,
    PackageAnalysis, PackageIssue, ParseFailure, PublicApi,
};
pub use products::{
    CompilerStage, FunctionProvenance, NormalizedMirFunction, StageFailure, TypedFunctionSignature,
    TypedHirFunction, VerifiedMirFunction, VerifiedRustIrFunction, VerifiedRustIrPackage,
};
pub use telemetry::{EngineEventCounts, QueryKind, TelemetrySnapshot};

/// Compiler-database lookup or stable-identity failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    /// No source input with this compiler-owned file identity exists.
    UnknownFile(FileId),
    /// No active package input with this compiler-owned identity exists.
    UnknownPackage(PackageId),
    /// The file index contains no function with this stable identity.
    UnknownFunction { file: FileId, function: DefId },
    /// Stable identity interning detected a full-key digest collision.
    IdentityCollision(Arc<str>),
    /// The database was observed before its required config input was installed.
    MissingBuildConfig,
    /// A tracked compiler stage rejected its immutable input.
    StageFailure(Arc<StageFailure>),
    /// Package indexing found deterministic cross-file or syntax issues.
    PackageIssues {
        package: PackageId,
        issues: Arc<[PackageIssue]>,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFile(file) => write!(formatter, "unknown source file {file:?}"),
            Self::UnknownPackage(package) => {
                write!(formatter, "unknown source package {package:?}")
            }
            Self::UnknownFunction { file, function } => {
                write!(
                    formatter,
                    "unknown function {function} in source file {file:?}"
                )
            }
            Self::IdentityCollision(message) => formatter.write_str(message),
            Self::MissingBuildConfig => formatter.write_str("compiler build config is missing"),
            Self::StageFailure(failure) => {
                write!(formatter, "{:?} query failed", failure.stage())
            }
            Self::PackageIssues { package, issues } => {
                write!(
                    formatter,
                    "package {package:?} has {} indexing issue(s)",
                    issues.len()
                )
            }
        }
    }
}

impl std::error::Error for QueryError {}

/// One explicitly owned red-green compiler database.
///
/// This first slice caches parse/index projections and separates public
/// function headers from bodies. It does not yet claim that HIR, MIR, or Rust
/// representation lowering are incremental.
#[salsa::db]
pub struct CompilerDatabase {
    storage: salsa::Storage<Self>,
    telemetry: Arc<Telemetry>,
    identities: IdentityInterner,
    sources: BTreeMap<FileId, SourceInput>,
    packages: BTreeMap<PackageId, PackageInput>,
    build: Option<BuildInput>,
}

/// Read-only query handle for one parallel worker.
///
/// Each handle owns Salsa's thread-local runtime state and shares the memo
/// store with its originating [`CompilerDatabase`]. No input mutation API is
/// exposed through this wrapper.
pub struct CompilerDatabaseSnapshot {
    database: CompilerDatabase,
}

impl CompilerDatabase {
    /// Create an empty query database with explicit build inputs.
    #[must_use]
    pub fn new(config: BuildConfig) -> Self {
        let telemetry = Arc::new(Telemetry::default());
        let event_telemetry = Arc::clone(&telemetry);
        let storage = salsa::Storage::builder()
            .event_callback(Box::new(move |event| {
                event_telemetry.record_event(&event.kind);
            }))
            .ingredient::<queries::file_projection>()
            .ingredient::<queries::file_analysis_product>()
            .ingredient::<queries::signature_product>()
            .ingredient::<queries::body_product>()
            .ingredient::<queries::public_api_product>()
            .ingredient::<queries::package_analysis_product>()
            .ingredient::<queries::semantic_status_product>()
            .ingredient::<queries::provenance_product>()
            .ingredient::<queries::typed_hir_product>()
            .ingredient::<queries::typed_signature_product>()
            .ingredient::<queries::package_function_product>()
            .ingredient::<queries::mir_signature_dependencies_product>()
            .ingredient::<queries::verified_mir_product>()
            .ingredient::<queries::normalized_mir_product>()
            .ingredient::<queries::rust_signature_dependencies_product>()
            .ingredient::<queries::executable_role_product>()
            .ingredient::<queries::verified_rust_ir_product>()
            .ingredient::<queries::rust_ir_package_product>()
            .ingredient::<SourceInput>()
            .ingredient::<PackageInput>()
            .ingredient::<BuildInput>()
            .ingredient::<FileFacts<'_>>()
            .ingredient::<FunctionProjection<'_>>()
            .build();
        let mut database = Self {
            storage,
            telemetry,
            identities: IdentityInterner::default(),
            sources: BTreeMap::new(),
            packages: BTreeMap::new(),
            build: None,
        };
        let build = BuildInput::builder(
            Arc::from(config.target()),
            Arc::from(config.go_version()),
            Arc::from(config.runtime_abi()),
        )
        .target_durability(Durability::HIGH)
        .go_version_durability(Durability::HIGH)
        .runtime_abi_durability(Durability::HIGH)
        .new(&database);
        database.build = Some(build);
        database
    }

    /// Insert or update one immutable source snapshot.
    ///
    /// `logical_path` is a workspace-relative identity, independent of the
    /// diagnostic path retained by `snapshot`. Setting an equal snapshot is a
    /// true no-op and creates no query revision.
    pub fn set_source(
        &mut self,
        workspace: &str,
        package: &str,
        logical_path: &str,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<FileId, QueryError> {
        let workspace = self
            .identities
            .workspace(workspace)
            .map_err(identity_error)?;
        let package = self
            .identities
            .package(workspace, package)
            .map_err(identity_error)?;
        let file = self
            .identities
            .file(package, logical_path)
            .map_err(identity_error)?;

        if let Some(source) = self.sources.get(&file).copied() {
            if source.snapshot(self).as_ref() != snapshot.as_ref() {
                source.set_snapshot(self).to(snapshot);
            }
        } else {
            let source = SourceInput::new(self, package, file, Arc::from(logical_path), snapshot);
            self.sources.insert(file, source);
            self.add_package_source(package, source);
        }
        Ok(file)
    }

    /// Evict one active source payload from the database facade.
    ///
    /// Salsa's small input identity remains internal, but the potentially
    /// large source snapshot is replaced before the facade forgets the input.
    /// Parsed ASTs are never retained, so dropping the caller's last `Arc`
    /// releases the old source bytes independently of every other file.
    pub fn remove_source(&mut self, file: FileId) -> Result<(), QueryError> {
        let source = self
            .sources
            .remove(&file)
            .ok_or(QueryError::UnknownFile(file))?;
        let package = source.package(self);
        self.remove_package_source(package, file)?;
        let tombstone = Arc::new(SourceSnapshot::from_source("", ""));
        drop(source.set_snapshot(self).to(tombstone));
        Ok(())
    }

    /// Retained bytes in active immutable source snapshots.
    #[must_use]
    pub fn retained_source_bytes(&self) -> usize {
        self.sources.values().fold(0_usize, |total, source| {
            total.saturating_add(source.snapshot(self).retained_bytes())
        })
    }

    /// Active stable file identities in deterministic order.
    #[must_use]
    pub fn active_files(&self) -> Vec<FileId> {
        self.sources.keys().copied().collect()
    }

    /// Create a read-only handle suitable for moving to another worker.
    ///
    /// IDs and input handles are copied deterministically; immutable snapshots
    /// and query memos remain reference counted by Salsa.
    #[must_use]
    pub fn snapshot(&self) -> CompilerDatabaseSnapshot {
        CompilerDatabaseSnapshot {
            database: CompilerDatabase {
                storage: self.storage.clone(),
                telemetry: Arc::clone(&self.telemetry),
                identities: IdentityInterner::default(),
                sources: self.sources.clone(),
                packages: self.packages.clone(),
                build: self.build,
            },
        }
    }

    /// Current immutable build input.
    pub fn build_config(&self) -> Result<Arc<BuildConfig>, QueryError> {
        self.build
            .map(|build| {
                Arc::new(BuildConfig::new(
                    build.target(self),
                    build.go_version(self),
                    build.runtime_abi(self),
                ))
            })
            .ok_or(QueryError::MissingBuildConfig)
    }

    /// Replace the explicit build input, skipping structurally equal updates.
    pub fn set_build_config(&mut self, config: BuildConfig) -> Result<(), QueryError> {
        let Some(build) = self.build else {
            return Err(QueryError::MissingBuildConfig);
        };
        if build.target(self).as_ref() != config.target() {
            build.set_target(self).to(Arc::from(config.target()));
        }
        if build.go_version(self).as_ref() != config.go_version() {
            build
                .set_go_version(self)
                .to(Arc::from(config.go_version()));
        }
        if build.runtime_abi(self).as_ref() != config.runtime_abi() {
            build
                .set_runtime_abi(self)
                .to(Arc::from(config.runtime_abi()));
        }
        Ok(())
    }

    /// Demand the deterministic body-independent file index.
    pub fn analyze_file(&self, file: FileId) -> Result<Arc<FileAnalysis>, QueryError> {
        let facts = self.file_facts(file)?;
        Ok(queries::file_analysis_product(self, facts))
    }

    /// Stable package owning an active source file.
    pub fn package_for_file(&self, file: FileId) -> Result<PackageId, QueryError> {
        self.sources
            .get(&file)
            .copied()
            .map(|source| source.package(self))
            .ok_or(QueryError::UnknownFile(file))
    }

    /// Demand the deterministic body-independent index for a source package.
    pub fn analyze_package(&self, package: PackageId) -> Result<Arc<PackageAnalysis>, QueryError> {
        let input = self
            .packages
            .get(&package)
            .copied()
            .ok_or(QueryError::UnknownPackage(package))?;
        Ok(queries::package_analysis_product(self, input))
    }

    /// Demand the public-header aggregate for one file.
    pub fn public_api(&self, file: FileId) -> Result<Arc<PublicApi>, QueryError> {
        let facts = self.file_facts(file)?;
        Ok(queries::public_api_product(self, facts))
    }

    /// Demand one stable function's structural header projection.
    pub fn function_signature(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionSignature>, QueryError> {
        let function = self.function_projection(file, function)?;
        Ok(queries::signature_product(self, function))
    }

    /// Demand one stable function's structural body projection.
    pub fn function_body(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionBody>, QueryError> {
        let function = self.function_projection(file, function)?;
        Ok(queries::body_product(self, function))
    }

    /// Demand file-level semantic validation without reparsing its snapshot.
    pub fn semantic_status(&self, file: FileId) -> Result<(), QueryError> {
        let facts = self.file_facts(file)?;
        queries::semantic_status_product(self, facts).map_err(QueryError::StageFailure)
    }

    /// Current source anchor for one stable function definition.
    pub fn function_provenance(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionProvenance>, QueryError> {
        let function = self.function_projection(file, function)?;
        Ok(queries::provenance_product(self, function))
    }

    /// Demand one stable definition's typed HIR product.
    pub fn typed_hir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<TypedHirFunction>, QueryError> {
        let function = self.function_projection(file, function)?;
        queries::typed_hir_product(self, function).map_err(QueryError::StageFailure)
    }

    /// Demand one stable definition's exact typed signature independently of its body.
    pub fn typed_signature(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<TypedFunctionSignature>, QueryError> {
        let function = self.function_projection(file, function)?;
        queries::typed_signature_product(self, function).map_err(QueryError::StageFailure)
    }

    /// Demand one stable definition's verified explicit-order Go MIR.
    pub fn verified_mir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<VerifiedMirFunction>, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        let function = self.function_projection(file, function)?;
        queries::verified_mir_product(self, input, function).map_err(QueryError::StageFailure)
    }

    /// Demand mandatory normalized and reverified Go MIR.
    pub fn normalized_mir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<NormalizedMirFunction>, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        let function = self.function_projection(file, function)?;
        queries::normalized_mir_product(self, input, function).map_err(QueryError::StageFailure)
    }

    /// Demand one stable definition's verified Rust representation IR.
    pub fn verified_rust_ir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<VerifiedRustIrFunction>, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        let function = self.function_projection(file, function)?;
        queries::verified_rust_ir_product(self, input, function).map_err(QueryError::StageFailure)
    }

    /// Assemble a complete verified Rust IR package from tracked definitions.
    pub fn verified_rust_ir_package(
        &self,
        package: PackageId,
    ) -> Result<Arc<VerifiedRustIrPackage>, QueryError> {
        let input = self.package_input(package)?;
        let analysis = queries::package_analysis_product(self, input);
        if !analysis.issues().is_empty() {
            return Err(QueryError::PackageIssues {
                package,
                issues: Arc::from(analysis.issues()),
            });
        }
        queries::rust_ir_package_product(self, input).map_err(QueryError::StageFailure)
    }

    /// Shared immutable source input for terminal provenance publication.
    pub fn source_snapshot(&self, file: FileId) -> Result<Arc<SourceSnapshot>, QueryError> {
        self.sources
            .get(&file)
            .copied()
            .map(|source| source.snapshot(self))
            .ok_or(QueryError::UnknownFile(file))
    }

    /// Restore an already-registered file payload during session transaction rollback.
    pub(in crate::compiler) fn restore_source_snapshot(
        &mut self,
        file: FileId,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<(), QueryError> {
        let source = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        if source.snapshot(self).as_ref() != snapshot.as_ref() {
            source.set_snapshot(self).to(snapshot);
        }
        Ok(())
    }

    /// Portable package-relative logical filename for one source input.
    pub fn logical_path(&self, file: FileId) -> Result<Arc<str>, QueryError> {
        self.sources
            .get(&file)
            .copied()
            .map(|source| source.logical_path(self))
            .ok_or(QueryError::UnknownFile(file))
    }

    /// Snapshot query execution counters and coarse engine events.
    #[must_use]
    pub fn telemetry(&self) -> TelemetrySnapshot {
        self.telemetry.snapshot()
    }

    /// Reset observations without changing semantic inputs or query revisions.
    pub fn reset_telemetry(&self) {
        self.telemetry.reset();
    }

    fn file_facts(&self, file: FileId) -> Result<FileFacts<'_>, QueryError> {
        let source = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        Ok(queries::file_projection(self, source))
    }

    fn function_projection(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<FunctionProjection<'_>, QueryError> {
        let facts = self.file_facts(file)?;
        facts
            .functions(self)
            .into_iter()
            .find(|candidate| candidate.id(self) == function)
            .ok_or(QueryError::UnknownFunction { file, function })
    }

    fn package_input(&self, package: PackageId) -> Result<PackageInput, QueryError> {
        self.packages
            .get(&package)
            .copied()
            .ok_or(QueryError::UnknownPackage(package))
    }

    fn add_package_source(&mut self, package: PackageId, source: SourceInput) {
        if let Some(input) = self.packages.get(&package).copied() {
            let mut sources = input.sources(self).iter().copied().collect::<Vec<_>>();
            sources.push(source);
            sources.sort_by_key(|source| source.file(self));
            input.set_sources(self).to(sources.into());
        } else {
            let input = PackageInput::new(self, package, Arc::from([source]));
            self.packages.insert(package, input);
        }
    }

    fn remove_package_source(
        &mut self,
        package: PackageId,
        file: FileId,
    ) -> Result<(), QueryError> {
        let input = self
            .packages
            .get(&package)
            .copied()
            .ok_or(QueryError::UnknownPackage(package))?;
        let sources = input
            .sources(self)
            .iter()
            .copied()
            .filter(|source| source.file(self) != file)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            input.set_sources(self).to(Arc::from([]));
            self.packages.remove(&package);
        } else {
            input.set_sources(self).to(sources.into());
        }
        Ok(())
    }
}

impl CompilerDatabaseSnapshot {
    /// Demand the deterministic body-independent file index.
    pub fn analyze_file(&self, file: FileId) -> Result<Arc<FileAnalysis>, QueryError> {
        self.database.analyze_file(file)
    }

    /// Stable package owning an active source file.
    pub fn package_for_file(&self, file: FileId) -> Result<PackageId, QueryError> {
        self.database.package_for_file(file)
    }

    /// Demand the deterministic body-independent package index.
    pub fn analyze_package(&self, package: PackageId) -> Result<Arc<PackageAnalysis>, QueryError> {
        self.database.analyze_package(package)
    }

    /// Demand the public-header aggregate for one file.
    pub fn public_api(&self, file: FileId) -> Result<Arc<PublicApi>, QueryError> {
        self.database.public_api(file)
    }

    /// Demand one stable function's structural header projection.
    pub fn function_signature(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionSignature>, QueryError> {
        self.database.function_signature(file, function)
    }

    /// Demand one stable function's structural body projection.
    pub fn function_body(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionBody>, QueryError> {
        self.database.function_body(file, function)
    }

    pub fn typed_hir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<TypedHirFunction>, QueryError> {
        self.database.typed_hir(file, function)
    }

    pub fn typed_signature(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<TypedFunctionSignature>, QueryError> {
        self.database.typed_signature(file, function)
    }

    pub fn verified_mir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<VerifiedMirFunction>, QueryError> {
        self.database.verified_mir(file, function)
    }

    pub fn normalized_mir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<NormalizedMirFunction>, QueryError> {
        self.database.normalized_mir(file, function)
    }

    pub fn verified_rust_ir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<VerifiedRustIrFunction>, QueryError> {
        self.database.verified_rust_ir(file, function)
    }

    pub fn verified_rust_ir_package(
        &self,
        package: PackageId,
    ) -> Result<Arc<VerifiedRustIrPackage>, QueryError> {
        self.database.verified_rust_ir_package(package)
    }
}

impl Default for CompilerDatabase {
    fn default() -> Self {
        Self::new(BuildConfig::default())
    }
}

#[salsa::db]
impl salsa::Database for CompilerDatabase {}

#[salsa::db]
impl queries::Db for CompilerDatabase {
    fn query_telemetry(&self) -> &Telemetry {
        &self.telemetry
    }

    fn query_build_input(&self) -> Option<BuildInput> {
        self.build
    }
}

fn identity_error(error: impl fmt::Display) -> QueryError {
    QueryError::IdentityCollision(Arc::from(error.to_string()))
}
