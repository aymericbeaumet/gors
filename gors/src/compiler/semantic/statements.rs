//! Statement, declaration, assignment, and place lowering.

use std::collections::BTreeSet;

use crate::ast;
use crate::token::Token;

use super::FunctionLowerer;
use super::expressions::*;
use super::positions::{expr_position, stmt_position};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::SourceSpan;
use crate::compiler::types::{ConstValue, IntTy, Ty};

impl FunctionLowerer<'_> {
    pub(super) fn lower_stmt(
        &mut self,
        stmt: &ast::Stmt<'_>,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        let span = self.file.span(&stmt_position(stmt));
        let kind = match stmt {
            ast::Stmt::EmptyStmt(_) => return Ok(None),
            ast::Stmt::BlockStmt(block) => hir::StmtKind::Block(self.lower_block(block, true)?),
            ast::Stmt::ExprStmt(expr) => {
                let expr = self.lower_expr_inner(&expr.x, None, true)?;
                if !matches!(expr.kind, hir::ExprKind::Call { .. }) {
                    return Err(Diagnostic::semantic(
                        "expression statement must be a call",
                        span,
                    ));
                }
                hir::StmtKind::Expr(expr)
            }
            ast::Stmt::DeclStmt(decl) => self.lower_local_decl(&decl.decl)?,
            ast::Stmt::AssignStmt(assign) => self.lower_assignment(assign)?,
            ast::Stmt::IncDecStmt(inc_dec) => {
                let destination = self.lower_place(&inc_dec.x)?;
                let ty = self.place_ty(destination)?.clone();
                if ty != Ty::Int(IntTy::Int) {
                    return Err(Diagnostic::semantic(
                        "increment and decrement require an int operand in the bootstrap backend",
                        span,
                    ));
                }
                let one = hir::Expr {
                    node: self.alloc_node()?,
                    kind: hir::ExprKind::Constant(ConstValue::Int("1".into())),
                    ty,
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    span: span.clone(),
                };
                hir::StmtKind::Assign {
                    destinations: vec![destination],
                    op: match inc_dec.tok {
                        Token::INC => hir::AssignOp::Add,
                        Token::DEC => hir::AssignOp::Sub,
                        _ => {
                            return Err(Diagnostic::semantic(
                                "invalid increment/decrement token",
                                span,
                            ));
                        }
                    },
                    values: vec![one],
                }
            }
            ast::Stmt::ReturnStmt(return_stmt) => {
                let values = if return_stmt.results.is_empty() && !self.named_results.is_empty() {
                    let named_results = self.named_results.clone();
                    let mut values = Vec::new();
                    for (index, result) in named_results.into_iter().enumerate() {
                        let Some(local) = result else {
                            return Err(Diagnostic::semantic(
                                "bare return requires every result to be named",
                                span,
                            ));
                        };
                        let ty = self.signature.results.get(index).cloned().ok_or_else(|| {
                            Diagnostic::backend(format!(
                                "named result {index} has no signature type"
                            ))
                        })?;
                        let node = self.alloc_node()?;
                        values.push(self.local_expr(node, local, ty, span.clone()));
                    }
                    values
                } else {
                    if return_stmt.results.len() != self.signature.results.len() {
                        return Err(Diagnostic::semantic(
                            format!(
                                "return has {} values; function requires {}",
                                return_stmt.results.len(),
                                self.signature.results.len()
                            ),
                            span,
                        ));
                    }
                    let result_types = self.signature.results.clone();
                    return_stmt
                        .results
                        .iter()
                        .zip(result_types)
                        .map(|(expr, expected)| self.lower_expr(expr, Some(&expected)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                hir::StmtKind::Return(values)
            }
            ast::Stmt::IfStmt(if_stmt) => {
                self.push_scope();
                let init = if_stmt
                    .init
                    .as_ref()
                    .as_ref()
                    .map(|stmt| self.lower_stmt(stmt))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                let condition = self.lower_expr(&if_stmt.cond, Some(&Ty::Bool))?;
                let then_block = self.lower_block(&if_stmt.body, true)?;
                let else_branch = if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .map(|stmt| self.lower_stmt(stmt))
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
            ast::Stmt::ForStmt(for_stmt) => {
                self.push_scope();
                let init = for_stmt
                    .init
                    .as_deref()
                    .map(|stmt| self.lower_stmt(stmt))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                let condition = for_stmt
                    .cond
                    .as_ref()
                    .map(|expr| self.lower_expr(expr, Some(&Ty::Bool)))
                    .transpose()?;
                self.loop_depth += 1;
                let body = self.lower_block(&for_stmt.body, true)?;
                let post = for_stmt
                    .post
                    .as_deref()
                    .map(|stmt| self.lower_stmt(stmt))
                    .transpose()?
                    .flatten()
                    .map(Box::new);
                self.loop_depth -= 1;
                self.pop_scope();
                hir::StmtKind::For {
                    init,
                    condition,
                    post,
                    body,
                }
            }
            ast::Stmt::BranchStmt(branch) => match branch.tok {
                Token::BREAK if branch.label.is_none() && self.loop_depth != 0 => {
                    hir::StmtKind::Break
                }
                Token::CONTINUE if branch.label.is_none() && self.loop_depth != 0 => {
                    hir::StmtKind::Continue
                }
                _ => {
                    return Err(Diagnostic::unsupported(
                        "only unlabeled break and continue in loops are implemented",
                        span,
                    ));
                }
            },
            _ => {
                return Err(Diagnostic::unsupported(
                    format!("statement {stmt:?} is not implemented by the HIR/MIR backend"),
                    span,
                ));
            }
        };
        Ok(Some(hir::Stmt {
            node: self.alloc_node()?,
            kind,
            span,
        }))
    }

    fn lower_local_decl(&mut self, decl: &ast::GenDecl<'_>) -> Result<hir::StmtKind, Diagnostic> {
        if decl.tok == Token::CONST {
            return Err(Diagnostic::unsupported(
                "local const declarations require immutable HIR bindings and are not implemented",
                self.file.span(&decl.tok_pos),
            ));
        }
        if decl.tok != Token::VAR {
            return Err(Diagnostic::unsupported(
                "local type and import declarations are not implemented",
                self.file.span(&decl.tok_pos),
            ));
        }
        let mut statements = Vec::new();
        for spec in &decl.specs {
            let ast::Spec::ValueSpec(spec) = spec else {
                return Err(Diagnostic::semantic(
                    "value declaration contains a non-value specification",
                    self.file.span(&decl.tok_pos),
                ));
            };
            let explicit_ty = spec
                .type_
                .as_ref()
                .map(|ty| self.file.lower_type(ty))
                .transpose()?;
            let raw_values = spec.values.as_deref().unwrap_or_default();
            if !raw_values.is_empty() && raw_values.len() != spec.names.len() {
                return Err(Diagnostic::unsupported(
                    "multi-valued declarations are not implemented",
                    self.file.span(&decl.tok_pos),
                ));
            }
            // Go evaluates every RHS in one ValueSpec before any of that
            // spec's names enter scope. A previous ValueSpec in the same
            // declaration is already initialized and visible.
            let mut values = if raw_values.is_empty() {
                let ty = explicit_ty.clone().ok_or_else(|| {
                    Diagnostic::semantic(
                        "declaration without initializer requires a type",
                        self.file.span(&decl.tok_pos),
                    )
                })?;
                spec.names
                    .iter()
                    .map(|name| {
                        let span = self.file.span(&name.name_pos);
                        let value = ty.zero().ok_or_else(|| {
                            Diagnostic::unsupported(
                                format!("zero value for {ty:?} is not implemented"),
                                span.clone(),
                            )
                        })?;
                        Ok(hir::Expr {
                            node: self.alloc_node()?,
                            kind: hir::ExprKind::Constant(value),
                            ty: ty.clone(),
                            category: hir::ValueCategory::Constant,
                            effects: hir::Effects::default(),
                            span,
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
                let span = self.file.span(&name.name_pos);
                let ty = explicit_ty
                    .clone()
                    .unwrap_or_else(|| value.ty.default_typed());
                ensure_bootstrap_value_type(&ty, &span)?;
                coerce_expr(value, &ty, &span)?;
                if name.name == "_" {
                    destinations.push(hir::Place::Discard);
                } else {
                    let id = self.alloc_local(
                        Some(name.name.to_string()),
                        ty,
                        hir::LocalKind::Variable,
                        span,
                    )?;
                    destinations.push(hir::Place::Local(id));
                }
            }
            statements.push(hir::Stmt {
                node: self.alloc_node()?,
                kind: hir::StmtKind::Let {
                    destinations,
                    values,
                },
                span: self.file.span(&decl.tok_pos),
            });
        }
        Ok(hir::StmtKind::Block(hir::Block {
            node: self.alloc_node()?,
            stmts: statements,
            span: self.file.span(&decl.tok_pos),
        }))
    }

    fn lower_assignment(
        &mut self,
        assign: &ast::AssignStmt<'_>,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if assign.lhs.len() != assign.rhs.len() {
            return Err(Diagnostic::unsupported(
                "multi-result assignment is not implemented by the HIR/MIR backend",
                self.file.span(&assign.tok_pos),
            ));
        }
        if assign.tok == Token::DEFINE {
            let mut names = BTreeSet::new();
            for lhs in &assign.lhs {
                let ast::Expr::Ident(name) = lhs else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        self.file.span(&expr_position(lhs)),
                    ));
                };
                if name.name != "_" && !names.insert(name.name) {
                    return Err(Diagnostic::semantic(
                        format!("{} appears more than once on the left of :=", name.name),
                        self.file.span(&name.name_pos),
                    ));
                }
            }
            // Go resolves all RHS expressions before adding the new bindings.
            let mut values = assign
                .rhs
                .iter()
                .map(|expr| self.lower_expr(expr, None))
                .collect::<Result<Vec<_>, _>>()?;
            let mut destinations = Vec::new();
            let mut introduced = false;
            for (lhs, value) in assign.lhs.iter().zip(&mut values) {
                let ast::Expr::Ident(name) = lhs else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        self.file.span(&expr_position(lhs)),
                    ));
                };
                if name.name == "_" {
                    destinations.push(hir::Place::Discard);
                    continue;
                }
                if let Some(local) = self.lookup_current_local(name.name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    coerce_expr(value, &ty, &self.file.span(&name.name_pos))?;
                    destinations.push(hir::Place::Local(local));
                } else {
                    introduced = true;
                    let ty = value.ty.default_typed();
                    ensure_bootstrap_value_type(&ty, &self.file.span(&name.name_pos))?;
                    coerce_expr(value, &ty, &self.file.span(&name.name_pos))?;
                    let local = self.alloc_local(
                        Some(name.name.to_string()),
                        ty,
                        hir::LocalKind::Variable,
                        self.file.span(&name.name_pos),
                    )?;
                    destinations.push(hir::Place::Local(local));
                }
            }
            if !introduced {
                return Err(Diagnostic::semantic(
                    "short declaration introduces no new variables",
                    self.file.span(&assign.tok_pos),
                ));
            }
            return Ok(hir::StmtKind::Let {
                destinations,
                values,
            });
        }

        let destinations = assign
            .lhs
            .iter()
            .map(|expr| self.lower_place(expr))
            .collect::<Result<Vec<_>, _>>()?;
        let destination_types = destinations
            .iter()
            .map(|destination| match destination {
                hir::Place::Local(_) => self.place_ty(*destination).cloned().map(Some),
                hir::Place::Discard => Ok(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let values = assign
            .rhs
            .iter()
            .zip(&destination_types)
            .map(|(expr, expected)| match expected {
                Some(expected) => self.lower_expr(expr, Some(expected)),
                None => self.lower_expr(expr, None).and_then(default_expr_type),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let op = match assign.tok {
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
                    format!("invalid assignment operator {:?}", assign.tok),
                    self.file.span(&assign.tok_pos),
                ));
            }
        };
        if op != hir::AssignOp::Set && destinations.len() != 1 {
            return Err(Diagnostic::semantic(
                "compound assignment requires one destination and one value",
                self.file.span(&assign.tok_pos),
            ));
        }
        if op != hir::AssignOp::Set {
            let ty = destination_types
                .first()
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    Diagnostic::semantic(
                        "compound assignment requires a non-blank destination",
                        self.file.span(&assign.tok_pos),
                    )
                })?;
            validate_binary_operator(
                assignment_binary_op(op),
                ty,
                &self.file.span(&assign.tok_pos),
            )?;
        }
        Ok(hir::StmtKind::Assign {
            destinations,
            op,
            values,
        })
    }

    fn lower_place(&self, expr: &ast::Expr<'_>) -> Result<hir::Place, Diagnostic> {
        let ast::Expr::Ident(ident) = expr else {
            return Err(Diagnostic::unsupported(
                "only local identifier assignment targets are implemented",
                self.file.span(&expr_position(expr)),
            ));
        };
        if ident.name == "_" {
            return Ok(hir::Place::Discard);
        }
        self.lookup_local(ident.name)
            .map(hir::Place::Local)
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("undefined variable {}", ident.name),
                    self.file.span(&ident.name_pos),
                )
            })
    }

    pub(super) fn place_ty(&self, place: hir::Place) -> Result<&Ty, Diagnostic> {
        match place {
            hir::Place::Local(id) => self
                .locals
                .get(id.0 as usize)
                .map(|local| &local.ty)
                .ok_or_else(|| Diagnostic::backend(format!("invalid local id {}", id.0))),
            hir::Place::Discard => Err(Diagnostic::semantic(
                "blank identifier has no expected type",
                SourceSpan::synthetic(),
            )),
        }
    }
}
