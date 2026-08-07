//! Statement, declaration, assignment, and place lowering.

use crate::token::Token;

use super::FunctionLowerer;
use super::expressions::*;
use super::function::LocalConstantSymbol;
use super::iteration::assigned_names_in_block;
use super::parameter_types;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    DeclSyntax, ExprSyntax, ExprSyntaxKind, LocalTypeSyntax, StmtSyntax, StmtSyntaxKind,
    SwitchCaseSyntax, SyntaxSource, ValueSpecSyntax,
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
                    hir::ExprKind::Call { .. } | hir::ExprKind::InterfaceCall { .. }
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
                let op = match token {
                    Token::INC => hir::AssignOp::Add,
                    Token::DEC => hir::AssignOp::Sub,
                    _ => {
                        return Err(Diagnostic::semantic(
                            "invalid increment/decrement token",
                            source,
                        ));
                    }
                };
                if let ExprSyntaxKind::Index { base, index } = &expression.kind {
                    // The specification defines `x++` as the assignment
                    // `x += 1`, so an indexed operand reuses the compound
                    // element-assignment lowering with a constant `1` operand.
                    let one = ExprSyntax {
                        source: expression.source,
                        kind: ExprSyntaxKind::Literal {
                            token: Token::INT,
                            spelling: "1".into(),
                        },
                    };
                    let assign_token = if op == hir::AssignOp::Add {
                        Token::ADD_ASSIGN
                    } else {
                        Token::SUB_ASSIGN
                    };
                    self.lower_single_index_assignment(base, index, assign_token, &one, source)?
                } else {
                    let destination = self.lower_place(expression, source)?;
                    let ty = self.place_ty(destination)?.clone();
                    if !matches!(ty.underlying(), Ty::Int(_) | Ty::Uint(_)) {
                        return Err(Diagnostic::semantic(
                            "increment and decrement require an integer operand",
                            source,
                        ));
                    }
                    let one_node = self.alloc_node(expression.source)?;
                    let one = hir::Expr {
                        node: one_node,
                        kind: hir::ExprKind::Constant(ConstValue::Int("1".into())),
                        ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source: SourceRef::node(one_node),
                    };
                    hir::StmtKind::Assign {
                        destinations: vec![destination],
                        op,
                        values: vec![one],
                    }
                }
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
                if self.range_yield_loop_depth.is_some() {
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
                self.loop_labels.push(label.clone());
                self.iteration_capture_scopes.push(iteration_captures);
                let body = self.lower_block(body, true)?;
                self.iteration_capture_scopes.pop();
                let post = post
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                self.loop_labels.pop();
                self.pop_scope();
                hir::StmtKind::For {
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
                return self.lower_switch(stmt, init.as_deref(), tag.as_ref(), cases, None, source);
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
                    source,
                );
            }
            StmtSyntaxKind::Select { cases } => {
                return self.lower_select(node, cases, source);
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
                if let StmtSyntaxKind::Switch { init, tag, cases } = &statement.kind {
                    return self.lower_switch(
                        statement,
                        init.as_deref(),
                        tag.as_ref(),
                        cases,
                        Some(&label),
                        source,
                    );
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
                if label.is_none()
                    && self.range_yield_loop_depth == Some(self.loop_labels.len())
                    && matches!(token, Token::BREAK | Token::CONTINUE)
                {
                    let value_node = self.alloc_node(stmt.source)?;
                    let value = hir::Expr {
                        node: value_node,
                        kind: hir::ExprKind::Constant(ConstValue::Bool(*token == Token::CONTINUE)),
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
                let label = label.as_ref().map(|label| label.name.to_string());
                let target_exists = label.as_ref().map_or_else(
                    || !self.loop_labels.is_empty(),
                    |label| {
                        self.loop_labels
                            .iter()
                            .rev()
                            .any(|candidate| candidate.as_deref() == Some(label))
                    },
                );
                match token {
                    Token::BREAK if target_exists => hir::StmtKind::Break(label),
                    Token::CONTINUE if target_exists => hir::StmtKind::Continue(label),
                    Token::GOTO if label.is_some() => {
                        let Some(label) = label else {
                            return Err(Diagnostic::backend("goto label disappeared"));
                        };
                        self.referenced_gotos.entry(label.clone()).or_insert(source);
                        hir::StmtKind::Goto(label)
                    }
                    _ => {
                        return Err(Diagnostic::unsupported(
                            "branch does not target a supported enclosing for loop",
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
        statement: &StmtSyntax,
        init: Option<&StmtSyntax>,
        tag: Option<&ExprSyntax>,
        cases: &[SwitchCaseSyntax],
        redundant_break_label: Option<&str>,
        source: SourceRef,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        self.push_scope();
        let mut statements = Vec::new();
        if let Some(init) = init
            && let Some(init) = self.lower_stmt(init)?
        {
            statements.push(init);
        }

        let tag_local = if let Some(tag) = tag {
            let tag = default_expr_type(self.lower_expr(tag, None)?, source)?;
            ensure_bootstrap_value_type(&tag.ty, source)?;
            let local = self.alloc_local(
                None,
                tag.ty.clone(),
                hir::LocalKind::Temporary,
                statement.source,
            )?;
            let node = self.alloc_node(statement.source)?;
            statements.push(hir::Stmt {
                node,
                kind: hir::StmtKind::Let {
                    destinations: vec![hir::Place::Local(local)],
                    values: vec![tag],
                },
                source: SourceRef::node(node),
            });
            Some(local)
        } else {
            None
        };

        let mut default = None;
        let mut branches = Vec::new();
        let mut bodies = self.lower_switch_case_bodies(cases, redundant_break_label, source)?;
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
                let condition = if let Some(tag_local) = tag_local {
                    let tag_ty = self.place_ty(hir::Place::Local(tag_local))?.clone();
                    let right = self.lower_expr(expression, Some(&tag_ty))?;
                    let left_node = self.alloc_node(expression.source)?;
                    let left = self.local_expr(left_node, tag_local, tag_ty);
                    let node = self.alloc_node(expression.source)?;
                    hir::Expr {
                        node,
                        ty: Ty::Bool,
                        category: hir::ValueCategory::Value,
                        effects: left.effects.union(right.effects),
                        kind: hir::ExprKind::Binary {
                            op: hir::BinaryOp::Equal,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        source: SourceRef::node(node),
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
        let block_node = self.alloc_node(statement.source)?;
        Ok(Some(hir::Stmt {
            node: block_node,
            kind: hir::StmtKind::Block(hir::Block {
                node: block_node,
                stmts: statements,
                source: SourceRef::node(block_node),
            }),
            source: SourceRef::node(block_node),
        }))
    }

    fn lower_local_decl(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.token == Token::CONST {
            return self.lower_local_const_declaration(declaration, source);
        }
        if declaration.token == Token::TYPE {
            return self.lower_local_type_declaration(declaration, source);
        }
        if declaration.token != Token::VAR {
            return Err(Diagnostic::unsupported(
                "local import declarations are not implemented",
                source,
            ));
        }
        if declaration.contains_import_spec || !declaration.type_specs.is_empty() {
            return Err(Diagnostic::semantic(
                "value declaration contains a non-value specification",
                source,
            ));
        }
        let statements = declaration
            .specs
            .iter()
            .map(|spec| self.lower_value_spec(spec, declaration.source, source))
            .collect::<Result<Vec<_>, _>>()?;
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: statements,
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_const_declaration(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.contains_import_spec || !declaration.type_specs.is_empty() {
            return Err(Diagnostic::semantic(
                "constant declaration contains a non-value specification",
                source,
            ));
        }
        let mut previous_explicit_type: Option<ExprSyntax> = None;
        let mut previous_values: Option<std::sync::Arc<[ExprSyntax]>> = None;
        for spec in &*declaration.specs {
            let (explicit_type, values) = if let Some(values) = &spec.values {
                previous_explicit_type = spec.explicit_type.clone();
                previous_values = Some(values.clone());
                (spec.explicit_type.as_ref(), values.as_ref())
            } else {
                let values = previous_values.as_deref().ok_or_else(|| {
                    Diagnostic::semantic(
                        "first local constant specification requires an expression",
                        source,
                    )
                })?;
                (previous_explicit_type.as_ref(), values)
            };
            if spec.names.len() != values.len() {
                return Err(Diagnostic::semantic(
                    format!(
                        "constant declaration has {} names and {} values",
                        spec.names.len(),
                        values.len()
                    ),
                    source,
                ));
            }
            let declared_type = explicit_type
                .map(|ty| self.lower_scoped_type(ty, source))
                .transpose()?;
            let evaluated = values
                .iter()
                .map(|value| self.eval_constant_expression(value, source, spec.iota))
                .collect::<Result<Vec<_>, _>>()?;
            for (name, (raw_ty, mut value)) in spec.names.iter().zip(evaluated) {
                let ty = declared_type.clone().unwrap_or_else(|| raw_ty.clone());
                ensure_bootstrap_value_type(&ty.default_typed(), source)?;
                if !is_assignable(&raw_ty, &ty) {
                    return Err(Diagnostic::semantic(
                        format!("constant {} is not assignable to {ty:?}", name.name),
                        source,
                    ));
                }
                if !value.is_representable_as(&ty) {
                    return Err(Diagnostic::semantic(
                        format!("constant {} is not representable as {ty:?}", name.name),
                        source,
                    ));
                }
                value = value.normalized_for(&ty);
                let node = self.alloc_node(name.source)?;
                if name.name.as_ref() != "_" {
                    self.bind_local_constant(
                        name.name.to_string(),
                        LocalConstantSymbol { ty, value },
                        SourceRef::node(node),
                    )?;
                }
            }
        }
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: Vec::new(),
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_type_declaration(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.contains_import_spec || !declaration.specs.is_empty() {
            return Err(Diagnostic::semantic(
                "type declaration contains a non-type specification",
                source,
            ));
        }
        for spec in &*declaration.type_specs {
            self.lower_local_type_spec(spec, source)?;
        }
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: Vec::new(),
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_type_spec(
        &mut self,
        spec: &LocalTypeSyntax,
        declaration_source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if spec.has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic local type declarations are not yet implemented",
                declaration_source,
            ));
        }
        let target = self.lower_scoped_type(&spec.target, declaration_source)?;
        ensure_bootstrap_value_type(&target, declaration_source)?;
        let ty = if spec.alias {
            target
        } else {
            Ty::LocalNamed {
                identity: self.alloc_local_type_identity()?,
                underlying: Box::new(target.underlying().clone()),
            }
        };
        let node = self.alloc_node(spec.name.source)?;
        self.bind_local_type(spec.name.name.to_string(), ty, SourceRef::node(node))
    }

    fn lower_value_spec(
        &mut self,
        spec: &ValueSpecSyntax,
        declaration_syntax_source: SyntaxSource,
        declaration_source: SourceRef,
    ) -> Result<hir::Stmt, Diagnostic> {
        let explicit_ty = spec
            .explicit_type
            .as_ref()
            .map(|ty| self.lower_scoped_type(ty, declaration_source))
            .transpose()?;
        let raw_values = spec.values.as_deref().unwrap_or_default();
        if !raw_values.is_empty() && raw_values.len() != spec.names.len() {
            return self.lower_multi_value_spec(
                spec,
                explicit_ty,
                raw_values,
                declaration_syntax_source,
                declaration_source,
            );
        }
        // Go evaluates every RHS in one ValueSpec before any of that
        // spec's names enter scope. A previous ValueSpec in the same
        // declaration is already initialized and visible.
        let mut values = if raw_values.is_empty() {
            let ty = explicit_ty.clone().ok_or_else(|| {
                Diagnostic::semantic(
                    "declaration without initializer requires a type",
                    declaration_source,
                )
            })?;
            spec.names
                .iter()
                .map(|name| {
                    let node = self.alloc_node(name.source)?;
                    let source = SourceRef::node(node);
                    self.zero_value_expr(node, source, ty.clone())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?
        } else {
            raw_values
                .iter()
                .map(|value| self.lower_expr(value, explicit_ty.as_ref()))
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut destinations = Vec::with_capacity(spec.names.len());
        for (name, value) in spec.names.iter().zip(&mut values) {
            let ty = explicit_ty
                .clone()
                .unwrap_or_else(|| value.ty.default_typed());
            let value_source = value.source;
            ensure_bootstrap_value_type(&ty, value_source)?;
            coerce_expr(value, &ty, value_source)?;
            if name.name.as_ref() == "_" {
                destinations.push(hir::Place::Discard);
            } else {
                let id = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::Variable,
                    name.source,
                )?;
                destinations.push(hir::Place::Local(id));
            }
        }
        let node = self.alloc_node(declaration_syntax_source)?;
        Ok(hir::Stmt {
            node,
            kind: hir::StmtKind::Let {
                destinations,
                values,
            },
            source: SourceRef::node(node),
        })
    }

    fn lower_multi_value_spec(
        &mut self,
        spec: &ValueSpecSyntax,
        explicit_ty: Option<Ty>,
        raw_values: &[ExprSyntax],
        declaration_syntax_source: SyntaxSource,
        declaration_source: SourceRef,
    ) -> Result<hir::Stmt, Diagnostic> {
        let [raw_value] = raw_values else {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} values",
                    spec.names.len(),
                    raw_values.len()
                ),
                declaration_source,
            ));
        };
        // The complete RHS is resolved before any name from this ValueSpec is
        // installed in the lexical scope.
        let value = self.lower_multi_result_expression(raw_value)?;
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} values",
                    spec.names.len(),
                    raw_values.len()
                ),
                declaration_source,
            ));
        };
        let component_types = component_types.clone();
        if component_types.len() != spec.names.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} result values",
                    spec.names.len(),
                    component_types.len()
                ),
                declaration_source,
            ));
        }
        if !matches!(value.kind, hir::ExprKind::Call { .. }) {
            return Err(Diagnostic::backend(
                "tuple-valued non-call reached multi-valued variable declaration",
            ));
        }

        let mut destination_types = Vec::with_capacity(component_types.len());
        let mut coercions = Vec::with_capacity(component_types.len());
        for component_ty in &component_types {
            let destination_ty = explicit_ty
                .clone()
                .unwrap_or_else(|| component_ty.default_typed());
            ensure_bootstrap_value_type(&destination_ty, value.source)?;
            coercions.push(self.assignment_value_coercion(
                component_ty,
                &destination_ty,
                value.source,
            )?);
            destination_types.push(destination_ty);
        }

        let mut destinations = Vec::with_capacity(spec.names.len());
        for (name, ty) in spec.names.iter().zip(destination_types) {
            if name.name.as_ref() == "_" {
                destinations.push(hir::Place::Discard);
            } else {
                let local = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::Variable,
                    name.source,
                )?;
                destinations.push(hir::Place::Local(local));
            }
        }

        let node = self.alloc_node(declaration_syntax_source)?;
        Ok(hir::Stmt {
            node,
            kind: hir::StmtKind::LetTuple {
                destinations,
                value,
                coercions,
            },
            source: SourceRef::node(node),
        })
    }
}
