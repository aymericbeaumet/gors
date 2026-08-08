//! Explicit-order lowering for dynamic slice literals.

use super::pointers::int_constant_operand;
use super::{FunctionLowerer, Operand, Place, Provenance};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_i64_variadic_operands(
        &mut self,
        values: Vec<Operand>,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let Ty::Slice(element) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "forwarded variadic arguments have a non-slice parameter",
            ));
        };
        if element.underlying() != &Ty::Int(crate::compiler::types::IntTy::Int) {
            return Err(Diagnostic::backend(
                "unsupported forwarded variadic element type reached MIR",
            ));
        }

        let slice = Place {
            local: self.new_temp(ty.clone()),
        };
        if values.is_empty() {
            self.lower_zero_value(slice, ty.clone(), Provenance::Source(source))?;
            return Ok(Operand::Read(slice));
        }

        let length = int_constant_operand(values.len());
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

    pub(super) fn lower_dynamic_i64_slice_literal(
        &mut self,
        elements: &[hir::Expr],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        self.lower_dynamic_value_slice_literal(
            elements,
            ty,
            hir::Builtin::SliceI64Make,
            hir::Builtin::SliceI64Set,
            source,
        )
    }

    pub(super) fn lower_dynamic_go_string_slice_literal(
        &mut self,
        elements: &[hir::Expr],
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        self.lower_dynamic_value_slice_literal(
            elements,
            ty,
            hir::Builtin::SliceGoStringMake,
            hir::Builtin::SliceGoStringSet,
            source,
        )
    }

    fn lower_dynamic_value_slice_literal(
        &mut self,
        elements: &[hir::Expr],
        ty: &Ty,
        make: hir::Builtin,
        set: hir::Builtin,
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
        self.emit_map_call(make, vec![length.clone(), length], vec![slice], source)?;

        for (index, value) in values.into_iter().enumerate() {
            self.emit_map_call(
                set,
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
        let Ty::Slice(_) = ty.underlying() else {
            return Err(Diagnostic::backend(
                "aggregate slice literal has a non-slice type",
            ));
        };
        let mut values = Vec::with_capacity(elements.len());
        for element in elements {
            let value = self.lower_expr(element)?;
            let element_ty = element.ty.clone();
            values.push((
                self.materialize(
                    value,
                    element_ty.clone(),
                    Provenance::Source(element.source),
                )?,
                element_ty,
            ));
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
        for (index, (value, value_ty)) in values.into_iter().enumerate() {
            let tagged =
                self.box_interface_operand(value, &value_ty, type_identity, &interface_ty, source)?;
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
