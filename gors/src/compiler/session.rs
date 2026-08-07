//! Stateful production compiler session backed by the red-green query graph.

mod admission;
mod prewarm;
mod readiness;

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use gors_runtime_abi::{RuntimeAbiManifest, RuntimeDependency};

use self::readiness::RustIrRoot;
use super::db::{
    BuildConfig, CompilerDatabase, Fingerprint, PackageAnalysis, PackageIssue, ParseFailure,
    QueryError, RuntimeAbiId, StageFailure,
};
use super::diagnostic::DiagnosticLocation;
use super::ids::{FileId, PackageId};
use super::input::ProgramInput;
use super::package_dag::PackageDag;
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
    admitted_package_dag: Option<Arc<PackageDag>>,
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
        validate_runtime_contract(&config)?;
        Ok(Self::from_validated_host(config, host))
    }

    fn from_validated_host(config: BuildConfig, host: CompilerHost) -> Self {
        Self {
            database: CompilerDatabase::new(config),
            host,
            ready_rust_ir_roots: BTreeMap::new(),
            admitted_package_dag: None,
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

    /// Canonical reachable-package graph from the latest committed admission.
    ///
    /// This remains available when a later semantic or representation stage
    /// rejects the admitted program. A failed admission transaction preserves
    /// the preceding graph together with the preceding database revision.
    #[must_use]
    pub fn admitted_package_dag(&self) -> Option<&PackageDag> {
        self.admitted_package_dag.as_deref()
    }

    /// Change explicit compiler inputs while preserving unrelated stage memos.
    pub fn set_build_config(&mut self, config: BuildConfig) -> Result<(), CompilerError> {
        validate_runtime_contract(&config)?;
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
        for package in installed.package_dag.topological_order() {
            if *package == installed.main_package {
                continue;
            }
            let analysis = self
                .database
                .analyze_package(*package)
                .map_err(|error| self.query_error(error))?;
            if !analysis.issues().is_empty() {
                return Err(self.package_issues(*package, analysis.issues()));
            }
        }
        self.validate_bootstrap_boundary(&installed, &main_analysis)?;

        let current_root_inputs = self.current_root_inputs(&main_analysis)?;
        let roots = self.roots_requiring_prewarm(&current_root_inputs);
        self.prewarm_rust_ir(&roots)?;
        // A completed wave marks each input-equivalent root ready even when
        // canonical package assembly selects a cached stage failure below.
        self.publish_ready_roots(current_root_inputs);
        let module_names = installed
            .package_dag
            .nodes()
            .iter()
            .filter(|node| node.package() != installed.main_package)
            .map(|node| {
                node.import_path()
                    .map(|path| (node.package(), crate::resolve::module_name(path.as_str())))
                    .ok_or_else(|| {
                        CompilerError::backend(
                            "a reachable dependency package has no canonical import path",
                        )
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut rust_ir_packages = BTreeMap::new();
        let mut runtime_requirement = super::rust_ir::RuntimeRequirement::default();
        for package in installed.package_dag.topological_order() {
            let verified = self
                .database
                .verified_rust_ir_package(*package)
                .map_err(|error| self.query_error(error))?;
            runtime_requirement = runtime_requirement.union(verified.runtime_requirement());
            let module = module_names.get(package).cloned();
            rust_ir_packages.insert(*package, (module, verified.file().clone()));
        }
        let runtime_contract = RuntimeAbiManifest::current();
        let runtime = RuntimeDependency::new(
            &runtime_contract,
            runtime_requirement,
        )
        .map_err(|error| {
            CompilerError::backend(format!(
                "verified Rust IR selected an operation outside the current runtime contract: {error}"
            ))
        })?;
        let main_rust_ir = rust_ir_packages
            .get(&installed.main_package)
            .map(|(_, file)| file)
            .ok_or_else(|| CompilerError::backend("program assembly omitted package main"))?;
        let source_map = with_source_map
            .then(|| self.source_map_plan(&installed, &main_analysis, main_rust_ir))
            .transpose()?;
        let (entry, modules) = emit::emit_program(installed.main_package, &rust_ir_packages)
            .map_err(CompilerError::terminal)?;
        Ok((
            CompiledProgram {
                entry,
                modules,
                runtime,
            },
            source_map,
        ))
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
                || analysis
                    .constants()
                    .iter()
                    .any(|constant| constant.id() == definition)
                || analysis
                    .variables()
                    .iter()
                    .any(|variable| variable.id() == definition)
                || analysis
                    .type_aliases()
                    .iter()
                    .any(|alias| alias.id() == definition)
                || analysis
                    .type_definitions()
                    .iter()
                    .any(|declared| declared.id() == definition)
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
                PackageIssue::LanguageVersion {
                    file,
                    feature,
                    selected,
                    range,
                } => self.project_stage_diagnostic(
                    &super::Diagnostic::semantic(
                        format!(
                            "{} requires {} or later (-lang was set to {selected}; check go.mod)",
                            feature.description(),
                            feature.required_version(),
                        ),
                        FileRange::new(*file, *range),
                    ),
                    FileRange::new(*file, *range),
                ),
                PackageIssue::UnusedImport {
                    file,
                    local_name,
                    path,
                    named,
                    line,
                    column,
                    virtual_file,
                } => CompilerDiagnostic {
                    code: "GORS2002",
                    message: if *named {
                        format!("\"{path}\" imported as {local_name} and not used")
                    } else {
                        format!("\"{path}\" imported and not used")
                    },
                    file: virtual_file
                        .as_deref()
                        .map_or_else(|| self.source_path_or_empty(*file), str::to_string),
                    line: *line,
                    column: *column,
                },
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
                PackageIssue::FunctionProjectionFailure {
                    file,
                    name,
                    message,
                } => CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "could not project function {name:?} into owned semantic syntax: {message}"
                    ),
                    file: self.source_path_or_empty(*file),
                    line: 0,
                    column: 0,
                },
                PackageIssue::ConstantProjectionFailure {
                    file,
                    name,
                    message,
                } => CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "could not project constant {name:?} into owned semantic syntax: {message}"
                    ),
                    file: self.source_path_or_empty(*file),
                    line: 0,
                    column: 0,
                },
                PackageIssue::VariableProjectionFailure {
                    file,
                    name,
                    message,
                } => CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "could not project variable {name:?} into owned semantic syntax: {message}"
                    ),
                    file: self.source_path_or_empty(*file),
                    line: 0,
                    column: 0,
                },
                PackageIssue::TypeProjectionFailure {
                    file,
                    name,
                    message,
                } => CompilerDiagnostic {
                    code: "GORS2003",
                    message: format!(
                        "could not project type {name:?} into owned semantic syntax: {message}"
                    ),
                    file: self.source_path_or_empty(*file),
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

fn validate_runtime_contract(config: &BuildConfig) -> Result<(), CompilerError> {
    let current = RuntimeAbiId::current();
    if config.runtime_abi() == current {
        return Ok(());
    }
    Err(CompilerError::backend(format!(
        "runtime ABI contract `{}` is not supported by this compiler; expected `{}`",
        config.runtime_abi(),
        current
    )))
}

struct InstalledProgram {
    main_package: PackageId,
    main_files: Vec<InstalledFile>,
    package_dag: Arc<PackageDag>,
}

#[derive(Clone)]
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
