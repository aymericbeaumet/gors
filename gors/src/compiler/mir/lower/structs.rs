//! Explicit-order lowering for struct construction and field reads.

use super::super::construct::{make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
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
}
