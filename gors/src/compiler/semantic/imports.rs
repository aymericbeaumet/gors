//! Package-qualified expression lowering over resolver-owned import bindings.

use super::expressions::coerce_expr;
use super::function::FunctionLowerer;
use super::{FunctionSymbol, hir};
use crate::compiler::Diagnostic;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, IdentSyntax};
use crate::compiler::types::Ty;

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_imported_selector_call(
        &mut self,
        base: &ExprSyntax,
        member: &IdentSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let ExprSyntaxKind::Ident(package) = &base.kind else {
            return Err(Diagnostic::unsupported(
                "only package-qualified selector calls are implemented",
                source,
            ));
        };
        if self.lookup_local(&package.name).is_some() {
            return Err(Diagnostic::unsupported(
                "method calls on local values are not implemented",
                source,
            ));
        }
        let key = (package.name.to_string(), member.name.to_string());
        let symbol = self.qualified_functions.get(&key).cloned().ok_or_else(|| {
            Diagnostic::semantic(
                format!("undefined package function {}.{}", key.0, key.1),
                source,
            )
        })?;
        self.lower_imported_function_call(
            symbol,
            arguments,
            spread,
            node,
            source,
            expected,
            allow_discarded_call_result,
        )
    }

    pub(super) fn lower_imported_selector(
        &self,
        base: &ExprSyntax,
        member: &IdentSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let ExprSyntaxKind::Ident(package) = &base.kind else {
            return Err(Diagnostic::unsupported(
                "only package-qualified selectors are implemented",
                source,
            ));
        };
        if self.lookup_local(&package.name).is_some() {
            return Err(Diagnostic::unsupported(
                "selectors on local values are not implemented",
                source,
            ));
        }
        let key = (package.name.to_string(), member.name.to_string());
        if let Some(constant) = self.qualified_constants.get(&key).cloned() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::GlobalConstant(constant.id, constant.value),
                ty: constant.ty,
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            });
        }
        if let Some(variable) = self.qualified_variables.get(&key).cloned() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::GlobalVariable(variable.id, variable.value),
                ty: variable.ty,
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_read: true,
                    ..hir::Effects::default()
                },
                source,
            });
        }
        if self.qualified_functions.contains_key(&key) {
            return Err(Diagnostic::unsupported(
                format!("function value {}.{} is not implemented", key.0, key.1),
                source,
            ));
        }
        Err(Diagnostic::semantic(
            format!("undefined package selector {}.{}", key.0, key.1),
            source,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_imported_function_call(
        &mut self,
        symbol: FunctionSymbol,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread && !symbol.signature.variadic {
            return Err(Diagnostic::semantic(
                "... is only valid when calling a variadic function",
                source,
            ));
        }
        if symbol.signature.variadic && !spread {
            return Err(Diagnostic::unsupported(
                "individual variadic arguments require slice-pack lowering",
                source,
            ));
        }
        if arguments.len() != symbol.signature.params.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "call has {} arguments; expected {}",
                    arguments.len(),
                    symbol.signature.params.len()
                ),
                source,
            ));
        }
        let args = arguments
            .iter()
            .zip(&symbol.signature.params)
            .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = match symbol.signature.results.as_slice() {
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
        let effects = args.iter().fold(
            hir::Effects {
                may_call: true,
                may_allocate: true,
                may_block: true,
                may_panic: true,
                may_write: true,
                ..hir::Effects::default()
            },
            |effects, argument| effects.union(argument.effects),
        );
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Function(symbol.id),
                args,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }
}
