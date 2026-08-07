//! Delta-based source input mutation and exact transaction rollback.

use std::sync::Arc;

use salsa::Setter as _;

use super::queries::{PackageInput, SourceInput};
use super::resolved_imports::{ResolvedFileImports, ResolvedImportsInput};
use super::{CompilerDatabase, QueryError, SourceUpdate, identity_error};
use crate::compiler::ids::{FileId, PackageId};
use crate::compiler::input::{
    GoLanguageVersion, PackageKey, SourceContent, SourceSnapshot, WorkspaceKey,
};

/// Opaque reversible record for one source-facade mutation.
///
/// Production sessions retain these records only until the complete manifest
/// installation commits. The record owns only the prior values touched by the
/// mutation: exact no-ops produce no record, and unchanged files are never
/// materialized as [`SourceSnapshot`] values for rollback.
pub(in crate::compiler) struct SourceInputMutation {
    kind: SourceInputMutationKind,
}

enum SourceInputMutationKind {
    Updated {
        source: SourceInput,
        previous_content: Option<Arc<SourceContent>>,
        previous_language_version: Option<GoLanguageVersion>,
        previous_diagnostic_path: Option<Arc<str>>,
    },
    Inserted {
        file: FileId,
        source: SourceInput,
        package: PackageId,
        previous_package: Option<(PackageInput, Arc<[SourceInput]>)>,
    },
    Removed {
        file: FileId,
        source: SourceInput,
        diagnostic_path: Arc<str>,
        package: PackageId,
        previous_package: PackageInput,
        previous_package_sources: Arc<[SourceInput]>,
    },
}

impl CompilerDatabase {
    /// Insert or update one source while returning an exact rollback record.
    ///
    /// This is compiler-session infrastructure rather than a second public
    /// mutation API. An exact no-op returns `None`, performs no Salsa setter,
    /// and does not synthesize a source snapshot.
    pub(in crate::compiler) fn set_source_transactional(
        &mut self,
        workspace: &WorkspaceKey,
        package: &PackageKey,
        logical_path: &str,
        language_version: GoLanguageVersion,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<(SourceUpdate, Option<SourceInputMutation>), QueryError> {
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

        if self.sources.contains_key(&file) {
            self.replace_source_revision(file, language_version, snapshot)
        } else {
            let previous_package = self
                .packages
                .get(&package)
                .copied()
                .map(|input| (input, input.sources(self)));
            let content = snapshot.content();
            let resolved_imports = ResolvedImportsInput::new(
                self,
                file,
                Arc::new(ResolvedFileImports::empty(file)),
                Arc::from([]),
            );
            let input = SourceInput::new(
                self,
                package,
                file,
                Arc::from(logical_path),
                language_version,
                content,
                resolved_imports,
            );
            self.sources.insert(file, input);
            self.diagnostic_paths
                .insert(file, snapshot.shared_diagnostic_path());
            self.add_package_source(package, input);
            Ok((
                SourceUpdate {
                    file,
                    semantic_changed: true,
                    diagnostic_path_changed: true,
                    inserted: true,
                },
                Some(SourceInputMutation {
                    kind: SourceInputMutationKind::Inserted {
                        file,
                        source: input,
                        package,
                        previous_package,
                    },
                }),
            ))
        }
    }

    /// Remove one active source while retaining enough state for exact
    /// transaction rollback.
    ///
    /// The source payload is deliberately not tombstoned until commit. This
    /// preserves the original Salsa input and package membership handle if a
    /// later manifest mutation fails.
    pub(in crate::compiler) fn remove_source_transactional(
        &mut self,
        file: FileId,
    ) -> Result<SourceInputMutation, QueryError> {
        let source = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        let diagnostic_path = self
            .diagnostic_paths
            .get(&file)
            .cloned()
            .ok_or(QueryError::UnknownFile(file))?;
        let package = source.package(self);
        let previous_package = self
            .packages
            .get(&package)
            .copied()
            .ok_or(QueryError::UnknownPackage(package))?;
        let previous_package_sources = previous_package.sources(self);

        self.sources.remove(&file);
        self.diagnostic_paths.remove(&file);
        self.remove_package_source(package, file)?;
        Ok(SourceInputMutation {
            kind: SourceInputMutationKind::Removed {
                file,
                source,
                diagnostic_path,
                package,
                previous_package,
                previous_package_sources,
            },
        })
    }

    /// Finalize source mutations after a complete manifest installation.
    ///
    /// Only removals need work at commit: their detached Salsa input is
    /// tombstoned so the previous source bytes can be released. Updates and
    /// inserts have already published their final values.
    pub(in crate::compiler) fn commit_source_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = SourceInputMutation>,
    ) {
        for mutation in mutations {
            if let SourceInputMutationKind::Removed { source, .. } = mutation.kind {
                source
                    .set_content(self)
                    .to(Arc::new(SourceContent::empty()));
            }
        }
    }

    /// Undo source mutations in reverse application order.
    ///
    /// Every required handle and prior value is captured before mutation, so
    /// rollback has no fallible identity lookup or reconstruction path.
    pub(in crate::compiler) fn rollback_source_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = SourceInputMutation>,
    ) {
        for mutation in mutations {
            match mutation.kind {
                SourceInputMutationKind::Updated {
                    source,
                    previous_content,
                    previous_language_version,
                    previous_diagnostic_path,
                } => {
                    if let Some(content) = previous_content {
                        source.set_content(self).to(content);
                    }
                    if let Some(language_version) = previous_language_version {
                        source.set_language_version(self).to(language_version);
                    }
                    if let Some(path) = previous_diagnostic_path {
                        self.diagnostic_paths.insert(source.file(self), path);
                    }
                }
                SourceInputMutationKind::Inserted {
                    file,
                    source,
                    package,
                    previous_package,
                } => {
                    self.sources.remove(&file);
                    self.diagnostic_paths.remove(&file);
                    self.restore_package_membership(package, previous_package);
                    source
                        .set_content(self)
                        .to(Arc::new(SourceContent::empty()));
                }
                SourceInputMutationKind::Removed {
                    file,
                    source,
                    diagnostic_path,
                    package,
                    previous_package,
                    previous_package_sources,
                } => {
                    self.sources.insert(file, source);
                    self.diagnostic_paths.insert(file, diagnostic_path);
                    self.restore_package_membership(
                        package,
                        Some((previous_package, previous_package_sources)),
                    );
                }
            }
        }
    }

    fn replace_source_revision(
        &mut self,
        file: FileId,
        language_version: GoLanguageVersion,
        snapshot: Arc<SourceSnapshot>,
    ) -> Result<(SourceUpdate, Option<SourceInputMutation>), QueryError> {
        let input = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        let content = snapshot.content();
        let previous_content = input.content(self);
        let semantic_changed = previous_content.as_ref() != content.as_ref();
        if semantic_changed {
            input.set_content(self).to(content);
        }
        let previous_language_version = input.language_version(self);
        let language_version_changed = previous_language_version != language_version;
        if language_version_changed {
            input.set_language_version(self).to(language_version);
        }
        let diagnostic_path = snapshot.shared_diagnostic_path();
        let previous_diagnostic_path = self
            .diagnostic_paths
            .get(&file)
            .cloned()
            .ok_or(QueryError::UnknownFile(file))?;
        let diagnostic_path_changed = previous_diagnostic_path.as_ref() != diagnostic_path.as_ref();
        if diagnostic_path_changed {
            self.diagnostic_paths.insert(file, diagnostic_path);
        }
        let mutation = (semantic_changed || language_version_changed || diagnostic_path_changed)
            .then(|| SourceInputMutation {
                kind: SourceInputMutationKind::Updated {
                    source: input,
                    previous_content: semantic_changed.then_some(previous_content),
                    previous_language_version: language_version_changed
                        .then_some(previous_language_version),
                    previous_diagnostic_path: diagnostic_path_changed
                        .then_some(previous_diagnostic_path),
                },
            });
        Ok((
            SourceUpdate {
                file,
                semantic_changed: semantic_changed || language_version_changed,
                diagnostic_path_changed,
                inserted: false,
            },
            mutation,
        ))
    }

    fn restore_package_membership(
        &mut self,
        package: PackageId,
        previous: Option<(PackageInput, Arc<[SourceInput]>)>,
    ) {
        match previous {
            Some((input, sources)) => {
                input.set_sources(self).to(sources);
                self.packages.insert(package, input);
            }
            None => {
                if let Some(input) = self.packages.remove(&package) {
                    input.set_sources(self).to(Arc::from([]));
                }
            }
        }
    }
}
