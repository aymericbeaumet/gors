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
}
