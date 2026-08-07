//! Validation and explicit expansion of typed method receiver plans.

use super::super::construct::{make_rvalue, make_statement};
use super::super::{Operand, Place, Provenance, RvalueKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{IntTy, StructField, Ty};

impl FunctionLowerer {
    pub(super) fn lower_method_receiver_expr(
        &mut self,
        receiver: &hir::Expr,
        plan: &hir::MethodReceiverPlan,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        validate_receiver_plan(&receiver.ty, plan, result_ty)?;

        if plan.adjustment == hir::MethodReceiverAdjustment::AutoAddress {
            if !plan.path.is_empty() {
                return Err(Diagnostic::unsupported(
                    "taking the address of a promoted value-embedded method receiver requires nested pointer storage",
                    source,
                ));
            }
            let hir::ExprKind::Local(local) = receiver.kind else {
                return Err(Diagnostic::unsupported(
                    "an implicitly addressed method receiver currently requires a local variable",
                    source,
                ));
            };
            return self.lower_address_of_local_expr(local, result_ty);
        }

        let operand = self.lower_expr(receiver)?;
        let operand = self.materialize(
            operand,
            receiver.ty.clone(),
            Provenance::Source(receiver.source),
        )?;
        self.lower_method_receiver_operand(operand, &receiver.ty, plan, result_ty, source)
    }

    pub(super) fn lower_method_receiver_operand(
        &mut self,
        operand: Operand,
        root_ty: &Ty,
        plan: &hir::MethodReceiverPlan,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        validate_receiver_plan(root_ty, plan, result_ty)?;
        if plan.adjustment == hir::MethodReceiverAdjustment::AutoAddress {
            return Err(Diagnostic::unsupported(
                "taking the address of a promoted value-embedded method receiver requires nested pointer storage",
                source,
            ));
        }
        let mut operand = operand;
        let mut current_ty = root_ty.clone();
        for step in &plan.path {
            operand =
                self.read_receiver_field(operand, &current_ty, step.field, &step.field_ty, source)?;
            current_ty = step.field_ty.clone();
        }

        match plan.adjustment {
            hir::MethodReceiverAdjustment::Identity => Ok(operand),
            hir::MethodReceiverAdjustment::AutoIndirect => {
                self.read_receiver_pointer(operand, &plan.receiver_ty, source)
            }
            hir::MethodReceiverAdjustment::AutoAddress => Err(Diagnostic::backend(
                "auto-address receiver plan bypassed its dedicated lowering path",
            )),
        }
    }

    fn read_receiver_field(
        &mut self,
        structure: Operand,
        structure_ty: &Ty,
        field: u32,
        field_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        let structure = if matches!(structure_ty.underlying(), Ty::Pointer(_)) {
            self.read_receiver_pointer(structure, pointer_element(structure_ty)?, source)?
        } else {
            structure
        };
        let result = Place {
            local: self.new_temp(field_ty.clone()),
        };
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::StructField { structure, field },
            hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            provenance.clone(),
        );
        self.push_statement(make_statement(result, value, provenance))?;
        Ok(Operand::Read(result))
    }

    fn read_receiver_pointer(
        &mut self,
        pointer: Operand,
        value_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if value_ty.underlying() == &Ty::Int(IntTy::Int) {
            let result = Place {
                local: self.new_temp(value_ty.clone()),
            };
            self.emit_pointer_call(
                hir::Builtin::PointerI64Get,
                vec![pointer],
                vec![result],
                Provenance::Source(source),
            )?;
            return Ok(Operand::Read(result));
        }
        if value_ty.bootstrap_i64_struct_fields().is_some() {
            return self.read_struct_pointer_value(pointer, value_ty, source);
        }
        if value_ty.uses_interface_aggregate_pointer_representation() {
            return self.read_aggregate_struct_pointer_value(pointer, value_ty, source);
        }
        Err(Diagnostic::backend(format!(
            "method receiver cannot dereference a pointer to {value_ty:?}"
        )))
    }
}

fn validate_receiver_plan(
    root_ty: &Ty,
    plan: &hir::MethodReceiverPlan,
    result_ty: &Ty,
) -> Result<(), Diagnostic> {
    same_type(root_ty, &plan.root_ty, "root type")?;
    same_type(result_ty, &plan.receiver_ty, "result type")?;
    let mut current = root_ty.clone();
    for step in &plan.path {
        same_type(&current, &step.owner_ty, "field owner")?;
        let fields = struct_fields(&current)
            .ok_or_else(|| Diagnostic::backend("method receiver path crossed a non-struct type"))?;
        let field = fields
            .get(usize::try_from(step.field).map_err(|_| {
                Diagnostic::backend("method receiver field index does not fit usize")
            })?)
            .ok_or_else(|| Diagnostic::backend("method receiver field index is out of bounds"))?;
        if !field.embedded {
            return Err(Diagnostic::backend(
                "method receiver path crossed a non-embedded field",
            ));
        }
        same_type(&field.ty, &step.field_ty, "field type")?;
        current = field.ty.clone();
    }
    same_type(&current, &plan.selected_ty, "selected type")?;
    match plan.adjustment {
        hir::MethodReceiverAdjustment::Identity => {
            same_type(&plan.selected_ty, &plan.receiver_ty, "identity adjustment")?;
        }
        hir::MethodReceiverAdjustment::AutoAddress => {
            same_type(
                &plan.selected_ty,
                pointer_element(&plan.receiver_ty)?,
                "auto-address adjustment",
            )?;
        }
        hir::MethodReceiverAdjustment::AutoIndirect => {
            same_type(
                pointer_element(&plan.selected_ty)?,
                &plan.receiver_ty,
                "auto-indirect adjustment",
            )?;
        }
    }
    Ok(())
}

fn pointer_element(ty: &Ty) -> Result<&Ty, Diagnostic> {
    let Ty::Pointer(element) = ty.underlying() else {
        return Err(Diagnostic::backend(
            "method receiver adjustment expected a pointer type",
        ));
    };
    Ok(element)
}

fn struct_fields(ty: &Ty) -> Option<&[StructField]> {
    match ty.underlying() {
        Ty::Struct(fields) => Some(fields),
        Ty::Pointer(element) => match element.underlying() {
            Ty::Struct(fields) => Some(fields),
            _ => None,
        },
        _ => None,
    }
}

fn same_type(actual: &Ty, expected: &Ty, part: &str) -> Result<(), Diagnostic> {
    if actual == expected {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "method receiver {part} mismatch: expected {expected:?}, found {actual:?}"
        )))
    }
}
