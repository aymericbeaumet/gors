//! Mandatory semantic MIR normalization before Rust representation lowering.
//!
//! Every pass runs between whole-file verification barriers.  The bootstrap
//! Normalization deliberately avoids integer folding until typed Go-width
//! arithmetic is represented explicitly.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::compiler::Diagnostic;
#[cfg(test)]
use crate::compiler::VerifiedMir;
use crate::compiler::hir;
#[cfg(test)]
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::ids::{BasicBlockId, LocalId, PackageId};
use crate::compiler::mir::{
    self, Operand, PanicEdge, Provenance, Rvalue, RvalueKind, SyntheticOrigin, Terminator,
    TerminatorKind,
};
use crate::compiler::types::{ConstValue, Ty};

#[cfg(test)]
pub(super) fn normalize(input: VerifiedMir) -> Result<VerifiedMir, Vec<Diagnostic>> {
    let boolean_control_flow = BooleanControlFlow;
    let unreachable_blocks = UnreachableBlocks;
    let passes: [&dyn MirPass; 2] = [&boolean_control_flow, &unreachable_blocks];
    PassManager::new(&passes).run(input)
}

pub(super) fn normalize_function(
    function: mir::Function,
    package: PackageId,
    signatures: &mir::SignatureIndex,
) -> Result<mir::Function, Vec<Diagnostic>> {
    let boolean_control_flow = BooleanControlFlow;
    let unreachable_blocks = UnreachableBlocks;
    let passes: [&dyn MirPass; 2] = [&boolean_control_flow, &unreachable_blocks];
    let mut file = mir::File {
        package_id: package,
        package: String::new(),
        functions: vec![function],
    };
    PassManager::new(&passes).run_file(&mut file, signatures)?;
    file.functions
        .pop()
        .ok_or_else(|| vec![Diagnostic::backend("MIR normalization lost its function")])
}

trait MirPass {
    fn name(&self) -> &'static str;

    fn run(&self, file: &mut mir::File) -> Result<(), Diagnostic>;
}

struct PassManager<'a> {
    passes: &'a [&'a dyn MirPass],
}

impl<'a> PassManager<'a> {
    fn new(passes: &'a [&'a dyn MirPass]) -> Self {
        Self { passes }
    }

    #[cfg(test)]
    fn run(&self, input: VerifiedMir) -> Result<VerifiedMir, Vec<Diagnostic>> {
        let mut file = input.into_inner();
        let signatures = file
            .functions
            .iter()
            .map(|function| {
                (
                    QualifiedDefId::new(file.package_id, function.id),
                    function.signature.clone(),
                )
            })
            .collect();
        self.run_file(&mut file, &signatures)?;
        Ok(VerifiedMir::from_verified(file))
    }

    fn run_file(
        &self,
        file: &mut mir::File,
        signatures: &mir::SignatureIndex,
    ) -> Result<(), Vec<Diagnostic>> {
        for pass in self.passes {
            verify_around_pass(file, signatures, pass.name(), "before")?;
            pass.run(file)
                .map_err(|diagnostic| vec![contextualize(diagnostic, pass.name(), "during")])?;
            verify_around_pass(file, signatures, pass.name(), "after")?;
        }
        Ok(())
    }
}

fn verify_around_pass(
    file: &mir::File,
    signatures: &mir::SignatureIndex,
    pass: &str,
    phase: &str,
) -> Result<(), Vec<Diagnostic>> {
    file.verify_with_signatures(signatures)
        .map_err(|diagnostic| vec![contextualize(diagnostic, pass, phase)])
}

fn contextualize(diagnostic: Diagnostic, pass: &str, phase: &str) -> Diagnostic {
    Diagnostic::backend(format!(
        "MIR verification {phase} pass `{pass}` failed: {}",
        diagnostic.message
    ))
}

/// Fold only exact boolean operations and boolean-controlled branches.
///
/// Integer operations stay untouched.  Folding them with unbounded constants
/// would disagree with the fixed-width Go `int` runtime ABI at overflow,
/// MIN/-1, and large shifts.
struct BooleanControlFlow;

impl MirPass for BooleanControlFlow {
    fn name(&self) -> &'static str {
        "boolean-control-flow"
    }

    fn run(&self, file: &mut mir::File) -> Result<(), Diagnostic> {
        for function in &mut file.functions {
            propagate_block_booleans(function);
        }
        Ok(())
    }
}

fn propagate_block_booleans(function: &mut mir::Function) {
    for block in &mut function.blocks {
        let mut constants = BTreeMap::<LocalId, bool>::new();
        for statement in &mut block.statements {
            rewrite_rvalue_boolean_reads(&mut statement.value, &constants);
            if let Some(value) = fold_boolean_rvalue(&statement.value) {
                statement.value.kind =
                    RvalueKind::Use(Operand::Constant(ConstValue::Bool(value), Ty::Bool));
                statement.value.effects = hir::Effects::default();
                statement.value.panic = PanicEdge::None;
                constants.insert(statement.destination.local, value);
            } else {
                refresh_rvalue_effects(&mut statement.value);
                constants.remove(&statement.destination.local);
            }
            statement.effects = statement.value.effects.union(hir::Effects {
                may_write: true,
                ..hir::Effects::default()
            });
        }
        rewrite_terminator_boolean_reads(&mut block.terminator, &constants);
        if !matches!(
            block.terminator.provenance,
            Provenance::Synthetic(SyntheticOrigin::PanicCleanupDispatch)
        ) && let TerminatorKind::SwitchBool {
            condition: Operand::Constant(ConstValue::Bool(value), Ty::Bool),
            then_target,
            else_target,
        } = &block.terminator.kind
        {
            block.terminator.kind =
                TerminatorKind::Goto(if *value { *then_target } else { *else_target });
            block.terminator.effects = hir::Effects::default();
            block.terminator.panic = PanicEdge::None;
        }
    }
}

fn rewrite_rvalue_boolean_reads(rvalue: &mut Rvalue, constants: &BTreeMap<LocalId, bool>) {
    match &mut rvalue.kind {
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => {
            rewrite_boolean_read(operand, constants);
        }
        RvalueKind::Binary { left, right, .. } => {
            rewrite_boolean_read(left, constants);
            rewrite_boolean_read(right, constants);
        }
        RvalueKind::ArrayIndexI64 { array, index } => {
            rewrite_boolean_read(array, constants);
            rewrite_boolean_read(index, constants);
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => {
            rewrite_boolean_read(array, constants);
            rewrite_boolean_read(index, constants);
            rewrite_boolean_read(value, constants);
        }
        RvalueKind::RecoverCompareNil { .. } => {}
        RvalueKind::SliceLiteralI64(_)
        | RvalueKind::SliceLiteralU8(_)
        | RvalueKind::ArrayLiteralI64(_) => {}
    }
}

fn rewrite_terminator_boolean_reads(
    terminator: &mut Terminator,
    constants: &BTreeMap<LocalId, bool>,
) {
    match &mut terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => rewrite_boolean_read(condition, constants),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            for argument in args {
                rewrite_boolean_read(argument, constants);
            }
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => {}
    }
    refresh_terminator_read_effect(terminator);
}

fn rewrite_boolean_read(operand: &mut Operand, constants: &BTreeMap<LocalId, bool>) {
    let Operand::Read(place) = operand else {
        return;
    };
    if let Some(value) = constants.get(&place.local) {
        *operand = Operand::Constant(ConstValue::Bool(*value), Ty::Bool);
    }
}

fn fold_boolean_rvalue(rvalue: &Rvalue) -> Option<bool> {
    match &rvalue.kind {
        RvalueKind::Use(Operand::Constant(ConstValue::Bool(value), Ty::Bool)) => Some(*value),
        RvalueKind::Unary {
            op: hir::UnaryOp::Not,
            operand: Operand::Constant(ConstValue::Bool(value), Ty::Bool),
            ty: Ty::Bool,
        } => Some(!value),
        RvalueKind::Binary {
            op,
            left: Operand::Constant(ConstValue::Bool(left), Ty::Bool),
            right: Operand::Constant(ConstValue::Bool(right), Ty::Bool),
            ty: Ty::Bool,
        } => match op {
            hir::BinaryOp::Equal => Some(left == right),
            hir::BinaryOp::NotEqual => Some(left != right),
            hir::BinaryOp::LogicalAnd => Some(*left && *right),
            hir::BinaryOp::LogicalOr => Some(*left || *right),
            _ => None,
        },
        _ => None,
    }
}

fn refresh_rvalue_effects(rvalue: &mut Rvalue) {
    let may_read = match &rvalue.kind {
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => operand_reads(operand),
        RvalueKind::Binary { left, right, .. } => operand_reads(left) || operand_reads(right),
        RvalueKind::ArrayIndexI64 { array, index } => operand_reads(array) || operand_reads(index),
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => operand_reads(array) || operand_reads(index) || operand_reads(value),
        RvalueKind::RecoverCompareNil { .. } => true,
        RvalueKind::SliceLiteralI64(_)
        | RvalueKind::SliceLiteralU8(_)
        | RvalueKind::ArrayLiteralI64(_) => false,
    };
    let may_panic = matches!(
        &rvalue.kind,
        RvalueKind::Binary {
            op: hir::BinaryOp::Div | hir::BinaryOp::Rem | hir::BinaryOp::Shl | hir::BinaryOp::Shr,
            ..
        } | RvalueKind::ArrayIndexI64 { .. }
            | RvalueKind::ArraySetI64 { .. }
    );
    let may_allocate = matches!(
        &rvalue.kind,
        RvalueKind::Binary {
            op: hir::BinaryOp::Add,
            ty: Ty::String,
            ..
        } | RvalueKind::SliceLiteralI64(_)
            | RvalueKind::SliceLiteralU8(_)
    );
    let recover = matches!(rvalue.kind, RvalueKind::RecoverCompareNil { .. });
    rvalue.effects = hir::Effects {
        may_read,
        may_write: recover,
        may_allocate,
        may_panic,
        ..hir::Effects::default()
    };
    rvalue.panic = if may_panic {
        match rvalue.panic {
            PanicEdge::Cleanup(target) => PanicEdge::Cleanup(target),
            PanicEdge::None | PanicEdge::Propagate => PanicEdge::Propagate,
        }
    } else {
        PanicEdge::None
    };
}

fn refresh_terminator_read_effect(terminator: &mut Terminator) {
    terminator.effects.may_read = match &terminator.kind {
        TerminatorKind::SwitchBool { condition, .. } => operand_reads(condition),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            args.iter().any(operand_reads)
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => false,
    };
}

fn operand_reads(operand: &Operand) -> bool {
    matches!(operand, Operand::Read(_))
}

struct UnreachableBlocks;

impl MirPass for UnreachableBlocks {
    fn name(&self) -> &'static str {
        "unreachable-blocks"
    }

    fn run(&self, file: &mut mir::File) -> Result<(), Diagnostic> {
        for function in &mut file.functions {
            remove_unreachable_blocks(function)?;
        }
        Ok(())
    }
}

fn remove_unreachable_blocks(function: &mut mir::Function) -> Result<(), Diagnostic> {
    let mut reachable = BTreeSet::new();
    let mut pending = VecDeque::from([function.entry]);
    while let Some(block) = pending.pop_front() {
        if !reachable.insert(block) {
            continue;
        }
        let Some(block_data) = function.blocks.get(block.0 as usize) else {
            return Err(Diagnostic::backend(format!(
                "unreachable-block pass saw invalid block {}",
                block.0
            )));
        };
        match &block_data.terminator.kind {
            TerminatorKind::Goto(target) => pending.push_back(*target),
            TerminatorKind::SwitchBool {
                then_target,
                else_target,
                ..
            } => {
                pending.push_back(*then_target);
                pending.push_back(*else_target);
            }
            TerminatorKind::Call { target, .. } => pending.push_back(*target),
            TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
        }
    }

    let mut remap = BTreeMap::new();
    for old in reachable.iter().copied() {
        remap.insert(old, BasicBlockId(remap.len() as u32));
    }
    let new_entry = remapped_block(function.entry, &remap)?;
    let mut new_blocks = Vec::with_capacity(reachable.len());
    for mut block in function
        .blocks
        .iter()
        .filter(|block| reachable.contains(&block.id))
        .cloned()
    {
        block.id = remapped_block(block.id, &remap)?;
        for statement in &mut block.statements {
            if let PanicEdge::Cleanup(target) = &mut statement.value.panic {
                *target = remapped_block(*target, &remap)?;
            }
        }
        remap_terminator(&mut block.terminator, &remap)?;
        new_blocks.push(block);
    }
    function.blocks = new_blocks;
    function.entry = new_entry;
    if let Some(cleanup) = &mut function.panic_cleanup {
        cleanup.entry = remapped_block(cleanup.entry, &remap)?;
    }
    Ok(())
}

fn remapped_block(
    block: BasicBlockId,
    remap: &BTreeMap<BasicBlockId, BasicBlockId>,
) -> Result<BasicBlockId, Diagnostic> {
    remap
        .get(&block)
        .copied()
        .ok_or_else(|| Diagnostic::backend(format!("missing MIR block remap for {}", block.0)))
}

fn remap_terminator(
    terminator: &mut Terminator,
    remap: &BTreeMap<BasicBlockId, BasicBlockId>,
) -> Result<(), Diagnostic> {
    match &mut terminator.kind {
        TerminatorKind::Goto(target) => *target = remapped_block(*target, remap)?,
        TerminatorKind::SwitchBool {
            then_target,
            else_target,
            ..
        } => {
            *then_target = remapped_block(*then_target, remap)?;
            *else_target = remapped_block(*else_target, remap)?;
        }
        TerminatorKind::Call { target, .. } => *target = remapped_block(*target, remap)?,
        TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
    }
    if let PanicEdge::Cleanup(target) = &mut terminator.panic {
        *target = remapped_block(*target, remap)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::unwrap_used)]
mod tests;
