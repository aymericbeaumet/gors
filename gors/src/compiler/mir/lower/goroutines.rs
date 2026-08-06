//! Explicit-order lowering for proven-empty goroutine calls.

use super::super::construct::{make_terminator, operand_ty, spawn_empty_effects};
use super::super::{Provenance, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::LocalId;
use crate::compiler::provenance::SourceRef;

impl FunctionLowerer {
    pub(super) fn lower_empty_goroutine(
        &mut self,
        parameters: &[LocalId],
        values: &[hir::Expr],
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if parameters.len() != values.len() || !body.stmts.is_empty() {
            return Err(Diagnostic::backend(
                "non-empty goroutine reached empty-goroutine MIR lowering",
            ));
        }
        for (parameter, value) in parameters.iter().zip(values) {
            let operand = self.lower_expr(value)?;
            let operand =
                self.materialize(operand, value.ty.clone(), Provenance::Source(value.source))?;
            let parameter_ty = self.local_ty(*parameter)?;
            if operand_ty(&operand, &self.locals)? != *parameter_ty {
                return Err(Diagnostic::backend(
                    "goroutine argument type changed before MIR lowering",
                ));
            }
        }

        let provenance = Provenance::Source(source);
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::SpawnEmpty { target },
            spawn_empty_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(())
    }
}
