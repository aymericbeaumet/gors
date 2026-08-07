//! Typed lowering for channel `select` statements.

use super::FunctionLowerer;
use super::control_targets::ControlTargetKind;
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
        label: Option<&str>,
        syntax_source: SyntaxSource,
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

        if communication.is_none() && default.is_none() {
            return Err(Diagnostic::unsupported(
                "an empty select requires scheduler-backed blocking",
                source,
            ));
        }

        let target =
            self.begin_control_target(ControlTargetKind::BreakOnly, label.map(str::to_owned))?;
        let body = match (communication, default) {
            (None, Some(default)) => self.lower_select_default(default),
            (Some(communication), None) => self.lower_blocking_select_communication(communication),
            (Some(communication), Some(default)) => {
                self.lower_try_select(communication, default, syntax_source)
            }
            (None, None) => Err(Diagnostic::backend(
                "empty select passed semantic target allocation",
            )),
        };
        self.end_control_target(target)?;
        let body = body?;
        Ok(Some(self.wrap_select_breakable(
            node,
            target,
            label,
            syntax_source,
            source,
            body,
        )?))
    }

    fn lower_try_select(
        &mut self,
        communication: &SelectCaseSyntax,
        default: &SelectCaseSyntax,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Block, Diagnostic> {
        let default_source = default.source;
        let default = self.lower_select_default(default)?;
        let (init, condition, then_block) = self.lower_select_communication(communication)?;
        let else_node = self.alloc_node(default_source)?;
        let else_branch = hir::Stmt {
            node: else_node,
            kind: hir::StmtKind::Block(default),
            source: SourceRef::node(else_node),
        };
        let if_node = self.alloc_node(syntax_source)?;
        let block_node = self.alloc_node(syntax_source)?;
        Ok(hir::Block {
            node: block_node,
            stmts: vec![hir::Stmt {
                node: if_node,
                kind: hir::StmtKind::If {
                    init,
                    condition,
                    then_block,
                    else_branch: Some(Box::new(else_branch)),
                },
                source: SourceRef::node(if_node),
            }],
            source: SourceRef::node(block_node),
        })
    }

    fn lower_blocking_select_communication(
        &mut self,
        case: &SelectCaseSyntax,
    ) -> Result<hir::Block, Diagnostic> {
        let communication = case
            .communication
            .as_deref()
            .ok_or_else(|| Diagnostic::backend("blocking select case lost its communication"))?;
        self.push_scope();
        let lowered = (|| {
            let communications =
                self.lower_blocking_select_communication_in_scope(communication)?;
            let mut body = self.lower_block(&case.body, false)?;
            body.stmts.splice(0..0, communications);
            Ok::<_, Diagnostic>(body)
        })();
        self.pop_scope();
        lowered
    }

    fn lower_blocking_select_communication_in_scope(
        &mut self,
        communication: &StmtSyntax,
    ) -> Result<Vec<hir::Stmt>, Diagnostic> {
        let node = self.alloc_node(communication.source)?;
        let source = SourceRef::node(node);
        match &communication.kind {
            StmtSyntaxKind::Send { channel, value } => Ok(vec![hir::Stmt {
                node,
                kind: self.lower_channel_send(channel, value, node, source)?,
                source,
            }]),
            StmtSyntaxKind::Expr(expression) => {
                let channel = receive_operand(expression).ok_or_else(|| {
                    Diagnostic::semantic(
                        "select case must contain a channel send or receive",
                        source,
                    )
                })?;
                let receive = self.lower_channel_receive(channel, false, node, source, None)?;
                Ok(vec![hir::Stmt {
                    node,
                    kind: hir::StmtKind::Expr(receive),
                    source,
                }])
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
                self.lower_blocking_select_receive_assignment(
                    communication,
                    channel,
                    left,
                    *token,
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

    fn lower_blocking_select_receive_assignment(
        &mut self,
        communication: &StmtSyntax,
        channel: &ExprSyntax,
        left: &[ExprSyntax],
        token: Token,
        receive_node: NodeId,
        receive_source: SourceRef,
    ) -> Result<Vec<hir::Stmt>, Diagnostic> {
        if !(1..=2).contains(&left.len()) {
            return Err(Diagnostic::semantic(
                "select receive assignment requires one or two destinations",
                receive_source,
            ));
        }
        let receive = self.lower_channel_receive(
            channel,
            left.len() == 2,
            receive_node,
            receive_source,
            None,
        )?;
        let component_types = if left.len() == 1 {
            vec![receive.ty.clone()]
        } else {
            let Ty::Tuple(component_types) = &receive.ty else {
                return Err(Diagnostic::backend(
                    "blocking select comma-ok receive lost its result tuple",
                ));
            };
            if component_types.len() != 2 {
                return Err(Diagnostic::backend(
                    "blocking select comma-ok receive produced the wrong result count",
                ));
            }
            component_types.clone()
        };
        let temporaries = component_types
            .iter()
            .map(|ty| {
                self.alloc_local(
                    None,
                    ty.clone(),
                    hir::LocalKind::Temporary,
                    communication.source,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let materialize_node = self.alloc_node(communication.source)?;
        let materialize_kind = match temporaries.as_slice() {
            [temporary] => hir::StmtKind::Let {
                destinations: vec![hir::Place::Local(*temporary)],
                values: vec![receive],
            },
            [_, _] => hir::StmtKind::LetTuple {
                destinations: temporaries.iter().copied().map(hir::Place::Local).collect(),
                value: receive,
                coercions: vec![hir::ValueCoercion::Identity; temporaries.len()],
            },
            _ => {
                return Err(Diagnostic::backend(
                    "blocking select receive produced an invalid temporary count",
                ));
            }
        };
        let materialize = hir::Stmt {
            node: materialize_node,
            kind: materialize_kind,
            source: SourceRef::node(materialize_node),
        };
        let values = temporaries
            .iter()
            .zip(&component_types)
            .map(|(local, ty)| {
                let node = self.alloc_node(communication.source)?;
                Ok(self.local_expr(node, *local, ty.clone()))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?;
        let assignment = self.lower_select_receive_values(
            left,
            token,
            &component_types,
            values,
            communication.source,
        )?;
        Ok(vec![materialize, assignment])
    }

    fn wrap_select_breakable(
        &mut self,
        node: NodeId,
        target: crate::compiler::ids::ControlTargetId,
        label: Option<&str>,
        syntax_source: SyntaxSource,
        source: SourceRef,
        body: hir::Block,
    ) -> Result<hir::Stmt, Diagnostic> {
        let (breakable_node, breakable_source) = if label.is_some() {
            let breakable_node = self.alloc_node(syntax_source)?;
            (breakable_node, SourceRef::node(breakable_node))
        } else {
            (node, source)
        };
        let breakable = hir::Stmt {
            node: breakable_node,
            kind: hir::StmtKind::Breakable { target, body },
            source: breakable_source,
        };
        Ok(if let Some(label) = label {
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
        })
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
        if !(1..=2).contains(&left.len()) {
            let node = self.alloc_node(communication.source)?;
            return Err(Diagnostic::semantic(
                "select receive assignment requires one or two destinations",
                SourceRef::node(node),
            ));
        }
        let mut component_types = vec![element_ty.clone()];
        if left.len() == 2 {
            component_types.push(Ty::Bool);
        }
        let syntax_source = communication.source;
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
        self.lower_select_receive_values(left, token, &component_types, values, syntax_source)
    }

    fn lower_select_receive_values(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        component_types: &[Ty],
        values: Vec<hir::Expr>,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Stmt, Diagnostic> {
        let node = self.alloc_node(syntax_source)?;
        let source = SourceRef::node(node);
        if !(1..=2).contains(&left.len()) || left.len() != component_types.len() {
            return Err(Diagnostic::semantic(
                "select receive assignment requires one or two destinations",
                source,
            ));
        }
        if values.len() != component_types.len() {
            return Err(Diagnostic::backend(
                "select receive assignment lost a materialized value",
            ));
        }
        let (kind, coercions) = match token {
            Token::DEFINE => {
                let (destinations, coercions, declares) =
                    self.lower_multi_result_destinations(left, token, component_types, source)?;
                if !declares {
                    return Err(Diagnostic::backend(
                        "select short declaration did not introduce a binding",
                    ));
                }
                (
                    SelectReceiveAssignment::Declaration(destinations),
                    coercions,
                )
            }
            Token::ASSIGN => {
                let destinations = self.lower_assignment_targets(left, source)?;
                let coercions =
                    self.assignment_target_coercions(&destinations, component_types, source)?;
                (SelectReceiveAssignment::Assignment(destinations), coercions)
            }
            _ => {
                return Err(Diagnostic::semantic(
                    "select receive assignment requires = or :=",
                    source,
                ));
            }
        };
        let values = values
            .into_iter()
            .zip(&coercions)
            .map(|(value, coercion)| {
                self.apply_assignment_value_coercion(value, coercion, syntax_source)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hir::Stmt {
            node,
            kind: match kind {
                SelectReceiveAssignment::Declaration(destinations) => hir::StmtKind::Let {
                    destinations,
                    values,
                },
                SelectReceiveAssignment::Assignment(destinations) => hir::StmtKind::Assign {
                    destinations,
                    op: hir::AssignOp::Set,
                    values,
                },
            },
            source,
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

enum SelectReceiveAssignment {
    Declaration(Vec<hir::Place>),
    Assignment(Vec<hir::AssignTarget>),
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
