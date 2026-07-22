//! Stateful production compiler session backed by the red-green query graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::parser::{ParsedPackage, ParsedProgram};

use super::db::{
    BuildConfig, CompilerDatabase, PackageAnalysis, PackageIssue, QueryError, StageFailure,
};
use super::ids::{FileId, PackageId};
use super::{CompiledProgram, CompilerDiagnostic, CompilerError, SourceMapPlan, emit};

const WORKSPACE_IDENTITY: &str = "gors:canonical-workspace";

/// One reusable production compiler context.
///
/// Reusing a session across edits preserves Salsa revisions and stage products.
/// The convenience free functions create a short-lived session, while editors,
/// build daemons, and performance harnesses should retain this value.
pub struct CompilerSession {
    database: CompilerDatabase,
    active_sources: BTreeSet<FileId>,
}

impl CompilerSession {
    /// Create a session from explicit, ambient-environment-free build inputs.
    #[must_use]
    pub fn new(config: BuildConfig) -> Self {
        Self {
            database: CompilerDatabase::new(config),
            active_sources: BTreeSet::new(),
        }
    }

    /// Read the owned database for telemetry and immutable stage inspection.
    #[must_use]
    pub const fn database(&self) -> &CompilerDatabase {
        &self.database
    }

    /// Change explicit build inputs while preserving target-independent memos.
    pub fn set_build_config(&mut self, config: BuildConfig) -> Result<(), CompilerError> {
        self.database
            .set_build_config(config)
            .map_err(|error| self.query_error(error))
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
        let rust_ir = self
            .database
            .verified_rust_ir_package(installed.main_package)
            .map_err(|error| self.query_error(error))?;
        let source_map = with_source_map
            .then(|| self.source_map_plan(&installed, main_analysis))
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
        let mut next_sources = BTreeSet::new();
        let mut packages = Vec::new();
        for package in program.imports() {
            let (_, package_id) = self.install_package(package, &mut next_sources)?;
            packages.push(package_id);
        }
        let (main_files, main_package) =
            self.install_package(program.main_package(), &mut next_sources)?;
        packages.push(main_package);
        packages.sort();
        packages.dedup();
        let stale = self
            .active_sources
            .difference(&next_sources)
            .copied()
            .collect::<Vec<_>>();
        for file in stale {
            self.database
                .remove_source(file)
                .map_err(|error| self.query_error(error))?;
        }
        self.active_sources = next_sources;
        Ok(InstalledProgram {
            main_package,
            main_files,
            packages,
        })
    }

    fn install_package(
        &mut self,
        package: &ParsedPackage,
        next_sources: &mut BTreeSet<FileId>,
    ) -> Result<(Vec<InstalledFile>, PackageId), CompilerError> {
        let package_identity = canonical_package_identity(package);
        let mut installed = Vec::with_capacity(package.files().len());
        let mut package_id = None;
        for file in package.files() {
            let logical_path = portable_logical_filename(file.path());
            let id = self
                .database
                .set_source(
                    WORKSPACE_IDENTITY,
                    &package_identity,
                    &logical_path,
                    file.snapshot(),
                )
                .map_err(|error| self.query_error(error))?;
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
        for function in analysis.functions() {
            let provenance = self
                .database
                .function_provenance(function.file(), function.id())
                .map_err(|error| self.query_error(error))?;
            let source = original_paths
                .get(provenance.logical_file())
                .map(|path| (*path).to_string());
            tracker.record_for_source(
                source,
                provenance.line() as u32,
                provenance.column() as u32,
                Some(function.name()),
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
        if let Some(definition) = failure.definition()
            && let Some(provenance) = self.definition_provenance(definition)
        {
            for diagnostic in &mut diagnostics {
                let span = &mut diagnostic.span;
                if span.file.is_empty() || span.line == 0 || span.column == 0 {
                    continue;
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
                span.file = provenance.logical_file().to_string();
            }
        }
        CompilerError::from(diagnostics)
    }

    fn definition_provenance(
        &self,
        definition: super::ids::DefId,
    ) -> Option<std::sync::Arc<super::db::FunctionProvenance>> {
        for file in self.database.active_files() {
            let analysis = self.database.analyze_file(file).ok()?;
            if analysis
                .functions()
                .iter()
                .any(|function| function.id() == definition)
            {
                return self.database.function_provenance(file, definition).ok();
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
                    file: self.logical_file_or_empty(*file),
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
                    file: self.logical_file_or_empty(*file),
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
                        self.logical_file_or_empty(*first_file),
                        self.logical_file_or_empty(*second_file)
                    ),
                    file: self.logical_file_or_empty(*second_file),
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

    fn logical_file_or_empty(&self, file: FileId) -> String {
        self.database
            .logical_path(file)
            .map_or_else(|_| String::new(), |path| path.to_string())
    }
}

impl Default for CompilerSession {
    fn default() -> Self {
        Self::new(BuildConfig::default())
    }
}

struct InstalledProgram {
    main_package: PackageId,
    main_files: Vec<InstalledFile>,
    packages: Vec<PackageId>,
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
