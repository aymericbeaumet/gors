//! Materialization of fully evaluated immutable package values.

use super::super::construct::{make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
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
        }
    }
}
