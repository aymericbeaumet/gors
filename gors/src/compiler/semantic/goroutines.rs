//! Typed lowering for goroutine calls with proven-empty bodies.

use super::{FunctionLowerer, field_types, parameter_types};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, ExprSyntax, FieldListSyntax, StmtSyntaxKind};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_empty_goroutine(
        &mut self,
        has_type_parameters: bool,
        params: &FieldListSyntax,
        results: Option<&FieldListSyntax>,
        body: &BlockSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic goroutine function literals are not yet implemented",
                source,
            ));
        }
        let result_types = results
            .map(|results| field_types(results, &self.type_aliases, source))
            .transpose()?
            .unwrap_or_default();
        if !result_types.is_empty() {
            return Err(Diagnostic::unsupported(
                "result-bearing goroutine function literals are not yet implemented",
                source,
            ));
        }
        if body
            .statements
            .iter()
            .any(|statement| !matches!(statement.kind, StmtSyntaxKind::Empty))
        {
            return Err(Diagnostic::unsupported(
                "goroutine bodies require the typed scheduler closure ABI",
                source,
            ));
        }

        let (parameter_types, variadic) = parameter_types(params, &self.type_aliases, source)?;
        if variadic || spread {
            return Err(Diagnostic::unsupported(
                "variadic goroutine function literals are not yet implemented",
                source,
            ));
        }
        let values = self
            .lower_call_arguments(
                arguments,
                &parameter_types,
                false,
                false,
                body.source,
                source,
                "goroutine",
            )?
            .into_explicit(source)?;

        self.push_scope();
        let parameters =
            self.declare_field_bindings(params, &parameter_types, hir::LocalKind::Parameter)?;
        let body = self.lower_block(body, false);
        self.pop_scope();
        Ok(hir::StmtKind::Go {
            parameters,
            values,
            body: body?,
        })
    }
}
