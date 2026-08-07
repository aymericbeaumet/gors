//! Mandatory representation decisions for panic cleanup actions.

use std::collections::{BTreeSet, VecDeque};

use crate::compiler::Diagnostic;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use gors_runtime_abi::RuntimeOp;

use super::lower_provenance;

pub(super) fn lower_panic_cleanup(cleanup: mir::PanicCleanup) -> out::PanicCleanup {
    out::PanicCleanup {
        entry: cleanup.entry,
        active: cleanup.active,
        recovered: cleanup.recovered,
        capture: payload_capture(),
        rethrow: out::PanicPayloadRethrow {
            operation: RuntimeOp::PanicGoInterface,
            effects: out::runtime_effects(RuntimeOp::PanicGoInterface),
            provenance: out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupDispatch),
        },
        actions: cleanup
            .actions
            .into_iter()
            .map(|action| out::DeferredAction {
                dispatch: action.dispatch,
                registered: action.registered,
                entry: action.entry,
                blocks: action.blocks,
                continuation: action.continuation,
                replacement: lower_replacement(action.replacement),
            })
            .collect(),
        completion: cleanup.completion,
    }
}

/// Records Rust-only blocks introduced while selecting concrete runtime calls.
///
/// Go MIR already owns the semantic action boundaries. Mandatory representation
/// lowering may split a block into several Rust runtime calls, so the final
/// verified Rust-IR plan must explicitly own those additional blocks too.
pub(super) fn complete_action_blocks(
    cleanup: Option<&mut out::PanicCleanup>,
    blocks: &[out::BasicBlock],
) -> Result<(), Diagnostic> {
    let Some(cleanup) = cleanup else {
        return Ok(());
    };
    for action in &mut cleanup.actions {
        let mut region = BTreeSet::new();
        let mut pending = VecDeque::from([action.entry]);
        while let Some(id) = pending.pop_front() {
            if id == action.continuation || !region.insert(id) {
                continue;
            }
            let block = blocks.get(id.0 as usize).ok_or_else(|| {
                Diagnostic::backend(format!(
                    "Rust representation lowering produced an invalid deferred-action block {}",
                    id.0
                ))
            })?;
            match &block.terminator.kind {
                out::TerminatorKind::Goto(target)
                | out::TerminatorKind::Call { next: target, .. } => pending.push_back(*target),
                out::TerminatorKind::SwitchBool {
                    then_target,
                    else_target,
                    ..
                } => {
                    pending.push_back(*then_target);
                    pending.push_back(*else_target);
                }
                out::TerminatorKind::Return(_) | out::TerminatorKind::Unreachable => {}
            }
        }
        action.blocks = region.into_iter().collect();
    }
    Ok(())
}

fn lower_replacement(replacement: mir::PanicReplacement) -> out::PanicReplacement {
    let capture = payload_capture();
    let capture_effects = capture.effects;
    out::PanicReplacement {
        target: replacement.target,
        active: replacement.active,
        recovered: replacement.recovered,
        capture,
        effects: out::Effects {
            may_write: true,
            ..capture_effects
        },
        provenance: lower_provenance(replacement.provenance),
    }
}

fn payload_capture() -> out::PanicPayloadCapture {
    out::PanicPayloadCapture {
        operation: RuntimeOp::GoPanicPayloadToInterface,
        effects: out::runtime_effects(RuntimeOp::GoPanicPayloadToInterface),
        provenance: out::Provenance::Synthetic(out::SyntheticOrigin::PanicCleanupDispatch),
    }
}
