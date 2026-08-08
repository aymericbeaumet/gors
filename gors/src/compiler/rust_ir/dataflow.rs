//! Storage and last-use proofs for Rust-IR slots.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    BasicBlock, BasicBlockId, Function, LocalId, Operand, ReadOp, RustType, Rvalue, RvalueKind,
    SlotInitialization, Terminator, TerminatorKind, rvalue_effects, statement_effects,
    terminator_effects,
};
use crate::compiler::Diagnostic;

/// Select the canonical read representation after the complete CFG is available.
///
/// Owned values move only when backwards liveness proves that the slot is dead
/// after that exact operand on every successor path. All other owned reads clone.
pub(in crate::compiler) fn select_read_operations(
    function: &mut Function,
) -> Result<(), Diagnostic> {
    let reachable = function.reachable_blocks()?;
    if reachable.len() != function.blocks.len() {
        return Err(Diagnostic::backend(
            "Rust IR read planning found a block outside the control-flow graph",
        ));
    }
    let plans = function.expected_read_plans(&reachable)?;
    function.apply_read_plans(&plans)?;
    refresh_effects(function);
    Ok(())
}

impl Function {
    pub(super) fn verify_storage_dataflow(&self) -> Result<(), Diagnostic> {
        let reachable = self.reachable_blocks()?;
        if reachable.len() != self.blocks.len() {
            return Err(Diagnostic::backend(
                "Rust IR contains a block outside the verified control-flow graph",
            ));
        }

        let read_plans = self.expected_read_plans(&reachable)?;
        self.verify_read_plans(&read_plans)?;

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

    fn expected_read_plans(
        &self,
        reachable: &BTreeSet<BasicBlockId>,
    ) -> Result<BTreeMap<BasicBlockId, Vec<ReadOp>>, Diagnostic> {
        let live_inputs = self.live_inputs(reachable)?;
        let local_types = self
            .locals
            .iter()
            .map(|local| local.ty.clone())
            .collect::<Vec<_>>();
        let mut plans = BTreeMap::new();
        for block_id in reachable {
            let block = self.block(*block_id)?;
            let mut live = live_output(block, &live_inputs)?;
            let mut reverse_plan = Vec::new();
            plan_terminator_backwards(
                &block.terminator,
                &mut live,
                &local_types,
                &mut reverse_plan,
            )?;
            for statement in block.statements.iter().rev() {
                live.remove(&statement.destination.local);
                plan_rvalue_backwards(
                    &statement.value,
                    &mut live,
                    &local_types,
                    &mut reverse_plan,
                )?;
            }
            reverse_plan.reverse();
            if self.panic_cleanup.is_some() {
                for operation in &mut reverse_plan {
                    if *operation == ReadOp::ProvenLastUseMove {
                        *operation = ReadOp::ProvenInitializedClone;
                    }
                }
            }
            plans.insert(*block_id, reverse_plan);
        }
        Ok(plans)
    }

    fn live_inputs(
        &self,
        reachable: &BTreeSet<BasicBlockId>,
    ) -> Result<BTreeMap<BasicBlockId, BTreeSet<LocalId>>, Diagnostic> {
        let mut inputs = reachable
            .iter()
            .copied()
            .map(|block| (block, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        loop {
            let mut changed = false;
            for block_id in reachable.iter().rev() {
                let block = self.block(*block_id)?;
                let mut live = live_output(block, &inputs)?;
                add_terminator_uses_backwards(&block.terminator, &mut live);
                for statement in block.statements.iter().rev() {
                    live.remove(&statement.destination.local);
                    add_rvalue_uses_backwards(&statement.value, &mut live);
                }
                let current = inputs.get_mut(block_id).ok_or_else(|| {
                    Diagnostic::backend(format!(
                        "missing Rust IR liveness input for block {}",
                        block_id.0
                    ))
                })?;
                if *current != live {
                    *current = live;
                    changed = true;
                }
            }
            if !changed {
                return Ok(inputs);
            }
        }
    }

    fn apply_read_plans(
        &mut self,
        plans: &BTreeMap<BasicBlockId, Vec<ReadOp>>,
    ) -> Result<(), Diagnostic> {
        for block in &mut self.blocks {
            let plan = plans.get(&block.id).ok_or_else(|| {
                Diagnostic::backend(format!(
                    "missing Rust IR read plan for block {}",
                    block.id.0
                ))
            })?;
            let mut cursor = 0;
            for statement in &mut block.statements {
                apply_rvalue_plan(&mut statement.value, plan, &mut cursor)?;
            }
            apply_terminator_plan(&mut block.terminator, plan, &mut cursor)?;
            if cursor != plan.len() {
                return Err(Diagnostic::backend(format!(
                    "Rust IR read plan for block {} has {} unused operation(s)",
                    block.id.0,
                    plan.len() - cursor
                )));
            }
        }
        Ok(())
    }

    fn verify_read_plans(
        &self,
        plans: &BTreeMap<BasicBlockId, Vec<ReadOp>>,
    ) -> Result<(), Diagnostic> {
        for block in &self.blocks {
            let expected = plans.get(&block.id).ok_or_else(|| {
                Diagnostic::backend(format!(
                    "missing canonical Rust IR read plan for block {}",
                    block.id.0
                ))
            })?;
            let observed = block_read_operations(block);
            if observed.len() != expected.len() {
                return Err(Diagnostic::backend(format!(
                    "Rust IR read plan length mismatch in block {}",
                    block.id.0
                )));
            }
            for ((local, observed), expected) in observed.into_iter().zip(expected) {
                if observed != *expected {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR read operation mismatch for local {}: CFG liveness requires {expected:?}, found {observed:?}",
                        local.0
                    )));
                }
            }
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
            RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
                self.transfer_operand(operand, state, check_reads)
            }
            RvalueKind::Binary { left, right, .. }
            | RvalueKind::AggregateEqualInteger { left, right, .. } => {
                self.transfer_operand(left, state, check_reads)?;
                self.transfer_operand(right, state, check_reads)
            }
            RvalueKind::ArrayIndexI64 { array, index }
            | RvalueKind::ArrayIndex { array, index } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)
            }
            RvalueKind::ArraySetI64 {
                array,
                index,
                value,
            }
            | RvalueKind::ArraySet {
                array,
                index,
                value,
            } => {
                self.transfer_operand(array, state, check_reads)?;
                self.transfer_operand(index, state, check_reads)?;
                self.transfer_operand(value, state, check_reads)
            }
            RvalueKind::ArrayLiteral {
                elements: fields, ..
            }
            | RvalueKind::StructLiteral { fields, .. }
            | RvalueKind::StructLiteralI64(fields) => {
                for field in fields {
                    self.transfer_operand(field, state, check_reads)?;
                }
                Ok(())
            }
            RvalueKind::StructField { structure, .. }
            | RvalueKind::StructFieldI64 { structure, .. } => {
                self.transfer_operand(structure, state, check_reads)
            }
            RvalueKind::StructSet {
                structure, value, ..
            }
            | RvalueKind::StructSetI64 {
                structure, value, ..
            } => {
                self.transfer_operand(structure, state, check_reads)?;
                self.transfer_operand(value, state, check_reads)
            }
            RvalueKind::Recover {
                state: recovery_state,
                value,
                ..
            } => {
                if check_reads && !state.contains(&recovery_state.local) {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR reads panic recovery local {} before initialization",
                        recovery_state.local.0
                    )));
                }
                self.transfer_operand(value, state, check_reads)
            }
        }
    }

    fn transfer_operand(
        &self,
        operand: &Operand,
        state: &mut BTreeSet<LocalId>,
        check_reads: bool,
    ) -> Result<(), Diagnostic> {
        let Operand::Read { place, op } = operand else {
            return Ok(());
        };
        if check_reads && !state.contains(&place.local) {
            return Err(Diagnostic::backend(format!(
                "Rust IR reads local {} before initialization in function {}",
                place.local.0, self.name
            )));
        }
        if *op == ReadOp::ProvenLastUseMove {
            state.remove(&place.local);
        }
        Ok(())
    }
}

fn live_output(
    block: &BasicBlock,
    live_inputs: &BTreeMap<BasicBlockId, BTreeSet<LocalId>>,
) -> Result<BTreeSet<LocalId>, Diagnostic> {
    let mut live = BTreeSet::new();
    for successor in block_successors(&block.terminator) {
        let successor_live = live_inputs.get(&successor).ok_or_else(|| {
            Diagnostic::backend(format!(
                "missing Rust IR liveness for successor {}",
                successor.0
            ))
        })?;
        live.extend(successor_live);
    }
    Ok(live)
}

fn add_terminator_uses_backwards(terminator: &Terminator, live: &mut BTreeSet<LocalId>) {
    match &terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => add_operand_use(condition, live),
        TerminatorKind::Call {
            args, destinations, ..
        } => {
            for destination in destinations {
                live.remove(&destination.local);
            }
            for argument in args.iter().rev() {
                add_operand_use(argument, live);
            }
        }
        TerminatorKind::Return(values) => {
            for value in values.iter().rev() {
                add_operand_use(value, live);
            }
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => {}
    }
}

fn add_rvalue_uses_backwards(rvalue: &Rvalue, live: &mut BTreeSet<LocalId>) {
    match &rvalue.kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
            add_operand_use(operand, live);
        }
        RvalueKind::Binary { left, right, .. }
        | RvalueKind::AggregateEqualInteger { left, right, .. } => {
            add_operand_use(right, live);
            add_operand_use(left, live);
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            add_operand_use(index, live);
            add_operand_use(array, live);
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => {
            add_operand_use(value, live);
            add_operand_use(index, live);
            add_operand_use(array, live);
        }
        RvalueKind::ArrayLiteral {
            elements: fields, ..
        }
        | RvalueKind::StructLiteral { fields, .. }
        | RvalueKind::StructLiteralI64(fields) => {
            for field in fields.iter().rev() {
                add_operand_use(field, live);
            }
        }
        RvalueKind::StructField { structure, .. }
        | RvalueKind::StructFieldI64 { structure, .. } => add_operand_use(structure, live),
        RvalueKind::StructSet {
            structure, value, ..
        }
        | RvalueKind::StructSetI64 {
            structure, value, ..
        } => {
            add_operand_use(value, live);
            add_operand_use(structure, live);
        }
        RvalueKind::Recover { state, value, .. } => {
            add_operand_use(value, live);
            live.insert(state.local);
        }
    }
}

fn add_operand_use(operand: &Operand, live: &mut BTreeSet<LocalId>) {
    if let Operand::Read { place, .. } = operand {
        live.insert(place.local);
    }
}

fn plan_terminator_backwards(
    terminator: &Terminator,
    live: &mut BTreeSet<LocalId>,
    local_types: &[RustType],
    reverse_plan: &mut Vec<ReadOp>,
) -> Result<(), Diagnostic> {
    match &terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => {
            plan_operand_backwards(condition, live, local_types, reverse_plan)
        }
        TerminatorKind::Call {
            args, destinations, ..
        } => {
            for destination in destinations {
                live.remove(&destination.local);
            }
            for argument in args.iter().rev() {
                plan_operand_backwards(argument, live, local_types, reverse_plan)?;
            }
            Ok(())
        }
        TerminatorKind::Return(values) => {
            for value in values.iter().rev() {
                plan_operand_backwards(value, live, local_types, reverse_plan)?;
            }
            Ok(())
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Ok(()),
    }
}

fn plan_rvalue_backwards(
    rvalue: &Rvalue,
    live: &mut BTreeSet<LocalId>,
    local_types: &[RustType],
    reverse_plan: &mut Vec<ReadOp>,
) -> Result<(), Diagnostic> {
    match &rvalue.kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
            plan_operand_backwards(operand, live, local_types, reverse_plan)
        }
        RvalueKind::Binary { left, right, .. }
        | RvalueKind::AggregateEqualInteger { left, right, .. } => {
            plan_operand_backwards(right, live, local_types, reverse_plan)?;
            plan_operand_backwards(left, live, local_types, reverse_plan)
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            plan_operand_backwards(index, live, local_types, reverse_plan)?;
            plan_operand_backwards(array, live, local_types, reverse_plan)
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => {
            plan_operand_backwards(value, live, local_types, reverse_plan)?;
            plan_operand_backwards(index, live, local_types, reverse_plan)?;
            plan_operand_backwards(array, live, local_types, reverse_plan)
        }
        RvalueKind::ArrayLiteral {
            elements: fields, ..
        }
        | RvalueKind::StructLiteral { fields, .. }
        | RvalueKind::StructLiteralI64(fields) => {
            for field in fields.iter().rev() {
                plan_operand_backwards(field, live, local_types, reverse_plan)?;
            }
            Ok(())
        }
        RvalueKind::StructField { structure, .. }
        | RvalueKind::StructFieldI64 { structure, .. } => {
            plan_operand_backwards(structure, live, local_types, reverse_plan)
        }
        RvalueKind::StructSet {
            structure, value, ..
        }
        | RvalueKind::StructSetI64 {
            structure, value, ..
        } => {
            plan_operand_backwards(value, live, local_types, reverse_plan)?;
            plan_operand_backwards(structure, live, local_types, reverse_plan)
        }
        RvalueKind::Recover { state, value, .. } => {
            plan_operand_backwards(value, live, local_types, reverse_plan)?;
            live.insert(state.local);
            Ok(())
        }
    }
}

fn plan_operand_backwards(
    operand: &Operand,
    live: &mut BTreeSet<LocalId>,
    local_types: &[RustType],
    reverse_plan: &mut Vec<ReadOp>,
) -> Result<(), Diagnostic> {
    let Operand::Read { place, .. } = operand else {
        return Ok(());
    };
    let ty = local_types.get(place.local.0 as usize).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid local during Rust IR read planning: {}",
            place.local.0
        ))
    })?;
    let op = ty
        .read_op_for_liveness(live.contains(&place.local))
        .ok_or_else(|| Diagnostic::backend("Rust IR cannot plan a read from a unit slot"))?;
    reverse_plan.push(op);
    live.insert(place.local);
    Ok(())
}

fn apply_rvalue_plan(
    rvalue: &mut Rvalue,
    plan: &[ReadOp],
    cursor: &mut usize,
) -> Result<(), Diagnostic> {
    match &mut rvalue.kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
            apply_operand_plan(operand, plan, cursor)
        }
        RvalueKind::Binary { left, right, .. }
        | RvalueKind::AggregateEqualInteger { left, right, .. } => {
            apply_operand_plan(left, plan, cursor)?;
            apply_operand_plan(right, plan, cursor)
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            apply_operand_plan(array, plan, cursor)?;
            apply_operand_plan(index, plan, cursor)
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => {
            apply_operand_plan(array, plan, cursor)?;
            apply_operand_plan(index, plan, cursor)?;
            apply_operand_plan(value, plan, cursor)
        }
        RvalueKind::ArrayLiteral {
            elements: fields, ..
        }
        | RvalueKind::StructLiteral { fields, .. }
        | RvalueKind::StructLiteralI64(fields) => {
            for field in fields {
                apply_operand_plan(field, plan, cursor)?;
            }
            Ok(())
        }
        RvalueKind::StructField { structure, .. }
        | RvalueKind::StructFieldI64 { structure, .. } => {
            apply_operand_plan(structure, plan, cursor)
        }
        RvalueKind::StructSet {
            structure, value, ..
        }
        | RvalueKind::StructSetI64 {
            structure, value, ..
        } => {
            apply_operand_plan(structure, plan, cursor)?;
            apply_operand_plan(value, plan, cursor)
        }
        RvalueKind::Recover { value, .. } => apply_operand_plan(value, plan, cursor),
    }
}

fn apply_terminator_plan(
    terminator: &mut Terminator,
    plan: &[ReadOp],
    cursor: &mut usize,
) -> Result<(), Diagnostic> {
    match &mut terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => apply_operand_plan(condition, plan, cursor),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            for argument in args {
                apply_operand_plan(argument, plan, cursor)?;
            }
            Ok(())
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Ok(()),
    }
}

fn apply_operand_plan(
    operand: &mut Operand,
    plan: &[ReadOp],
    cursor: &mut usize,
) -> Result<(), Diagnostic> {
    let Operand::Read { op, .. } = operand else {
        return Ok(());
    };
    *op = *plan.get(*cursor).ok_or_else(|| {
        Diagnostic::backend("Rust IR read plan ended before all operands were assigned")
    })?;
    *cursor += 1;
    Ok(())
}

fn block_read_operations(block: &BasicBlock) -> Vec<(LocalId, ReadOp)> {
    let mut reads = Vec::new();
    for statement in &block.statements {
        collect_rvalue_reads(&statement.value, &mut reads);
    }
    collect_terminator_reads(&block.terminator, &mut reads);
    reads
}

fn collect_rvalue_reads(rvalue: &Rvalue, reads: &mut Vec<(LocalId, ReadOp)>) {
    match &rvalue.kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => {
            collect_operand_read(operand, reads);
        }
        RvalueKind::Binary { left, right, .. }
        | RvalueKind::AggregateEqualInteger { left, right, .. } => {
            collect_operand_read(left, reads);
            collect_operand_read(right, reads);
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            collect_operand_read(array, reads);
            collect_operand_read(index, reads);
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => {
            collect_operand_read(array, reads);
            collect_operand_read(index, reads);
            collect_operand_read(value, reads);
        }
        RvalueKind::ArrayLiteral {
            elements: fields, ..
        }
        | RvalueKind::StructLiteral { fields, .. }
        | RvalueKind::StructLiteralI64(fields) => {
            for field in fields {
                collect_operand_read(field, reads);
            }
        }
        RvalueKind::StructField { structure, .. }
        | RvalueKind::StructFieldI64 { structure, .. } => {
            collect_operand_read(structure, reads);
        }
        RvalueKind::StructSet {
            structure, value, ..
        }
        | RvalueKind::StructSetI64 {
            structure, value, ..
        } => {
            collect_operand_read(structure, reads);
            collect_operand_read(value, reads);
        }
        RvalueKind::Recover { value, .. } => collect_operand_read(value, reads),
    }
}

fn collect_terminator_reads(terminator: &Terminator, reads: &mut Vec<(LocalId, ReadOp)>) {
    match &terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => collect_operand_read(condition, reads),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            for argument in args {
                collect_operand_read(argument, reads);
            }
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => {}
    }
}

fn collect_operand_read(operand: &Operand, reads: &mut Vec<(LocalId, ReadOp)>) {
    if let Operand::Read { place, op } = operand {
        reads.push((place.local, *op));
    }
}

fn refresh_effects(function: &mut Function) {
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            let effects = rvalue_effects(&statement.value.kind);
            statement.value.effects = effects;
            statement.value.panic = refreshed_panic_edge(statement.value.panic, effects);
            statement.effects = statement_effects(&statement.value);
        }
        let effects = terminator_effects(&block.terminator.kind);
        block.terminator.effects = effects;
        block.terminator.panic = refreshed_panic_edge(block.terminator.panic, effects);
    }
}

fn refreshed_panic_edge(edge: super::PanicEdge, effects: super::Effects) -> super::PanicEdge {
    if !effects.may_panic {
        return super::PanicEdge::None;
    }
    match edge {
        super::PanicEdge::Cleanup(target) => super::PanicEdge::Cleanup(target),
        super::PanicEdge::None | super::PanicEdge::Propagate => super::PanicEdge::Propagate,
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
