//! Transactional admission of compiler-owned source manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{CompilerSession, InstalledFile, InstalledProgram};
use crate::compiler::db::{
    CompilerDatabase, DirectImport, ResolvedFileImports, ResolvedImport,
    ResolvedImportInputMutation, SourceInputMutation,
};
use crate::compiler::ids::{FileId, PackageId};
use crate::compiler::input::{
    PackageInputManifest, PackageKey, ProgramInput, SourceFileInput, WorkspaceKey,
};
use crate::compiler::package_dag::{
    PackageDagError, PackageDagImport, PackageDagNode, build_package_dag,
};
use crate::compiler::provenance::FileRange;
use crate::compiler::{CompilerDiagnostic, CompilerError};

enum AdmissionInputMutation {
    Source(SourceInputMutation),
    ResolvedImports(ResolvedImportInputMutation),
}

impl CompilerSession {
    pub(super) fn install_program(
        &mut self,
        program: &ProgramInput,
    ) -> Result<InstalledProgram, CompilerError> {
        self.install_program_transaction(program, |_| Ok(()))
    }

    pub(super) fn install_program_transaction<F>(
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
            self.install_resolved_import_inputs(
                &installed.package_dag,
                &next_sources,
                &mut mutations,
            )?;
            let stale_resolved_imports = self
                .database
                .active_resolved_import_files()
                .into_iter()
                .filter(|file| !next_sources.contains(file))
                .collect::<Vec<_>>();
            for file in stale_resolved_imports {
                let mutation = self
                    .database
                    .remove_resolved_file_imports_transactional(file)
                    .map_err(|error| self.query_error(error))?;
                mutations.push(AdmissionInputMutation::ResolvedImports(mutation));
            }
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
                mutations.push(AdmissionInputMutation::Source(mutation));
            }
            before_commit(&self.database)?;
            Ok((installed, next_sources))
        })();
        match result {
            Ok((installed, next_sources)) => {
                self.commit_admission_mutations(mutations);
                self.retain_ready_roots_for_files(&next_sources);
                self.admitted_package_dag = Some(Arc::clone(&installed.package_dag));
                Ok(installed)
            }
            Err(error) => {
                self.rollback_admission_mutations(mutations);
                Err(error)
            }
        }
    }

    fn install_program_inputs(
        &mut self,
        program: &ProgramInput,
        mutations: &mut Vec<AdmissionInputMutation>,
    ) -> Result<(InstalledProgram, BTreeSet<FileId>), CompilerError> {
        let mut next_sources = BTreeSet::new();
        let entry_key = program.entry_package().key().clone();
        let (main_files, main_package) = self.install_package(
            program.workspace(),
            program.entry_package(),
            &mut next_sources,
            mutations,
        )?;
        let mut packages = BTreeMap::from([(entry_key.clone(), main_package)]);
        let mut frontier = vec![entry_key];
        let mut resolved_imports = Vec::new();
        let catalog = program.package_catalog();

        while !frontier.is_empty() {
            frontier.sort();
            frontier.dedup();
            let mut requested = BTreeMap::<PackageKey, Vec<(PackageId, DirectImport)>>::new();
            for package_key in std::mem::take(&mut frontier) {
                let package = packages.get(&package_key).copied().ok_or_else(|| {
                    CompilerError::backend(format!(
                        "admission frontier lost installed package {package_key}"
                    ))
                })?;
                let analysis = self
                    .database
                    .analyze_package(package)
                    .map_err(|error| self.query_error(error))?;
                for file in analysis.files() {
                    let imports = self
                        .database
                        .file_imports(*file)
                        .map_err(|error| self.query_error(error))?;
                    for import in imports.direct() {
                        let dependency = PackageKey::ImportPath(import.canonical_path().clone());
                        requested
                            .entry(dependency)
                            .or_default()
                            .push((package, import.clone()));
                    }
                }
            }

            let mut next_frontier = Vec::new();
            for dependency in requested.keys() {
                if packages.contains_key(dependency) {
                    continue;
                }
                let occurrences = requested.get(dependency).ok_or_else(|| {
                    CompilerError::backend("package request disappeared during admission")
                })?;
                let first = occurrences
                    .first()
                    .map(|(_, import)| import)
                    .ok_or_else(|| {
                        CompilerError::backend("reachable package request has no import occurrence")
                    })?;
                let manifest = match intrinsic_package_manifest(dependency)? {
                    Some(manifest) => manifest,
                    None => catalog
                        .materialize(dependency)
                        .map_err(|error| self.catalog_error(first, &error))?
                        .ok_or_else(|| self.unresolved_import(first))?,
                };
                if manifest.key() != dependency {
                    return Err(self.import_diagnostic(
                        first,
                        "GORS2003",
                        format!(
                            "package catalog returned {} for requested {dependency}",
                            manifest.key()
                        ),
                    ));
                }
                let (_, package) = self.install_package(
                    program.workspace(),
                    &manifest,
                    &mut next_sources,
                    mutations,
                )?;
                packages.insert(dependency.clone(), package);
                next_frontier.push(dependency.clone());
            }

            for (dependency, occurrences) in requested {
                let dependency_id = packages.get(&dependency).copied().ok_or_else(|| {
                    CompilerError::backend(format!(
                        "resolved package {dependency} was not installed"
                    ))
                })?;
                resolved_imports.extend(occurrences.into_iter().map(|(importer, occurrence)| {
                    PackageDagImport::new(importer, dependency_id, occurrence)
                }));
            }
            frontier = next_frontier;
        }

        let nodes = packages
            .into_iter()
            .map(|(key, package)| PackageDagNode::new(package, key));
        let package_dag = build_package_dag(main_package, nodes, resolved_imports)
            .map_err(|error| self.package_dag_error(&error))?;
        Ok((
            InstalledProgram {
                main_package,
                main_files,
                package_dag: Arc::new(package_dag),
            },
            next_sources,
        ))
    }

    fn install_package(
        &mut self,
        workspace: &WorkspaceKey,
        package: &PackageInputManifest,
        next_sources: &mut BTreeSet<FileId>,
        mutations: &mut Vec<AdmissionInputMutation>,
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
            mutations.extend(mutation.map(AdmissionInputMutation::Source));
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

    fn install_resolved_import_inputs(
        &mut self,
        package_dag: &crate::compiler::package_dag::PackageDag,
        next_sources: &BTreeSet<FileId>,
        mutations: &mut Vec<AdmissionInputMutation>,
    ) -> Result<(), CompilerError> {
        let mut package_names = BTreeMap::new();
        for node in package_dag.nodes() {
            let analysis = self
                .database
                .analyze_package(node.package())
                .map_err(|error| self.query_error(error))?;
            package_names.insert(node.package(), Arc::<str>::from(analysis.package_name()));
        }

        let mut by_file = next_sources
            .iter()
            .copied()
            .map(|file| (file, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for edge in package_dag.edges() {
            let target_package_name = package_names.get(&edge.dependency()).ok_or_else(|| {
                CompilerError::backend(format!(
                    "resolved package DAG omitted package name for {:?}",
                    edge.dependency()
                ))
            })?;
            for occurrence in edge.occurrences() {
                let importer = self
                    .database
                    .package_for_file(occurrence.file())
                    .map_err(|error| self.query_error(error))?;
                if importer != edge.importer() {
                    return Err(CompilerError::backend(format!(
                        "resolved import occurrence in {:?} belongs to {importer:?}, not DAG importer {:?}",
                        occurrence.file(),
                        edge.importer()
                    )));
                }
                let imports = by_file.get_mut(&occurrence.file()).ok_or_else(|| {
                    CompilerError::backend(format!(
                        "resolved import occurrence belongs to inactive source file {:?}",
                        occurrence.file()
                    ))
                })?;
                imports.push(ResolvedImport::from_occurrence(
                    edge.dependency(),
                    Arc::clone(target_package_name),
                    occurrence,
                ));
            }
        }

        for (file, mut imports) in by_file {
            imports.sort_by_key(ResolvedImport::source);
            let resolved = ResolvedFileImports::try_new(file, imports).map_err(|error| {
                CompilerError::backend(format!(
                    "failed to build resolved imports for source file {file:?}: {error}"
                ))
            })?;
            let (_, mutation) = self
                .database
                .set_resolved_file_imports_transactional(Arc::new(resolved))
                .map_err(|error| self.query_error(error))?;
            mutations.extend(mutation.map(AdmissionInputMutation::ResolvedImports));
        }
        Ok(())
    }

    fn commit_admission_mutations(&mut self, mutations: Vec<AdmissionInputMutation>) {
        for mutation in mutations {
            match mutation {
                AdmissionInputMutation::Source(mutation) => {
                    self.database.commit_source_mutations(Some(mutation));
                }
                AdmissionInputMutation::ResolvedImports(mutation) => {
                    self.database
                        .commit_resolved_import_mutations(Some(mutation));
                }
            }
        }
    }

    fn rollback_admission_mutations(&mut self, mutations: Vec<AdmissionInputMutation>) {
        for mutation in mutations.into_iter().rev() {
            match mutation {
                AdmissionInputMutation::Source(mutation) => {
                    self.database.rollback_source_mutations(Some(mutation));
                }
                AdmissionInputMutation::ResolvedImports(mutation) => {
                    self.database
                        .rollback_resolved_import_mutations(Some(mutation));
                }
            }
        }
    }

    fn unresolved_import(&self, import: &DirectImport) -> CompilerError {
        self.import_diagnostic(
            import,
            "GORS2004",
            format!(
                "unresolved import {:?}: no package catalog owns this canonical path",
                import.path()
            ),
        )
    }

    fn catalog_error(
        &self,
        import: &DirectImport,
        error: &crate::compiler::input::PackageCatalogError,
    ) -> CompilerError {
        self.import_diagnostic(
            import,
            "GORS2004",
            format!("failed to load import {:?}: {error}", import.path()),
        )
    }

    fn import_diagnostic(
        &self,
        import: &DirectImport,
        code: &'static str,
        message: String,
    ) -> CompilerError {
        CompilerError {
            diagnostics: vec![CompilerDiagnostic {
                code,
                message,
                file: import
                    .virtual_file()
                    .map_or_else(|| self.source_path_or_empty(import.file()), str::to_string),
                line: import.line(),
                column: import.column(),
            }],
        }
    }

    fn package_dag_error(&self, error: &PackageDagError) -> CompilerError {
        let Some(cycles) = error.cycles() else {
            return CompilerError::backend(format!("invalid resolved package graph: {error}"));
        };
        let diagnostics = cycles
            .iter()
            .filter_map(|cycle| {
                let first = cycle.hops().first()?;
                let mut path = cycle
                    .hops()
                    .iter()
                    .map(|hop| hop.import_path().as_str())
                    .collect::<Vec<_>>();
                if let Some(start) = path.first().copied() {
                    path.push(start);
                }
                Some(self.range_diagnostic(
                    first.source(),
                    "GORS2004",
                    format!("import cycle: {}", path.join(" -> ")),
                ))
            })
            .collect::<Vec<_>>();
        if diagnostics.is_empty() {
            return CompilerError::backend("package cycle contained no import occurrences");
        }
        CompilerError { diagnostics }
    }

    fn range_diagnostic(
        &self,
        range: FileRange,
        code: &'static str,
        message: String,
    ) -> CompilerDiagnostic {
        let fallback_file = self.source_path_or_empty(range.file());
        let coordinate = self
            .database
            .source_coordinate_map(range.file())
            .ok()
            .and_then(|map| {
                map.adjusted_coordinate_for(range.range().start(), &fallback_file)
                    .ok()
                    .flatten()
            });
        if let Some(coordinate) = coordinate {
            CompilerDiagnostic {
                code,
                message,
                file: coordinate.filename().to_string(),
                line: usize::try_from(coordinate.position().line().get()).unwrap_or(usize::MAX),
                column: usize::try_from(coordinate.position().column().to_go_column())
                    .unwrap_or(usize::MAX),
            }
        } else {
            CompilerDiagnostic {
                code,
                message,
                file: fallback_file,
                line: 1,
                column: 1,
            }
        }
    }
}

fn intrinsic_package_manifest(
    package: &PackageKey,
) -> Result<Option<Arc<PackageInputManifest>>, CompilerError> {
    if package.as_import_path() != Some("unsafe") {
        return Ok(None);
    }
    let file = SourceFileInput::from_source(
        "unsafe.go",
        "gors://intrinsics/unsafe.go",
        "package unsafe\n",
    )
    .map_err(|error| CompilerError::backend(format!("invalid unsafe package source: {error}")))?;
    let manifest = PackageInputManifest::new(package.clone(), [file]).map_err(|error| {
        CompilerError::backend(format!("invalid unsafe package manifest: {error}"))
    })?;
    Ok(Some(Arc::new(manifest)))
}
