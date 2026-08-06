//! Typed lowering for directly called, non-escaping function literals.

use super::expressions::coerce_expr;
use super::{FunctionLowerer, field_types, parameter_types};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::ClosureId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, IdentSyntax, SyntaxSource,
};
use crate::compiler::types::{Signature, Ty};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn try_lower_closure_binding(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        if let (
            [
                ExprSyntax {
                    kind: ExprSyntaxKind::Ident(name),
                    ..
                },
            ],
            [
                ExprSyntax {
                    kind: ExprSyntaxKind::Selector { base, member },
                    source: method_source,
                },
            ],
        ) = (left, right)
        {
            if name.name.as_ref() == "_" || self.is_local_struct_field(base, &member.name) {
                return None;
            }
            let is_non_value_identifier = matches!(&base.kind, ExprSyntaxKind::Ident(base) if self.lookup_local(&base.name).is_none());
            if !is_non_value_identifier {
                return Some(if token == Token::DEFINE {
                    self.lower_method_value_binding(name, base, member, *method_source, source)
                } else {
                    Err(Diagnostic::unsupported(
                        "method values currently require a non-escaping short declaration",
                        source,
                    ))
                });
            }
        }
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

    fn is_local_struct_field(&self, base: &ExprSyntax, member: &str) -> bool {
        let ExprSyntaxKind::Ident(base) = &base.kind else {
            return false;
        };
        let Some(local) = self.lookup_local(&base.name) else {
            return false;
        };
        let Some(ty) = self.locals.get(local.0 as usize).map(|local| &local.ty) else {
            return false;
        };
        let fields = match ty.underlying() {
            Ty::Struct(fields) => Some(fields.as_slice()),
            Ty::Pointer(element) => element.bootstrap_i64_struct_fields(),
            _ => None,
        };
        fields.is_some_and(|fields| fields.iter().any(|field| field.name == member))
    }

    fn lower_method_value_binding(
        &mut self,
        name: &IdentSyntax,
        base: &ExprSyntax,
        member: &IdentSyntax,
        method_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if self.inside_local_closure {
            return Err(Diagnostic::unsupported(
                "method values inside local function literals are not yet implemented",
                source,
            ));
        }
        if name.name.as_ref() == "_" {
            return Err(Diagnostic::semantic(
                "a method value binding cannot use the blank identifier",
                source,
            ));
        }

        let mut receiver = self.lower_expr(base, None)?;
        let symbol = self.resolve_method_symbol(&receiver.ty, &member.name, source)?;
        if symbol.pointer_receiver {
            return Err(Diagnostic::unsupported(
                "pointer-receiver method values are not yet represented",
                source,
            ));
        }
        let Some((receiver_ty, params)) = symbol.signature.params.split_first() else {
            return Err(Diagnostic::backend("method signature omitted its receiver"));
        };
        coerce_expr(&mut receiver, receiver_ty, source)?;
        let capture = self.alloc_local(
            None,
            receiver_ty.clone(),
            hir::LocalKind::Variable,
            base.source,
        )?;
        let closure_id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );
        let mut closure_params = Vec::with_capacity(params.len());
        for parameter_ty in params {
            closure_params.push(self.alloc_local(
                None,
                parameter_ty.clone(),
                hir::LocalKind::Parameter,
                member.source,
            )?);
        }

        let mut arguments = Vec::with_capacity(closure_params.len().saturating_add(1));
        let receiver_node = self.alloc_node(base.source)?;
        arguments.push(self.local_expr(receiver_node, capture, receiver_ty.clone()));
        for (parameter, parameter_ty) in closure_params.iter().zip(params) {
            let parameter_node = self.alloc_node(member.source)?;
            arguments.push(self.local_expr(parameter_node, *parameter, parameter_ty.clone()));
        }
        let result_ty = match symbol.signature.results.as_slice() {
            [] => Ty::Unit,
            [single] => single.clone(),
            many => Ty::Tuple(many.to_vec()),
        };
        let call_node = self.alloc_node(method_source)?;
        let call_source = SourceRef::node(call_node);
        let call = hir::Expr {
            node: call_node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Function(symbol.id),
                args: arguments,
            },
            ty: result_ty.clone(),
            category: hir::ValueCategory::Value,
            effects: hir::Effects {
                may_call: true,
                may_allocate: true,
                may_block: true,
                may_panic: true,
                may_write: true,
                may_read: true,
            },
            source: call_source,
        };
        let statement_node = self.alloc_node(method_source)?;
        let statement = hir::Stmt {
            node: statement_node,
            kind: if result_ty == Ty::Unit {
                hir::StmtKind::Expr(call)
            } else {
                hir::StmtKind::Return(vec![call])
            },
            source: SourceRef::node(statement_node),
        };
        let block_node = self.alloc_node(method_source)?;
        self.closures.push(hir::Closure {
            id: closure_id,
            signature: Signature {
                params: params.to_vec(),
                results: symbol.signature.results,
                variadic: symbol.signature.variadic,
            },
            params: closure_params,
            named_results: Vec::new(),
            body: hir::Block {
                node: block_node,
                stmts: vec![statement],
                source: SourceRef::node(block_node),
            },
            source: call_source,
        });
        self.bind_closure(&name.name, closure_id, source)?;
        Ok(hir::StmtKind::Let {
            destinations: vec![hir::Place::Local(capture)],
            values: vec![receiver],
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
