//! Source and synthetic provenance projection into Rust IR.

use crate::compiler::mir;
use crate::compiler::rust_ir as out;

pub(super) fn lower_provenance(provenance: mir::Provenance) -> out::Provenance {
    match provenance {
        mir::Provenance::Source(source) => out::Provenance::Source(source),
        mir::Provenance::Synthetic(mir::SyntheticOrigin::NamedResultInitialization) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::NamedResultInitialization)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::PanicCleanupInitialization) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupInitialization)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::ZeroValueCall) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::ZeroValueCall)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::PanicCleanupDispatch) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupDispatch)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::ImplicitReturn) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::ImplicitReturn)
        }
    }
}
