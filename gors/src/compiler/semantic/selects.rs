//! Typed lowering for channel `select` statements.

use super::FunctionLowerer;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ExprSyntax, ExprSyntaxKind, SelectCaseSyntax, StmtSyntax, StmtSyntaxKind, SyntaxSource,
};
use crate::compiler::types::{ConstValue, IntTy, Ty};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn lower_select(
        &mut self,
        node: NodeId,
        cases: &[SelectCaseSyntax],
        source: SourceRef,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        let mut communication = None;
        let mut default = None;
        for case in cases {
            if case.communication.is_some() {
                if communication.replace(case).is_some() {
                    return Err(Diagnostic::unsupported(
                        "select with multiple communication cases requires channel arbitration",
                        source,
                    ));
                }
            } else if default.replace(case).is_some() {
                return Err(Diagnostic::semantic(
                    "select has multiple default clauses",
                    source,
                ));
            }
        }

        let Some(communication) = communication else {
            let Some(default) = default else {
                return Err(Diagnostic::unsupported(
                    "an empty select requires scheduler-backed blocking",
                    source,
                ));
            };
            let body = self.lower_select_default(default)?;
            return Ok(Some(hir::Stmt {
                node,
                kind: hir::StmtKind::Block(body),
                source,
            }));
        };
        let Some(default) = default else {
            return Err(Diagnostic::unsupported(
                "select without a default requires scheduler-backed blocking",
                source,
            ));
        };

        let default_source = default.source;
        let default = self.lower_select_default(default)?;
        let (init, condition, then_block) = self.lower_select_communication(communication)?;
        let else_node = self.alloc_node(default_source)?;
        let else_branch = hir::Stmt {
            node: else_node,
            kind: hir::StmtKind::Block(default),
            source: SourceRef::node(else_node),
        };
        Ok(Some(hir::Stmt {
            node,
            kind: hir::StmtKind::If {
                init,
                condition,
                then_block,
                else_branch: Some(Box::new(else_branch)),
            },
            source,
        }))
    }

    fn lower_select_default(&mut self, case: &SelectCaseSyntax) -> Result<hir::Block, Diagnostic> {
        self.push_scope();
        let body = self.lower_block(&case.body, false);
        self.pop_scope();
        body
    }

    fn lower_select_communication(
        &mut self,
        case: &SelectCaseSyntax,
    ) -> Result<(Option<Box<hir::Stmt>>, hir::Expr, hir::Block), Diagnostic> {
        let communication = case
            .communication
            .as_deref()
            .ok_or_else(|| Diagnostic::backend("select communication clause lost its operation"))?;
        self.push_scope();
        let lowered = self.lower_select_communication_in_scope(case, communication);
        self.pop_scope();
        lowered
    }

    fn lower_select_communication_in_scope(
        &mut self,
        case: &SelectCaseSyntax,
        communication: &StmtSyntax,
    ) -> Result<(Option<Box<hir::Stmt>>, hir::Expr, hir::Block), Diagnostic> {
        let node = self.alloc_node(communication.source)?;
        let source = SourceRef::node(node);
        match &communication.kind {
            StmtSyntaxKind::Send { channel, value } => {
                let condition = self.lower_channel_try_send(channel, value, node, source)?;
                let body = self.lower_block(&case.body, false)?;
                Ok((None, condition, body))
            }
            StmtSyntaxKind::Expr(expression) => {
                let channel = receive_operand(expression).ok_or_else(|| {
                    Diagnostic::semantic(
                        "select case must contain a channel send or receive",
                        source,
                    )
                })?;
                self.lower_select_receive(case, communication, channel, None, node, source)
            }
            StmtSyntaxKind::Assign { left, token, right } => {
                let [right] = &**right else {
                    return Err(Diagnostic::semantic(
                        "select receive assignment requires one receive expression",
                        source,
                    ));
                };
                let channel = receive_operand(right).ok_or_else(|| {
                    Diagnostic::semantic("select assignment must receive from a channel", source)
                })?;
                self.lower_select_receive(
                    case,
                    communication,
                    channel,
                    Some((left, *token)),
                    node,
                    source,
                )
            }
            _ => Err(Diagnostic::semantic(
                "select case must contain a channel send or receive",
                source,
            )),
        }
    }

    fn lower_select_receive(
        &mut self,
        case: &SelectCaseSyntax,
        communication: &StmtSyntax,
        channel: &ExprSyntax,
        assignment: Option<(&[ExprSyntax], Token)>,
        call_node: NodeId,
        call_source: SourceRef,
    ) -> Result<(Option<Box<hir::Stmt>>, hir::Expr, hir::Block), Diagnostic> {
        let receive = self.lower_channel_try_receive(channel, call_node, call_source)?;
        let Ty::Tuple(result_types) = &receive.ty else {
            return Err(Diagnostic::backend(
                "select receive did not produce its value and readiness status",
            ));
        };
        let element_ty = result_types
            .first()
            .cloned()
            .ok_or_else(|| Diagnostic::backend("select receive lost its element type"))?;
        let value_local = self.alloc_local(
            None,
            element_ty.clone(),
            hir::LocalKind::Temporary,
            communication.source,
        )?;
        let status_local = self.alloc_local(
            None,
            Ty::Int(IntTy::Int),
            hir::LocalKind::Temporary,
            communication.source,
        )?;
        let init_node = self.alloc_node(communication.source)?;
        let init = hir::Stmt {
            node: init_node,
            kind: hir::StmtKind::LetTuple {
                destinations: vec![
                    hir::Place::Local(value_local),
                    hir::Place::Local(status_local),
                ],
                value: receive,
                coercions: vec![hir::ValueCoercion::Identity, hir::ValueCoercion::Identity],
            },
            source: SourceRef::node(init_node),
        };
        let condition = self.select_status_comparison(
            status_local,
            hir::BinaryOp::NotEqual,
            0,
            communication.source,
        )?;

        let assignment = assignment
            .map(|(left, token)| {
                self.lower_select_receive_assignment(
                    left,
                    token,
                    value_local,
                    status_local,
                    element_ty,
                    communication,
                )
            })
            .transpose()?;
        let mut body = self.lower_block(&case.body, false)?;
        if let Some(assignment) = assignment {
            body.stmts.insert(0, assignment);
        }
        Ok((Some(Box::new(init)), condition, body))
    }

    fn lower_select_receive_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        value_local: LocalId,
        status_local: LocalId,
        element_ty: Ty,
        communication: &StmtSyntax,
    ) -> Result<hir::Stmt, Diagnostic> {
        let syntax_source = communication.source;
        let node = self.alloc_node(syntax_source)?;
        let source = SourceRef::node(node);
        if !(1..=2).contains(&left.len()) {
            return Err(Diagnostic::semantic(
                "select receive assignment requires one or two destinations",
                source,
            ));
        }
        let mut component_types = vec![element_ty.clone()];
        if left.len() == 2 {
            component_types.push(Ty::Bool);
        }
        let (destinations, coercions, declares) =
            self.lower_multi_result_destinations(left, token, &component_types, source)?;
        let value_node = self.alloc_node(syntax_source)?;
        let mut values = vec![self.local_expr(value_node, value_local, element_ty)];
        if left.len() == 2 {
            values.push(self.select_status_comparison(
                status_local,
                hir::BinaryOp::Equal,
                2,
                syntax_source,
            )?);
        }
        let values = values
            .into_iter()
            .zip(&coercions)
            .map(|(value, coercion)| {
                self.apply_assignment_value_coercion(value, coercion, syntax_source)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let kind = if declares {
            hir::StmtKind::Let {
                destinations,
                values,
            }
        } else {
            hir::StmtKind::Assign {
                destinations: self.lower_assignment_targets(left, source)?,
                op: hir::AssignOp::Set,
                values,
            }
        };
        Ok(hir::Stmt {
            node,
            kind,
            source: SourceRef::node(node),
        })
    }

    fn select_status_comparison(
        &mut self,
        status: LocalId,
        op: hir::BinaryOp,
        expected: i64,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let status_node = self.alloc_node(syntax_source)?;
        let status = self.local_expr(status_node, status, Ty::Int(IntTy::Int));
        let expected_node = self.alloc_node(syntax_source)?;
        let expected = hir::Expr {
            node: expected_node,
            kind: hir::ExprKind::Constant(ConstValue::Int(expected.to_string())),
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(expected_node),
        };
        let effects = status.effects.union(expected.effects);
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Binary {
                op,
                left: Box::new(status),
                right: Box::new(expected),
            },
            ty: Ty::Bool,
            category: hir::ValueCategory::Value,
            effects,
            source: SourceRef::node(node),
        })
    }
}

fn receive_operand(expression: &ExprSyntax) -> Option<&ExprSyntax> {
    match &expression.kind {
        ExprSyntaxKind::Paren(expression) => receive_operand(expression),
        ExprSyntaxKind::Unary {
            token: Token::ARROW,
            expression,
        } => Some(expression),
        _ => None,
    }
}
