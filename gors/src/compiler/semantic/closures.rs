//! Typed lowering for directly called, non-escaping function literals.

use super::{FunctionLowerer, field_types, parameter_types};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::ClosureId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, IdentSyntax, SyntaxSource,
};
use crate::compiler::types::Signature;
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn try_lower_closure_binding(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        let (
            [
                ExprSyntax {
                    kind: ExprSyntaxKind::Ident(name),
                    ..
                },
            ],
            [
                ExprSyntax {
                    kind:
                        ExprSyntaxKind::FunctionLiteral {
                            has_type_parameters,
                            params,
                            results,
                            body,
                        },
                    source: closure_source,
                },
            ],
        ) = (left, right)
        else {
            return None;
        };
        Some(if token == Token::DEFINE {
            self.lower_closure_binding(
                name,
                *has_type_parameters,
                params,
                results.as_ref(),
                body,
                *closure_source,
                source,
            )
        } else {
            Err(Diagnostic::unsupported(
                "function literals currently require a non-escaping short declaration",
                source,
            ))
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_closure_binding(
        &mut self,
        name: &IdentSyntax,
        has_type_parameters: bool,
        params: &FieldListSyntax,
        results: Option<&FieldListSyntax>,
        body: &BlockSyntax,
        closure_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if self.inside_local_closure {
            return Err(Diagnostic::unsupported(
                "nested local function literals are not yet implemented",
                source,
            ));
        }
        if name.name.as_ref() == "_" {
            return Err(Diagnostic::semantic(
                "a function literal binding cannot use the blank identifier",
                source,
            ));
        }
        if has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic function literals are not implemented",
                source,
            ));
        }
        let (parameter_types, variadic) = parameter_types(params, &self.type_aliases, source)?;
        if variadic {
            return Err(Diagnostic::unsupported(
                "variadic local function literals are not yet implemented",
                source,
            ));
        }
        let result_types = results
            .map(|results| field_types(results, &self.type_aliases, source))
            .transpose()?
            .unwrap_or_default();
        let signature = Signature {
            params: parameter_types.clone(),
            results: result_types.clone(),
            variadic: false,
        };
        let id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );

        self.push_scope();
        let previous_signature = std::mem::replace(&mut self.signature, signature.clone());
        let previous_named_results = std::mem::take(&mut self.named_results);
        let previous_loops = std::mem::take(&mut self.loop_labels);
        let previous_labels = std::mem::take(&mut self.declared_labels);
        let previous_gotos = std::mem::take(&mut self.referenced_gotos);
        let previous_inside = self.inside_local_closure;
        self.inside_local_closure = true;

        let lowered = (|| {
            let params =
                self.declare_field_bindings(params, &parameter_types, hir::LocalKind::Parameter)?;
            let named_results = results.map_or_else(
                || Ok(Vec::new()),
                |results| self.declare_result_bindings(results, &result_types),
            )?;
            self.named_results = named_results.clone();
            let body = self.lower_block(body, false)?;
            Ok::<_, Diagnostic>((params, named_results, body))
        })();

        self.inside_local_closure = previous_inside;
        self.signature = previous_signature;
        self.named_results = previous_named_results;
        self.loop_labels = previous_loops;
        self.declared_labels = previous_labels;
        self.referenced_gotos = previous_gotos;
        self.pop_scope();

        let (params, named_results, body) = lowered?;
        let closure_node = self.alloc_node(closure_source)?;
        self.closures.push(hir::Closure {
            id,
            signature,
            params,
            named_results,
            body,
            source: SourceRef::node(closure_node),
        });
        self.bind_closure(&name.name, id, source)?;
        Ok(hir::StmtKind::ClosureBinding(id))
    }
}
