//! Statement, declaration, assignment, and place lowering.

use std::collections::BTreeSet;

use crate::token::Token;

use super::FunctionLowerer;
use super::expressions::*;
use super::lower_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    DeclSyntax, ExprSyntax, ExprSyntaxKind, StmtSyntax, StmtSyntaxKind, SwitchCaseSyntax,
    SyntaxSource, ValueSpecSyntax,
};
use crate::compiler::types::{ConstValue, IntTy, Ty};

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
                if !matches!(expression.kind, hir::ExprKind::Call { .. }) {
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
                let destination = self.lower_place(expression, source)?;
                let ty = self.place_ty(destination)?.clone();
                if *ty.underlying() != Ty::Int(IntTy::Int) {
                    return Err(Diagnostic::semantic(
                        "increment and decrement require an int operand in the bootstrap backend",
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
                    op: match token {
                        Token::INC => hir::AssignOp::Add,
                        Token::DEC => hir::AssignOp::Sub,
                        _ => {
                            return Err(Diagnostic::semantic(
                                "invalid increment/decrement token",
                                source,
                            ));
                        }
                    },
                    values: vec![one],
                }
            }
            StmtSyntaxKind::Return(results) => {
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
            } => {
                self.push_scope();
                let init = init
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                let condition = self.lower_expr(condition, Some(&Ty::Bool))?;
                let then_block = self.lower_block(then_block, true)?;
                let else_branch = else_branch
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                self.pop_scope();
                hir::StmtKind::If {
                    init,
                    condition,
                    then_block,
                    else_branch,
                }
            }
            StmtSyntaxKind::For {
                label,
                init,
                condition,
                post,
                body,
            } => {
                let label = label.as_ref().map(|label| label.name.to_string());
                if let Some(label) = &label
                    && !self.declared_labels.insert(label.clone())
                {
                    return Err(Diagnostic::semantic(
                        format!("label {label} already defined"),
                        source,
                    ));
                }
                self.push_scope();
                let init = init
                    .as_deref()
                    .map(|statement| self.lower_stmt(statement))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                let condition = condition
                    .as_ref()
                    .map(|expression| self.lower_expr(expression, Some(&Ty::Bool)))
                    .transpose()?;
                self.loop_labels.push(label.clone());
                let body = self.lower_block(body, true)?;
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
            StmtSyntaxKind::Switch { init, tag, cases } => {
                return self.lower_switch(stmt, init.as_deref(), tag.as_ref(), cases, source);
            }
            StmtSyntaxKind::Labeled { label, statement } => {
                let label = label.name.to_string();
                if !self.declared_labels.insert(label.clone()) {
                    return Err(Diagnostic::semantic(
                        format!("label {label} already defined"),
                        source,
                    ));
                }
                hir::StmtKind::Label {
                    name: label,
                    statement: self.lower_stmt(statement)?.map(Box::new),
                }
            }
            StmtSyntaxKind::Branch { token, label } => {
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
                    format!("statement {description} is not implemented by the HIR/MIR backend"),
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
        for case in cases {
            let body = self.lower_block(&case.body, true)?;
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
            return Err(Diagnostic::unsupported(
                "local const declarations require immutable HIR bindings and are not implemented",
                source,
            ));
        }
        if declaration.token != Token::VAR {
            return Err(Diagnostic::unsupported(
                "local type and import declarations are not implemented",
                source,
            ));
        }
        if declaration.contains_non_value_spec {
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

    fn lower_value_spec(
        &mut self,
        spec: &ValueSpecSyntax,
        declaration_syntax_source: SyntaxSource,
        declaration_source: SourceRef,
    ) -> Result<hir::Stmt, Diagnostic> {
        let explicit_ty = spec
            .explicit_type
            .as_ref()
            .map(|ty| lower_type(ty, &self.type_aliases, declaration_source))
            .transpose()?;
        let raw_values = spec.values.as_deref().unwrap_or_default();
        if !raw_values.is_empty() && raw_values.len() != spec.names.len() {
            return Err(Diagnostic::unsupported(
                "multi-valued declarations are not implemented",
                declaration_source,
            ));
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
                    let value = ty.zero().ok_or_else(|| {
                        Diagnostic::unsupported(
                            format!("zero value for {ty:?} is not implemented"),
                            source,
                        )
                    })?;
                    Ok(hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: ty.clone(),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    })
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

    fn lower_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if left.len() != right.len() {
            return Err(Diagnostic::unsupported(
                "multi-result assignment is not implemented by the HIR/MIR backend",
                source,
            ));
        }
        if token == Token::DEFINE {
            let mut names = BTreeSet::new();
            for expression in left {
                let ExprSyntaxKind::Ident(name) = &expression.kind else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        source,
                    ));
                };
                if name.name.as_ref() != "_" && !names.insert(name.name.as_ref()) {
                    return Err(Diagnostic::semantic(
                        format!("{} appears more than once on the left of :=", name.name),
                        source,
                    ));
                }
            }
            // Go resolves all RHS expressions before adding the new bindings.
            let mut values = right
                .iter()
                .map(|expression| self.lower_expr(expression, None))
                .collect::<Result<Vec<_>, _>>()?;
            let mut destinations = Vec::new();
            let mut introduced = false;
            for (expression, value) in left.iter().zip(&mut values) {
                let ExprSyntaxKind::Ident(name) = &expression.kind else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        source,
                    ));
                };
                if name.name.as_ref() == "_" {
                    destinations.push(hir::Place::Discard);
                    continue;
                }
                if let Some(local) = self.lookup_current_local(&name.name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    let value_source = value.source;
                    coerce_expr(value, &ty, value_source)?;
                    destinations.push(hir::Place::Local(local));
                } else {
                    introduced = true;
                    let ty = value.ty.default_typed();
                    let value_source = value.source;
                    ensure_bootstrap_value_type(&ty, value_source)?;
                    coerce_expr(value, &ty, value_source)?;
                    let local = self.alloc_local(
                        Some(name.name.to_string()),
                        ty,
                        hir::LocalKind::Variable,
                        name.source,
                    )?;
                    destinations.push(hir::Place::Local(local));
                }
            }
            if !introduced {
                return Err(Diagnostic::semantic(
                    "short declaration introduces no new variables",
                    source,
                ));
            }
            return Ok(hir::StmtKind::Let {
                destinations,
                values,
            });
        }

        let destinations = left
            .iter()
            .map(|expression| self.lower_place(expression, source))
            .collect::<Result<Vec<_>, _>>()?;
        let destination_types = destinations
            .iter()
            .map(|destination| match destination {
                hir::Place::Local(_) => self.place_ty(*destination).cloned().map(Some),
                hir::Place::Discard => Ok(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let values = right
            .iter()
            .zip(&destination_types)
            .map(|(expression, expected)| match expected {
                Some(expected) => self.lower_expr(expression, Some(expected)),
                None => self.lower_expr(expression, None).and_then(|expression| {
                    let source = expression.source;
                    default_expr_type(expression, source)
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let op = match token {
            Token::ASSIGN => hir::AssignOp::Set,
            Token::ADD_ASSIGN => hir::AssignOp::Add,
            Token::SUB_ASSIGN => hir::AssignOp::Sub,
            Token::MUL_ASSIGN => hir::AssignOp::Mul,
            Token::QUO_ASSIGN => hir::AssignOp::Div,
            Token::REM_ASSIGN => hir::AssignOp::Rem,
            Token::AND_ASSIGN => hir::AssignOp::BitAnd,
            Token::OR_ASSIGN => hir::AssignOp::BitOr,
            Token::XOR_ASSIGN => hir::AssignOp::BitXor,
            Token::SHL_ASSIGN => hir::AssignOp::Shl,
            Token::SHR_ASSIGN => hir::AssignOp::Shr,
            Token::AND_NOT_ASSIGN => hir::AssignOp::AndNot,
            _ => {
                return Err(Diagnostic::semantic(
                    format!("invalid assignment operator {token:?}"),
                    source,
                ));
            }
        };
        if op != hir::AssignOp::Set && destinations.len() != 1 {
            return Err(Diagnostic::semantic(
                "compound assignment requires one destination and one value",
                source,
            ));
        }
        if op != hir::AssignOp::Set {
            let ty = destination_types
                .first()
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    Diagnostic::semantic(
                        "compound assignment requires a non-blank destination",
                        source,
                    )
                })?;
            validate_binary_operator(assignment_binary_op(op), ty, source)?;
        }
        Ok(hir::StmtKind::Assign {
            destinations,
            op,
            values,
        })
    }

    fn lower_place(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let ExprSyntaxKind::Ident(ident) = &expression.kind else {
            return Err(Diagnostic::unsupported(
                "only local identifier assignment targets are implemented",
                source,
            ));
        };
        if ident.name.as_ref() == "_" {
            return Ok(hir::Place::Discard);
        }
        self.lookup_local(&ident.name)
            .map(hir::Place::Local)
            .ok_or_else(|| {
                Diagnostic::semantic(format!("undefined variable {}", ident.name), source)
            })
    }

    pub(super) fn place_ty(&self, place: hir::Place) -> Result<&Ty, Diagnostic> {
        match place {
            hir::Place::Local(id) => self
                .locals
                .get(id.0 as usize)
                .map(|local| &local.ty)
                .ok_or_else(|| Diagnostic::backend(format!("invalid local id {}", id.0))),
            hir::Place::Discard => Err(Diagnostic::backend(
                "blank identifier unexpectedly required an inferred type",
            )),
        }
    }
}
