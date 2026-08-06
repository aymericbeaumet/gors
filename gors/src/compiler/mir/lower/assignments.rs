//! Explicit two-phase lowering for assignments with dynamic destinations.

use super::super::construct::{call_effects, make_terminator};
use super::super::{Operand, Place, Provenance, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

enum PreparedTarget {
    Local(LocalId),
    Discard,
    SliceIndex {
        slice: Operand,
        index: Operand,
        set: hir::Builtin,
    },
    MapIndex {
        map: Operand,
        key: Operand,
    },
}

impl FunctionLowerer {
    pub(super) fn lower_parallel_assignment(
        &mut self,
        destinations: &[hir::AssignTarget],
        values: &[hir::Expr],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if destinations.len() != values.len() {
            return Err(Diagnostic::backend(
                "parallel assignment arity changed before MIR lowering",
            ));
        }
        let prepared = self.prepare_assignment_targets(destinations)?;
        let mut operands = Vec::with_capacity(values.len());
        for value in values {
            let operand = self.lower_expr(value)?;
            operands.push(self.materialize(
                operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?);
        }
        self.write_prepared_assignments(prepared, operands, source)
    }

    pub(super) fn lower_parallel_tuple_assignment(
        &mut self,
        destinations: &[hir::AssignTarget],
        value: &hir::Expr,
        coercions: &[hir::ValueCoercion],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::backend(
                "parallel tuple assignment value is not a tuple",
            ));
        };
        if destinations.len() != component_types.len() || coercions.len() != component_types.len() {
            return Err(Diagnostic::backend(
                "parallel tuple assignment arity changed before MIR lowering",
            ));
        }
        let prepared = self.prepare_assignment_targets(destinations)?;
        let temporary_results = component_types
            .iter()
            .map(|ty| Place {
                local: self.new_temp(ty.clone()),
            })
            .collect::<Vec<_>>();
        self.lower_call_into(value, temporary_results.clone())?;
        let mut assigned_operands = Vec::with_capacity(temporary_results.len());
        for ((result, source_ty), coercion) in temporary_results
            .into_iter()
            .zip(component_types)
            .zip(coercions)
        {
            assigned_operands.push(self.lower_value_coercion(
                Operand::Read(result),
                source_ty,
                coercion,
                value.source,
            )?);
        }
        self.write_prepared_assignments(prepared, assigned_operands, source)
    }

    fn prepare_assignment_targets(
        &mut self,
        destinations: &[hir::AssignTarget],
    ) -> Result<Vec<PreparedTarget>, Diagnostic> {
        let mut prepared = Vec::with_capacity(destinations.len());
        for destination in destinations {
            prepared.push(match destination {
                hir::AssignTarget::Local(local) => PreparedTarget::Local(*local),
                hir::AssignTarget::Discard => PreparedTarget::Discard,
                hir::AssignTarget::SliceIndex { slice, index, set } => {
                    let slice_operand = self.lower_expr(slice)?;
                    let slice_operand = self.materialize(
                        slice_operand,
                        slice.ty.clone(),
                        Provenance::Source(slice.source),
                    )?;
                    let index_operand = self.lower_expr(index)?;
                    let index_operand = self.materialize(
                        index_operand,
                        index.ty.clone(),
                        Provenance::Source(index.source),
                    )?;
                    PreparedTarget::SliceIndex {
                        slice: slice_operand,
                        index: index_operand,
                        set: *set,
                    }
                }
                hir::AssignTarget::MapIndex { map, key } => {
                    let map_operand = self.lower_expr(map)?;
                    let map_operand = self.materialize(
                        map_operand,
                        map.ty.clone(),
                        Provenance::Source(map.source),
                    )?;
                    let key_operand = self.lower_expr(key)?;
                    let key_operand = self.materialize(
                        key_operand,
                        key.ty.clone(),
                        Provenance::Source(key.source),
                    )?;
                    PreparedTarget::MapIndex {
                        map: map_operand,
                        key: key_operand,
                    }
                }
            });
        }
        Ok(prepared)
    }

    fn write_prepared_assignments(
        &mut self,
        prepared: Vec<PreparedTarget>,
        operands: Vec<Operand>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if prepared.len() != operands.len() {
            return Err(Diagnostic::backend(
                "prepared assignment arity changed before destination writes",
            ));
        }
        for (destination, operand) in prepared.into_iter().zip(operands) {
            match destination {
                PreparedTarget::Local(destination) => self.write_semantic_local(
                    destination,
                    operand,
                    Provenance::Source(source),
                    false,
                )?,
                PreparedTarget::Discard => {}
                PreparedTarget::SliceIndex { slice, index, set } => {
                    let provenance = Provenance::Source(source);
                    let target = self.new_block(provenance.clone());
                    self.terminate(make_terminator(
                        TerminatorKind::Call {
                            callee: hir::Callee::Builtin(set),
                            args: vec![slice, index, operand],
                            destinations: Vec::new(),
                            target,
                        },
                        call_effects(),
                        provenance,
                    ))?;
                    self.current = target;
                }
                PreparedTarget::MapIndex { map, key } => {
                    self.emit_map_call(
                        hir::Builtin::MapStringI64Set,
                        vec![map, key, operand],
                        Vec::new(),
                        source,
                    )?;
                }
            }
        }
        Ok(())
    }
}
