//! Control-flow expression helpers for MIR construction.

use super::super::construct::{make_rvalue, make_statement, make_terminator};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Ty};

impl FunctionLowerer {
    pub(super) fn lower_short_circuit(
        &mut self,
        op: hir::BinaryOp,
        left: &hir::Expr,
        right: &hir::Expr,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let left = self.lower_expr(left)?;
        let left = self.materialize(left, Ty::Bool, Provenance::Source(source))?;
        let provenance = Provenance::Source(source);
        let evaluate_right = self.new_block(provenance.clone());
        let short_value = self.new_block(provenance.clone());
        let join = self.new_block(provenance.clone());
        let result = Place {
            local: self.new_temp(Ty::Bool),
        };
        let (then_target, else_target, short_constant) = match op {
            hir::BinaryOp::LogicalAnd => (evaluate_right, short_value, false),
            hir::BinaryOp::LogicalOr => (short_value, evaluate_right, true),
            _ => return Err(Diagnostic::backend("non-logical short-circuit operation")),
        };
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: left,
                then_target,
                else_target,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = short_value;
        let value = make_rvalue(
            RvalueKind::Use(Operand::Constant(
                ConstValue::Bool(short_constant),
                Ty::Bool,
            )),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = evaluate_right;
        let right = self.lower_expr(right)?;
        let right = self.materialize(right, Ty::Bool, Provenance::Source(source))?;
        let value = make_rvalue(
            RvalueKind::Use(right),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(join),
            hir::Effects::default(),
            provenance,
        ))?;
        self.current = join;
        Ok(Operand::Read(result))
    }
}
