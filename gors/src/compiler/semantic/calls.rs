//! Shared argument typing and variadic slice packing for calls.

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, SyntaxSource};
use crate::compiler::types::{IntTy, Ty};

pub(super) enum LoweredCallArguments {
    Explicit(Vec<hir::Expr>),
    Forwarded {
        source_call: hir::Expr,
        coercions: Vec<hir::ValueCoercion>,
        fixed_results: u32,
        variadic_slice: Option<Ty>,
    },
}

impl LoweredCallArguments {
    pub(super) fn effects(&self) -> hir::Effects {
        match self {
            Self::Explicit(arguments) => arguments
                .iter()
                .fold(hir::Effects::default(), |effects, argument| {
                    effects.union(argument.effects)
                }),
            Self::Forwarded { source_call, .. } => source_call.effects,
        }
    }

    pub(super) fn into_call_kind(
        self,
        callee: hir::Callee,
        prefix: Vec<hir::Expr>,
    ) -> hir::ExprKind {
        match self {
            Self::Explicit(mut arguments) => {
                if !prefix.is_empty() {
                    let mut combined = prefix;
                    combined.append(&mut arguments);
                    arguments = combined;
                }
                hir::ExprKind::Call {
                    callee,
                    args: arguments,
                }
            }
            Self::Forwarded {
                source_call,
                coercions,
                fixed_results,
                variadic_slice,
            } => hir::ExprKind::ForwardedCall {
                callee,
                prefix,
                source_call: Box::new(source_call),
                coercions,
                fixed_results,
                variadic_slice,
            },
        }
    }

    pub(super) fn into_explicit(self, source: SourceRef) -> Result<Vec<hir::Expr>, Diagnostic> {
        match self {
            Self::Explicit(arguments) => Ok(arguments),
            Self::Forwarded { .. } => Err(Diagnostic::unsupported(
                "multi-result call forwarding is not represented for this call target",
                source,
            )),
        }
    }
}

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
    ) -> Result<LoweredCallArguments, Diagnostic> {
        if spread && !variadic {
            return Err(Diagnostic::semantic(
                format!("... is only valid when calling a variadic {callable}"),
                source,
            ));
        }

        let mut lowered_single_call = if let [argument] = arguments
            && could_be_call(argument)
        {
            Some(self.lower_expr(argument, None)?)
        } else {
            None
        };
        if let Some(argument) = lowered_single_call.as_ref()
            && let Ty::Tuple(results) = &argument.ty
            && is_forwardable_call(argument)
        {
            if spread {
                return Err(Diagnostic::semantic(
                    format!("cannot use ... with {}-valued function call", results.len()),
                    source,
                ));
            }
            return self.plan_forwarded_call_arguments(
                argument.clone(),
                results.clone(),
                parameters,
                variadic,
                source,
                callable,
            );
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
            let lowered = arguments
                .iter()
                .zip(parameters)
                .map(|(argument, expected)| {
                    if lowered_single_call.is_some() {
                        let value = lowered_single_call.take().ok_or_else(|| {
                            Diagnostic::backend("sole call argument was lowered more than once")
                        })?;
                        self.coerce_pre_lowered_call_argument(value, expected, argument.source)
                    } else {
                        self.lower_expr(argument, Some(expected))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(LoweredCallArguments::Explicit(lowered));
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
        let Ty::Slice(element) = variadic_parameter.underlying() else {
            return Err(Diagnostic::backend(
                "variadic signature final parameter is not a slice",
            ));
        };
        let mut lowered = fixed_arguments
            .iter()
            .zip(fixed_parameters)
            .map(|(argument, expected)| {
                if lowered_single_call.is_some() {
                    let value = lowered_single_call.take().ok_or_else(|| {
                        Diagnostic::backend("sole call argument was lowered more than once")
                    })?;
                    self.coerce_pre_lowered_call_argument(value, expected, argument.source)
                } else {
                    self.lower_expr(argument, Some(expected))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if variadic_arguments.is_empty() {
            // A variadic call without final arguments passes the parameter's
            // nil slice, not an allocated empty slice.
            let node = self.alloc_node(pack_source)?;
            let nil =
                self.zero_value_expr(node, SourceRef::node(node), variadic_parameter.clone())?;
            lowered.push(nil);
            return Ok(LoweredCallArguments::Explicit(lowered));
        }
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "variadic slice packing currently supports int elements",
                source,
            ));
        }
        let values = variadic_arguments
            .iter()
            .map(|argument| {
                if lowered_single_call.is_some() {
                    let value = lowered_single_call.take().ok_or_else(|| {
                        Diagnostic::backend("sole call argument was lowered more than once")
                    })?;
                    self.coerce_pre_lowered_call_argument(value, element, argument.source)
                } else {
                    self.lower_expr(argument, Some(element))
                }
            })
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
        Ok(LoweredCallArguments::Explicit(lowered))
    }

    pub(super) fn plan_forwarded_call_arguments(
        &self,
        source_call: hir::Expr,
        results: Vec<Ty>,
        parameters: &[Ty],
        variadic: bool,
        source: SourceRef,
        callable: &str,
    ) -> Result<LoweredCallArguments, Diagnostic> {
        let (fixed_parameters, variadic_slice, variadic_element) = if variadic {
            let Some((slice, fixed)) = parameters.split_last() else {
                return Err(Diagnostic::backend(
                    "variadic signature has no final slice parameter",
                ));
            };
            let Ty::Slice(element) = slice.underlying() else {
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
            (fixed, Some(slice.clone()), Some(element.as_ref()))
        } else {
            (parameters, None, None)
        };

        if (!variadic && results.len() != fixed_parameters.len())
            || (variadic && results.len() < fixed_parameters.len())
        {
            let expectation = if variadic {
                format!("requires at least {}", fixed_parameters.len())
            } else {
                format!("expected {}", fixed_parameters.len())
            };
            return Err(Diagnostic::semantic(
                format!(
                    "{callable} call receives {} forwarded results; {expectation}",
                    results.len()
                ),
                source,
            ));
        }

        let coercions = results
            .iter()
            .enumerate()
            .map(|(index, actual)| {
                let expected = fixed_parameters
                    .get(index)
                    .or(variadic_element)
                    .ok_or_else(|| {
                        Diagnostic::backend("forwarded call result has no target parameter")
                    })?;
                self.assignment_value_coercion(actual, expected, source)
                    .map_err(|_| {
                        Diagnostic::semantic(
                            format!(
                                "forwarded result {index} of type {actual:?} is not assignable to parameter type {expected:?}"
                            ),
                            source,
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let fixed_results = u32::try_from(fixed_parameters.len())
            .map_err(|_| Diagnostic::backend("forwarded call fixed parameter count exceeds u32"))?;
        Ok(LoweredCallArguments::Forwarded {
            source_call,
            coercions,
            fixed_results,
            variadic_slice,
        })
    }

    fn coerce_pre_lowered_call_argument(
        &mut self,
        value: hir::Expr,
        expected: &Ty,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let coercion = self.assignment_value_coercion(&value.ty, expected, value.source)?;
        self.apply_assignment_value_coercion(value, &coercion, syntax_source)
    }
}

fn could_be_call(expression: &ExprSyntax) -> bool {
    match &expression.kind {
        crate::compiler::syntax::ExprSyntaxKind::Call { .. } => true,
        crate::compiler::syntax::ExprSyntaxKind::Paren(expression) => could_be_call(expression),
        _ => false,
    }
}

pub(super) fn forwarded_call_result_types(expression: &hir::Expr) -> Option<&[Ty]> {
    let Ty::Tuple(results) = &expression.ty else {
        return None;
    };
    is_forwardable_call(expression).then_some(results)
}

fn is_forwardable_call(expression: &hir::Expr) -> bool {
    matches!(
        expression.kind,
        hir::ExprKind::Call {
            callee: hir::Callee::Function(_) | hir::Callee::Closure(_),
            ..
        } | hir::ExprKind::ForwardedCall { .. }
    )
}
