//! Demand-driven compiler query database.
//!
//! Salsa is an implementation detail of this module. Public callers exchange
//! compiler-owned stable IDs and immutable products only; no Salsa handle or
//! lifetime crosses this facade.

mod model;
mod mutation;
mod products;
mod queries;
mod resolved_imports;
mod source_metadata;
mod source_projection;
mod telemetry;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use salsa::{Durability, Setter as _};

use super::ids::{DefId, FileId, IdentityInterner, PackageId};
use super::input::{PackageKey, SourceSnapshot, WorkspaceKey};
use super::provenance::DefinitionSourceTable;
pub use super::syntax::FunctionLayout;
use crate::source::SourceCoordinateMap;
use queries::{
    BuildInput, ConstantProjection, FileFacts, FunctionProjection, PackageInput, SourceInput,
};
use resolved_imports::ResolvedImportsInput;
use telemetry::Telemetry;

pub use super::fingerprint::Fingerprint;
pub use model::{
    BuildConfig, ConstantDescriptor, FileAnalysis, FileIssue, FunctionBody, FunctionDescriptor,
    FunctionSignature, PackageAnalysis, PackageIssue, ParseFailure, PublicApi, RuntimeAbiId,
};
pub(in crate::compiler) use mutation::SourceInputMutation;
pub use products::{
    CompilerStage, NormalizedMirFunction, StageFailure, TypedFunctionSignature, TypedHirFunction,
    VerifiedMirFunction, VerifiedRustIrFunction, VerifiedRustIrPackage,
};
pub(in crate::compiler) use resolved_imports::ResolvedImportInputMutation;
pub use resolved_imports::{
    ResolvedFileImports, ResolvedImport, ResolvedImportBinding, ResolvedImportBuildError,
    ResolvedImportsUpdate,
};
pub use source_metadata::{
    DirectImport, FileComments, FileImports, ImportBinding, InvalidImport, SourceComment,
};
pub use telemetry::{EngineEventCounts, QueryKind, TelemetrySnapshot};

/// Compiler-database lookup or stable-identity failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryError {
    /// No source input with this compiler-owned file identity exists.
    UnknownFile(FileId),
    /// No active package input with this compiler-owned identity exists.
    UnknownPackage(PackageId),
    /// No resolved-import input for this active source file exists.
    UnknownResolvedImports(FileId),
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
            Self::UnknownResolvedImports(file) => {
                write!(
                    formatter,
                    "no resolved imports installed for source file {file:?}"
                )
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

/// Exact classification of one source installation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceUpdate {
    file: FileId,
    semantic_changed: bool,
    diagnostic_path_changed: bool,
    inserted: bool,
}

impl SourceUpdate {
    /// Stable identity of the installed logical source file.
    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    /// Whether source bytes or logical membership changed.
    #[must_use]
    pub const fn semantic_changed(self) -> bool {
        self.semantic_changed
    }

    /// Whether the user-visible physical path or URI changed.
    #[must_use]
    pub const fn diagnostic_path_changed(self) -> bool {
        self.diagnostic_path_changed
    }

    /// Whether this request introduced a new logical file identity.
    #[must_use]
    pub const fn inserted(self) -> bool {
        self.inserted
    }
}

/// One explicitly owned red-green compiler database.
///
/// Stable logical identities and immutable source content are semantic inputs;
/// user-facing source locations remain owner-side presentation state. The
/// tracked pipeline reaches function-relative typed HIR, verified and
/// normalized MIR, mandatory Rust representation lowering, and deterministic
/// package assembly.
#[salsa::db]
pub struct CompilerDatabase {
    storage: salsa::Storage<Self>,
    telemetry: Arc<Telemetry>,
    identities: IdentityInterner,
    sources: BTreeMap<FileId, SourceInput>,
    diagnostic_paths: BTreeMap<FileId, Arc<str>>,
    packages: BTreeMap<PackageId, PackageInput>,
    resolved_imports: BTreeMap<FileId, ResolvedImportsInput>,
    build: Option<BuildInput>,
}

/// Read-only query handle for one parallel worker.
///
/// Each handle owns Salsa's thread-local runtime state and shares the memo
/// store with its originating [`CompilerDatabase`]. No input mutation API is
/// exposed through this wrapper.
pub(in crate::compiler) struct CompilerDatabaseSnapshot {
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
            .ingredient::<queries::function_layout_product>()
            .ingredient::<queries::public_api_product>()
            .ingredient::<queries::package_analysis_product>()
            .ingredient::<queries::definition_source_table_product>()
            .ingredient::<queries::constant_source_table_product>()
            .ingredient::<queries::semantic_function_product>()
            .ingredient::<queries::typed_hir_product>()
            .ingredient::<queries::typed_signature_product>()
            .ingredient::<queries::typed_constant_product>()
            .ingredient::<queries::package_function_product>()
            .ingredient::<queries::package_function_named_product>()
            .ingredient::<queries::package_constant_named_product>()
            .ingredient::<queries::mir_signature_dependencies_product>()
            .ingredient::<queries::verified_mir_product>()
            .ingredient::<queries::normalized_mir_product>()
            .ingredient::<queries::rust_signature_dependencies_product>()
            .ingredient::<queries::executable_role_product>()
            .ingredient::<queries::rust_ir_root_inputs_product>()
            .ingredient::<queries::verified_rust_ir_product>()
            .ingredient::<queries::rust_ir_package_product>()
            .ingredient::<SourceInput>()
            .ingredient::<PackageInput>()
            .ingredient::<ResolvedImportsInput>()
            .ingredient::<BuildInput>()
            .ingredient::<FileFacts<'_>>()
            .ingredient::<FunctionProjection<'_>>()
            .ingredient::<ConstantProjection<'_>>()
            .build();
        let mut database = Self {
            storage,
            telemetry,
            identities: IdentityInterner::default(),
            sources: BTreeMap::new(),
            diagnostic_paths: BTreeMap::new(),
            packages: BTreeMap::new(),
            resolved_imports: BTreeMap::new(),
            build: None,
        };
        let build = BuildInput::builder(Arc::from(config.go_version()), config.runtime_abi())
            .go_version_durability(Durability::HIGH)
            .runtime_abi_durability(Durability::HIGH)
            .new(&database);
        database.build = Some(build);
        database
    }

    /// Insert or update one immutable source revision.
    ///
    /// `logical_path` is a workspace-relative identity, independent of the
    /// diagnostic path retained by `snapshot`. Salsa tracks only path-independent
    /// [`crate::compiler::input::SourceContent`], so a diagnostic-path-only
    /// update creates no query revision or worker cancellation.
    pub fn set_source(
        &mut self,
        workspace: &WorkspaceKey,
        package: &PackageKey,
        logical_path: &str,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<SourceUpdate, QueryError> {
        let (update, mutation) =
            self.set_source_transactional(workspace, package, logical_path, snapshot)?;
        self.commit_source_mutations(mutation);
        Ok(update)
    }

    /// Evict one active source payload from the database facade.
    ///
    /// Salsa's small input identity remains internal, but the potentially
    /// large source snapshot is replaced before the facade forgets the input.
    /// Parsed ASTs are never retained, so dropping the caller's last `Arc`
    /// releases the old source bytes independently of every other file.
    pub fn remove_source(&mut self, file: FileId) -> Result<(), QueryError> {
        let mutation = self.remove_source_transactional(file)?;
        self.commit_source_mutations(Some(mutation));
        Ok(())
    }

    /// Retained bytes in active immutable source snapshots.
    #[must_use]
    pub fn retained_source_bytes(&self) -> usize {
        self.sources.iter().fold(0_usize, |total, (file, input)| {
            total
                .saturating_add(input.content(self).retained_bytes())
                .saturating_add(self.diagnostic_paths.get(file).map_or(0, |path| path.len()))
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
    pub(in crate::compiler) fn snapshot(&self) -> CompilerDatabaseSnapshot {
        CompilerDatabaseSnapshot {
            database: CompilerDatabase {
                storage: self.storage.clone(),
                telemetry: Arc::clone(&self.telemetry),
                identities: IdentityInterner::default(),
                sources: self.sources.clone(),
                diagnostic_paths: BTreeMap::new(),
                packages: self.packages.clone(),
                resolved_imports: self.resolved_imports.clone(),
                build: self.build,
            },
        }
    }

    /// Current immutable build input.
    pub fn build_config(&self) -> Result<Arc<BuildConfig>, QueryError> {
        self.build
            .map(|build| {
                Arc::new(BuildConfig::new(
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
        if build.go_version(self).as_ref() != config.go_version() {
            build
                .set_go_version(self)
                .to(Arc::from(config.go_version()));
        }
        if build.runtime_abi(self) != config.runtime_abi() {
            build.set_runtime_abi(self).to(config.runtime_abi());
        }
        Ok(())
    }

    /// Demand the deterministic body-independent file index.
    pub fn analyze_file(&self, file: FileId) -> Result<Arc<FileAnalysis>, QueryError> {
        let facts = self.file_facts(file)?;
        Ok(queries::file_analysis_product(self, facts))
    }

    /// Demand owned direct-import facts from the file's shared parse query.
    pub fn file_imports(&self, file: FileId) -> Result<Arc<FileImports>, QueryError> {
        Ok(self.file_facts(file)?.imports(self))
    }

    /// Demand owned checkout-independent comments from the shared parse query.
    pub fn file_comments(&self, file: FileId) -> Result<Arc<FileComments>, QueryError> {
        Ok(self.file_facts(file)?.comments(self))
    }

    /// Demand the scanner-built physical-to-adjusted map from the shared parse query.
    pub fn source_coordinate_map(
        &self,
        file: FileId,
    ) -> Result<Arc<SourceCoordinateMap>, QueryError> {
        Ok(self.file_facts(file)?.coordinate_map(self))
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

    /// Demand one stable function's revision-local physical layout.
    pub fn function_layout(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<FunctionLayout>, QueryError> {
        let function = self.function_projection(file, function)?;
        Ok(queries::function_layout_product(self, function))
    }

    /// Current physical source table for one stable function definition.
    pub fn definition_source_table(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<DefinitionSourceTable>, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        if let Ok(function) = self.function_projection(file, function) {
            return queries::definition_source_table_product(self, input, function)
                .map_err(QueryError::StageFailure);
        }
        let constant = self.constant_projection(file, function)?;
        queries::constant_source_table_product(self, file, constant)
            .map_err(QueryError::StageFailure)
    }

    /// Demand one stable definition's typed HIR product.
    pub fn typed_hir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<TypedHirFunction>, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        let function = self.function_projection(file, function)?;
        queries::typed_hir_product(self, input, function).map_err(QueryError::StageFailure)
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

    /// Canonical invalidation inputs for one stable Rust-IR function root.
    pub(in crate::compiler) fn rust_ir_root_inputs(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Fingerprint, QueryError> {
        let package = self.package_for_file(file)?;
        let input = self.package_input(package)?;
        let function = self.function_projection(file, function)?;
        queries::rust_ir_root_inputs_product(self, input, function)
            .map(|fingerprint| *fingerprint)
            .map_err(QueryError::StageFailure)
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
        let input = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        let path = self
            .diagnostic_paths
            .get(&file)
            .cloned()
            .ok_or(QueryError::UnknownFile(file))?;
        Ok(Arc::new(SourceSnapshot::from_content(
            path,
            input.content(self),
        )))
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

    fn constant_projection(
        &self,
        file: FileId,
        definition: DefId,
    ) -> Result<ConstantProjection<'_>, QueryError> {
        let facts = self.file_facts(file)?;
        facts
            .constants(self)
            .into_iter()
            .find(|candidate| candidate.id(self) == definition)
            .ok_or(QueryError::UnknownFunction {
                file,
                function: definition,
            })
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
    pub(in crate::compiler) fn verified_rust_ir(
        &self,
        file: FileId,
        function: DefId,
    ) -> Result<Arc<VerifiedRustIrFunction>, QueryError> {
        self.database.verified_rust_ir(file, function)
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
