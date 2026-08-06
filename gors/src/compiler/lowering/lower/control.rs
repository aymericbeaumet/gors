//! Terminal control-flow metadata selected during representation lowering.

use crate::compiler::mir;
use crate::compiler::rust_ir as out;

pub(super) fn finish_terminator(
    kind: out::TerminatorKind,
    provenance: out::Provenance,
    panic: mir::PanicEdge,
) -> out::Terminator {
    let effects = out::terminator_effects(&kind);
    out::Terminator {
        kind,
        effects,
        panic: lower_panic_edge(panic, effects),
        provenance,
    }
}

pub(super) fn lower_panic_edge(edge: mir::PanicEdge, effects: out::Effects) -> out::PanicEdge {
    if !effects.may_panic {
        return out::PanicEdge::None;
    }
    match edge {
        mir::PanicEdge::Cleanup(target) => out::PanicEdge::Cleanup(target),
        mir::PanicEdge::None | mir::PanicEdge::Propagate => out::PanicEdge::Propagate,
    }
}
