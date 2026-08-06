//! Explicit-order MIR construction for fixed integer arrays.

use super::super::construct::{assignment_binary_op, binary_effects, make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

fn array_operation_effects() -> hir::Effects {
    hir::Effects {
        may_panic: true,
        ..hir::Effects::default()
    }
}

impl FunctionLowerer {
    pub(super) fn lower_array_literal_expr(
        &mut self,
        elements: &[i64],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let place = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::ArrayLiteralI64(elements.to_vec()),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(place, value, provenance))?;
        Ok(Operand::Read(place))
    }

    pub(super) fn lower_scalar_array_literal_expr(
        &mut self,
        elements: &[(u64, hir::Expr)],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Ty::Array(length, _) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "scalar array literal reached MIR lowering with a non-array type",
            ));
        };
        let length = usize::try_from(*length)
            .map_err(|_| Diagnostic::backend("scalar array length does not fit usize"))?;
        let mut operands = vec![None; length];
        for (index, element) in elements {
            let operand = self.lower_expr(element)?;
            let operand = self.materialize(
                operand,
                element.ty.clone(),
                Provenance::Source(element.source),
            )?;
            let index = usize::try_from(*index)
                .map_err(|_| Diagnostic::backend("scalar array index does not fit usize"))?;
            let slot = operands.get_mut(index).ok_or_else(|| {
                Diagnostic::backend("scalar array index is outside its declared length")
            })?;
            if slot.replace(operand).is_some() {
                return Err(Diagnostic::backend(
                    "scalar array literal repeats an initialized index",
                ));
            }
        }
        let operands = operands
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Diagnostic::backend("scalar array literal omitted an element"))?;
        let place = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::ArrayLiteral {
                elements: operands,
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(place, value, provenance))?;
        Ok(Operand::Read(place))
    }

    pub(super) fn lower_array_index_expr(
        &mut self,
        array: &hir::Expr,
        index: &hir::Expr,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let array_operand = self.lower_expr(array)?;
        let array_operand = self.materialize(
            array_operand,
            array.ty.clone(),
            Provenance::Source(array.source),
        )?;
        let index_operand = self.lower_expr(index)?;
        let index_operand = self.materialize(
            index_operand,
            index.ty.clone(),
            Provenance::Source(index.source),
        )?;
        let result = Place {
            local: self.new_temp(result_ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::ArrayIndexI64 {
                array: array_operand,
                index: index_operand,
            },
            array_operation_effects(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_scalar_array_index_expr(
        &mut self,
        array: &hir::Expr,
        index: &hir::Expr,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let array_operand = self.lower_expr(array)?;
        let array_operand = self.materialize(
            array_operand,
            array.ty.clone(),
            Provenance::Source(array.source),
        )?;
        let index_operand = self.lower_expr(index)?;
        let index_operand = self.materialize(
            index_operand,
            index.ty.clone(),
            Provenance::Source(index.source),
        )?;
        let result = Place {
            local: self.new_temp(result_ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::ArrayIndex {
                array: array_operand,
                index: index_operand,
            },
            array_operation_effects(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_array_len_expr(
        &mut self,
        array: &hir::Expr,
        length: u64,
        _source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let _ = self.lower_expr(array)?;
        let length = i64::try_from(length)
            .map_err(|_| Diagnostic::backend("verified array length does not fit Go int"))?;
        Ok(Operand::Constant(
            ConstValue::Int(length.to_string()),
            Ty::Int(IntTy::Int),
        ))
    }

    pub(super) fn lower_array_assignment_stmt(
        &mut self,
        array: crate::compiler::ids::LocalId,
        index: &hir::Expr,
        op: hir::AssignOp,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(source);
        let array_place = Place { local: array };
        let array_ty = self.local_ty(array)?.clone();
        let Ty::Array(_, element) = array_ty.underlying() else {
            return Err(Diagnostic::backend(
                "array assignment reached MIR with a non-array local",
            ));
        };
        let element_ty = element.as_ref().clone();
        let index_operand = self.lower_expr(index)?;
        let index_operand = self.materialize(
            index_operand,
            index.ty.clone(),
            Provenance::Source(index.source),
        )?;

        let assigned = if op == hir::AssignOp::Set {
            let value_operand = self.lower_expr(value)?;
            self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?
        } else {
            let old = Place {
                local: self.new_temp(element_ty.clone()),
            };
            let kind = if element_ty.underlying() == &Ty::Int(IntTy::Int) {
                RvalueKind::ArrayIndexI64 {
                    array: Operand::Read(array_place),
                    index: index_operand.clone(),
                }
            } else {
                RvalueKind::ArrayIndex {
                    array: Operand::Read(array_place),
                    index: index_operand.clone(),
                }
            };
            let read = make_rvalue(kind, array_operation_effects(), provenance.clone());
            self.push_statement(make_statement(old, read, provenance.clone()))?;
            let value_operand = self.lower_expr(value)?;
            let value_operand = self.materialize(
                value_operand,
                value.ty.clone(),
                Provenance::Source(value.source),
            )?;
            let result = Place {
                local: self.new_temp(element_ty.clone()),
            };
            let binary_op = assignment_binary_op(op);
            let binary = make_rvalue(
                RvalueKind::Binary {
                    op: binary_op,
                    left: Operand::Read(old),
                    right: value_operand,
                    ty: element_ty.clone(),
                },
                binary_effects(binary_op, &value.ty),
                provenance.clone(),
            );
            self.push_statement(make_statement(result, binary, provenance.clone()))?;
            Operand::Read(result)
        };

        let kind = if element_ty.underlying() == &Ty::Int(IntTy::Int) {
            RvalueKind::ArraySetI64 {
                array: Operand::Read(array_place),
                index: index_operand,
                value: assigned,
            }
        } else {
            RvalueKind::ArraySet {
                array: Operand::Read(array_place),
                index: index_operand,
                value: assigned,
            }
        };
        let updated = make_rvalue(kind, array_operation_effects(), provenance.clone());
        self.push_statement(make_statement(array_place, updated, provenance))
    }
}
