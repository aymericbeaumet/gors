//! Typed lowering for slice and map range statements.

use std::collections::BTreeSet;

use super::FunctionLowerer;
use super::expressions::{coerce_expr, default_expr_type, is_assignable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::ClosureId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, FunctionBodySyntax, FunctionHeaderSyntax, IdentSyntax,
    SyntaxSource,
};
use crate::compiler::types::{ConstValue, IntTy, Signature, Ty};
use crate::token::Token;

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_range(
        &mut self,
        label: Option<&IdentSyntax>,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        token: Option<Token>,
        range_expression: &ExprSyntax,
        body: &BlockSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if let ExprSyntaxKind::Ident(identifier) = &range_expression.kind
            && self.lookup_local(&identifier.name).is_none()
            && self.lookup_closure(&identifier.name).is_none()
            && let Some(symbol) = self.functions.get(identifier.name.as_ref()).cloned()
            && let (Some(header), Some(iterator_body)) =
                (symbol.range_header.as_deref(), symbol.range_body.as_deref())
        {
            return self.lower_function_range(
                label,
                key,
                value,
                token,
                range_expression,
                body,
                header,
                iterator_body,
                &symbol.signature,
                source,
            );
        }
        let mut expression = self.lower_expr(range_expression, None)?;
        if expression.ty == Ty::Untyped(crate::compiler::types::UntypedTy::String) {
            expression = default_expr_type(expression, source)?;
        }
        let range_ty = expression.ty.clone();
        let (key_ty, value_ty, max_variables) = match range_ty.underlying() {
            Ty::Array(_, element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                (Ty::Int(IntTy::Int), element.as_ref().clone(), 2)
            }
            Ty::Slice(element)
                if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
            {
                (Ty::Int(IntTy::Int), element.as_ref().clone(), 2)
            }
            Ty::String => (Ty::Int(IntTy::Int), Ty::Int(IntTy::Int32), 2),
            Ty::Map(key, value)
                if key.underlying() == &Ty::String
                    && value.underlying() == &Ty::Int(IntTy::Int) =>
            {
                (key.as_ref().clone(), value.as_ref().clone(), 2)
            }
            Ty::Channel(direction, element)
                if direction.can_receive() && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                (element.as_ref().clone(), Ty::Unit, 1)
            }
            Ty::Untyped(crate::compiler::types::UntypedTy::Int) => {
                let iteration_ty = self.untyped_integer_range_ty(key, token, source)?;
                coerce_expr(&mut expression, &iteration_ty, source)?;
                (iteration_ty, Ty::Unit, 1)
            }
            Ty::Int(IntTy::Int | IntTy::Int32)
            | Ty::Uint(crate::compiler::types::UintTy::Uint8) => (range_ty.clone(), Ty::Unit, 1),
            ty => {
                return Err(Diagnostic::unsupported(
                    format!("range is not yet implemented for {ty:?}"),
                    source,
                ));
            }
        };
        if max_variables == 1 && value.is_some() {
            return Err(Diagnostic::semantic(
                "range expression permits at most one iteration variable",
                source,
            ));
        }
        let label = label.map(|label| label.name.to_string());
        if self.inside_local_closure && label.is_some() {
            return Err(Diagnostic::unsupported(
                "labeled loops in local function literals are not yet implemented",
                source,
            ));
        }
        if let Some(label) = &label
            && !self.declared_labels.insert(label.clone())
        {
            return Err(Diagnostic::semantic(
                format!("label {label} already defined"),
                source,
            ));
        }

        self.push_scope();
        let lowered = (|| {
            let (key, value) = match token {
                None if key.is_none() && value.is_none() => (None, None),
                Some(Token::DEFINE) => {
                    self.declare_range_bindings(key, value, &key_ty, &value_ty, source)?
                }
                Some(Token::ASSIGN) => {
                    self.resolve_range_bindings(key, value, &key_ty, &value_ty, source)?
                }
                _ => {
                    return Err(Diagnostic::semantic(
                        "invalid range assignment clause",
                        source,
                    ));
                }
            };
            self.loop_labels.push(label.clone());
            let assigned_in_body = super::iteration::assigned_names_in_block(body);
            let iteration_captures = [key, value]
                .into_iter()
                .flatten()
                .filter_map(|place| match place {
                    hir::Place::Local(local) => Some(local),
                    hir::Place::Discard => None,
                })
                .filter(|local| {
                    self.locals
                        .get(local.0 as usize)
                        .and_then(|local| local.name.as_ref())
                        .is_some_and(|name| !assigned_in_body.contains(name))
                })
                .collect();
            self.iteration_capture_scopes.push(iteration_captures);
            let body = self.lower_block(body, true);
            self.iteration_capture_scopes.pop();
            self.loop_labels.pop();
            Ok::<_, Diagnostic>((key, value, body?))
        })();
        self.pop_scope();
        let (key, value, body) = lowered?;
        Ok(hir::StmtKind::Range {
            label,
            key,
            value,
            expression,
            body,
        })
    }

    fn untyped_integer_range_ty(
        &self,
        key: Option<&ExprSyntax>,
        token: Option<Token>,
        source: SourceRef,
    ) -> Result<Ty, Diagnostic> {
        if token != Some(Token::ASSIGN) {
            return Ok(Ty::Int(IntTy::Int));
        }
        let Some(key) = key else {
            return Err(Diagnostic::semantic(
                "integer range assignment requires an iteration variable",
                source,
            ));
        };
        let place = self.lower_place(key, source)?;
        let hir::Place::Local(local) = place else {
            return Ok(Ty::Int(IntTy::Int));
        };
        let ty = self.place_ty(hir::Place::Local(local))?.clone();
        if !ty.is_integer() {
            return Err(Diagnostic::semantic(
                format!("integer range value is not assignable to {ty:?}"),
                source,
            ));
        }
        Ok(ty)
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_function_range(
        &mut self,
        label: Option<&IdentSyntax>,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        token: Option<Token>,
        range_expression: &ExprSyntax,
        body: &BlockSyntax,
        iterator_header: &FunctionHeaderSyntax,
        iterator_body: &FunctionBodySyntax,
        iterator_signature: &Signature,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if label.is_some() {
            return Err(Diagnostic::unsupported(
                "labeled range-over-function loops are not yet represented",
                source,
            ));
        }
        if iterator_signature.variadic || !iterator_signature.results.is_empty() {
            return Err(Diagnostic::semantic(
                "range iterator must accept one yield function and return no values",
                source,
            ));
        }
        let [yield_ty] = iterator_signature.params.as_slice() else {
            return Err(Diagnostic::semantic(
                "range iterator must accept exactly one yield function",
                source,
            ));
        };
        let Ty::Function(yield_signature) = yield_ty.underlying() else {
            return Err(Diagnostic::semantic(
                "range iterator parameter must be a yield function",
                source,
            ));
        };
        if yield_signature.variadic
            || yield_signature.params.len() > 2
            || yield_signature.results.as_slice() != [Ty::Bool]
        {
            return Err(Diagnostic::semantic(
                "range yield function must accept zero, one, or two values and return bool",
                source,
            ));
        }
        self.validate_function_range_clause(
            key,
            value,
            token,
            yield_signature.params.len(),
            source,
        )?;
        let iterator_block = iterator_body
            .block
            .as_ref()
            .ok_or_else(|| Diagnostic::unsupported("range iterator must have a Go body", source))?;
        let yield_name = iterator_parameter_name(iterator_header, source)?;

        self.push_scope();
        let lowered = (|| {
            let key_ty = yield_signature.params.first().cloned().unwrap_or(Ty::Unit);
            let value_ty = yield_signature.params.get(1).cloned().unwrap_or(Ty::Unit);
            let (key, value) = match token {
                None => (None, None),
                Some(Token::DEFINE) => {
                    self.declare_range_bindings(key, value, &key_ty, &value_ty, source)?
                }
                Some(Token::ASSIGN) => {
                    self.resolve_range_bindings(key, value, &key_ty, &value_ty, source)?
                }
                _ => {
                    return Err(Diagnostic::semantic(
                        "invalid range assignment clause",
                        source,
                    ));
                }
            };
            let yield_id = self.lower_range_yield_closure(
                (key, value).into(),
                body,
                yield_signature,
                range_expression.source,
                source,
            )?;
            let iterator_id = self.lower_range_iterator_closure(
                &yield_name,
                yield_id,
                iterator_block,
                range_expression.source,
                source,
            )?;
            self.function_range_block(yield_id, iterator_id, range_expression.source)
        })();
        self.pop_scope();
        lowered.map(hir::StmtKind::Block)
    }

    fn validate_function_range_clause(
        &self,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        token: Option<Token>,
        yielded: usize,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let variables = usize::from(key.is_some()) + usize::from(value.is_some());
        if variables > yielded {
            return Err(Diagnostic::semantic(
                format!(
                    "range clause declares {variables} variables but iterator yields {yielded} values"
                ),
                source,
            ));
        }
        if variables == 0 && token.is_some() || variables != 0 && token.is_none() {
            return Err(Diagnostic::semantic(
                "invalid range-over-function assignment clause",
                source,
            ));
        }
        Ok(())
    }

    fn lower_range_yield_closure(
        &mut self,
        bindings: [Option<hir::Place>; 2],
        body: &BlockSyntax,
        signature: &Signature,
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<ClosureId, Diagnostic> {
        let id = self.reserve_closure(signature, syntax_source)?;
        self.push_scope();
        let previous_signature = std::mem::replace(&mut self.signature, signature.clone());
        let previous_named_results = std::mem::take(&mut self.named_results);
        let previous_loops = std::mem::take(&mut self.loop_labels);
        let previous_labels = std::mem::take(&mut self.declared_labels);
        let previous_gotos = std::mem::take(&mut self.referenced_gotos);
        let previous_inside = self.inside_local_closure;
        let previous_boundary = self.range_yield_loop_depth;
        self.inside_local_closure = true;

        let lowered = (|| {
            let params = signature
                .params
                .iter()
                .map(|ty| {
                    self.alloc_local(None, ty.clone(), hir::LocalKind::Parameter, syntax_source)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut statements = Vec::new();
            let mut destinations = Vec::new();
            let mut values = Vec::new();
            for ((binding, parameter), ty) in
                bindings.into_iter().zip(&params).zip(&signature.params)
            {
                let Some(binding) = binding else {
                    continue;
                };
                destinations.push(binding);
                let node = self.alloc_node(syntax_source)?;
                values.push(self.local_expr(node, *parameter, ty.clone()));
            }
            if !destinations.is_empty() {
                let node = self.alloc_node(syntax_source)?;
                statements.push(hir::Stmt {
                    node,
                    kind: hir::StmtKind::Assign {
                        destinations,
                        op: hir::AssignOp::Set,
                        values,
                    },
                    source: SourceRef::node(node),
                });
            }

            self.loop_labels.push(None);
            self.range_yield_loop_depth = Some(self.loop_labels.len());
            let assigned_in_body = super::iteration::assigned_names_in_block(body);
            let captures = bindings
                .into_iter()
                .flatten()
                .filter_map(|place| match place {
                    hir::Place::Local(local) => Some(local),
                    hir::Place::Discard => None,
                })
                .filter(|local| {
                    self.locals
                        .get(local.0 as usize)
                        .and_then(|local| local.name.as_ref())
                        .is_some_and(|name| !assigned_in_body.contains(name))
                })
                .collect();
            self.iteration_capture_scopes.push(captures);
            let lowered_body = self.lower_block(body, true);
            self.iteration_capture_scopes.pop();
            self.range_yield_loop_depth = None;
            self.loop_labels.pop();
            let lowered_body = lowered_body?;
            statements.extend(lowered_body.stmts);
            statements.push(self.bool_return(true, syntax_source)?);
            let block_node = self.alloc_node(syntax_source)?;
            Ok::<_, Diagnostic>((
                params,
                hir::Block {
                    node: block_node,
                    stmts: statements,
                    source: SourceRef::node(block_node),
                },
            ))
        })();

        self.range_yield_loop_depth = previous_boundary;
        self.inside_local_closure = previous_inside;
        self.signature = previous_signature;
        self.named_results = previous_named_results;
        self.loop_labels = previous_loops;
        self.declared_labels = previous_labels;
        self.referenced_gotos = previous_gotos;
        self.pop_scope();

        let (params, body) = lowered?;
        let closure = self
            .closures
            .get_mut(id.index() as usize)
            .ok_or_else(|| Diagnostic::backend("reserved range yield closure disappeared"))?;
        *closure = hir::Closure {
            id,
            signature: signature.clone(),
            params,
            named_results: Vec::new(),
            body,
            source,
        };
        Ok(id)
    }

    fn lower_range_iterator_closure(
        &mut self,
        yield_name: &str,
        yield_id: ClosureId,
        body: &BlockSyntax,
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<ClosureId, Diagnostic> {
        let signature = Signature {
            params: Vec::new(),
            results: Vec::new(),
            variadic: false,
        };
        let id = self.reserve_closure(&signature, syntax_source)?;
        self.push_scope();
        let previous_signature = std::mem::replace(&mut self.signature, signature.clone());
        let previous_named_results = std::mem::take(&mut self.named_results);
        let previous_loops = std::mem::take(&mut self.loop_labels);
        let previous_labels = std::mem::take(&mut self.declared_labels);
        let previous_gotos = std::mem::take(&mut self.referenced_gotos);
        let previous_inside = self.inside_local_closure;
        let previous_override = self.source_override.replace(syntax_source);
        self.inside_local_closure = true;
        let lowered = (|| {
            self.bind_closure(yield_name, yield_id, source)?;
            self.lower_block(body, false)
        })();
        self.source_override = previous_override;
        self.inside_local_closure = previous_inside;
        self.signature = previous_signature;
        self.named_results = previous_named_results;
        self.loop_labels = previous_loops;
        self.declared_labels = previous_labels;
        self.referenced_gotos = previous_gotos;
        self.pop_scope();

        let body = lowered?;
        let closure = self
            .closures
            .get_mut(id.index() as usize)
            .ok_or_else(|| Diagnostic::backend("reserved range iterator closure disappeared"))?;
        *closure = hir::Closure {
            id,
            signature,
            params: Vec::new(),
            named_results: Vec::new(),
            body,
            source,
        };
        Ok(id)
    }

    fn reserve_closure(
        &mut self,
        signature: &Signature,
        syntax_source: SyntaxSource,
    ) -> Result<ClosureId, Diagnostic> {
        let id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );
        let node = self.alloc_node(syntax_source)?;
        self.closures.push(hir::Closure {
            id,
            signature: signature.clone(),
            params: Vec::new(),
            named_results: Vec::new(),
            body: hir::Block {
                node,
                stmts: Vec::new(),
                source: SourceRef::node(node),
            },
            source: SourceRef::node(node),
        });
        Ok(id)
    }

    fn bool_return(
        &mut self,
        value: bool,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Stmt, Diagnostic> {
        let value_node = self.alloc_node(syntax_source)?;
        let value = hir::Expr {
            node: value_node,
            kind: hir::ExprKind::Constant(ConstValue::Bool(value)),
            ty: Ty::Bool,
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(value_node),
        };
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Stmt {
            node,
            kind: hir::StmtKind::Return(vec![value]),
            source: SourceRef::node(node),
        })
    }

    fn function_range_block(
        &mut self,
        yield_id: ClosureId,
        iterator: ClosureId,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Block, Diagnostic> {
        let yield_binding_node = self.alloc_node(syntax_source)?;
        let iterator_binding_node = self.alloc_node(syntax_source)?;
        let call_node = self.alloc_node(syntax_source)?;
        let call = hir::Expr {
            node: call_node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Closure(iterator),
                args: Vec::new(),
            },
            ty: Ty::Unit,
            category: hir::ValueCategory::Value,
            effects: hir::Effects {
                may_read: true,
                may_write: true,
                may_call: true,
                may_allocate: true,
                may_block: true,
                may_panic: true,
            },
            source: SourceRef::node(call_node),
        };
        let call_statement_node = self.alloc_node(syntax_source)?;
        let block_node = self.alloc_node(syntax_source)?;
        Ok(hir::Block {
            node: block_node,
            stmts: vec![
                hir::Stmt {
                    node: yield_binding_node,
                    kind: hir::StmtKind::ClosureBinding(yield_id),
                    source: SourceRef::node(yield_binding_node),
                },
                hir::Stmt {
                    node: iterator_binding_node,
                    kind: hir::StmtKind::ClosureBinding(iterator),
                    source: SourceRef::node(iterator_binding_node),
                },
                hir::Stmt {
                    node: call_statement_node,
                    kind: hir::StmtKind::Expr(call),
                    source: SourceRef::node(call_statement_node),
                },
            ],
            source: SourceRef::node(block_node),
        })
    }

    fn declare_range_bindings(
        &mut self,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        key_ty: &Ty,
        value_ty: &Ty,
        source: SourceRef,
    ) -> Result<(Option<hir::Place>, Option<hir::Place>), Diagnostic> {
        let mut names = BTreeSet::new();
        let key = key
            .map(|target| self.declare_range_binding(target, key_ty, &mut names, source))
            .transpose()?;
        let value = value
            .map(|target| self.declare_range_binding(target, value_ty, &mut names, source))
            .transpose()?;
        if !matches!(key, Some(hir::Place::Local(_)))
            && !matches!(value, Some(hir::Place::Local(_)))
        {
            return Err(Diagnostic::semantic(
                "range short declaration introduces no variables",
                source,
            ));
        }
        Ok((key, value))
    }

    fn declare_range_binding(
        &mut self,
        target: &ExprSyntax,
        ty: &Ty,
        names: &mut BTreeSet<String>,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let ExprSyntaxKind::Ident(name) = &target.kind else {
            return Err(Diagnostic::semantic(
                "range short declaration target must be an identifier",
                source,
            ));
        };
        if name.name.as_ref() == "_" {
            return Ok(hir::Place::Discard);
        }
        if !names.insert(name.name.to_string()) {
            return Err(Diagnostic::semantic(
                format!("{} appears more than once on the left of :=", name.name),
                source,
            ));
        }
        let local = self.alloc_local(
            Some(name.name.to_string()),
            ty.clone(),
            hir::LocalKind::Variable,
            name.source,
        )?;
        Ok(hir::Place::Local(local))
    }

    fn resolve_range_bindings(
        &self,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        key_ty: &Ty,
        value_ty: &Ty,
        source: SourceRef,
    ) -> Result<(Option<hir::Place>, Option<hir::Place>), Diagnostic> {
        let key = key
            .map(|target| self.resolve_range_binding(target, key_ty, source))
            .transpose()?;
        let value = value
            .map(|target| self.resolve_range_binding(target, value_ty, source))
            .transpose()?;
        Ok((key, value))
    }

    fn resolve_range_binding(
        &self,
        target: &ExprSyntax,
        expected: &Ty,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let place = self.lower_place(target, source)?;
        if let hir::Place::Local(_) = place {
            let actual = self.place_ty(place)?;
            if !is_assignable(expected, actual) {
                return Err(Diagnostic::semantic(
                    format!("range value {expected:?} is not assignable to {actual:?}"),
                    source,
                ));
            }
        }
        Ok(place)
    }
}

fn iterator_parameter_name(
    header: &FunctionHeaderSyntax,
    source: SourceRef,
) -> Result<String, Diagnostic> {
    let [parameter] = header.params.fields.as_ref() else {
        return Err(Diagnostic::backend(
            "range iterator signature changed after type checking",
        ));
    };
    let Some(names) = &parameter.names else {
        return Err(Diagnostic::semantic(
            "range iterator yield parameter must be named",
            source,
        ));
    };
    let [name] = names.as_ref() else {
        return Err(Diagnostic::semantic(
            "range iterator must declare exactly one yield parameter",
            source,
        ));
    };
    Ok(name.name.to_string())
}
