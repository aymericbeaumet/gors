//! Verification for panic cleanup actions, payload replacement, and `recover`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::super::{
    Constant, DeferredAction, Effects, Function, Operand, PanicEdge, PanicPayloadCapture, Place,
    Provenance, RuntimeOp, RustType, RvalueKind, StoreOp, SyntheticOrigin, TerminatorKind,
};
use super::{verify_effects, verify_same, verify_source_provenance};
use crate::compiler::Diagnostic;
use crate::compiler::ids::{BasicBlockId, LocalId};
use gors_runtime_abi::RuntimeType;

pub(super) fn verify_panic_cleanup(function: &Function) -> Result<(), Diagnostic> {
    let Some(cleanup) = &function.panic_cleanup else {
        return verify_cleanup_edges(function, None, &BTreeSet::new());
    };
    {
        function.verify_target(cleanup.entry)?;
        function.verify_target(cleanup.completion)?;
        verify_same(
            function.place_ty(Place {
                local: cleanup.active,
            })?,
            RustType::Bool,
            "panic cleanup state",
        )?;
        verify_same(
            function.place_ty(Place {
                local: cleanup.recovered,
            })?,
            RustType::GoInterface,
            "panic cleanup recovered value",
        )?;
        verify_capture(function, cleanup.capture, "panic payload capture")?;
        if cleanup.rethrow.operation != RuntimeOp::PanicGoInterface
            || cleanup.rethrow.operation.signature().parameters() != [RuntimeType::GoInterface]
            || cleanup.rethrow.operation.signature().result() != RuntimeType::Unit
        {
            return Err(Diagnostic::backend(
                "Rust IR panic cleanup has an invalid payload rethrow operation",
            ));
        }
        verify_effects(
            cleanup.rethrow.effects,
            super::super::runtime_effects(cleanup.rethrow.operation),
            "panic payload rethrow",
        )?;
        verify_source_provenance(
            &cleanup.rethrow.provenance,
            function.id,
            "panic payload rethrow",
        )?;
    }

    if cleanup.actions.is_empty() {
        return Err(Diagnostic::backend(
            "Rust IR panic cleanup has no deferred actions",
        ));
    }
    let dispatches = cleanup
        .actions
        .iter()
        .map(|action| action.dispatch)
        .collect::<BTreeSet<_>>();
    if dispatches.len() != cleanup.actions.len() {
        return Err(Diagnostic::backend(
            "Rust IR panic cleanup repeats a deferred-action dispatch",
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
                "Rust IR deferred actions do not form one ordered cleanup chain",
            ));
        }
        verify_action(function, action, cleanup.active, cleanup.recovered)?;
        let region = action_region(function, action)?;
        if action.blocks != region.iter().copied().collect::<Vec<_>>() {
            return Err(Diagnostic::backend(
                "Rust IR deferred-action block set is not canonical",
            ));
        }
        for block in &region {
            if dispatches.contains(block) || *block == cleanup.completion || !claimed.insert(*block)
            {
                return Err(Diagnostic::backend(
                    "Rust IR deferred-action control-flow regions overlap",
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
    active: LocalId,
    recovered: LocalId,
) -> Result<(), Diagnostic> {
    for target in [
        action.dispatch,
        action.entry,
        action.continuation,
        action.replacement.target,
    ] {
        function.verify_target(target)?;
    }
    verify_same(
        function.place_ty(Place {
            local: action.registered,
        })?,
        RustType::Bool,
        "defer registration flag",
    )?;

    let dispatch = block(function, action.dispatch)?;
    if !dispatch.statements.is_empty()
        || !matches!(
            dispatch.terminator.kind,
            TerminatorKind::SwitchBool {
                condition: Operand::Read { place: Place { local }, .. },
                then_target,
                else_target,
            } if local == action.registered
                && then_target == action.entry
                && else_target == action.continuation
        )
    {
        return Err(Diagnostic::backend(
            "Rust IR deferred-action dispatch does not test its registration flag",
        ));
    }

    let entry = block(function, action.entry)?;
    let Some(clear) = entry.statements.first() else {
        return Err(Diagnostic::backend(
            "Rust IR deferred action does not clear its registration flag",
        ));
    };
    let write_only = Effects {
        may_write: true,
        ..Effects::default()
    };
    if clear.destination.local != action.registered
        || clear.store != StoreOp::SetSome
        || clear.effects != write_only
        || clear.value.effects != Effects::default()
        || clear.value.panic != PanicEdge::None
        || !matches!(
            clear.value.kind,
            RvalueKind::Use(Operand::Constant(Constant::Bool(false)))
        )
    {
        return Err(Diagnostic::backend(
            "Rust IR deferred action must begin by clearing its registration flag",
        ));
    }

    let replacement = action.replacement;
    if replacement.target != action.continuation
        || replacement.active != active
        || replacement.recovered != recovered
    {
        return Err(Diagnostic::backend(
            "Rust IR deferred-action panic replacement has an invalid continuation or state",
        ));
    }
    verify_capture(
        function,
        replacement.capture,
        "deferred-action panic replacement capture",
    )?;
    let capture = replacement.capture.effects;
    let expected = Effects {
        may_write: true,
        ..capture
    };
    verify_effects(
        replacement.effects,
        expected,
        "deferred-action panic replacement",
    )?;
    if replacement.provenance != Provenance::Synthetic(SyntheticOrigin::PanicCleanupDispatch) {
        return Err(Diagnostic::backend(
            "Rust IR deferred-action panic replacement has invalid provenance",
        ));
    }
    verify_source_provenance(
        &replacement.provenance,
        function.id,
        "deferred-action panic replacement",
    )
}

fn verify_capture(
    function: &Function,
    capture: PanicPayloadCapture,
    context: &str,
) -> Result<(), Diagnostic> {
    if capture.operation != RuntimeOp::GoPanicPayloadToInterface
        || capture.operation.signature().parameters() != [RuntimeType::GoPanicPayload]
        || capture.operation.signature().result() != RuntimeType::GoInterface
    {
        return Err(Diagnostic::backend(format!(
            "Rust IR {context} has an invalid payload capture operation"
        )));
    }
    verify_effects(
        capture.effects,
        super::super::runtime_effects(capture.operation),
        context,
    )?;
    verify_source_provenance(&capture.provenance, function.id, context)
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
                "Rust IR deferred-action region returns instead of reaching its continuation",
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
                    "Rust IR deferred-action panic bypasses its owned replacement boundary",
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
                    "Rust IR deferred-action region has an external control-flow predecessor",
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
                "Rust IR panic cleanup completion re-enters a deferred action",
            ));
        }
        let block = block(function, id)?;
        match &block.terminator.kind {
            TerminatorKind::Return(_) => returns += 1,
            TerminatorKind::Unreachable => {
                return Err(Diagnostic::backend(
                    "Rust IR panic cleanup completion does not terminate with a return",
                ));
            }
            kind => pending.extend(successors(kind)),
        }
    }
    if returns == 0 {
        return Err(Diagnostic::backend(
            "Rust IR panic cleanup completion has no terminal return",
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
                        "Rust IR deferred-action panic bypasses its owned replacement boundary",
                    ));
                }
                if cleanup_entry != Some(target) {
                    return Err(Diagnostic::backend(
                        "Rust IR panic edge does not target the function cleanup entry",
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
        TerminatorKind::Goto(target) | TerminatorKind::Call { next: target, .. } => vec![*target],
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
            "Rust IR deferred action references invalid block {}",
            id.0
        ))
    })
}

pub(super) fn verify_recover(
    function: &Function,
    state: Place,
    value: &Operand,
    nil: RuntimeOp,
) -> Result<RustType, Diagnostic> {
    verify_same(
        function.place_ty(state)?,
        RustType::Bool,
        "panic recovery state",
    )?;
    verify_same(
        function.operand_ty(value)?,
        RustType::GoInterface,
        "panic recovery value",
    )?;
    if nil != RuntimeOp::GoInterfaceNil
        || !nil.signature().parameters().is_empty()
        || nil.signature().result() != RuntimeType::GoInterface
    {
        return Err(Diagnostic::backend(
            "Rust IR recover uses an invalid nil-interface operation",
        ));
    }
    Ok(RustType::GoInterface)
}
