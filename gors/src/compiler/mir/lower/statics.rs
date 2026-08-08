//! Materialization of fully evaluated immutable package values.

use super::super::construct::{make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
use super::pointers::int_constant_operand;
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{StaticValue, Ty};

impl FunctionLowerer {
    pub(super) fn lower_static_value(
        &mut self,
        value: &StaticValue,
        ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        match value {
            StaticValue::Constant(value) => Ok(Operand::Constant(value.clone(), ty.clone())),
            StaticValue::Nil => {
                let result = Place {
                    local: self.new_temp(ty.clone()),
                };
                self.lower_zero_value(result, ty.clone(), Provenance::Source(source))?;
                Ok(Operand::Read(result))
            }
            StaticValue::Struct(values) => {
                let Ty::Struct(fields) = ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "static struct value has a non-struct Go type",
                    ));
                };
                if values.len() != fields.len() {
                    return Err(Diagnostic::backend(
                        "static struct value field count changed before MIR lowering",
                    ));
                }
                let mut operands = Vec::with_capacity(values.len());
                for (value, field) in values.iter().zip(fields) {
                    operands.push(self.lower_static_value(value, &field.ty, source)?);
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
                    crate::compiler::hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            StaticValue::Array(values) => {
                let Ty::Array(length, element) = ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "static array value has a non-array Go type",
                    ));
                };
                if u64::try_from(values.len()).ok() != Some(*length) {
                    return Err(Diagnostic::backend(
                        "static array value length changed before MIR lowering",
                    ));
                }
                let mut operands = Vec::with_capacity(values.len());
                for value in values {
                    operands.push(self.lower_static_value(value, element, source)?);
                }
                let result = Place {
                    local: self.new_temp(ty.clone()),
                };
                let provenance = Provenance::Source(source);
                let value = make_rvalue(
                    RvalueKind::ArrayLiteral {
                        elements: operands,
                        ty: ty.clone(),
                    },
                    crate::compiler::hir::Effects::default(),
                    provenance.clone(),
                );
                self.push_statement(make_statement(result, value, provenance))?;
                Ok(Operand::Read(result))
            }
            StaticValue::Slice(values) => {
                let Ty::Slice(element) = ty.underlying() else {
                    return Err(Diagnostic::backend(
                        "static slice value has a non-slice Go type",
                    ));
                };
                let slice = Place {
                    local: self.new_temp(ty.clone()),
                };
                let length = int_constant_operand(values.len());
                if element.underlying() == &Ty::String {
                    self.emit_map_call(
                        crate::compiler::hir::Builtin::SliceGoStringMake,
                        vec![length.clone(), length],
                        vec![slice],
                        source,
                    )?;
                    for (index, value) in values.iter().enumerate() {
                        let value = self.lower_static_value(value, element, source)?;
                        self.emit_map_call(
                            crate::compiler::hir::Builtin::SliceGoStringSet,
                            vec![Operand::Read(slice), int_constant_operand(index), value],
                            Vec::new(),
                            source,
                        )?;
                    }
                    return Ok(Operand::Read(slice));
                }
                let type_identity = element.dynamic_type_identity().ok_or_else(|| {
                    Diagnostic::backend("static slice element has no dynamic type identity")
                })?;
                self.emit_map_call(
                    crate::compiler::hir::Builtin::AggregateSliceMake,
                    vec![length.clone(), length],
                    vec![slice],
                    source,
                )?;
                let interface_ty = Ty::Interface(Vec::new());
                for (index, value) in values.iter().enumerate() {
                    let value = self.lower_static_value(value, element, source)?;
                    let tagged = self.box_interface_operand(
                        value,
                        element,
                        &type_identity,
                        &interface_ty,
                        source,
                    )?;
                    self.emit_map_call(
                        crate::compiler::hir::Builtin::AggregateSliceSetTagged,
                        vec![Operand::Read(slice), int_constant_operand(index), tagged],
                        Vec::new(),
                        source,
                    )?;
                }
                Ok(Operand::Read(slice))
            }
        }
    }
}
