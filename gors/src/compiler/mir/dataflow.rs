//! Definite-initialization analysis over MIR control flow.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{BasicBlock, Function, Operand, Rvalue, RvalueKind, Terminator, TerminatorKind};
use crate::compiler::Diagnostic;
use crate::compiler::ids::{BasicBlockId, LocalId};

impl Function {
    pub(super) fn verify_definite_initialization(&self) -> Result<(), Diagnostic> {
        let reachable = self.reachable_blocks()?;
        let universe = self
            .locals
            .iter()
            .map(|local| local.id)
            .collect::<BTreeSet<_>>();
        let entry_state = self.params.iter().copied().collect::<BTreeSet<_>>();
        let mut predecessors = BTreeMap::<BasicBlockId, Vec<BasicBlockId>>::new();
        for block_id in &reachable {
            let block = self.block(*block_id)?;
            for successor in block_successors(&block.terminator) {
                if reachable.contains(&successor) {
                    predecessors.entry(successor).or_default().push(*block_id);
                }
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
                                "missing initialization state for block {}",
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
                        "reachable MIR block {} has no predecessor",
                        block_id.0
                    )));
                };
                for candidate in candidates {
                    incoming = incoming.intersection(&candidate).copied().collect();
                }
                let current = inputs.get_mut(block_id).ok_or_else(|| {
                    Diagnostic::backend(format!(
                        "missing initialization input for block {}",
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
                    "missing final initialization state for block {}",
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
        self.blocks
            .get(block.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR block {}", block.0)))
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
                args, destinations, ..
            } => {
                for argument in args {
                    self.transfer_operand(argument, state, check_reads)?;
                }
                for destination in destinations {
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
        state: &mut BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        match &rvalue.kind {
            RvalueKind::Use(operand)
            | RvalueKind::Unary { operand, .. }
            | RvalueKind::Conversion { operand, .. } => {
                self.transfer_operand(operand, state, check_reads)
            }
            RvalueKind::Binary { left, right, .. } => {
                self.transfer_operand(left, state, check_reads)?;
                self.transfer_operand(right, state, check_reads)
            }
            RvalueKind::ArrayIndexI64 { array, index } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)
            }
            RvalueKind::ArrayIndex { array, index } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)
            }
            RvalueKind::ArraySetI64 {
                array,
                index,
                value,
            } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)?;
                self.transfer_operand(value, state, check_reads)
            }
            RvalueKind::ArraySet {
                array,
                index,
                value,
            } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)?;
                self.transfer_operand(value, state, check_reads)
            }
            RvalueKind::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.transfer_operand(element, state, check_reads)?;
                }
                Ok(())
            }
            RvalueKind::StructLiteral { fields, .. } => {
                for field in fields {
                    self.transfer_operand(field, state, check_reads)?;
                }
                Ok(())
            }
            RvalueKind::StructField { structure, .. } => {
                self.transfer_operand(structure, state, check_reads)
            }
            RvalueKind::StructSet {
                structure, value, ..
            } => {
                self.transfer_operand(structure, state, check_reads)?;
                self.transfer_operand(value, state, check_reads)
            }
            RvalueKind::RecoverCompareNil {
                state: recovery_state,
                ..
            } => self.transfer_operand(&Operand::Read(*recovery_state), state, check_reads),
            RvalueKind::SliceLiteralI64(_)
            | RvalueKind::SliceLiteralU8(_)
            | RvalueKind::ArrayLiteralI64(_) => Ok(()),
        }
    }

    fn transfer_operand(
        &self,
        operand: &Operand,
        state: &mut BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        let place = match operand {
            Operand::Read(place) => place,
            Operand::Constant(_, _) | Operand::Unit => return Ok(()),
        };
        if check_reads && !state.contains(&place.local) {
            return Err(Diagnostic::backend(format!(
                "MIR reads local {} before initialization in function {}",
                place.local.0, self.name
            )));
        }
        Ok(())
    }
}

fn block_successors(terminator: &Terminator) -> Vec<BasicBlockId> {
    match &terminator.kind {
        TerminatorKind::Goto(target) | TerminatorKind::Call { target, .. } => vec![*target],
        TerminatorKind::SwitchBool {
            then_target,
            else_target,
            ..
        } => vec![*then_target, *else_target],
        TerminatorKind::Return(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}
