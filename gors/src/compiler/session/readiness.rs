//! Per-definition scheduler readiness for retained compiler sessions.

use std::collections::{BTreeMap, BTreeSet};

use super::CompilerSession;
use crate::compiler::CompilerError;
use crate::compiler::db::{Fingerprint, PackageAnalysis};
use crate::compiler::ids::{DefId, FileId, PackageId};

/// One stable, independently schedulable Rust-IR query root.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RustIrRoot {
    package: PackageId,
    file: FileId,
    definition: DefId,
}

impl RustIrRoot {
    fn new(package: PackageId, file: FileId, definition: DefId) -> Self {
        Self {
            package,
            file,
            definition,
        }
    }

    pub(super) const fn file(self) -> FileId {
        self.file
    }

    pub(super) const fn definition(self) -> DefId {
        self.definition
    }
}

pub(super) type RootInputFingerprints = BTreeMap<RustIrRoot, Fingerprint>;

impl CompilerSession {
    /// Drop removed, renamed, or relocated roots before any early diagnostic
    /// can leave obsolete readiness attached to this package.
    pub(super) fn reconcile_ready_roots(&mut self, analysis: &PackageAnalysis) {
        let package = analysis.package();
        let current = analysis
            .functions()
            .iter()
            .map(|function| RustIrRoot::new(package, function.file(), function.id()))
            .collect::<BTreeSet<_>>();
        self.ready_rust_ir_roots
            .retain(|root, _| root.package != package || current.contains(root));
    }

    /// Drop roots whose logical file is no longer part of the installed
    /// manifest without forcing semantic analysis of unrelated packages.
    pub(super) fn retain_ready_roots_for_files(&mut self, files: &BTreeSet<FileId>) {
        self.ready_rust_ir_roots
            .retain(|root, _| files.contains(&root.file));
    }

    /// Compute the complete cheap invalidation digest for every current root.
    pub(super) fn current_root_inputs(
        &self,
        analysis: &PackageAnalysis,
    ) -> Result<RootInputFingerprints, CompilerError> {
        let package = analysis.package();
        analysis
            .functions()
            .iter()
            .filter_map(|function| {
                match self
                    .database
                    .is_generic_function(function.file(), function.id())
                {
                    Ok(true) => return None,
                    Ok(false) => {}
                    Err(error) => return Some(Err(self.query_error(error))),
                }
                let root = RustIrRoot::new(package, function.file(), function.id());
                Some(
                    self.database
                        .rust_ir_root_inputs(function.file(), function.id())
                        .map(|fingerprint| (root, fingerprint))
                        .map_err(|error| self.query_error(error)),
                )
            })
            .collect()
    }

    /// Select only roots whose complete Rust-IR inputs differ from the last
    /// fully joined wave in this retained session.
    pub(super) fn roots_requiring_prewarm(
        &self,
        current: &RootInputFingerprints,
    ) -> Vec<RustIrRoot> {
        current
            .iter()
            .filter_map(|(root, fingerprint)| {
                (self.ready_rust_ir_roots.get(root) != Some(fingerprint)).then_some(*root)
            })
            .collect()
    }

    /// Publish readiness only after every worker snapshot has been joined.
    pub(super) fn publish_ready_roots(&mut self, current: RootInputFingerprints) {
        self.ready_rust_ir_roots.extend(current);
    }
}
