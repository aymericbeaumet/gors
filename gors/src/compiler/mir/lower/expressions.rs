//! Control-flow expression helpers for MIR construction.

use super::super::construct::{
    binary_effects, call_effects, make_rvalue, make_statement, make_terminator, operand_ty,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Ty};

impl FunctionLowerer {
    pub(super) fn lower_recover_expr(
        &mut self,
        ty: &Ty,
        effects: hir::Effects,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let state = self.recover_active.ok_or_else(|| {
            Diagnostic::backend("recover reached a function without cleanup state")
        })?;
        let recovered = self.recover_value.ok_or_else(|| {
            Diagnostic::backend("recover reached a function without a captured value")
        })?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Recover {
                state: Place { local: state },
                value: Operand::Read(Place { local: recovered }),
            },
            effects,
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_unary_expr(
        &mut self,
        op: hir::UnaryOp,
        operand: &hir::Expr,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let operand_provenance = Provenance::Source(operand.source);
        let operand = self.lower_expr(operand)?;
        let operand_ty = operand_ty(&operand, &self.locals)?;
        let operand = self.materialize(operand, operand_ty, operand_provenance)?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Unary {
                op,
                operand,
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_conversion_expr(
        &mut self,
        value: &hir::Expr,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let operand = self.lower_expr(value)?;
        let operand =
            self.materialize(operand, value.ty.clone(), Provenance::Source(value.source))?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let converted = make_rvalue(
            RvalueKind::Conversion {
                operand,
                from: value.ty.clone(),
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, converted, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_binary_expr(
        &mut self,
        op: hir::BinaryOp,
        left: &hir::Expr,
        right: &hir::Expr,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        // Freeze each operand immediately after evaluation so evaluation of
        // the right operand cannot observe a delayed read of the left.
        let left_operand = self.lower_expr(left)?;
        let left_operand = self.materialize(
            left_operand,
            left.ty.clone(),
            Provenance::Source(left.source),
        )?;
        let right_operand = self.lower_expr(right)?;
        let right_operand = self.materialize(
            right_operand,
            right.ty.clone(),
            Provenance::Source(right.source),
        )?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Binary {
                op,
                left: left_operand,
                right: right_operand,
                ty: ty.clone(),
            },
            binary_effects(op, ty, &right.ty),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_call_expr(
        &mut self,
        callee: hir::Callee,
        args: &[hir::Expr],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if callee == hir::Callee::Builtin(hir::Builtin::InterfaceAssert) {
            return self.lower_interface_assertion_expr(args, ty, source);
        }
        if matches!(
            callee,
            hir::Callee::Builtin(
                hir::Builtin::InterfaceSatisfies
                    | hir::Builtin::InterfaceSatisfiesRuntimeError
                    | hir::Builtin::InterfaceSatisfiesNonNil
            )
        ) {
            if ty != &Ty::Bool {
                return Err(Diagnostic::backend(
                    "interface satisfaction value bypassed tuple lowering",
                ));
            }
            return self.lower_interface_satisfaction_test(
                args,
                callee == hir::Callee::Builtin(hir::Builtin::InterfaceSatisfiesNonNil),
                callee == hir::Callee::Builtin(hir::Builtin::InterfaceSatisfiesRuntimeError),
                source,
            );
        }
        if let hir::Callee::Closure(id) = callee {
            if matches!(ty, Ty::Tuple(_)) {
                return Err(Diagnostic::backend(
                    "tuple-valued local function call bypassed tuple lowering",
                ));
            }
            let destination = (ty != &Ty::Unit).then(|| Place {
                local: self.new_temp(ty.clone()),
            });
            self.lower_closure_call(id, args, destination.into_iter().collect(), source)?;
            return Ok(destination.map_or(Operand::Unit, Operand::Read));
        }
        let mut operands = Vec::new();
        for arg in args {
            let operand = self.lower_expr(arg)?;
            operands.push(self.materialize(
                operand,
                arg.ty.clone(),
                Provenance::Source(arg.source),
            )?);
        }
        let provenance = Provenance::Source(source);
        let target = self.new_block(provenance.clone());
        let diverges = callee == hir::Callee::Builtin(hir::Builtin::Panic);
        let destination = (ty != &Ty::Unit).then(|| Place {
            local: self.new_temp(ty.clone()),
        });
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee,
                args: operands,
                destinations: destination.into_iter().collect(),
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        if diverges {
            self.terminate(make_terminator(
                TerminatorKind::Unreachable,
                hir::Effects::default(),
                Provenance::Source(source),
            ))?;
        }
        Ok(destination.map_or(Operand::Unit, Operand::Read))
    }

    pub(super) fn lower_slice_expr(&mut self, expr: &hir::Expr) -> Result<Operand, Diagnostic> {
        match &expr.kind {
            hir::ExprKind::SliceLiteralI64(elements) => {
                let result = Place {
                    local: self.new_temp(expr.ty.clone()),
                };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralI64 {
                        elements: elements.clone(),
                        ty: expr.ty.clone(),
                    },
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            hir::ExprKind::DynamicSliceLiteralI64(elements) => {
                self.lower_dynamic_i64_slice_literal(elements, &expr.ty, expr.source)
            }
            hir::ExprKind::AggregateSliceLiteral {
                elements,
                type_identity,
            } => self.lower_aggregate_slice_literal(elements, type_identity, &expr.ty, expr.source),
            hir::ExprKind::AggregateSliceIndex {
                slice,
                index,
                type_identity,
            } => {
                self.lower_aggregate_slice_index(slice, index, type_identity, &expr.ty, expr.source)
            }
            hir::ExprKind::SliceLiteralU8(elements) => {
                let result = Place {
                    local: self.new_temp(expr.ty.clone()),
                };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralU8(elements.clone()),
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            hir::ExprKind::SliceLiteralBool(elements) => {
                let result = Place {
                    local: self.new_temp(expr.ty.clone()),
                };
                let provenance = Provenance::Source(expr.source);
                let value = make_rvalue(
                    RvalueKind::SliceLiteralBool(elements.clone()),
                    expr.effects,
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            hir::ExprKind::SliceLiteralGoString(elements) => {
                self.lower_dynamic_go_string_slice_literal(elements, &expr.ty, expr.source)
            }
            _ => Err(Diagnostic::backend(
                "non-slice HIR expression reached slice MIR lowering",
            )),
        }
    }

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
