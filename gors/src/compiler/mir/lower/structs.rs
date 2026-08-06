//! Explicit-order lowering for struct construction and field reads.

use super::super::construct::{assignment_binary_op, binary_effects, make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
use super::pointers::int_constant_operand;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_struct_literal_expr(
        &mut self,
        fields: &[hir::Expr],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let mut operands = Vec::with_capacity(fields.len());
        for field in fields {
            let operand = self.lower_expr(field)?;
            operands.push(self.materialize(
                operand,
                field.ty.clone(),
                Provenance::Source(field.source),
            )?);
        }
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::StructLiteral {
                fields: operands,
                ty: ty.clone(),
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_struct_field_expr(
        &mut self,
        structure: &hir::Expr,
        field: u32,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let operand = self.lower_expr(structure)?;
        let operand = self.materialize(
            operand,
            structure.ty.clone(),
            Provenance::Source(structure.source),
        )?;
        let result = Place {
            local: self.new_temp(ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::StructField {
                structure: operand,
                field,
            },
            hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    pub(super) fn lower_struct_field_assignment_stmt(
        &mut self,
        structure: crate::compiler::ids::LocalId,
        field: u32,
        op: hir::AssignOp,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let structure_ty = self.local_ty(structure)?.clone();
        if structure_ty.bootstrap_i64_struct_pointer_fields().is_some() {
            return self
                .lower_pointer_struct_field_assignment_stmt(structure, field, op, value, source);
        }
        let field_ty = value.ty.clone();
        let provenance = Provenance::Source(source);
        let structure_operand = self.read_semantic_local(structure, source)?;
        let structure_operand =
            self.materialize(structure_operand, structure_ty.clone(), provenance.clone())?;

        let assigned = if op == hir::AssignOp::Set {
            let operand = self.lower_expr(value)?;
            self.materialize(operand, field_ty, Provenance::Source(value.source))?
        } else {
            let old = Place {
                local: self.new_temp(field_ty.clone()),
            };
            let read = make_rvalue(
                RvalueKind::StructField {
                    structure: structure_operand.clone(),
                    field,
                },
                hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                provenance.clone(),
            );
            self.push_statement(make_statement(old, read, provenance.clone()))?;
            let rhs = self.lower_expr(value)?;
            let rhs = self.materialize(rhs, field_ty.clone(), Provenance::Source(value.source))?;
            let result = Place {
                local: self.new_temp(field_ty.clone()),
            };
            let binary_op = assignment_binary_op(op);
            let binary = make_rvalue(
                RvalueKind::Binary {
                    op: binary_op,
                    left: Operand::Read(old),
                    right: rhs,
                    ty: field_ty.clone(),
                },
                binary_effects(binary_op, &field_ty),
                provenance.clone(),
            );
            self.push_statement(make_statement(result, binary, provenance.clone()))?;
            Operand::Read(result)
        };
        let result = Place {
            local: self.new_temp(structure_ty),
        };
        let updated = make_rvalue(
            RvalueKind::StructSet {
                structure: structure_operand,
                field,
                value: assigned,
            },
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(result, updated, provenance.clone()))?;
        self.write_semantic_local(structure, Operand::Read(result), provenance, false)
    }

    fn lower_pointer_struct_field_assignment_stmt(
        &mut self,
        structure: crate::compiler::ids::LocalId,
        field: u32,
        op: hir::AssignOp,
        value: &hir::Expr,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let pointer_ty = self.local_ty(structure)?.clone();
        let fields = pointer_ty
            .bootstrap_i64_struct_pointer_fields()
            .ok_or_else(|| Diagnostic::backend("pointer field update has a non-struct pointee"))?;
        let field_ty = fields
            .get(
                usize::try_from(field)
                    .map_err(|_| Diagnostic::backend("struct field index does not fit usize"))?,
            )
            .map(|definition| definition.ty.clone())
            .ok_or_else(|| Diagnostic::backend("pointer struct field update is out of bounds"))?;
        if value.ty != field_ty {
            return Err(Diagnostic::backend(
                "pointer struct field update changed its field type",
            ));
        }
        let provenance = Provenance::Source(source);
        let pointer = self.read_semantic_local(structure, source)?;
        let pointer = self.materialize(pointer, pointer_ty, provenance.clone())?;
        let field_index = int_constant_operand(
            usize::try_from(field)
                .map_err(|_| Diagnostic::backend("struct field index does not fit usize"))?,
        );
        let assigned = if op == hir::AssignOp::Set {
            let operand = self.lower_expr(value)?;
            self.materialize(operand, field_ty, Provenance::Source(value.source))?
        } else {
            let old = Place {
                local: self.new_temp(field_ty.clone()),
            };
            self.emit_pointer_call(
                hir::Builtin::PointerStructI64Get,
                vec![pointer.clone(), field_index.clone()],
                vec![old],
                provenance.clone(),
            )?;
            let rhs = self.lower_expr(value)?;
            let rhs = self.materialize(rhs, field_ty.clone(), Provenance::Source(value.source))?;
            let result = Place {
                local: self.new_temp(field_ty.clone()),
            };
            let binary_op = assignment_binary_op(op);
            let binary = make_rvalue(
                RvalueKind::Binary {
                    op: binary_op,
                    left: Operand::Read(old),
                    right: rhs,
                    ty: field_ty.clone(),
                },
                binary_effects(binary_op, &field_ty),
                provenance.clone(),
            );
            self.push_statement(make_statement(result, binary, provenance.clone()))?;
            Operand::Read(result)
        };
        self.emit_pointer_call(
            hir::Builtin::PointerStructI64Set,
            vec![pointer, field_index, assigned],
            Vec::new(),
            provenance,
        )
    }
}
