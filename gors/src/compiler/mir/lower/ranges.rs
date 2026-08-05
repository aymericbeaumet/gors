//! Explicit-order MIR construction for slice range loops.

use super::super::construct::{
    binary_effects, call_effects, make_rvalue, make_statement, make_terminator,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::{FunctionLowerer, LoopTargets};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_range(
        &mut self,
        label: Option<&str>,
        key: Option<hir::Place>,
        value: Option<hir::Place>,
        expression: &hir::Expr,
        body: &hir::Block,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if let Some(label) = label {
            self.enter_label(label, source)?;
        }
        let provenance = Provenance::Source(source);
        let slice = self.lower_expr(expression)?;
        let slice = self.materialize(
            slice,
            expression.ty.clone(),
            Provenance::Source(expression.source),
        )?;

        let length = Place {
            local: self.new_temp(Ty::Int(IntTy::Int)),
        };
        let after_length = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::SliceI64Len),
                args: vec![slice.clone()],
                destinations: vec![length],
                target: after_length,
            },
            call_effects(),
            provenance.clone(),
        ))?;
        self.current = after_length;

        let index = Place {
            local: self.new_temp(Ty::Int(IntTy::Int)),
        };
        let zero = make_rvalue(
            RvalueKind::Use(Operand::Constant(
                ConstValue::Int("0".into()),
                Ty::Int(IntTy::Int),
            )),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(index, zero, provenance.clone()))?;

        let header = self.new_block(provenance.clone());
        let body_target = self.new_block(provenance.clone());
        let post_target = self.new_block(provenance.clone());
        let exit_target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Goto(header),
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.current = header;
        let condition = Place {
            local: self.new_temp(Ty::Bool),
        };
        let comparison = make_rvalue(
            RvalueKind::Binary {
                op: hir::BinaryOp::Less,
                left: Operand::Read(index),
                right: Operand::Read(length),
                ty: Ty::Bool,
            },
            binary_effects(hir::BinaryOp::Less, &Ty::Bool),
            provenance.clone(),
        );
        self.push_statement(make_statement(condition, comparison, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::SwitchBool {
                condition: Operand::Read(condition),
                then_target: body_target,
                else_target: exit_target,
            },
            hir::Effects::default(),
            provenance.clone(),
        ))?;

        self.loops.push(LoopTargets {
            label: label.map(str::to_owned),
            break_target: exit_target,
            continue_target: post_target,
            break_used: false,
        });
        self.current = body_target;
        if let Some(hir::Place::Local(local)) = key {
            self.assign_range_local(Place { local }, Operand::Read(index), source)?;
        }
        if let Some(hir::Place::Local(local)) = value {
            let after_value = self.new_block(provenance.clone());
            self.terminate(make_terminator(
                TerminatorKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::SliceI64Index),
                    args: vec![slice, Operand::Read(index)],
                    destinations: vec![Place { local }],
                    target: after_value,
                },
                call_effects(),
                provenance.clone(),
            ))?;
            self.current = after_value;
        }
        self.lower_block(body)?;
        if !self.is_terminated(self.current)? {
            self.terminate(make_terminator(
                TerminatorKind::Goto(post_target),
                hir::Effects::default(),
                provenance.clone(),
            ))?;
        }

        self.current = post_target;
        let increment = make_rvalue(
            RvalueKind::Binary {
                op: hir::BinaryOp::Add,
                left: Operand::Read(index),
                right: Operand::Constant(ConstValue::Int("1".into()), Ty::Int(IntTy::Int)),
                ty: Ty::Int(IntTy::Int),
            },
            binary_effects(hir::BinaryOp::Add, &Ty::Int(IntTy::Int)),
            provenance.clone(),
        );
        self.push_statement(make_statement(index, increment, provenance.clone()))?;
        self.terminate(make_terminator(
            TerminatorKind::Goto(header),
            hir::Effects::default(),
            provenance,
        ))?;
        self.loops.pop();
        self.current = exit_target;
        Ok(())
    }

    fn assign_range_local(
        &mut self,
        destination: Place,
        operand: Operand,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Use(operand),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(destination, value, provenance))
    }
}
