//! Panic cleanup construction for statically registered deferred closures.

use std::collections::BTreeSet;

use super::super::construct::{make_rvalue, make_statement, make_terminator, operand_ty};
use super::super::{Operand, PanicCleanup, PanicEdge, Place, Provenance, RvalueKind};
use super::{FunctionLowerer, SyntheticOrigin, TerminatorKind};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, LocalId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Ty};

impl FunctionLowerer {
    pub(super) fn register_defer(
        &mut self,
        parameters: &[LocalId],
        values: &[hir::Expr],
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if parameters.len() != values.len() {
            return Err(Diagnostic::backend(
                "deferred HIR call argument arity changed before MIR lowering",
            ));
        }
        let mut operands = Vec::with_capacity(values.len());
        for value in values {
            let operand = self.lower_expr(value)?;
            operands.push(self.materialize(
                operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?);
        }
        for (parameter, operand) in parameters.iter().zip(operands) {
            let parameter_ty = self.local_ty(*parameter)?.clone();
            if parameter_ty != operand_ty(&operand, &self.locals)? {
                return Err(Diagnostic::backend(
                    "deferred HIR call argument type changed before MIR lowering",
                ));
            }
            let provenance = Provenance::Source(source);
            let value = make_rvalue(
                RvalueKind::Use(operand),
                hir::Effects::default(),
                provenance.clone(),
            );
            self.push_statement(make_statement(
                Place { local: *parameter },
                value,
                provenance,
            ))?;
        }
        let registered = self
            .defer_flags
            .get(self.next_defer)
            .copied()
            .ok_or_else(|| {
                Diagnostic::backend("deferred HIR statement has no registration flag")
            })?;
        self.next_defer += 1;
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Use(Operand::Constant(ConstValue::Bool(true), Ty::Bool)),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(
            Place { local: registered },
            value,
            provenance,
        ))?;
        self.deferred.push(body.clone());
        Ok(())
    }

    pub(super) fn initialize_panic_cleanup_locals(
        &mut self,
        function: &hir::Function,
    ) -> Result<(), Diagnostic> {
        let parameters = function.params.iter().copied().collect::<BTreeSet<_>>();
        let locals = self
            .locals
            .iter()
            .filter(|local| !parameters.contains(&local.id))
            .map(|local| (local.id, local.ty.clone(), local.kind))
            .collect::<Vec<_>>();
        for (local, ty, kind) in locals {
            let provenance = Provenance::Synthetic(if kind == hir::LocalKind::NamedResult {
                SyntheticOrigin::NamedResultInitialization
            } else {
                SyntheticOrigin::PanicCleanupInitialization
            });
            self.lower_zero_value(Place { local }, ty, provenance)?;
        }
        Ok(())
    }

    pub(super) fn build_panic_cleanup(
        &mut self,
        function: &hir::Function,
        active: LocalId,
    ) -> Result<PanicCleanup, Diagnostic> {
        let provenance = Provenance::Synthetic(SyntheticOrigin::PanicCleanupDispatch);
        let entry = self.new_block(provenance.clone());
        self.current = entry;
        let deferred = self.all_deferred.clone();
        for action in deferred.iter().rev() {
            let body = self.new_block(provenance.clone());
            let next = self.new_block(provenance.clone());
            self.terminate(make_terminator(
                TerminatorKind::SwitchBool {
                    condition: Operand::Read(Place {
                        local: action.registered,
                    }),
                    then_target: body,
                    else_target: next,
                },
                hir::Effects::default(),
                provenance.clone(),
            ))?;
            self.current = body;
            self.lower_block(&action.body)?;
            if !self.is_terminated(self.current)? {
                self.terminate(make_terminator(
                    TerminatorKind::Goto(next),
                    hir::Effects::default(),
                    provenance.clone(),
                ))?;
            }
            self.current = next;
        }

        let mut returned = Vec::with_capacity(function.signature.results.len());
        for (index, ty) in function.signature.results.iter().enumerate() {
            if let Some(local) = function.named_results.get(index).copied().flatten() {
                returned.push(Operand::Read(Place { local }));
            } else {
                let zero = Place {
                    local: self.new_temp(ty.clone()),
                };
                self.lower_zero_value(zero, ty.clone(), provenance.clone())?;
                returned.push(Operand::Read(zero));
            }
        }
        self.terminate(make_terminator(
            TerminatorKind::Return(returned),
            hir::Effects::default(),
            provenance,
        ))?;
        Ok(PanicCleanup { entry, active })
    }

    pub(super) fn retarget_body_panics(&mut self, cleanup: BasicBlockId) {
        for block in self.blocks.iter_mut().take(cleanup.0 as usize) {
            for statement in &mut block.statements {
                if statement.value.panic == PanicEdge::Propagate {
                    statement.value.panic = PanicEdge::Cleanup(cleanup);
                }
            }
            if let Some(terminator) = &mut block.terminator
                && terminator.panic == PanicEdge::Propagate
            {
                terminator.panic = PanicEdge::Cleanup(cleanup);
            }
        }
    }
}
