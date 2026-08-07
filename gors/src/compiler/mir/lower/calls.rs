//! MIR binding for one sole multi-valued call used as call arguments.

use super::super::construct::{call_effects, make_terminator};
use super::super::{Operand, Place, Provenance, TerminatorKind};
use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::Ty;

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_forwarded_call_expr(
        &mut self,
        callee: hir::Callee,
        prefix: &[hir::Expr],
        source_call: &hir::Expr,
        coercions: &[hir::ValueCoercion],
        fixed_results: u32,
        variadic_slice: Option<&Ty>,
        result_ty: &Ty,
        source: SourceRef,
    ) -> Result<Operand, Diagnostic> {
        if matches!(result_ty, Ty::Tuple(_)) {
            return Err(Diagnostic::backend(
                "tuple-valued forwarded call bypassed tuple lowering",
            ));
        }
        let destination = (result_ty != &Ty::Unit).then(|| Place {
            local: self.new_temp(result_ty.clone()),
        });
        self.lower_forwarded_call_into(
            callee,
            prefix,
            source_call,
            coercions,
            fixed_results,
            variadic_slice,
            destination.into_iter().collect(),
            source,
        )?;
        Ok(destination.map_or(Operand::Unit, Operand::Read))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_forwarded_call_into(
        &mut self,
        callee: hir::Callee,
        prefix: &[hir::Expr],
        source_call: &hir::Expr,
        coercions: &[hir::ValueCoercion],
        fixed_results: u32,
        variadic_slice: Option<&Ty>,
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let Ty::Tuple(component_types) = &source_call.ty else {
            return Err(Diagnostic::backend(
                "forwarded call source lost its tuple type before MIR lowering",
            ));
        };
        if component_types.len() != coercions.len() {
            return Err(Diagnostic::backend(
                "forwarded call coercion arity changed before MIR lowering",
            ));
        }
        let fixed_results = usize::try_from(fixed_results)
            .map_err(|_| Diagnostic::backend("forwarded call fixed arity exceeds usize"))?;
        if fixed_results > component_types.len()
            || (variadic_slice.is_none() && fixed_results != component_types.len())
        {
            return Err(Diagnostic::backend(
                "forwarded call binding arity changed before MIR lowering",
            ));
        }

        let mut arguments = Vec::with_capacity(
            prefix.len() + fixed_results + usize::from(variadic_slice.is_some()),
        );
        for expression in prefix {
            let operand = self.lower_expr(expression)?;
            arguments.push(self.materialize(
                operand,
                expression.ty.clone(),
                Provenance::Source(expression.source),
            )?);
        }

        let temporary_results = component_types
            .iter()
            .map(|ty| Place {
                local: self.new_temp(ty.clone()),
            })
            .collect::<Vec<_>>();
        self.lower_call_into(source_call, temporary_results.clone())?;

        let mut forwarded = Vec::with_capacity(temporary_results.len());
        for ((result, source_ty), coercion) in temporary_results
            .into_iter()
            .zip(component_types)
            .zip(coercions)
        {
            forwarded.push(self.lower_value_coercion(
                Operand::Read(result),
                source_ty,
                coercion,
                source_call.source,
            )?);
        }
        let variadic_values = forwarded.split_off(fixed_results);
        arguments.extend(forwarded);
        if let Some(variadic_slice) = variadic_slice {
            arguments.push(self.lower_i64_variadic_operands(
                variadic_values,
                variadic_slice,
                source,
            )?);
        } else if !variadic_values.is_empty() {
            return Err(Diagnostic::backend(
                "fixed forwarded call retained unexpected variadic values",
            ));
        }

        self.emit_prepared_call(callee, arguments, destinations, source)
    }

    pub(super) fn emit_prepared_call(
        &mut self,
        callee: hir::Callee,
        arguments: Vec<Operand>,
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if let hir::Callee::Closure(id) = callee {
            return self.lower_closure_call_operands(id, arguments, destinations, source);
        }
        if matches!(callee, hir::Callee::Builtin(_)) {
            return Err(Diagnostic::backend(
                "builtin received forwarded multi-valued arguments",
            ));
        }
        let provenance = Provenance::Source(source);
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee,
                args: arguments,
                destinations,
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(())
    }
}
