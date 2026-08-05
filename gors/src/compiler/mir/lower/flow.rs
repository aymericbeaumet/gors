//! Function-exit and label control-flow helpers.

use super::super::construct::{make_rvalue, make_statement, make_terminator};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::BasicBlockId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_return(
        &mut self,
        values: &[hir::Expr],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let operands = self.lower_return_values(values)?;
        let returned = if self.named_results.is_empty() {
            operands
        } else {
            if self.named_results.len() != operands.len() {
                return Err(Diagnostic::backend(
                    "named-result arity changed before MIR lowering",
                ));
            }
            let mut returned = Vec::with_capacity(operands.len());
            let named_results = self.named_results.clone();
            for (named_result, operand) in named_results.into_iter().zip(operands) {
                if let Some(local) = named_result {
                    let provenance = Provenance::Source(source);
                    let value = make_rvalue(
                        RvalueKind::Use(operand),
                        hir::Effects::default(),
                        provenance.clone(),
                    );
                    self.push_statement(make_statement(Place { local }, value, provenance))?;
                    returned.push(Operand::Read(Place { local }));
                } else {
                    returned.push(operand);
                }
            }
            returned
        };
        self.lower_deferred()?;
        if !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Return(returned),
                hir::Effects::default(),
                Provenance::Source(source),
            ))?;
        }
        Ok(())
    }

    pub(super) fn lower_return_values(
        &mut self,
        values: &[hir::Expr],
    ) -> Result<Vec<Operand>, Diagnostic> {
        if let [value] = values
            && let Ty::Tuple(component_types) = &value.ty
        {
            let destinations = component_types
                .iter()
                .map(|ty| Place {
                    local: self.new_temp(ty.clone()),
                })
                .collect::<Vec<_>>();
            self.lower_call_into(value, destinations.clone())?;
            return Ok(destinations.into_iter().map(Operand::Read).collect());
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
        Ok(operands)
    }

    pub(super) fn lower_deferred(&mut self) -> Result<(), Diagnostic> {
        let deferred = self.deferred.clone();
        for body in deferred.iter().rev() {
            if self.is_terminated(self.current)? {
                break;
            }
            self.lower_block(body)?;
        }
        Ok(())
    }

    pub(super) fn label_target(&self, label: &str) -> Result<BasicBlockId, Diagnostic> {
        self.labels
            .get(label)
            .copied()
            .ok_or_else(|| Diagnostic::backend(format!("unknown HIR label {label}")))
    }

    pub(super) fn enter_label(&mut self, label: &str, source: SourceRef) -> Result<(), Diagnostic> {
        let target = self.label_target(label)?;
        if self.current != target && !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Goto(target),
                hir::Effects::default(),
                Provenance::Source(source),
            ))?;
        }
        self.current = target;
        Ok(())
    }
}
