//! Typed lowering for directly called, non-escaping function literals.

use super::{FunctionLowerer, field_types, lower_type, parameter_types};
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
    pub(super) fn lower_snapshot_function_capture(
        &mut self,
        expression: &ExprSyntax,
        expected: &Ty,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let ExprSyntaxKind::FunctionLiteral {
            has_type_parameters,
            params,
            results,
            body,
        } = &expression.kind
        else {
            return Err(Diagnostic::semantic(
                "function-slice append requires a function literal",
                source,
            ));
        };
        if *has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic function literals are not implemented",
                source,
            ));
        }
        let (params, variadic) = parameter_types(params, &self.type_aliases, source)?;
        let results = results
            .as_ref()
            .map(|results| field_types(results, &self.type_aliases, source))
            .transpose()?
            .unwrap_or_default();
        let signature = Signature {
            params,
            results,
            variadic,
        };
        if expected != &Ty::Function(signature) {
            return Err(Diagnostic::semantic(
                "function literal does not match the slice element type",
                source,
            ));
        }
        let result_ty = expected.snapshot_function_result().ok_or_else(|| {
            Diagnostic::unsupported(
                "escaping function literals require a supported representation pattern",
                source,
            )
        })?;
        let [statement] = body.statements.as_ref() else {
            return Err(Diagnostic::unsupported(
                "escaping function literal must contain one return statement",
                source,
            ));
        };
        let crate::compiler::syntax::StmtSyntaxKind::Return(values) = &statement.kind else {
            return Err(Diagnostic::unsupported(
                "escaping function literal must contain one return statement",
                source,
            ));
        };
        let [value] = values.as_ref() else {
            return Err(Diagnostic::unsupported(
                "escaping function literal must return one captured value",
                source,
            ));
        };
        let ExprSyntaxKind::Ident(name) = &value.kind else {
            return Err(Diagnostic::unsupported(
                "escaping function literal must return one captured iteration variable",
                source,
            ));
        };
        let local = self.lookup_local(&name.name).ok_or_else(|| {
            Diagnostic::semantic(format!("undefined captured variable {}", name.name), source)
        })?;
        if !self
            .iteration_capture_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(&local))
        {
            return Err(Diagnostic::unsupported(
                "escaping closure capture is not a stable per-iteration variable",
                source,
            ));
        }
        self.lower_expr(value, Some(result_ty))
    }

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
            if !self.selector_base_refers_to_local(base)
                && let Ok(receiver_ty) = lower_type(base, &self.type_aliases, source)
                && self
                    .resolve_method_symbol(&receiver_ty, &member.name, source)
                    .is_ok()
            {
                return Some(if token == Token::DEFINE {
                    self.lower_method_expression_binding(
                        name,
                        receiver_ty,
                        member,
                        *method_source,
                        source,
                    )
                } else {
                    Err(Diagnostic::unsupported(
                        "method expressions currently require a non-escaping short declaration",
                        source,
                    ))
                });
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

    fn selector_base_refers_to_local(&self, base: &ExprSyntax) -> bool {
        match &base.kind {
            ExprSyntaxKind::Ident(base) => self.lookup_local(&base.name).is_some(),
            ExprSyntaxKind::Paren(base) => self.selector_base_refers_to_local(base),
            _ => false,
        }
    }

    fn lower_method_expression_binding(
        &mut self,
        name: &IdentSyntax,
        expression_receiver_ty: Ty,
        member: &IdentSyntax,
        method_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if self.inside_local_closure {
            return Err(Diagnostic::unsupported(
                "method expressions inside local function literals are not yet implemented",
                source,
            ));
        }
        let symbol = self.resolve_method_symbol(&expression_receiver_ty, &member.name, source)?;
        if symbol.pointer_receiver && !matches!(expression_receiver_ty.underlying(), Ty::Pointer(_))
        {
            return Err(Diagnostic::semantic(
                format!(
                    "type {expression_receiver_ty:?} has no method expression {}",
                    member.name
                ),
                source,
            ));
        }
        if symbol.signature.variadic {
            return Err(Diagnostic::unsupported(
                "variadic method expressions are not yet implemented",
                source,
            ));
        }
        let Some((method_receiver_ty, params)) = symbol.signature.params.split_first() else {
            return Err(Diagnostic::backend("method signature omitted its receiver"));
        };
        let mut closure_types = Vec::with_capacity(params.len().saturating_add(1));
        closure_types.push(expression_receiver_ty.clone());
        closure_types.extend_from_slice(params);
        let closure_id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );
        let closure_params = closure_types
            .iter()
            .map(|ty| self.alloc_local(None, ty.clone(), hir::LocalKind::Parameter, member.source))
            .collect::<Result<Vec<_>, _>>()?;
        let Some((receiver_parameter, argument_parameters)) = closure_params.split_first() else {
            return Err(Diagnostic::backend(
                "method expression closure omitted its receiver parameter",
            ));
        };

        let receiver_node = self.alloc_node(member.source)?;
        let receiver = self.local_expr(receiver_node, *receiver_parameter, expression_receiver_ty);
        let receiver = self.adjust_method_receiver(
            receiver,
            method_receiver_ty,
            symbol.pointer_receiver,
            member.source,
            source,
        )?;
        let mut arguments = Vec::with_capacity(closure_params.len());
        arguments.push(receiver);
        for (parameter, parameter_ty) in argument_parameters.iter().zip(params) {
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
            effects: method_call_effects(),
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
                params: closure_types,
                results: symbol.signature.results,
                variadic: false,
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
        Ok(hir::StmtKind::ClosureBinding(closure_id))
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

        let receiver = self.lower_expr(base, None)?;
        if matches!(receiver.ty.underlying(), Ty::Interface(_)) {
            return self.lower_interface_method_value_binding(
                name,
                receiver,
                member,
                method_source,
                source,
            );
        }
        let symbol = self.resolve_method_symbol(&receiver.ty, &member.name, source)?;
        let Some((receiver_ty, params)) = symbol.signature.params.split_first() else {
            return Err(Diagnostic::backend("method signature omitted its receiver"));
        };
        let receiver = self.adjust_method_receiver(
            receiver,
            receiver_ty,
            symbol.pointer_receiver,
            base.source,
            source,
        )?;
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
            effects: method_call_effects(),
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

    fn lower_interface_method_value_binding(
        &mut self,
        name: &IdentSyntax,
        receiver: hir::Expr,
        member: &IdentSyntax,
        method_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let signature =
            super::interfaces::interface_method_signature(&receiver.ty, &member.name, source)?;
        if signature.variadic {
            return Err(Diagnostic::unsupported(
                "variadic interface method values are not yet implemented",
                source,
            ));
        }
        let capture_ty = receiver.ty.clone();
        let capture = self.alloc_local(
            None,
            capture_ty.clone(),
            hir::LocalKind::Variable,
            method_source,
        )?;
        let closure_id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );
        let closure_params = signature
            .params
            .iter()
            .map(|parameter_ty| {
                self.alloc_local(
                    None,
                    parameter_ty.clone(),
                    hir::LocalKind::Parameter,
                    member.source,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let receiver_node = self.alloc_node(method_source)?;
        let captured_receiver = self.local_expr(receiver_node, capture, capture_ty);
        let mut arguments = Vec::with_capacity(closure_params.len());
        for (parameter, parameter_ty) in closure_params.iter().zip(&signature.params) {
            let parameter_node = self.alloc_node(member.source)?;
            arguments.push(self.local_expr(parameter_node, *parameter, parameter_ty.clone()));
        }
        let call_node = self.alloc_node(method_source)?;
        let call_source = SourceRef::node(call_node);
        let call = self.build_interface_call(
            captured_receiver,
            &member.name,
            arguments,
            call_node,
            call_source,
            None,
            true,
        )?;
        let result_ty = call.ty.clone();
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
            signature,
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

fn method_call_effects() -> hir::Effects {
    hir::Effects {
        may_call: true,
        may_allocate: true,
        may_block: true,
        may_panic: true,
        may_write: true,
        may_read: true,
    }
}
