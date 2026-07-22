//! Stateful production compiler session backed by the red-green query graph.

mod prewarm;
mod readiness;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

use self::readiness::RustIrRoot;
use super::db::{
    BuildConfig, CompilerDatabase, Fingerprint, PackageAnalysis, PackageIssue, ParseFailure,
    QueryError, SourceInputMutation, StageFailure,
};
use super::diagnostic::DiagnosticLocation;
use super::ids::{FileId, PackageId};
use super::input::{PackageInputManifest, ProgramInput, WorkspaceKey};
use super::provenance::{DefinitionSourceTable, FileRange, SourceRef};
use super::scheduler::{CompilerHost, SchedulerTelemetry};
use super::{CompiledProgram, CompilerDiagnostic, CompilerError, SourceMapPlan, emit};

/// One reusable production compiler context.
///
/// Reusing a session across edits preserves Salsa revisions and stage products.
/// The convenience free functions create a short-lived session, while editors,
/// build daemons, and performance harnesses should retain this value.
pub struct CompilerSession {
    database: CompilerDatabase,
    host: CompilerHost,
    ready_rust_ir_roots: BTreeMap<RustIrRoot, Fingerprint>,
}

impl CompilerSession {
    /// Create a session from explicit, ambient-environment-free build inputs.
    ///
    /// The production source packager embeds exactly one runtime ABI. Synthetic
    /// multi-ABI query tests may construct [`CompilerDatabase`] directly, but a
    /// production session must never label generated Rust with an ABI that its
    /// terminal artifact cannot package.
    pub fn new(config: BuildConfig) -> Result<Self, CompilerError> {
        CompilerHost::inline().session(config)
    }

    /// Create a private host with an exact positive compiler job budget.
    ///
    /// Callers managing multiple sessions should construct one
    /// [`CompilerHost`] and use [`CompilerHost::session`] so they share a
    /// single bounded worker pool.
    pub fn with_job_budget(
        config: BuildConfig,
        job_budget: NonZeroUsize,
    ) -> Result<Self, CompilerError> {
        CompilerHost::new(job_budget)?.session(config)
    }

    pub(super) fn from_host(
        config: BuildConfig,
        host: CompilerHost,
    ) -> Result<Self, CompilerError> {
        validate_packaged_runtime_abi(&config)?;
        Ok(Self::from_validated_host(config, host))
    }

    fn from_validated_host(config: BuildConfig, host: CompilerHost) -> Self {
        Self {
            database: CompilerDatabase::new(config),
            host,
            ready_rust_ir_roots: BTreeMap::new(),
        }
    }

    /// Read the owned database for telemetry and immutable stage inspection.
    #[must_use]
    pub const fn database(&self) -> &CompilerDatabase {
        &self.database
    }

    /// Host-level maximum parallelism shared by this session.
    #[must_use]
    pub fn job_budget(&self) -> NonZeroUsize {
        self.host.job_budget()
    }

    /// Current non-semantic scheduler counters.
    #[must_use]
    pub fn scheduler_telemetry(&self) -> SchedulerTelemetry {
        self.host.telemetry()
    }

    /// Change explicit build inputs while preserving target-independent memos.
    pub fn set_build_config(&mut self, config: BuildConfig) -> Result<(), CompilerError> {
        validate_packaged_runtime_abi(&config)?;
        let changed = self
            .database
            .build_config()
            .map_err(|error| self.query_error(error))?
            .as_ref()
            != &config;
        self.database
            .set_build_config(config)
            .map_err(|error| self.query_error(error))?;
        if changed {
            self.ready_rust_ir_roots.clear();
        }
        Ok(())
    }

    /// Compile through the complete tracked semantic and representation spine.
    pub fn compile_program(
        &mut self,
        program: ProgramInput,
    ) -> Result<CompiledProgram, CompilerError> {
        self.compile(program, false).map(|(compiled, _)| compiled)
    }

    /// Compile and retain independently owned source-map inputs.
    pub fn compile_program_with_source_map(
        &mut self,
        program: ProgramInput,
    ) -> Result<(CompiledProgram, SourceMapPlan), CompilerError> {
        self.compile(program, true).and_then(|(compiled, plan)| {
            plan.map(|plan| (compiled, plan)).ok_or_else(|| {
                CompilerError::backend("source-map plan was not constructed by the session")
            })
        })
    }

    fn compile(
        &mut self,
        program: ProgramInput,
        with_source_map: bool,
    ) -> Result<(CompiledProgram, Option<SourceMapPlan>), CompilerError> {
        let installed = self.install_program(&program)?;
        let main_analysis = self
            .database
            .analyze_package(installed.main_package)
            .map_err(|error| self.query_error(error))?;
        self.reconcile_ready_roots(&main_analysis);
        if !main_analysis.issues().is_empty() {
            return Err(self.package_issues(installed.main_package, main_analysis.issues()));
        }
        self.validate_bootstrap_boundary(&installed, &main_analysis)?;

        for file in &installed.main_files {
            self.database
                .semantic_status(file.id)
                .map_err(|error| self.query_error(error))?;
        }
        let current_root_inputs = self.current_root_inputs(&main_analysis)?;
        let roots = self.roots_requiring_prewarm(&current_root_inputs);
        self.prewarm_rust_ir(&roots)?;
        // A completed wave marks each input-equivalent root ready even when
        // canonical package assembly selects a cached stage failure below.
        self.publish_ready_roots(current_root_inputs);
        let rust_ir = self
            .database
            .verified_rust_ir_package(installed.main_package)
            .map_err(|error| self.query_error(error))?;
        let source_map = with_source_map
            .then(|| self.source_map_plan(&installed, &main_analysis, rust_ir.file()))
            .transpose()?;
        let entry = emit::emit_file(rust_ir.file()).map_err(CompilerError::terminal)?;
        Ok((
            CompiledProgram {
                entry,
                modules: BTreeMap::new(),
            },
            source_map,
        ))
    }

    fn install_program(
        &mut self,
        program: &ProgramInput,
    ) -> Result<InstalledProgram, CompilerError> {
        self.install_program_transaction(program, |_| Ok(()))
    }

    fn install_program_transaction<F>(
        &mut self,
        program: &ProgramInput,
        before_commit: F,
    ) -> Result<InstalledProgram, CompilerError>
    where
        F: FnOnce(&CompilerDatabase) -> Result<(), CompilerError>,
    {
        let mut mutations = Vec::new();
        let result = (|| {
            let (installed, next_sources) = self.install_program_inputs(program, &mut mutations)?;
            let stale = self
                .database
                .active_files()
                .into_iter()
                .filter(|file| !next_sources.contains(file))
                .collect::<Vec<_>>();
            for file in stale {
                let mutation = self
                    .database
                    .remove_source_transactional(file)
                    .map_err(|error| self.query_error(error))?;
                mutations.push(mutation);
            }
            before_commit(&self.database)?;
            Ok((installed, next_sources))
        })();
        match result {
            Ok((installed, next_sources)) => {
                self.database.commit_source_mutations(mutations);
                self.retain_ready_roots_for_files(&next_sources);
                Ok(installed)
            }
            Err(error) => {
                self.database
                    .rollback_source_mutations(mutations.into_iter().rev());
                Err(error)
            }
        }
    }

    fn install_program_inputs(
        &mut self,
        program: &ProgramInput,
        mutations: &mut Vec<SourceInputMutation>,
    ) -> Result<(InstalledProgram, BTreeSet<FileId>), CompilerError> {
        let mut next_sources = BTreeSet::new();
        let (main_files, main_package) = self.install_package(
            program.workspace(),
            program.entry_package(),
            &mut next_sources,
            mutations,
        )?;
        Ok((
            InstalledProgram {
                main_package,
                main_files,
            },
            next_sources,
        ))
    }

    fn install_package(
        &mut self,
        workspace: &WorkspaceKey,
        package: &PackageInputManifest,
        next_sources: &mut BTreeSet<FileId>,
        mutations: &mut Vec<SourceInputMutation>,
    ) -> Result<(Vec<InstalledFile>, PackageId), CompilerError> {
        let mut installed = Vec::with_capacity(package.files().len());
        let mut package_id = None;
        for file in package.files() {
            let logical_path = file.logical_path().to_string();
            let snapshot = file.snapshot();
            let (update, mutation) = self
                .database
                .set_source_transactional(
                    workspace,
                    package.key(),
                    &logical_path,
                    Arc::clone(&snapshot),
                )
                .map_err(|error| self.query_error(error))?;
            mutations.extend(mutation);
            let id = update.file();
            let current_package = self
                .database
                .package_for_file(id)
                .map_err(|error| self.query_error(error))?;
            if package_id
                .replace(current_package)
                .is_some_and(|old| old != current_package)
            {
                return Err(CompilerError::backend(
                    "one package input produced multiple stable package identities",
                ));
            }
            next_sources.insert(id);
            installed.push(InstalledFile {
                id,
                logical_path,
                original_path: snapshot.diagnostic_path().to_string(),
            });
        }
        let package_id = package_id.ok_or_else(|| {
            CompilerError::backend("validated package input contains no Go source files")
        })?;
        installed.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        Ok((installed, package_id))
    }

    fn validate_bootstrap_boundary(
        &self,
        installed: &InstalledProgram,
        analysis: &PackageAnalysis,
    ) -> Result<(), CompilerError> {
        let file = installed
            .main_files
            .first()
            .map(|file| file.original_path.clone())
            .unwrap_or_default();
        if installed.main_files.len() != 1 {
            return Err(boundary_error(
                file,
                "the bootstrap backend requires exactly one Go source file",
            ));
        }
        if !analysis.direct_imports().is_empty() {
            return Err(boundary_error(
                file,
                "imports are not implemented by the HIR/MIR backend",
            ));
        }
        if analysis.package_name() != "main" {
            return Err(boundary_error(
                file,
                "executable compilation requires package main",
            ));
        }
        let main = analysis
            .functions()
            .iter()
            .filter(|function| function.name() == "main")
            .collect::<Vec<_>>();
        let [main] = main.as_slice() else {
            return Err(boundary_error(
                file,
                "package main must declare exactly one main function",
            ));
        };
        let signature = self
            .database
            .function_signature(main.file(), main.id())
            .map_err(|error| self.query_error(error))?;
        if signature.has_parameters() || signature.has_results() {
            return Err(boundary_error(
                file,
                "func main must have no parameters or results",
            ));
        }
        Ok(())
    }

    fn source_map_plan(
        &self,
        installed: &InstalledProgram,
        analysis: &PackageAnalysis,
        rust_ir: &super::rust_ir::File,
    ) -> Result<SourceMapPlan, CompilerError> {
        let entry_file = installed.main_files.first().ok_or_else(|| {
            CompilerError::backend("source-map plan requires one installed entry source")
        })?;
        let entry_comments = self
            .database
            .file_comments(entry_file.id)
            .map_err(|error| self.query_error(error))?;
        let entry_source_name: Arc<str> = Arc::from(entry_file.original_path.as_str());
        let mut tracker = crate::sourcemap::SourceMapTracker::new();
        let sources = installed
            .main_files
            .iter()
            .map(|file| {
                self.database.source_snapshot(file.id).map(|snapshot| {
                    (
                        file.original_path.clone(),
                        Some(snapshot.source().to_string()),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| self.query_error(error))?;
        tracker.start_many(sources, "main.rs");
        let original_paths = installed
            .main_files
            .iter()
            .map(|file| (file.id, file.original_path.as_str()))
            .collect::<BTreeMap<_, _>>();
        let emitted_functions = rust_ir
            .functions
            .iter()
            .map(|function| (function.id, function))
            .collect::<BTreeMap<_, _>>();
        for function in analysis.functions() {
            let source_table = self
                .database
                .definition_source_table(function.file(), function.id())
                .map_err(|error| self.query_error(error))?;
            let source = original_paths
                .get(&source_table.file())
                .map(|path| (*path).to_string());
            let definition_range = source_table
                .resolve(SourceRef::definition(function.id()))
                .map_err(|error| CompilerError::backend(error.to_string()))?;
            let coordinate_map = self
                .database
                .source_coordinate_map(definition_range.file())
                .map_err(|error| self.query_error(error))?;
            let physical = coordinate_map
                .physical_coordinate(definition_range.range().start())
                .map_err(|error| CompilerError::backend(error.to_string()))?
                .ok_or_else(|| {
                    CompilerError::backend(format!(
                        "definition source anchor for {} is outside its source map",
                        function.id()
                    ))
                })?;
            let emitted = emitted_functions.get(&function.id()).ok_or_else(|| {
                CompilerError::backend(format!(
                    "verified Rust IR omitted source function DefId {}",
                    function.id()
                ))
            })?;
            tracker.record_for_source_with_generated_token(
                source,
                physical.line().get(),
                physical.byte_column().get(),
                function.name(),
                emitted.artifact.symbol.as_str(),
            );
        }
        tracker.pause();
        Ok(SourceMapPlan {
            tracker,
            entry_source_name,
            entry_comments,
        })
    }

    fn query_error(&self, error: QueryError) -> CompilerError {
        match error {
            QueryError::StageFailure(failure) => self.stage_failure(&failure),
            QueryError::PackageIssues { package, issues } => self.package_issues(package, &issues),
            other => CompilerError::backend(other.to_string()),
        }
    }

    fn stage_failure(&self, failure: &StageFailure) -> CompilerError {
        let mut diagnostics = failure
            .diagnostics()
            .iter()
            .map(|diagnostic| {
                match self.resolve_diagnostic_location(failure, diagnostic.location) {
                    Ok(Some(range)) => self.project_stage_diagnostic(diagnostic, range),
                    Ok(None) => CompilerDiagnostic {
                        code: diagnostic.code,
                        message: diagnostic.message.clone(),
                        file: String::new(),
                        line: 0,
                        column: 0,
                    },
                    Err(detail) => CompilerDiagnostic {
                        code: "GORS2003",
                        message: format!("{} ({detail})", diagnostic.message),
                        file: String::new(),
                        line: 0,
                        column: 0,
                    },
                }
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.file
                .cmp(&right.file)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.column.cmp(&right.column))
                .then_with(|| left.code.cmp(right.code))
                .then_with(|| left.message.cmp(&right.message))
        });
        CompilerError { diagnostics }
    }

    fn resolve_diagnostic_location(
        &self,
        failure: &StageFailure,
        location: DiagnosticLocation,
    ) -> Result<Option<FileRange>, String> {
        match location {
            DiagnosticLocation::Synthetic => Ok(None),
            DiagnosticLocation::Physical(range) => Ok(Some(range)),
            DiagnosticLocation::Source(source) => {
                if failure.definition() != Some(source.owner()) {
                    return Err(format!(
                        "diagnostic source owner {} does not match stage definition {:?}",
                        source.owner(),
                        failure.definition()
                    ));
                }
                let source_table =
                    self.definition_source_table(source.owner())
                        .ok_or_else(|| {
                            format!(
                                "current source table for diagnostic owner {} is unavailable",
                                source.owner()
                            )
                        })?;
                source_table
                    .resolve(source)
                    .map(Some)
                    .map_err(|error| error.to_string())
            }
        }
    }

    fn project_stage_diagnostic(
        &self,
        diagnostic: &super::Diagnostic,
        range: FileRange,
    ) -> CompilerDiagnostic {
        let snapshot = match self.database.source_snapshot(range.file()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "{} (source snapshot is unavailable: {error})",
                        diagnostic.message
                    ),
                    file: String::new(),
                    line: 0,
                    column: 0,
                };
            }
        };
        let coordinate_map = match self.database.source_coordinate_map(range.file()) {
            Ok(map) => map,
            Err(error) => {
                return CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "{} (source coordinate map is unavailable: {error})",
                        diagnostic.message
                    ),
                    file: snapshot.diagnostic_path().to_string(),
                    line: 0,
                    column: 0,
                };
            }
        };
        match coordinate_map
            .adjusted_coordinate_for(range.range().start(), snapshot.diagnostic_path())
        {
            Ok(Some(coordinate)) => CompilerDiagnostic {
                code: diagnostic.code,
                message: diagnostic.message.clone(),
                file: coordinate.filename().to_string(),
                line: coordinate_component(coordinate.position().line().get()),
                column: coordinate_component(coordinate.position().column().to_go_column()),
            },
            Ok(None) => CompilerDiagnostic {
                code: "GORS2003",
                message: format!(
                    "{} (source coordinate map does not cover the diagnostic anchor)",
                    diagnostic.message
                ),
                file: snapshot.diagnostic_path().to_string(),
                line: 0,
                column: 0,
            },
            Err(error) => CompilerDiagnostic {
                code: "GORS2003",
                message: format!(
                    "{} (source coordinate projection failed: {error})",
                    diagnostic.message
                ),
                file: snapshot.diagnostic_path().to_string(),
                line: 0,
                column: 0,
            },
        }
    }

    fn definition_source_table(
        &self,
        definition: super::ids::DefId,
    ) -> Option<Arc<DefinitionSourceTable>> {
        for file in self.database.active_files() {
            let Ok(analysis) = self.database.analyze_file(file) else {
                continue;
            };
            if analysis
                .functions()
                .iter()
                .any(|function| function.id() == definition)
            {
                return self.database.definition_source_table(file, definition).ok();
            }
        }
        None
    }

    fn package_issues(&self, _package: PackageId, issues: &[PackageIssue]) -> CompilerError {
        let mut diagnostics = issues
            .iter()
            .map(|issue| match issue {
                PackageIssue::FileParseFailure { file, failure } => {
                    self.parse_failure_diagnostic(*file, failure)
                }
                PackageIssue::InvalidImportPath {
                    file,
                    literal,
                    line,
                    column,
                    virtual_file,
                    issue,
                } => CompilerDiagnostic {
                    code: "GORS2002",
                    message: format!("invalid import path literal {literal}: {issue}"),
                    file: virtual_file
                        .as_deref()
                        .map_or_else(|| self.source_path_or_empty(*file), str::to_string),
                    line: *line,
                    column: *column,
                },
                PackageIssue::PackageClauseMismatch {
                    file,
                    expected,
                    found,
                } => CompilerDiagnostic {
                    code: "GORS2002",
                    message: format!(
                        "package clause {found:?} does not match expected {expected:?}"
                    ),
                    file: self.source_path_or_empty(*file),
                    line: 0,
                    column: 0,
                },
                PackageIssue::DuplicateDefinition {
                    name,
                    first_file,
                    second_file,
                } => CompilerDiagnostic {
                    code: "GORS2002",
                    message: format!(
                        "duplicate package definition {name:?} in {} and {}",
                        self.source_path_or_empty(*first_file),
                        self.source_path_or_empty(*second_file)
                    ),
                    file: self.source_path_or_empty(*second_file),
                    line: 0,
                    column: 0,
                },
                PackageIssue::IdentityCollision {
                    id,
                    existing_key,
                    requested_key,
                } => CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "stable identity collision for {id}: {existing_key} vs {requested_key}"
                    ),
                    file: String::new(),
                    line: 0,
                    column: 0,
                },
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.file
                .cmp(&right.file)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.column.cmp(&right.column))
                .then_with(|| left.code.cmp(right.code))
                .then_with(|| left.message.cmp(&right.message))
        });
        CompilerError { diagnostics }
    }

    fn parse_failure_diagnostic(&self, file: FileId, failure: &ParseFailure) -> CompilerDiagnostic {
        let presentation_path = self.source_path_or_empty(file);
        let coordinate = match self.database.source_coordinate_map(file) {
            Ok(map) => match map
                .adjusted_coordinate_for(failure.physical_range().start(), &presentation_path)
            {
                Ok(Some(coordinate)) => coordinate,
                Ok(None) => {
                    return coordinate_projection_failure(
                        presentation_path,
                        failure,
                        "parser coordinate map does not cover its physical failure anchor",
                    );
                }
                Err(error) => {
                    return coordinate_projection_failure(
                        presentation_path,
                        failure,
                        &format!("parser coordinate projection failed: {error}"),
                    );
                }
            },
            Err(error) => {
                return coordinate_projection_failure(
                    presentation_path,
                    failure,
                    &format!("parser coordinate map is unavailable: {error}"),
                );
            }
        };
        CompilerDiagnostic {
            code: "GORS2002",
            message: failure.message().to_string(),
            file: coordinate.filename().to_string(),
            line: coordinate_component(coordinate.position().line().get()),
            column: coordinate_component(coordinate.position().column().to_go_column()),
        }
    }

    fn source_path_or_empty(&self, file: FileId) -> String {
        self.database.source_snapshot(file).map_or_else(
            |_| String::new(),
            |snapshot| snapshot.diagnostic_path().to_string(),
        )
    }
}

#[allow(clippy::cast_lossless)]
const fn coordinate_component(value: u32) -> usize {
    value as usize
}

fn coordinate_projection_failure(
    file: String,
    failure: &ParseFailure,
    detail: &str,
) -> CompilerDiagnostic {
    CompilerDiagnostic {
        code: "GORS2002",
        message: format!("{} ({detail})", failure.message()),
        file,
        line: 1,
        column: 1,
    }
}

impl Default for CompilerSession {
    fn default() -> Self {
        Self::from_validated_host(BuildConfig::default(), CompilerHost::inline())
    }
}

fn validate_packaged_runtime_abi(config: &BuildConfig) -> Result<(), CompilerError> {
    if config.runtime_abi() == crate::RUNTIME_ABI_ID {
        return Ok(());
    }
    Err(CompilerError::backend(format!(
        "runtime ABI `{}` cannot be packaged by this compiler; expected `{}`",
        config.runtime_abi(),
        crate::RUNTIME_ABI_ID
    )))
}

struct InstalledProgram {
    main_package: PackageId,
    main_files: Vec<InstalledFile>,
}

struct InstalledFile {
    id: FileId,
    logical_path: String,
    original_path: String,
}

fn boundary_error(file: String, message: impl Into<String>) -> CompilerError {
    CompilerError {
        diagnostics: vec![CompilerDiagnostic {
            code: "GORS2001",
            message: message.into(),
            file,
            line: 1,
            column: 1,
        }],
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
