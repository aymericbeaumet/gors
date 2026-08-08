//! Named function and builtin call-expression lowering.

use super::FunctionLowerer;
use super::calls::LoweredCallArguments;
use super::conversions::is_predeclared_conversion_name;
use super::expressions::{coerce_expr, default_expr_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, SyntaxSource};
use crate::compiler::types::Ty;

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_named_call_expression(
        &mut self,
        name: &str,
        callee_expression: &ExprSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        if name == "make" {
            return self.lower_make_builtin_call(arguments, spread, node, source, expected);
        }
        if name == "new" {
            return self.lower_new_builtin_call(arguments, spread, node, source, expected);
        }
        if name == "recover" {
            return self.lower_recover_builtin_call(arguments, spread, node, source, expected);
        }
        if name == "len" && self.resolves_to_predeclared(name) {
            return self.lower_length_capacity_builtin(
                super::length_capacity::LengthCapacityOp::Len,
                arguments,
                spread,
                node,
                source,
                expected,
            );
        }
        if name == "cap" && self.resolves_to_predeclared(name) {
            return self.lower_length_capacity_builtin(
                super::length_capacity::LengthCapacityOp::Cap,
                arguments,
                spread,
                node,
                source,
                expected,
            );
        }
        if name == "close" {
            return self
                .lower_channel_close_builtin_call(arguments, spread, node, source, expected);
        }
        if name == "clear" {
            return self.lower_clear_builtin_call(arguments, spread, node, source, expected);
        }
        if name == "delete" {
            return self.lower_delete_builtin_call(arguments, spread, node, source, expected);
        }
        if matches!(name, "append" | "copy") {
            return self.lower_slice_builtin_call(name, arguments, spread, node, source, expected);
        }
        if matches!(name, "min" | "max" | "complex" | "real" | "imag") {
            return self
                .lower_numeric_builtin_call(name, arguments, spread, node, source, expected);
        }
        if self.type_aliases.contains_key(name)
            || (is_predeclared_conversion_name(name) && self.resolves_to_predeclared(name))
        {
            return self.lower_conversion_call(
                callee_expression,
                arguments,
                spread,
                node,
                source,
                expected,
            );
        }
        if self.generic_functions.contains_key(name) {
            return self.lower_generic_function_call(
                name,
                arguments,
                spread,
                node,
                syntax_source,
                source,
                expected,
                allow_discarded_call_result,
            );
        }

        let (callee, params, results, variadic) = if let Some(id) = self.lookup_closure(name) {
            let closure = self
                .closures
                .get(id.index() as usize)
                .ok_or_else(|| Diagnostic::backend(format!("unknown local function {name}")))?;
            (
                hir::Callee::Closure(id),
                closure.signature.params.clone(),
                closure.signature.results.clone(),
                closure.signature.variadic,
            )
        } else if self.lookup_local(name).is_some() {
            return Err(Diagnostic::unsupported(
                format!("calling the function value {name} is not yet supported"),
                source,
            ));
        } else if let Some(symbol) = self.functions.get(name).cloned() {
            (
                hir::Callee::Function(symbol.id),
                symbol.signature.params,
                symbol.signature.results,
                symbol.signature.variadic,
            )
        } else if self.lookup_local_constant(name).is_some() || self.constants.contains_key(name) {
            return Err(Diagnostic::semantic(
                format!("constant {name} is not callable"),
                source,
            ));
        } else {
            match name {
                "print" => (
                    hir::Callee::Builtin(hir::Builtin::Print),
                    vec![],
                    vec![],
                    false,
                ),
                "println" => (
                    hir::Callee::Builtin(hir::Builtin::Println),
                    vec![],
                    vec![],
                    false,
                ),
                "panic" => (
                    hir::Callee::Builtin(hir::Builtin::Panic),
                    vec![],
                    vec![],
                    false,
                ),
                name => {
                    return Err(Diagnostic::semantic(
                        format!("undefined function {name}"),
                        source,
                    ));
                }
            }
        };
        match callee {
            hir::Callee::Builtin(hir::Builtin::Panic) if arguments.len() != 1 => {
                return Err(Diagnostic::semantic(
                    format!(
                        "call to panic has {} arguments; expected 1",
                        arguments.len()
                    ),
                    source,
                ));
            }
            hir::Callee::Builtin(_) if spread => {
                return Err(Diagnostic::semantic(
                    "... is not valid for this built-in call",
                    source,
                ));
            }
            _ => {}
        }
        let args = if callee == hir::Callee::Builtin(hir::Builtin::Panic) {
            let any = Ty::Interface(Vec::new());
            LoweredCallArguments::Explicit(
                arguments
                    .iter()
                    .map(|argument| self.lower_expr(argument, Some(&any)))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else if matches!(callee, hir::Callee::Function(_) | hir::Callee::Closure(_)) {
            self.lower_call_arguments(
                arguments,
                &params,
                variadic,
                spread,
                syntax_source,
                source,
                "function",
            )?
        } else {
            LoweredCallArguments::Explicit(
                arguments
                    .iter()
                    .map(|argument| {
                        self.lower_expr(argument, None).and_then(|expression| {
                            let expression_source = expression.source;
                            default_expr_type(expression, expression_source)
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        };
        let ty = match results.as_slice() {
            [] => Ty::Unit,
            [single] => single.clone(),
            many => Ty::Tuple(many.to_vec()),
        };
        if ty == Ty::Unit && !allow_discarded_call_result {
            return Err(Diagnostic::unsupported(
                "a no-result call cannot be used as a value",
                source,
            ));
        }
        let effects = hir::Effects {
            may_read: false,
            may_call: true,
            may_allocate: true,
            may_block: true,
            may_panic: true,
            may_write: true,
        }
        .union(args.effects());
        let mut call = hir::Expr {
            node,
            kind: args.into_call_kind(callee, Vec::new()),
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut call, expected, source)?;
        }
        Ok(call)
    }
}
