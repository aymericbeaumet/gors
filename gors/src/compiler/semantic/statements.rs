//! Statement and structured-control-flow lowering.

use crate::token::Token;

use super::FunctionLowerer;
use super::control_targets::ControlTargetKind;
use super::expressions::*;
use super::iteration::assigned_names_in_block;
use super::parameter_types;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ExprSyntax, StmtSyntax, StmtSyntaxKind, SwitchCaseSyntax, SyntaxSource,
};
use crate::compiler::types::{ConstValue, Ty};

impl FunctionLowerer {
    pub(super) fn lower_stmt(
        &mut self,
        stmt: &StmtSyntax,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        if matches!(&stmt.kind, StmtSyntaxKind::Empty) {
            return Ok(None);
        }
        let node = self.alloc_node(stmt.source)?;
        let source = SourceRef::node(node);
        let kind = match &stmt.kind {
            StmtSyntaxKind::Empty => return Ok(None),
            StmtSyntaxKind::Block(block) => hir::StmtKind::Block(self.lower_block(block, true)?),
            StmtSyntaxKind::Expr(expression) => {
                let expression = self.lower_expr_inner(expression, None, true)?;
                if !matches!(
                    expression.kind,
                    hir::ExprKind::Call { .. }
                        | hir::ExprKind::ForwardedCall { .. }
                        | hir::ExprKind::InterfaceCall { .. }
                ) {
                    return Err(Diagnostic::semantic(
                        "expression statement must be a call",
                        source,
                    ));
                }
                hir::StmtKind::Expr(expression)
            }
            StmtSyntaxKind::Decl(declaration) => self.lower_local_decl(declaration, source)?,
            StmtSyntaxKind::Assign { left, token, right } => {
                self.lower_assignment(left, *token, right, source)?
            }
            StmtSyntaxKind::IncDec { expression, token } => {
                self.lower_inc_dec(expression, *token, source)?
            }
            StmtSyntaxKind::Send { channel, value } => {
                self.lower_channel_send(channel, value, node, source)?
            }
            StmtSyntaxKind::Defer {
                has_type_parameters,
                params,
                results,
                body,
                arguments,
                spread,
            } => {
                if self.inside_deferred_closure || self.inside_local_closure {
                    return Err(Diagnostic::unsupported(
                        "defer statements in function literals are not yet implemented",
                        source,
                    ));
                }
                if self.defer_registration_depth != 0 {
                    return Err(Diagnostic::unsupported(
                        "defer statements in conditional, loop, or nested blocks are not yet implemented",
                        source,
                    ));
                }
                if *has_type_parameters {
                    return Err(Diagnostic::unsupported(
                        "generic deferred function literals are not implemented",
                        source,
                    ));
                }
                if results
                    .as_ref()
                    .is_some_and(|results| !results.fields.is_empty())
                {
                    return Err(Diagnostic::unsupported(
                        "result-bearing deferred function literals are not yet implemented",
                        source,
                    ));
                }
                let (parameter_types, variadic) =
                    parameter_types(params, &self.type_aliases, source)?;
                if variadic || *spread {
                    return Err(Diagnostic::unsupported(
                        "variadic deferred function literals are not yet implemented",
                        source,
                    ));
                }
                if parameter_types.len() != arguments.len() {
                    return Err(Diagnostic::semantic(
                        format!(
                            "deferred call has {} arguments; function requires {}",
                            arguments.len(),
                            parameter_types.len()
                        ),
                        source,
                    ));
                }
                let values = arguments
                    .iter()
                    .zip(&parameter_types)
                    .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
                    .collect::<Result<Vec<_>, _>>()?;

                self.push_scope();
                let parameters = self.declare_field_bindings(
                    params,
                    &parameter_types,
                    hir::LocalKind::Temporary,
                )?;
                let previous_inside = self.inside_deferred_closure;
                self.inside_deferred_closure = true;
                let body = self.lower_block(body, false);
                self.inside_deferred_closure = previous_inside;
                self.pop_scope();
                hir::StmtKind::Defer {
                    parameters,
                    values,
                    body: body?,
                }
            }
            StmtSyntaxKind::Go {
                has_type_parameters,
                params,
                results,
                body,
                arguments,
                spread,
            } => self.lower_empty_goroutine(
                *has_type_parameters,
                params,
                results.as_ref(),
                body,
                arguments,
                *spread,
                source,
            )?,
            StmtSyntaxKind::Return(results) => {
                if self.range_yield_target.is_some() {
                    return Err(Diagnostic::unsupported(
                        "return from a range-over-function body is not yet represented",
                        source,
                    ));
                }
                if self.inside_deferred_closure {
                    return Err(Diagnostic::unsupported(
                        "return statements in deferred function literals are not yet implemented",
                        source,
                    ));
                }
                let values = if results.is_empty() && !self.named_results.is_empty() {
                    let named_results = self.named_results.clone();
                    let mut values = Vec::new();
                    for (index, result) in named_results.into_iter().enumerate() {
                        let Some(local) = result else {
                            return Err(Diagnostic::semantic(
                                "bare return requires every result to be named",
                                source,
                            ));
                        };
                        let ty = self.signature.results.get(index).cloned().ok_or_else(|| {
                            Diagnostic::backend(format!(
                                "named result {index} has no signature type"
                            ))
                        })?;
                        let value_node = self.alloc_node(stmt.source)?;
                        values.push(self.local_expr(value_node, local, ty));
                    }
                    values
                } else if let [result] = &**results {
                    if let [expected] = self.signature.results.as_slice() {
                        vec![self.lower_expr(result, Some(&expected.clone()))?]
                    } else {
                        let value = self.lower_expr(result, None)?;
                        match &value.ty {
                            Ty::Tuple(types) if types == &self.signature.results => vec![value],
                            _ => {
                                return Err(Diagnostic::semantic(
                                    format!(
                                        "return has 1 value; function requires {}",
                                        self.signature.results.len()
                                    ),
                                    source,
                                ));
                            }
                        }
                    }
                } else {
                    if results.len() != self.signature.results.len() {
                        return Err(Diagnostic::semantic(
                            format!(
                                "return has {} values; function requires {}",
                                results.len(),
                                self.signature.results.len()
                            ),
                            source,
                        ));
                    }
                    let result_types = self.signature.results.clone();
                    results
                        .iter()
                        .zip(result_types)
                        .map(|(expression, expected)| self.lower_expr(expression, Some(&expected)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                hir::StmtKind::Return(values)
            }
            StmtSyntaxKind::If {
                init,
                condition,
                then_block,
                else_branch,
            } => self.lower_if_statement(
                init.as_deref(),
                condition,
                then_block,
                else_branch.as_deref(),
                source,
            )?,
            StmtSyntaxKind::For {
                label,
                init,
                condition,
                post,
                body,
            } => {
                let label = label.as_ref().map(|label| label.name.to_string());
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
                let iteration_local_start = self.locals.len();
                let init = init
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                let assigned_in_body = assigned_names_in_block(body);
                let iteration_captures = self
                    .locals
                    .get(iteration_local_start..)
                    .ok_or_else(|| Diagnostic::backend("iteration local boundary moved"))?
                    .iter()
                    .filter(|local| {
                        local
                            .name
                            .as_ref()
                            .is_some_and(|name| !assigned_in_body.contains(name))
                    })
                    .map(|local| local.id)
                    .collect();
                let condition = condition
                    .as_ref()
                    .map(|expression| self.lower_expr(expression, Some(&Ty::Bool)))
                    .transpose()?;
                let target = self.begin_control_target(ControlTargetKind::Loop, label.clone())?;
                self.iteration_capture_scopes.push(iteration_captures);
                let body = self.lower_block(body, true)?;
                self.iteration_capture_scopes.pop();
                self.end_control_target(target)?;
                let post = post
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                self.pop_scope();
                hir::StmtKind::For {
                    target,
                    label,
                    init,
                    condition,
                    post,
                    body,
                }
            }
            StmtSyntaxKind::Range {
                label,
                key,
                value,
                token,
                expression,
                body,
            } => self.lower_range(
                label.as_ref(),
                key.as_ref(),
                value.as_ref(),
                *token,
                expression,
                body,
                source,
            )?,
            StmtSyntaxKind::Switch { init, tag, cases } => {
                return self.lower_switch(
                    node,
                    stmt.source,
                    init.as_deref(),
                    tag.as_ref(),
                    cases,
                    None,
                );
            }
            StmtSyntaxKind::TypeSwitch {
                init,
                binding,
                expression,
                cases,
            } => {
                return self.lower_type_switch(
                    node,
                    init.as_deref(),
                    binding.as_ref(),
                    expression,
                    cases,
                    stmt.source,
                    None,
                    source,
                );
            }
            StmtSyntaxKind::Select { cases } => {
                return self.lower_select(node, cases, None, stmt.source, source);
            }
            StmtSyntaxKind::Labeled { label, statement } => {
                if self.inside_deferred_closure || self.inside_local_closure {
                    return Err(Diagnostic::unsupported(
                        "labels in function literals are not yet implemented",
                        source,
                    ));
                }
                let label = label.name.to_string();
                if !self.declared_labels.insert(label.clone()) {
                    return Err(Diagnostic::semantic(
                        format!("label {label} already defined"),
                        source,
                    ));
                }
                match &statement.kind {
                    StmtSyntaxKind::Switch { init, tag, cases } => {
                        return self.lower_switch(
                            node,
                            statement.source,
                            init.as_deref(),
                            tag.as_ref(),
                            cases,
                            Some(&label),
                        );
                    }
                    StmtSyntaxKind::TypeSwitch {
                        init,
                        binding,
                        expression,
                        cases,
                    } => {
                        return self.lower_type_switch(
                            node,
                            init.as_deref(),
                            binding.as_ref(),
                            expression,
                            cases,
                            statement.source,
                            Some(&label),
                            source,
                        );
                    }
                    StmtSyntaxKind::Select { cases } => {
                        return self.lower_select(
                            node,
                            cases,
                            Some(&label),
                            statement.source,
                            source,
                        );
                    }
                    _ => {}
                }
                hir::StmtKind::Label {
                    name: label,
                    statement: self.lower_stmt(statement)?.map(Box::new),
                }
            }
            StmtSyntaxKind::Branch { token, label } => {
                if self.inside_deferred_closure
                    || self.inside_local_closure && (*token == Token::GOTO || label.is_some())
                {
                    return Err(Diagnostic::unsupported(
                        "labeled branches in function literals are not yet implemented",
                        source,
                    ));
                }
                let label = label.as_ref().map(|label| label.name.to_string());
                match token {
                    Token::GOTO if label.is_some() => {
                        let Some(label) = label else {
                            return Err(Diagnostic::backend("goto label disappeared"));
                        };
                        self.referenced_gotos.entry(label.clone()).or_insert(source);
                        hir::StmtKind::Goto(label)
                    }
                    Token::BREAK | Token::CONTINUE => {
                        let target =
                            self.resolve_control_target(*token, label.as_deref(), source)?;
                        if self.range_yield_target == Some(target) {
                            let value_node = self.alloc_node(stmt.source)?;
                            let value = hir::Expr {
                                node: value_node,
                                kind: hir::ExprKind::Constant(ConstValue::Bool(
                                    *token == Token::CONTINUE,
                                )),
                                ty: Ty::Bool,
                                category: hir::ValueCategory::Constant,
                                effects: hir::Effects::default(),
                                source: SourceRef::node(value_node),
                            };
                            return Ok(Some(hir::Stmt {
                                node,
                                kind: hir::StmtKind::Return(vec![value]),
                                source,
                            }));
                        }
                        if *token == Token::BREAK {
                            hir::StmtKind::Break(target)
                        } else {
                            hir::StmtKind::Continue(target)
                        }
                    }
                    _ => {
                        return Err(Diagnostic::semantic(
                            "branch is not valid in this statement context",
                            source,
                        ));
                    }
                }
            }
            StmtSyntaxKind::Unsupported(description) => {
                return Err(Diagnostic::unsupported(
                    format!("statement {description} is not yet supported"),
                    source,
                ));
            }
        };
        Ok(Some(hir::Stmt { node, kind, source }))
    }

    fn lower_switch(
        &mut self,
        node: crate::compiler::ids::NodeId,
        syntax_source: SyntaxSource,
        init: Option<&StmtSyntax>,
        tag: Option<&ExprSyntax>,
        cases: &[SwitchCaseSyntax],
        label: Option<&str>,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        let source = SourceRef::node(node);
        self.push_scope();
        let mut statements = Vec::new();
        if let Some(init) = init
            && let Some(init) = self.lower_stmt(init)?
        {
            statements.push(init);
        }

        let tag_local = if let Some(tag) = tag {
            let tag_source = tag.source;
            let tag = default_expr_type(self.lower_expr(tag, None)?, source)?;
            ensure_bootstrap_value_type(&tag.ty, source)?;
            let local = self.alloc_local(
                None,
                tag.ty.clone(),
                hir::LocalKind::Temporary,
                syntax_source,
            )?;
            let node = self.alloc_node(syntax_source)?;
            statements.push(hir::Stmt {
                node,
                kind: hir::StmtKind::Let {
                    destinations: vec![hir::Place::Local(local)],
                    values: vec![tag],
                },
                source: SourceRef::node(node),
            });
            Some((local, tag_source))
        } else {
            None
        };

        let mut default = None;
        let mut branches = Vec::new();
        let target =
            self.begin_control_target(ControlTargetKind::BreakOnly, label.map(str::to_owned))?;
        let bodies = self.lower_switch_case_bodies(cases, source);
        self.end_control_target(target)?;
        let mut bodies = bodies?;
        for case in cases {
            let body = bodies.remove(0);
            if case.expressions.is_empty() {
                if default.replace((case.source, body)).is_some() {
                    self.pop_scope();
                    return Err(Diagnostic::semantic(
                        "expression switch has multiple default cases",
                        source,
                    ));
                }
                continue;
            }
            let mut conditions = Vec::new();
            for expression in &*case.expressions {
                let condition = if let Some((tag_local, tag_source)) = tag_local {
                    let tag_ty = self.place_ty(hir::Place::Local(tag_local))?.clone();
                    let mut right = if super::expression_lower::is_nil_identifier(expression) {
                        self.lower_expr(expression, Some(&tag_ty))?
                    } else {
                        self.lower_expr(expression, None)?
                    };
                    if matches!(right.ty, Ty::Untyped(_)) {
                        if matches!(tag_ty.underlying(), Ty::Interface(_)) {
                            right =
                                self.coerce_interface_value(right, &tag_ty, expression.source)?;
                        } else {
                            let right_source = right.source;
                            coerce_expr(&mut right, &tag_ty, right_source)?;
                        }
                    }
                    let left_node = self.alloc_node(expression.source)?;
                    let left = self.local_expr(left_node, tag_local, tag_ty);
                    let node = self.alloc_node(expression.source)?;
                    let comparison_source = SourceRef::node(node);
                    if matches!(left.ty.underlying(), Ty::Interface(_))
                        || matches!(right.ty.underlying(), Ty::Interface(_))
                    {
                        self.lower_interface_comparison(
                            left,
                            tag_source,
                            right,
                            expression.source,
                            true,
                            node,
                            comparison_source,
                            None,
                        )?
                    } else if super::pointers::pointer_comparison_builtin(&left.ty).is_some()
                        || super::pointers::pointer_comparison_builtin(&right.ty).is_some()
                    {
                        self.lower_pointer_comparison(
                            left,
                            right,
                            true,
                            node,
                            comparison_source,
                            None,
                        )?
                    } else {
                        super::expression_lower::lower_regular_binary_expression(
                            hir::BinaryOp::Equal,
                            left,
                            right,
                            node,
                            comparison_source,
                        )?
                    }
                } else {
                    self.lower_expr(expression, Some(&Ty::Bool))?
                };
                conditions.push(condition);
            }
            let mut conditions = conditions.into_iter();
            let mut condition = conditions
                .next()
                .ok_or_else(|| Diagnostic::backend("switch case lost its expressions"))?;
            for right in conditions {
                let node = self.alloc_node(case.source)?;
                condition = hir::Expr {
                    node,
                    ty: Ty::Bool,
                    category: hir::ValueCategory::Value,
                    effects: condition.effects.union(right.effects),
                    kind: hir::ExprKind::Binary {
                        op: hir::BinaryOp::LogicalOr,
                        left: Box::new(condition),
                        right: Box::new(right),
                    },
                    source: SourceRef::node(node),
                };
            }
            branches.push((case.source, condition, body));
        }

        let mut tail = if let Some((case_source, block)) = default {
            let node = self.alloc_node(case_source)?;
            Some(Box::new(hir::Stmt {
                node,
                source: SourceRef::node(node),
                kind: hir::StmtKind::Block(block),
            }))
        } else {
            None
        };
        for (case_source, condition, then_block) in branches.into_iter().rev() {
            let node = self.alloc_node(case_source)?;
            tail = Some(Box::new(hir::Stmt {
                node,
                kind: hir::StmtKind::If {
                    init: None,
                    condition,
                    then_block,
                    else_branch: tail,
                },
                source: SourceRef::node(node),
            }));
        }
        if let Some(tail) = tail {
            statements.push(*tail);
        }
        self.pop_scope();
        let block_node = self.alloc_node(syntax_source)?;
        let block = hir::Block {
            node: block_node,
            stmts: statements,
            source: SourceRef::node(block_node),
        };
        let (breakable_node, breakable_source) = if label.is_some() {
            let breakable_node = self.alloc_node(syntax_source)?;
            (breakable_node, SourceRef::node(breakable_node))
        } else {
            (node, source)
        };
        let breakable = hir::Stmt {
            node: breakable_node,
            kind: hir::StmtKind::Breakable {
                target,
                body: block,
            },
            source: breakable_source,
        };
        Ok(Some(if let Some(label) = label {
            hir::Stmt {
                node,
                kind: hir::StmtKind::Label {
                    name: label.to_owned(),
                    statement: Some(Box::new(breakable)),
                },
                source,
            }
        } else {
            breakable
        }))
    }
}
