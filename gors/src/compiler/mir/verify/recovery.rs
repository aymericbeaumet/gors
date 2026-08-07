//! Verification for ordered, independently unwindable deferred actions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::super::{
    DeferredAction, Function, Operand, PanicEdge, Place, Provenance, RvalueKind, SyntheticOrigin,
    TerminatorKind,
};
use super::provenance::verify_source_provenance;
use super::type_rules::verify_same_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::BasicBlockId;
use crate::compiler::types::{ConstValue, Ty};

pub(super) fn verify_panic_cleanup(function: &Function) -> Result<(), Diagnostic> {
    let Some(cleanup) = &function.panic_cleanup else {
        return verify_cleanup_edges(function, None, &BTreeSet::new());
    };

    verify_target(function, cleanup.entry, "panic cleanup entry")?;
    verify_target(function, cleanup.completion, "panic cleanup completion")?;
    verify_same_type(
        place_ty(function, cleanup.active)?,
        &Ty::Bool,
        "panic cleanup state",
    )?;
    verify_same_type(
        place_ty(function, cleanup.recovered)?,
        &Ty::Interface(Vec::new()),
        "panic cleanup recovered value",
    )?;
    if cleanup.actions.is_empty() {
        return Err(Diagnostic::backend(
            "MIR panic cleanup has no deferred actions",
        ));
    }

    let dispatches = cleanup
        .actions
        .iter()
        .map(|action| action.dispatch)
        .collect::<BTreeSet<_>>();
    if dispatches.len() != cleanup.actions.len() {
        return Err(Diagnostic::backend(
            "MIR panic cleanup repeats a deferred-action dispatch",
        ));
    }

    let predecessors = predecessors(function);
    let mut claimed = BTreeSet::new();
    let mut expected_dispatch = cleanup.entry;
    for (index, action) in cleanup.actions.iter().enumerate() {
        let expected_continuation = cleanup
            .actions
            .get(index + 1)
            .map_or(cleanup.completion, |next| next.dispatch);
        if action.dispatch != expected_dispatch || action.continuation != expected_continuation {
            return Err(Diagnostic::backend(
                "MIR deferred actions do not form one ordered cleanup chain",
            ));
        }
        verify_action(function, action, cleanup.active, cleanup.recovered)?;
        let region = action_region(function, action)?;
        if action.blocks != region.iter().copied().collect::<Vec<_>>() {
            return Err(Diagnostic::backend(
                "MIR deferred-action block set is not canonical",
            ));
        }
        for block in &region {
            if dispatches.contains(block) || *block == cleanup.completion || !claimed.insert(*block)
            {
                return Err(Diagnostic::backend(
                    "MIR deferred-action control-flow regions overlap",
                ));
            }
        }
        verify_region_predecessors(action, &region, &predecessors)?;
        expected_dispatch = action.continuation;
    }

    verify_completion(function, cleanup.completion, &dispatches, &claimed)?;
    verify_cleanup_edges(function, Some(cleanup.entry), &claimed)
}

fn verify_action(
    function: &Function,
    action: &DeferredAction,
    active: crate::compiler::ids::LocalId,
    recovered: crate::compiler::ids::LocalId,
) -> Result<(), Diagnostic> {
    for (target, context) in [
        (action.dispatch, "deferred-action dispatch"),
        (action.entry, "deferred-action entry"),
        (action.continuation, "deferred-action continuation"),
        (
            action.replacement.target,
            "deferred-action panic replacement",
        ),
    ] {
        verify_target(function, target, context)?;
    }
    verify_same_type(
        place_ty(function, action.registered)?,
        &Ty::Bool,
        "defer registration flag",
    )?;

    let dispatch = block(function, action.dispatch)?;
    if !dispatch.statements.is_empty()
        || !matches!(
            dispatch.terminator.kind,
            TerminatorKind::SwitchBool {
                condition: Operand::Read(Place { local }),
                then_target,
                else_target,
            } if local == action.registered
                && then_target == action.entry
                && else_target == action.continuation
        )
    {
        return Err(Diagnostic::backend(
            "MIR deferred-action dispatch does not test its registration flag",
        ));
    }

    let entry = block(function, action.entry)?;
    let Some(clear) = entry.statements.first() else {
        return Err(Diagnostic::backend(
            "MIR deferred action does not clear its registration flag",
        ));
    };
    let write_only = hir::Effects {
        may_write: true,
        ..hir::Effects::default()
    };
    if clear.destination.local != action.registered
        || clear.effects != write_only
        || clear.value.effects != hir::Effects::default()
        || clear.value.panic != PanicEdge::None
        || !matches!(
            clear.value.kind,
            RvalueKind::Use(Operand::Constant(ConstValue::Bool(false), Ty::Bool))
        )
    {
        return Err(Diagnostic::backend(
            "MIR deferred action must begin by clearing its registration flag",
        ));
    }

    let replacement = &action.replacement;
    if replacement.target != action.continuation
        || replacement.active != active
        || replacement.recovered != recovered
    {
        return Err(Diagnostic::backend(
            "MIR deferred-action panic replacement has an invalid continuation or state",
        ));
    }
    if replacement.effects != write_only {
        return Err(Diagnostic::backend(
            "MIR deferred-action panic replacement effect mismatch",
        ));
    }
    if replacement.provenance != Provenance::Synthetic(SyntheticOrigin::PanicCleanupDispatch) {
        return Err(Diagnostic::backend(
            "MIR deferred-action panic replacement has invalid provenance",
        ));
    }
    verify_source_provenance(
        &replacement.provenance,
        function.id,
        "deferred-action panic replacement",
    )
}

fn action_region(
    function: &Function,
    action: &DeferredAction,
) -> Result<BTreeSet<BasicBlockId>, Diagnostic> {
    let mut region = BTreeSet::new();
    let mut pending = VecDeque::from([action.entry]);
    while let Some(id) = pending.pop_front() {
        if id == action.continuation {
            continue;
        }
        if !region.insert(id) {
            continue;
        }
        let block = block(function, id)?;
        if matches!(block.terminator.kind, TerminatorKind::Return(_)) {
            return Err(Diagnostic::backend(
                "MIR deferred-action region returns instead of reaching its continuation",
            ));
        }
        for edge in block
            .statements
            .iter()
            .map(|statement| statement.value.panic)
            .chain(std::iter::once(block.terminator.panic))
        {
            if edge != PanicEdge::None && edge != PanicEdge::Propagate {
                return Err(Diagnostic::backend(
                    "MIR deferred-action panic bypasses its owned replacement boundary",
                ));
            }
        }
        pending.extend(successors(&block.terminator.kind));
    }
    Ok(region)
}

fn verify_region_predecessors(
    action: &DeferredAction,
    region: &BTreeSet<BasicBlockId>,
    predecessors: &BTreeMap<BasicBlockId, Vec<BasicBlockId>>,
) -> Result<(), Diagnostic> {
    for target in region {
        for predecessor in predecessors.get(target).into_iter().flatten() {
            let valid = if *target == action.entry {
                *predecessor == action.dispatch
            } else {
                region.contains(predecessor)
            };
            if !valid {
                return Err(Diagnostic::backend(
                    "MIR deferred-action region has an external control-flow predecessor",
                ));
            }
        }
    }
    Ok(())
}

fn verify_completion(
    function: &Function,
    completion: BasicBlockId,
    dispatches: &BTreeSet<BasicBlockId>,
    actions: &BTreeSet<BasicBlockId>,
) -> Result<(), Diagnostic> {
    let mut seen = BTreeSet::new();
    let mut pending = VecDeque::from([completion]);
    let mut returns = 0usize;
    while let Some(id) = pending.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        if dispatches.contains(&id) || actions.contains(&id) {
            return Err(Diagnostic::backend(
                "MIR panic cleanup completion re-enters a deferred action",
            ));
        }
        let block = block(function, id)?;
        match &block.terminator.kind {
            TerminatorKind::Return(_) => returns += 1,
            TerminatorKind::Unreachable => {
                return Err(Diagnostic::backend(
                    "MIR panic cleanup completion does not terminate with a return",
                ));
            }
            kind => pending.extend(successors(kind)),
        }
    }
    if returns == 0 {
        return Err(Diagnostic::backend(
            "MIR panic cleanup completion has no terminal return",
        ));
    }
    Ok(())
}

fn verify_cleanup_edges(
    function: &Function,
    cleanup_entry: Option<BasicBlockId>,
    action_blocks: &BTreeSet<BasicBlockId>,
) -> Result<(), Diagnostic> {
    for block in &function.blocks {
        for edge in block
            .statements
            .iter()
            .map(|statement| statement.value.panic)
            .chain(std::iter::once(block.terminator.panic))
        {
            if let PanicEdge::Cleanup(target) = edge {
                if action_blocks.contains(&block.id) {
                    return Err(Diagnostic::backend(
                        "MIR deferred-action panic bypasses its owned replacement boundary",
                    ));
                }
                if cleanup_entry != Some(target) {
                    return Err(Diagnostic::backend(
                        "MIR panic edge does not target the function cleanup entry",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn predecessors(function: &Function) -> BTreeMap<BasicBlockId, Vec<BasicBlockId>> {
    let mut predecessors = BTreeMap::<BasicBlockId, Vec<BasicBlockId>>::new();
    for block in &function.blocks {
        for target in successors(&block.terminator.kind) {
            predecessors.entry(target).or_default().push(block.id);
        }
    }
    predecessors
}

fn successors(terminator: &TerminatorKind) -> Vec<BasicBlockId> {
    match terminator {
        TerminatorKind::Goto(target)
        | TerminatorKind::Call { target, .. }
        | TerminatorKind::SpawnEmpty { target } => vec![*target],
        TerminatorKind::SwitchBool {
            then_target,
            else_target,
            ..
        } => vec![*then_target, *else_target],
        TerminatorKind::Return(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}

fn block(function: &Function, id: BasicBlockId) -> Result<&super::super::BasicBlock, Diagnostic> {
    function.blocks.get(id.0 as usize).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR deferred action references invalid block {}",
            id.0
        ))
    })
}

fn place_ty(function: &Function, local: crate::compiler::ids::LocalId) -> Result<&Ty, Diagnostic> {
    function
        .locals
        .get(local.0 as usize)
        .map(|local| &local.ty)
        .ok_or_else(|| {
            Diagnostic::backend(format!(
                "MIR deferred action references invalid local {}",
                local.0
            ))
        })
}

fn verify_target(
    function: &Function,
    target: BasicBlockId,
    context: &str,
) -> Result<(), Diagnostic> {
    ((target.0 as usize) < function.blocks.len())
        .then_some(())
        .ok_or_else(|| Diagnostic::backend(format!("MIR {context} block does not exist")))
}
