//! Shared argument typing and variadic slice packing for calls.

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, SyntaxSource};
use crate::compiler::types::{IntTy, Ty};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_call_arguments(
        &mut self,
        arguments: &[ExprSyntax],
        parameters: &[Ty],
        variadic: bool,
        spread: bool,
        pack_source: SyntaxSource,
        source: SourceRef,
        callable: &str,
    ) -> Result<Vec<hir::Expr>, Diagnostic> {
        if spread && !variadic {
            return Err(Diagnostic::semantic(
                format!("... is only valid when calling a variadic {callable}"),
                source,
            ));
        }
        if !variadic || spread {
            if arguments.len() != parameters.len() {
                return Err(Diagnostic::semantic(
                    format!(
                        "{callable} call has {} arguments; expected {}",
                        arguments.len(),
                        parameters.len()
                    ),
                    source,
                ));
            }
            return arguments
                .iter()
                .zip(parameters)
                .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
                .collect();
        }

        let Some((variadic_parameter, fixed_parameters)) = parameters.split_last() else {
            return Err(Diagnostic::backend(
                "variadic signature has no final slice parameter",
            ));
        };
        if arguments.len() < fixed_parameters.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "{callable} call has {} arguments; requires at least {}",
                    arguments.len(),
                    fixed_parameters.len()
                ),
                source,
            ));
        }
        let (fixed_arguments, variadic_arguments) = arguments.split_at(fixed_parameters.len());
        if variadic_arguments.is_empty() {
            return Err(Diagnostic::unsupported(
                "a variadic call with no final arguments requires nil-slice lowering",
                source,
            ));
        }
        let Ty::Slice(element) = variadic_parameter.underlying() else {
            return Err(Diagnostic::backend(
                "variadic signature final parameter is not a slice",
            ));
        };
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "variadic slice packing currently supports int elements",
                source,
            ));
        }

        let mut lowered = fixed_arguments
            .iter()
            .zip(fixed_parameters)
            .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
            .collect::<Result<Vec<_>, _>>()?;
        let values = variadic_arguments
            .iter()
            .map(|argument| self.lower_expr(argument, Some(element)))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = values
            .iter()
            .fold(hir::Effects::default(), |effects, value| {
                effects.union(value.effects)
            })
            .union(hir::Effects {
                may_call: true,
                may_allocate: true,
                may_write: true,
                may_panic: true,
                ..hir::Effects::default()
            });
        let node = self.alloc_node(pack_source)?;
        lowered.push(hir::Expr {
            node,
            kind: hir::ExprKind::DynamicSliceLiteralI64(values),
            ty: variadic_parameter.clone(),
            category: hir::ValueCategory::Value,
            effects,
            source: SourceRef::node(node),
        });
        Ok(lowered)
    }
}
