//! Recognition of verified CFG shapes that have a direct idiomatic Rust form.

use std::collections::BTreeSet;

use super::{BasicBlockId, ControlFlowPlan, Function, TerminatorKind};
use crate::compiler::Diagnostic;

pub(in crate::compiler) fn select_control_flow_plan(
    function: &mut Function,
) -> Result<(), Diagnostic> {
    function.control_flow = recognize_linear_order(function)?
        .map_or(ControlFlowPlan::PcDispatchU32, |order| {
            ControlFlowPlan::StructuredLinear { order }
        });
    Ok(())
}

pub(super) fn recognize_linear_order(
    function: &Function,
) -> Result<Option<Vec<BasicBlockId>>, Diagnostic> {
    let mut order = Vec::with_capacity(function.blocks.len());
    let mut visited = BTreeSet::new();
    let mut current = function.entry;

    loop {
        if !visited.insert(current) {
            return Ok(None);
        }
        let block = function.blocks.get(current.0 as usize).ok_or_else(|| {
            Diagnostic::backend(format!(
                "Rust idiom recognition saw missing block {}",
                current.0
            ))
        })?;
        if block.id != current {
            return Err(Diagnostic::backend(format!(
                "Rust idiom recognition expected block {}, found {}",
                current.0, block.id.0
            )));
        }
        order.push(current);
        match &block.terminator.kind {
            TerminatorKind::Goto(next) | TerminatorKind::Call { next, .. } => current = *next,
            TerminatorKind::Return(_) if order.len() == function.blocks.len() => {
                return Ok(Some(order));
            }
            TerminatorKind::Return(_)
            | TerminatorKind::SwitchBool { .. }
            | TerminatorKind::Unreachable => return Ok(None),
        }
    }
}
