//! Stateful production compiler session backed by the red-green query graph.

mod prewarm;

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use crate::parser::{ParsedPackage, ParsedProgram};

use super::db::{
    BuildConfig, CompilerDatabase, PackageAnalysis, PackageIssue, QueryError, StageFailure,
};
use super::ids::{FileId, PackageId};
use super::scheduler::{CompilerHost, SchedulerTelemetry};
use super::{CompiledProgram, CompilerDiagnostic, CompilerError, SourceMapPlan, emit};

const WORKSPACE_IDENTITY: &str = "gors:canonical-workspace";

/// One reusable production compiler context.
///
/// Reusing a session across edits preserves Salsa revisions and stage products.
/// The convenience free functions create a short-lived session, while editors,
/// build daemons, and performance harnesses should retain this value.
pub struct CompilerSession {
    database: CompilerDatabase,
    host: CompilerHost,
    ready_package_roots: BTreeSet<PackageId>,
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
            ready_package_roots: BTreeSet::new(),
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
            self.ready_package_roots.clear();
        }
        Ok(())
    }

    /// Compile through the complete tracked semantic and representation spine.
    pub fn compile_program(
        &mut self,
        program: ParsedProgram,
    ) -> Result<CompiledProgram, CompilerError> {
        self.compile(program, false).map(|(compiled, _)| compiled)
    }

    /// Compile and retain independently owned source-map inputs.
    pub fn compile_program_with_source_map(
        &mut self,
        program: ParsedProgram,
    ) -> Result<(CompiledProgram, SourceMapPlan), CompilerError> {
        self.compile(program, true).and_then(|(compiled, plan)| {
            plan.map(|plan| (compiled, plan)).ok_or_else(|| {
                CompilerError::backend("source-map plan was not constructed by the session")
            })
        })
    }

    fn compile(
        &mut self,
        program: ParsedProgram,
        with_source_map: bool,
    ) -> Result<(CompiledProgram, Option<SourceMapPlan>), CompilerError> {
        let installed = self.install_program(&program)?;
        if installed.inputs_changed {
            self.ready_package_roots.clear();
        }
        let mut analyses = BTreeMap::new();
        for package in &installed.packages {
            let analysis = self
                .database
                .analyze_package(*package)
                .map_err(|error| self.query_error(error))?;
            if !analysis.issues().is_empty() {
                return Err(self.package_issues(*package, analysis.issues()));
            }
            analyses.insert(*package, analysis);
        }
        let main_analysis = analyses.get(&installed.main_package).ok_or_else(|| {
            CompilerError::backend("installed entry package has no semantic package index")
        })?;
        self.validate_bootstrap_boundary(&program, &installed, main_analysis)?;

        for file in &installed.main_files {
            self.database
                .semantic_status(file.id)
                .map_err(|error| self.query_error(error))?;
        }
        // A completed wave marks every per-definition root ready even when
        // canonical package assembly will select a cached stage failure below.
        if self.ready_package_roots.insert(installed.main_package)
            && let Err(error) = self.prewarm_rust_ir(main_analysis)
        {
            self.ready_package_roots.remove(&installed.main_package);
            return Err(error);
        }
        let rust_ir = self
            .database
            .verified_rust_ir_package(installed.main_package)
            .map_err(|error| self.query_error(error))?;
        let source_map = with_source_map
            .then(|| self.source_map_plan(&installed, main_analysis, rust_ir.file()))
            .transpose()?;
        let entry = emit::emit_file(rust_ir.file())
            .map_err(|diagnostic| CompilerError::from(vec![diagnostic]))?;
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
        program: &ParsedProgram,
    ) -> Result<InstalledProgram, CompilerError> {
        let previous_sources = self
            .database
            .active_files()
            .into_iter()
            .map(|file| {
                self.database
                    .source_snapshot(file)
                    .map(|snapshot| (file, snapshot))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(|error| self.query_error(error))?;
        match self.install_program_inputs(program) {
            Ok((mut installed, next_sources)) => {
                installed.inputs_changed |= previous_sources.len() != next_sources.len()
                    || previous_sources
                        .keys()
                        .any(|file| !next_sources.contains(file));
                let stale = previous_sources
                    .keys()
                    .filter(|file| !next_sources.contains(file))
                    .copied()
                    .collect::<Vec<_>>();
                for file in stale {
                    self.database
                        .remove_source(file)
                        .map_err(|error| self.query_error(error))?;
                }
                Ok(installed)
            }
            Err(error) => {
                if let Err(rollback) = self.rollback_install(&previous_sources) {
                    let mut diagnostics = error.diagnostics;
                    diagnostics.push(CompilerDiagnostic {
                        code: "GORS2003",
                        message: format!("source transaction rollback failed: {rollback}"),
                        file: String::new(),
                        line: 0,
                        column: 0,
                    });
                    return Err(CompilerError { diagnostics });
                }
                Err(error)
            }
        }
    }

    fn install_program_inputs(
        &mut self,
        program: &ParsedProgram,
    ) -> Result<(InstalledProgram, BTreeSet<FileId>), CompilerError> {
        let mut next_sources = BTreeSet::new();
        let mut packages = Vec::new();
        let mut inputs_changed = false;
        for package in program.imports() {
            let (_, package_id) =
                self.install_package(package, &mut next_sources, &mut inputs_changed)?;
            packages.push(package_id);
        }
        let (main_files, main_package) = self.install_package(
            program.main_package(),
            &mut next_sources,
            &mut inputs_changed,
        )?;
        packages.push(main_package);
        packages.sort();
        packages.dedup();
        Ok((
            InstalledProgram {
                main_package,
                main_files,
                packages,
                inputs_changed,
            },
            next_sources,
        ))
    }

    fn rollback_install(
        &mut self,
        previous_sources: &BTreeMap<FileId, std::sync::Arc<crate::parser::SourceSnapshot>>,
    ) -> Result<(), QueryError> {
        for file in self.database.active_files() {
            if let Some(snapshot) = previous_sources.get(&file) {
                self.database
                    .restore_source_snapshot(file, std::sync::Arc::clone(snapshot))?;
            } else {
                self.database.remove_source(file)?;
            }
        }
        Ok(())
    }

    fn install_package(
        &mut self,
        package: &ParsedPackage,
        next_sources: &mut BTreeSet<FileId>,
        inputs_changed: &mut bool,
    ) -> Result<(Vec<InstalledFile>, PackageId), CompilerError> {
        let package_identity = canonical_package_identity(package);
        let mut installed = Vec::with_capacity(package.files().len());
        let mut package_id = None;
        for file in package.files() {
            let logical_path = portable_logical_filename(file.path());
            let update = self
                .database
                .set_source(
                    WORKSPACE_IDENTITY,
                    &package_identity,
                    &logical_path,
                    file.snapshot(),
                )
                .map_err(|error| self.query_error(error))?;
            *inputs_changed |= update.semantic_changed();
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
                    "one parsed package produced multiple stable package identities",
                ));
            }
            next_sources.insert(id);
            installed.push(InstalledFile {
                id,
                logical_path,
                original_path: file.path().to_string(),
            });
        }
        let package_id = package_id.ok_or_else(|| {
            CompilerError::unsupported("the parsed entry package contains no Go source files")
        })?;
        installed.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
        Ok((installed, package_id))
    }

    fn validate_bootstrap_boundary(
        &self,
        program: &ParsedProgram,
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
        if !program.imports().is_empty() || !program.stdlib_imports().is_empty() {
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
            .map(|file| (file.logical_path.as_str(), file.original_path.as_str()))
            .collect::<BTreeMap<_, _>>();
        let emitted_functions = rust_ir
            .functions
            .iter()
            .map(|function| (function.id, function))
            .collect::<BTreeMap<_, _>>();
        for function in analysis.functions() {
            let provenance = self
                .database
                .function_provenance(function.file(), function.id())
                .map_err(|error| self.query_error(error))?;
            let source = original_paths
                .get(provenance.logical_file())
                .map(|path| (*path).to_string());
            let emitted = emitted_functions.get(&function.id()).ok_or_else(|| {
                CompilerError::backend(format!(
                    "verified Rust IR omitted source function DefId {}",
                    function.id()
                ))
            })?;
            tracker.record_for_source_with_generated_token(
                source,
                provenance.line() as u32,
                provenance.column() as u32,
                function.name(),
                emitted.artifact.symbol.as_str(),
            );
        }
        tracker.pause();
        Ok(SourceMapPlan { tracker })
    }

    fn query_error(&self, error: QueryError) -> CompilerError {
        match error {
            QueryError::StageFailure(failure) => self.stage_failure(&failure),
            QueryError::PackageIssues { package, issues } => self.package_issues(package, &issues),
            other => CompilerError::backend(other.to_string()),
        }
    }

    fn stage_failure(&self, failure: &StageFailure) -> CompilerError {
        let mut diagnostics = failure.diagnostics().to_vec();
        let definition_location = failure
            .definition()
            .and_then(|definition| self.definition_provenance(definition));
        if let Some((_, provenance)) = &definition_location {
            for diagnostic in &mut diagnostics {
                rebase_function_diagnostic(diagnostic, provenance);
            }
        }
        let source_file = definition_location
            .as_ref()
            .map(|(file, _)| *file)
            .or_else(|| failure.source_file());
        if let Some(source_file) = source_file
            && let Ok(snapshot) = self.database.source_snapshot(source_file)
        {
            for diagnostic in &mut diagnostics {
                diagnostic.span.file = snapshot.diagnostic_path().to_string();
            }
        }
        CompilerError::from(diagnostics)
    }

    fn definition_provenance(
        &self,
        definition: super::ids::DefId,
    ) -> Option<(FileId, std::sync::Arc<super::db::FunctionProvenance>)> {
        for file in self.database.active_files() {
            let Ok(analysis) = self.database.analyze_file(file) else {
                continue;
            };
            if analysis
                .functions()
                .iter()
                .any(|function| function.id() == definition)
            {
                return self
                    .database
                    .function_provenance(file, definition)
                    .ok()
                    .map(|provenance| (file, provenance));
            }
        }
        None
    }

    fn package_issues(&self, _package: PackageId, issues: &[PackageIssue]) -> CompilerError {
        let mut diagnostics = issues
            .iter()
            .map(|issue| match issue {
                PackageIssue::FileParseFailure { file, failure } => CompilerDiagnostic {
                    code: "GORS2002",
                    message: failure.message().to_string(),
                    file: self.source_path_or_empty(*file),
                    line: failure.line().unwrap_or(0),
                    column: failure.column().unwrap_or(0),
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

    fn source_path_or_empty(&self, file: FileId) -> String {
        self.database.source_snapshot(file).map_or_else(
            |_| String::new(),
            |snapshot| snapshot.diagnostic_path().to_string(),
        )
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
    packages: Vec<PackageId>,
    inputs_changed: bool,
}

struct InstalledFile {
    id: FileId,
    logical_path: String,
    original_path: String,
}

fn canonical_package_identity(package: &ParsedPackage) -> String {
    if package.import_path().is_empty() {
        format!("command-line-package:{}", package.name())
    } else {
        format!("import:{}", package.import_path())
    }
}

fn portable_logical_filename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
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

fn rebase_function_diagnostic(
    diagnostic: &mut super::Diagnostic,
    provenance: &super::db::FunctionProvenance,
) {
    let span = &mut diagnostic.span;
    if span.file.is_empty() || span.line == 0 || span.column == 0 {
        return;
    }
    span.start = span.start.saturating_add(provenance.byte_offset());
    span.end = span.end.saturating_add(provenance.byte_offset());
    if span.line == 1 {
        span.column = span
            .column
            .saturating_add(provenance.column().saturating_sub(1));
    }
    span.line = span
        .line
        .saturating_add(provenance.line().saturating_sub(1));
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
