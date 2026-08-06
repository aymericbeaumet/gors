//! Explicit-order lowering for dynamic slice literals.

use super::pointers::int_constant_operand;
use super::{FunctionLowerer, Operand, Place, Provenance};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_dynamic_i64_slice_literal(
        &mut self,
        elements: &[hir::Expr],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let mut values = Vec::with_capacity(elements.len());
        for element in elements {
            let value = self.lower_expr(element)?;
            values.push(self.materialize(
                value,
                element.ty.clone(),
                Provenance::Source(element.source),
            )?);
        }

        let slice = Place {
            local: self.new_temp(ty.clone()),
        };
        let length = int_constant_operand(elements.len());
        self.emit_map_call(
            hir::Builtin::SliceI64Make,
            vec![length.clone(), length],
            vec![slice],
            source,
        )?;

        for (index, value) in values.into_iter().enumerate() {
            self.emit_map_call(
                hir::Builtin::SliceI64Set,
                vec![Operand::Read(slice), int_constant_operand(index), value],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(slice))
    }

    pub(super) fn lower_aggregate_slice_literal(
        &mut self,
        elements: &[hir::Expr],
        type_identity: &[u8],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Ty::Slice(element_ty) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "aggregate slice literal has a non-slice type",
            ));
        };
        let element_ty = element_ty.as_ref().clone();
        let mut values = Vec::with_capacity(elements.len());
        for element in elements {
            let value = self.lower_expr(element)?;
            values.push(self.materialize(
                value,
                element.ty.clone(),
                Provenance::Source(element.source),
            )?);
        }

        let slice = Place {
            local: self.new_temp(ty.clone()),
        };
        let length = int_constant_operand(elements.len());
        self.emit_map_call(
            hir::Builtin::AggregateSliceMake,
            vec![length.clone(), length],
            vec![slice],
            source,
        )?;
        let interface_ty = Ty::Interface(Vec::new());
        for (index, value) in values.into_iter().enumerate() {
            let tagged = self.box_interface_operand(
                value,
                &element_ty,
                type_identity,
                &interface_ty,
                source,
            )?;
            self.emit_map_call(
                hir::Builtin::AggregateSliceSetTagged,
                vec![Operand::Read(slice), int_constant_operand(index), tagged],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(slice))
    }

    pub(super) fn lower_aggregate_slice_index(
        &mut self,
        slice: &hir::Expr,
        index: &hir::Expr,
        type_identity: &[u8],
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
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
        let tagged = Place {
            local: self.new_temp(Ty::Interface(Vec::new())),
        };
        self.emit_map_call(
            hir::Builtin::AggregateSliceIndexTagged,
            vec![slice_operand, index_operand],
            vec![tagged],
            source,
        )?;
        self.unbox_interface_value(Operand::Read(tagged), type_identity, result_ty, source)
    }
}
