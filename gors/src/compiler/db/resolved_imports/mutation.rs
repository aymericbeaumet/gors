//! Exact delta mutation and rollback for resolved-import inputs.

use std::sync::Arc;

use salsa::Setter as _;

use super::{ResolvedFileImports, ResolvedImportsInput};
use crate::compiler::db::{CompilerDatabase, QueryError};
use crate::compiler::ids::FileId;

/// Exact classification of one resolved-import installation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedImportsUpdate {
    file: FileId,
    changed: bool,
    inserted: bool,
}

impl ResolvedImportsUpdate {
    /// Stable source file whose resolution input was installed.
    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    /// Whether the immutable resolution product changed.
    #[must_use]
    pub const fn changed(self) -> bool {
        self.changed
    }

    /// Whether the request created the file's first resolution input.
    #[must_use]
    pub const fn inserted(self) -> bool {
        self.inserted
    }
}

/// Opaque reversible record for one resolved-import input mutation.
pub(in crate::compiler) struct ResolvedImportInputMutation {
    kind: ResolvedImportInputMutationKind,
}

enum ResolvedImportInputMutationKind {
    Updated {
        input: ResolvedImportsInput,
        previous: Arc<ResolvedFileImports>,
        previous_targets: Arc<
            [(
                crate::compiler::ids::PackageId,
                super::super::queries::PackageInput,
            )],
        >,
    },
    Inserted {
        file: FileId,
        input: ResolvedImportsInput,
    },
    Removed {
        file: FileId,
        input: ResolvedImportsInput,
        previous: Arc<ResolvedFileImports>,
    },
}

impl CompilerDatabase {
    /// Install one immutable file-scoped resolution product.
    pub fn set_resolved_file_imports(
        &mut self,
        resolved: Arc<ResolvedFileImports>,
    ) -> Result<ResolvedImportsUpdate, QueryError> {
        let (update, mutation) = self.set_resolved_file_imports_transactional(resolved)?;
        self.commit_resolved_import_mutations(mutation);
        Ok(update)
    }

    /// Remove one file-scoped resolution product.
    pub fn remove_resolved_file_imports(&mut self, file: FileId) -> Result<(), QueryError> {
        let mutation = self.remove_resolved_file_imports_transactional(file)?;
        self.commit_resolved_import_mutations(Some(mutation));
        Ok(())
    }

    /// Demand the exact installed file-scoped resolution product.
    pub fn resolved_file_imports(
        &self,
        file: FileId,
    ) -> Result<Arc<ResolvedFileImports>, QueryError> {
        self.resolved_imports
            .get(&file)
            .copied()
            .map(|input| input.value(self))
            .ok_or(QueryError::UnknownResolvedImports(file))
    }

    /// Active resolved-import input keys in deterministic order.
    #[must_use]
    pub fn active_resolved_import_files(&self) -> Vec<FileId> {
        self.resolved_imports.keys().copied().collect()
    }

    /// Approximate bytes retained by active resolution products.
    #[must_use]
    pub fn retained_resolved_import_bytes(&self) -> usize {
        self.resolved_imports
            .values()
            .fold(0_usize, |total, input| {
                total.saturating_add(input.value(self).retained_bytes())
            })
    }

    /// Install one product and return the exact record required for rollback.
    ///
    /// Structural no-ops do not call a Salsa setter and return no mutation
    /// record.
    pub(in crate::compiler) fn set_resolved_file_imports_transactional(
        &mut self,
        resolved: Arc<ResolvedFileImports>,
    ) -> Result<(ResolvedImportsUpdate, Option<ResolvedImportInputMutation>), QueryError> {
        let file = resolved.file();
        let source = self
            .sources
            .get(&file)
            .copied()
            .ok_or(QueryError::UnknownFile(file))?;
        let mut targets = resolved
            .imports()
            .iter()
            .map(|import| {
                self.packages
                    .get(&import.target_package())
                    .copied()
                    .map(|input| (import.target_package(), input))
                    .ok_or_else(|| QueryError::UnknownPackage(import.target_package()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        targets.sort_by_key(|(package, _)| *package);
        targets.dedup_by_key(|(package, _)| *package);
        let targets: Arc<
            [(
                crate::compiler::ids::PackageId,
                super::super::queries::PackageInput,
            )],
        > = targets.into();

        if let Some(input) = self.resolved_imports.get(&file).copied() {
            let previous = input.value(self);
            let previous_targets = input.targets(self);
            if previous.as_ref() == resolved.as_ref() && previous_targets == targets {
                return Ok((
                    ResolvedImportsUpdate {
                        file,
                        changed: false,
                        inserted: false,
                    },
                    None,
                ));
            }
            input.set_value(self).to(resolved);
            input.set_targets(self).to(targets);
            return Ok((
                ResolvedImportsUpdate {
                    file,
                    changed: true,
                    inserted: false,
                },
                Some(ResolvedImportInputMutation {
                    kind: ResolvedImportInputMutationKind::Updated {
                        input,
                        previous,
                        previous_targets,
                    },
                }),
            ));
        }

        let input = source.resolved_imports(self);
        input.set_value(self).to(resolved);
        input.set_targets(self).to(targets);
        self.resolved_imports.insert(file, input);
        Ok((
            ResolvedImportsUpdate {
                file,
                changed: true,
                inserted: true,
            },
            Some(ResolvedImportInputMutation {
                kind: ResolvedImportInputMutationKind::Inserted { file, input },
            }),
        ))
    }

    /// Detach one product while preserving the exact handle for rollback.
    pub(in crate::compiler) fn remove_resolved_file_imports_transactional(
        &mut self,
        file: FileId,
    ) -> Result<ResolvedImportInputMutation, QueryError> {
        let input = self
            .resolved_imports
            .remove(&file)
            .ok_or(QueryError::UnknownResolvedImports(file))?;
        let previous = input.value(self);
        Ok(ResolvedImportInputMutation {
            kind: ResolvedImportInputMutationKind::Removed {
                file,
                input,
                previous,
            },
        })
    }

    /// Finalize detached or rolled-forward resolution inputs.
    pub(in crate::compiler) fn commit_resolved_import_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = ResolvedImportInputMutation>,
    ) {
        for mutation in mutations {
            if let ResolvedImportInputMutationKind::Removed { file, input, .. } = mutation.kind {
                input
                    .set_value(self)
                    .to(Arc::new(ResolvedFileImports::empty(file)));
                input.set_targets(self).to(Arc::from([]));
            }
        }
    }

    /// Undo resolution-input mutations in reverse application order.
    pub(in crate::compiler) fn rollback_resolved_import_mutations(
        &mut self,
        mutations: impl IntoIterator<Item = ResolvedImportInputMutation>,
    ) {
        for mutation in mutations {
            match mutation.kind {
                ResolvedImportInputMutationKind::Updated {
                    input,
                    previous,
                    previous_targets,
                } => {
                    input.set_value(self).to(previous);
                    input.set_targets(self).to(previous_targets);
                }
                ResolvedImportInputMutationKind::Inserted { file, input } => {
                    self.resolved_imports.remove(&file);
                    input
                        .set_value(self)
                        .to(Arc::new(ResolvedFileImports::empty(file)));
                    input.set_targets(self).to(Arc::from([]));
                }
                ResolvedImportInputMutationKind::Removed {
                    file,
                    input,
                    previous,
                } => {
                    input.set_value(self).to(previous);
                    self.resolved_imports.insert(file, input);
                }
            }
        }
    }
}
