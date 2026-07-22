//! Definite-initialization proof for Rust-IR storage slots.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    BasicBlock, BasicBlockId, Function, LocalId, Operand, Rvalue, RvalueKind, SlotInitialization,
    Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;

impl Function {
    pub(super) fn verify_storage_dataflow(&self) -> Result<(), Diagnostic> {
        let reachable = self.reachable_blocks()?;
        if reachable.len() != self.blocks.len() {
            return Err(Diagnostic::backend(
                "Rust IR contains a block outside the verified control-flow graph",
            ));
        }

        let universe = self
            .locals
            .iter()
            .map(|local| local.id)
            .collect::<BTreeSet<_>>();
        let entry_state = self
            .locals
            .iter()
            .filter_map(|local| {
                matches!(local.initialization, SlotInitialization::Parameter(_)).then_some(local.id)
            })
            .collect::<BTreeSet<_>>();
        let mut predecessors = BTreeMap::<BasicBlockId, Vec<BasicBlockId>>::new();
        for block_id in &reachable {
            let block = self.block(*block_id)?;
            for successor in block_successors(&block.terminator) {
                predecessors.entry(successor).or_default().push(*block_id);
            }
        }
        let mut inputs = reachable
            .iter()
            .copied()
            .map(|block| (block, universe.clone()))
            .collect::<BTreeMap<_, _>>();

        loop {
            let mut changed = false;
            for block_id in &reachable {
                let mut candidates = Vec::new();
                if *block_id == self.entry {
                    candidates.push(entry_state.clone());
                }
                if let Some(block_predecessors) = predecessors.get(block_id) {
                    for predecessor in block_predecessors {
                        let predecessor_input = inputs.get(predecessor).ok_or_else(|| {
                            Diagnostic::backend(format!(
                                "missing Rust IR storage state for block {}",
                                predecessor.0
                            ))
                        })?;
                        let mut output = predecessor_input.clone();
                        self.transfer_block(self.block(*predecessor)?, &mut output, false)?;
                        candidates.push(output);
                    }
                }
                let Some(mut incoming) = candidates.pop() else {
                    return Err(Diagnostic::backend(format!(
                        "reachable Rust IR block {} has no predecessor",
                        block_id.0
                    )));
                };
                for candidate in candidates {
                    incoming = incoming.intersection(&candidate).copied().collect();
                }
                let current = inputs.get_mut(block_id).ok_or_else(|| {
                    Diagnostic::backend(format!(
                        "missing Rust IR storage input for block {}",
                        block_id.0
                    ))
                })?;
                if *current != incoming {
                    *current = incoming;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for block_id in reachable {
            let mut state = inputs.get(&block_id).cloned().ok_or_else(|| {
                Diagnostic::backend(format!(
                    "missing final Rust IR storage state for block {}",
                    block_id.0
                ))
            })?;
            self.transfer_block(self.block(block_id)?, &mut state, true)?;
        }
        Ok(())
    }

    fn reachable_blocks(&self) -> Result<BTreeSet<BasicBlockId>, Diagnostic> {
        let mut reachable = BTreeSet::new();
        let mut pending = VecDeque::from([self.entry]);
        while let Some(block_id) = pending.pop_front() {
            if !reachable.insert(block_id) {
                continue;
            }
            let block = self.block(block_id)?;
            pending.extend(block_successors(&block.terminator));
        }
        Ok(reachable)
    }

    fn block(&self, block: BasicBlockId) -> Result<&BasicBlock, Diagnostic> {
        self.blocks.get(block.0 as usize).ok_or_else(|| {
            Diagnostic::backend(format!("invalid Rust IR dataflow block {}", block.0))
        })
    }

    fn transfer_block(
        &self,
        block: &BasicBlock,
        state: &mut BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        for statement in &block.statements {
            self.transfer_rvalue(&statement.value, state, check_reads)?;
            state.insert(statement.destination.local);
        }
        match &block.terminator.kind {
            TerminatorKind::SwitchBool { condition, .. } => {
                self.transfer_operand(condition, state, check_reads)?;
            }
            TerminatorKind::Call {
                args, destination, ..
            } => {
                for argument in args {
                    self.transfer_operand(argument, state, check_reads)?;
                }
                if let Some(destination) = destination {
                    state.insert(destination.local);
                }
            }
            TerminatorKind::Return(values) => {
                for value in values {
                    self.transfer_operand(value, state, check_reads)?;
                }
            }
            TerminatorKind::Goto(_) | TerminatorKind::Unreachable => {}
        }
        Ok(())
    }

    fn transfer_rvalue(
        &self,
        rvalue: &Rvalue,
        state: &BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        match &rvalue.kind {
            RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
                self.transfer_operand(operand, state, check_reads)
            }
            RvalueKind::Binary { left, right, .. } => {
                self.transfer_operand(left, state, check_reads)?;
                self.transfer_operand(right, state, check_reads)
            }
        }
    }

    fn transfer_operand(
        &self,
        operand: &Operand,
        state: &BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        let Operand::Read { place, .. } = operand else {
            return Ok(());
        };
        if check_reads && !state.contains(&place.local) {
            return Err(Diagnostic::backend(format!(
                "Rust IR reads local {} before initialization in function {}",
                place.local.0, self.name
            )));
        }
        Ok(())
    }
}

fn block_successors(terminator: &Terminator) -> Vec<BasicBlockId> {
    match &terminator.kind {
        TerminatorKind::Goto(target) => vec![*target],
        TerminatorKind::Call { next, .. } => vec![*next],
        TerminatorKind::SwitchBool {
            then_target,
            else_target,
            ..
        } => vec![*then_target, *else_target],
        TerminatorKind::Return(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}
