//! MIR provenance constraints kept separate from structural verification.

use super::super::{Provenance, SyntheticOrigin};
use crate::compiler::Diagnostic;
use crate::compiler::ids::DefId;
use crate::compiler::provenance::SourceRef;

pub(super) fn verify_source_provenance(
    provenance: &Provenance,
    owner: DefId,
    context: &str,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, context),
        Provenance::Synthetic(
            SyntheticOrigin::PanicCleanupDispatch | SyntheticOrigin::ZeroValueCall,
        ) => Ok(()),
        Provenance::Synthetic(origin) => Err(Diagnostic::backend(format!(
            "synthetic provenance {origin:?} is invalid for {context}"
        ))),
    }
}

pub(super) fn verify_statement_provenance(
    provenance: &Provenance,
    owner: DefId,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "statement"),
        Provenance::Synthetic(
            SyntheticOrigin::NamedResultInitialization
            | SyntheticOrigin::PanicCleanupInitialization,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a statement"
        ))),
    }
}

pub(super) fn verify_rvalue_provenance(
    provenance: &Provenance,
    owner: DefId,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "rvalue"),
        Provenance::Synthetic(
            SyntheticOrigin::NamedResultInitialization
            | SyntheticOrigin::PanicCleanupInitialization,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for an rvalue"
        ))),
    }
}

pub(super) fn verify_terminator_provenance(
    provenance: &Provenance,
    owner: DefId,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "terminator"),
        Provenance::Synthetic(
            SyntheticOrigin::ImplicitReturn
            | SyntheticOrigin::PanicCleanupDispatch
            | SyntheticOrigin::ZeroValueCall,
        ) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a terminator"
        ))),
    }
}

pub(super) fn verify_source_ref(
    source: SourceRef,
    owner: DefId,
    context: &str,
) -> Result<(), Diagnostic> {
    (source.owner() == owner).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} source reference is owned by DefId {}, expected {owner}",
            source.owner()
        ))
    })
}
