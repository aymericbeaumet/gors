//! Explicit-order lowering for append expressions.

use super::pointers::int_constant_operand;
use super::{FunctionLowerer, Operand, Place, Provenance};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{Ty, UintTy};

impl FunctionLowerer {
    pub(super) fn lower_append_expr(
        &mut self,
        slice: &hir::Expr,
        arguments: &hir::AppendArguments,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let destination = self.lower_expr(slice)?;
        let destination = self.materialize(
            destination,
            slice.ty.clone(),
            Provenance::Source(slice.source),
        )?;
        match arguments {
            hir::AppendArguments::Elements(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for element in elements {
                    let value = self.lower_expr(element)?;
                    values.push(self.materialize(
                        value,
                        element.ty.clone(),
                        Provenance::Source(element.source),
                    )?);
                }
                self.lower_append_elements(destination, values, result_ty, source)
            }
            hir::AppendArguments::Spread(spread) => {
                let spread_value = self.lower_expr(spread)?;
                let spread_value = self.materialize(
                    spread_value,
                    spread.ty.clone(),
                    Provenance::Source(spread.source),
                )?;
                let builtin = match result_ty.underlying() {
                    Ty::Slice(element) if element.uses_i64_slice_carrier() => {
                        hir::Builtin::SliceI64AppendSlice
                    }
                    Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
                        if spread.ty.underlying() == &Ty::String {
                            hir::Builtin::SliceU8AppendString
                        } else {
                            hir::Builtin::SliceU8AppendSlice
                        }
                    }
                    Ty::Slice(element) if matches!(element.underlying(), Ty::Interface(_)) => {
                        hir::Builtin::AggregateSliceAppendTagged
                    }
                    _ => {
                        return Err(Diagnostic::backend(
                            "unsupported spread append reached MIR lowering",
                        ));
                    }
                };
                self.emit_append_call(builtin, destination, spread_value, result_ty, source)
            }
        }
    }

    fn lower_append_elements(
        &mut self,
        destination: Operand,
        values: Vec<Operand>,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if values.is_empty() {
            return Ok(destination);
        }
        let (builtin, appended) = match result_ty.underlying() {
            Ty::Slice(element) if element.uses_i64_slice_carrier() => (
                hir::Builtin::SliceI64AppendSlice,
                self.pack_i64_append_values(values, element.as_ref().clone(), source)?,
            ),
            Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => (
                hir::Builtin::SliceU8AppendSlice,
                self.pack_u8_append_values(values, element.as_ref().clone(), source)?,
            ),
            Ty::Slice(element) if element.underlying() == &Ty::String => {
                let [value] = values.as_slice() else {
                    return Err(Diagnostic::backend(
                        "multi-element string append reached MIR lowering",
                    ));
                };
                return self.emit_append_call(
                    hir::Builtin::SliceGoStringAppend,
                    destination,
                    value.clone(),
                    result_ty,
                    source,
                );
            }
            Ty::Slice(element) if matches!(element.underlying(), Ty::Interface(_)) => (
                hir::Builtin::AggregateSliceAppendTagged,
                self.pack_interface_append_values(values, element.as_ref().clone(), source)?,
            ),
            Ty::Slice(element) if element.snapshot_function_result().is_some() => {
                let [value] = values.as_slice() else {
                    return Err(Diagnostic::backend(
                        "multi-element function append reached MIR lowering",
                    ));
                };
                return self.emit_append_call(
                    hir::Builtin::SnapshotFunctionSliceAppend,
                    destination,
                    value.clone(),
                    result_ty,
                    source,
                );
            }
            _ => {
                return Err(Diagnostic::backend(
                    "unsupported append element representation reached MIR lowering",
                ));
            }
        };
        self.emit_append_call(builtin, destination, appended, result_ty, source)
    }

    fn pack_i64_append_values(
        &mut self,
        values: Vec<Operand>,
        element_ty: Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let slice_ty = Ty::Slice(Box::new(element_ty));
        self.pack_append_values(
            values,
            slice_ty,
            hir::Builtin::SliceI64Make,
            hir::Builtin::SliceI64Set,
            source,
        )
    }

    fn pack_u8_append_values(
        &mut self,
        values: Vec<Operand>,
        element_ty: Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let slice_ty = Ty::Slice(Box::new(element_ty));
        self.pack_append_values(
            values,
            slice_ty,
            hir::Builtin::SliceU8Make,
            hir::Builtin::SliceU8Set,
            source,
        )
    }

    fn pack_interface_append_values(
        &mut self,
        values: Vec<Operand>,
        element_ty: Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let slice_ty = Ty::Slice(Box::new(element_ty));
        self.pack_append_values(
            values,
            slice_ty,
            hir::Builtin::AggregateSliceMake,
            hir::Builtin::AggregateSliceSetTagged,
            source,
        )
    }

    fn pack_append_values(
        &mut self,
        values: Vec<Operand>,
        slice_ty: Ty,
        make: hir::Builtin,
        set: hir::Builtin,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let packed = Place {
            local: self.new_temp(slice_ty),
        };
        let length = int_constant_operand(values.len());
        self.emit_map_call(make, vec![length.clone(), length], vec![packed], source)?;
        for (index, value) in values.into_iter().enumerate() {
            self.emit_map_call(
                set,
                vec![Operand::Read(packed), int_constant_operand(index), value],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(packed))
    }

    fn emit_append_call(
        &mut self,
        builtin: hir::Builtin,
        destination: Operand,
        appended: Operand,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let result = Place {
            local: self.new_temp(result_ty.clone()),
        };
        self.emit_map_call(builtin, vec![destination, appended], vec![result], source)?;
        Ok(Operand::Read(result))
    }
}
