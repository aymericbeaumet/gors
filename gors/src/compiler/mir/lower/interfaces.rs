//! Explicit-order lowering for interface dynamic values.

use super::super::construct::{make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
use super::pointers::int_constant_operand;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, IntTy, Ty};

impl FunctionLowerer {
    pub(super) fn lower_interface_value_expr(
        &mut self,
        value: &hir::Expr,
        type_identity: &[u8],
        interface_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if !matches!(interface_ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::backend(
                "interface value has a non-interface result type",
            ));
        }
        let value_operand = self.lower_expr(value)?;
        let value_operand = self.materialize(
            value_operand,
            value.ty.clone(),
            Provenance::Source(value.source),
        )?;
        let identity = Operand::Constant(ConstValue::String(type_identity.to_vec()), Ty::String);
        let (builtin, payload) = match value.ty.underlying() {
            Ty::Bool => (hir::Builtin::InterfaceBoxBool, value_operand),
            Ty::Int(IntTy::Int) => (hir::Builtin::InterfaceBoxI64, value_operand),
            Ty::String => (hir::Builtin::InterfaceBoxGoString, value_operand),
            Ty::Struct(_) if value.ty.bootstrap_i64_struct_fields().is_some() => (
                hir::Builtin::InterfaceBoxStructI64,
                self.snapshot_interface_struct(value_operand, &value.ty, source)?,
            ),
            Ty::Pointer(_) if value.ty.bootstrap_i64_struct_pointer_fields().is_some() => {
                (hir::Builtin::InterfaceBoxPointerStructI64, value_operand)
            }
            _ => {
                return Err(Diagnostic::backend(format!(
                    "unsupported interface dynamic value type {:?}",
                    value.ty
                )));
            }
        };
        let result = Place {
            local: self.new_temp(interface_ty.clone()),
        };
        self.emit_map_call(builtin, vec![identity, payload], vec![result], source)?;
        Ok(Operand::Read(result))
    }

    fn snapshot_interface_struct(
        &mut self,
        structure: Operand,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let fields = ty.bootstrap_i64_struct_fields().ok_or_else(|| {
            Diagnostic::backend("interface struct snapshot has a non-integer struct type")
        })?;
        let slice_ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
        let slice = Place {
            local: self.new_temp(slice_ty),
        };
        let no_capacity = Operand::Constant(ConstValue::Int("-1".to_owned()), Ty::Int(IntTy::Int));
        self.emit_map_call(
            hir::Builtin::SliceI64Make,
            vec![int_constant_operand(fields.len()), no_capacity],
            vec![slice],
            source,
        )?;
        for (index, field) in fields.iter().enumerate() {
            let field_value = Place {
                local: self.new_temp(field.ty.clone()),
            };
            let provenance = Provenance::Source(source);
            let read = make_rvalue(
                RvalueKind::StructField {
                    structure: structure.clone(),
                    field: u32::try_from(index).map_err(|_| {
                        Diagnostic::backend("interface struct field index exceeds u32")
                    })?,
                },
                hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                provenance.clone(),
            );
            self.push_statement(make_statement(field_value, read, provenance))?;
            self.emit_map_call(
                hir::Builtin::SliceI64Set,
                vec![
                    Operand::Read(slice),
                    int_constant_operand(index),
                    Operand::Read(field_value),
                ],
                Vec::new(),
                source,
            )?;
        }
        Ok(Operand::Read(slice))
    }
}
