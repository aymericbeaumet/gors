//! Type-switch lowering through explicit interface tests and checked unboxing.

use std::collections::HashSet;

use super::FunctionLowerer;
use super::control_targets::ControlTargetKind;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, IdentSyntax, StmtSyntax, StmtSyntaxKind,
    SwitchCaseSyntax, SyntaxSource,
};
use crate::compiler::types::Ty;
use crate::token::Token;

struct TypeSwitchBranch {
    source: SyntaxSource,
    init: Option<Box<hir::Stmt>>,
    condition: hir::Expr,
    body: hir::Block,
}

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_type_switch(
        &mut self,
        node: NodeId,
        init: Option<&StmtSyntax>,
        binding: Option<&IdentSyntax>,
        expression: &ExprSyntax,
        cases: &[SwitchCaseSyntax],
        syntax_source: SyntaxSource,
        label: Option<&str>,
        source: SourceRef,
    ) -> Result<Option<hir::Stmt>, Diagnostic> {
        self.push_scope();
        let target =
            self.begin_control_target(ControlTargetKind::BreakOnly, label.map(str::to_owned))?;
        let lowered = self.lower_type_switch_in_scope(
            init,
            binding,
            expression,
            cases,
            syntax_source,
            source,
        );
        self.end_control_target(target)?;
        self.pop_scope();
        let statements = lowered?;
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

    fn lower_type_switch_in_scope(
        &mut self,
        init: Option<&StmtSyntax>,
        binding: Option<&IdentSyntax>,
        expression: &ExprSyntax,
        cases: &[SwitchCaseSyntax],
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<Vec<hir::Stmt>, Diagnostic> {
        let mut statements = Vec::new();
        if let Some(init) = init
            && let Some(init) = self.lower_stmt(init)?
        {
            statements.push(init);
        }

        let interface = self.lower_expr(expression, None)?;
        if !matches!(interface.ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::semantic(
                "type switch requires an interface expression",
                source,
            ));
        }
        let interface_ty = interface.ty.clone();
        let interface_local = self.alloc_local(
            None,
            interface_ty.clone(),
            hir::LocalKind::Temporary,
            syntax_source,
        )?;
        let guard_node = self.alloc_node(syntax_source)?;
        statements.push(hir::Stmt {
            node: guard_node,
            kind: hir::StmtKind::Let {
                destinations: vec![hir::Place::Local(interface_local)],
                values: vec![interface],
            },
            source: SourceRef::node(guard_node),
        });

        let mut default = None;
        let mut branches = Vec::new();
        let mut seen_types = HashSet::new();
        let mut seen_nil = false;
        for case in cases {
            reject_type_switch_fallthrough(case, source)?;
            if case.expressions.is_empty() {
                if default.is_some() {
                    return Err(Diagnostic::semantic(
                        "type switch has multiple default cases",
                        source,
                    ));
                }
                default = Some((
                    case.source,
                    self.lower_interface_bound_case_body(
                        &case.body,
                        binding,
                        interface_local,
                        &interface_ty,
                        expression.source,
                    )?,
                ));
                continue;
            }

            let single_concrete = match case.expressions.as_ref() {
                [asserted] => !is_nil_identifier(asserted),
                _ => false,
            };
            let branch = if single_concrete {
                self.lower_single_type_case(
                    case,
                    binding,
                    interface_local,
                    &interface_ty,
                    &mut seen_types,
                )?
            } else {
                self.lower_multi_type_case(
                    case,
                    binding,
                    interface_local,
                    &interface_ty,
                    expression.source,
                    &mut seen_types,
                    &mut seen_nil,
                )?
            };
            branches.push(branch);
        }

        let mut tail = if let Some((case_source, body)) = default {
            let node = self.alloc_node(case_source)?;
            Some(Box::new(hir::Stmt {
                node,
                kind: hir::StmtKind::Block(body),
                source: SourceRef::node(node),
            }))
        } else {
            None
        };
        for branch in branches.into_iter().rev() {
            let branch_node = self.alloc_node(branch.source)?;
            tail = Some(Box::new(hir::Stmt {
                node: branch_node,
                kind: hir::StmtKind::If {
                    init: branch.init,
                    condition: branch.condition,
                    then_block: branch.body,
                    else_branch: tail,
                },
                source: SourceRef::node(branch_node),
            }));
        }
        if let Some(tail) = tail {
            statements.push(*tail);
        }
        Ok(statements)
    }

    fn lower_single_type_case(
        &mut self,
        case: &SwitchCaseSyntax,
        binding: Option<&IdentSyntax>,
        interface_local: LocalId,
        interface_ty: &Ty,
        seen_types: &mut HashSet<Ty>,
    ) -> Result<TypeSwitchBranch, Diagnostic> {
        let [asserted] = case.expressions.as_ref() else {
            return Err(Diagnostic::backend(
                "single type-switch case lost its asserted type",
            ));
        };
        let value_node = self.alloc_node(asserted.source)?;
        let interface = self.local_expr(value_node, interface_local, interface_ty.clone());
        let assertion_node = self.alloc_node(asserted.source)?;
        let assertion_source = SourceRef::node(assertion_node);
        let assertion = self.build_interface_type_assertion(
            interface,
            asserted,
            true,
            assertion_node,
            assertion_source,
        )?;
        let Ty::Tuple(result_types) = &assertion.ty else {
            return Err(Diagnostic::backend(
                "checked type-switch assertion did not produce a tuple",
            ));
        };
        let [asserted_ty, ok_ty] = result_types.as_slice() else {
            return Err(Diagnostic::backend(
                "checked type-switch assertion lost its result shape",
            ));
        };
        let asserted_ty = asserted_ty.clone();
        if !seen_types.insert(asserted_ty.clone()) {
            return Err(Diagnostic::semantic(
                format!("duplicate type-switch case {asserted_ty:?}"),
                assertion_source,
            ));
        }

        self.push_scope();
        let value_local = self.alloc_local(
            binding.map(|binding| binding.name.to_string()),
            asserted_ty,
            hir::LocalKind::Variable,
            binding.map_or(case.source, |binding| binding.source),
        )?;
        let ok_local =
            self.alloc_local(None, ok_ty.clone(), hir::LocalKind::Temporary, case.source)?;
        let body = self.lower_block(&case.body, false);
        self.pop_scope();
        let body = body?;

        let init_node = self.alloc_node(case.source)?;
        let init = hir::Stmt {
            node: init_node,
            kind: hir::StmtKind::LetTuple {
                destinations: vec![hir::Place::Local(value_local), hir::Place::Local(ok_local)],
                value: assertion,
                coercions: vec![hir::ValueCoercion::Identity, hir::ValueCoercion::Identity],
            },
            source: SourceRef::node(init_node),
        };
        let condition_node = self.alloc_node(case.source)?;
        let condition = self.local_expr(condition_node, ok_local, Ty::Bool);
        Ok(TypeSwitchBranch {
            source: case.source,
            init: Some(Box::new(init)),
            condition,
            body,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_multi_type_case(
        &mut self,
        case: &SwitchCaseSyntax,
        binding: Option<&IdentSyntax>,
        interface_local: LocalId,
        interface_ty: &Ty,
        expression_source: SyntaxSource,
        seen_types: &mut HashSet<Ty>,
        seen_nil: &mut bool,
    ) -> Result<TypeSwitchBranch, Diagnostic> {
        let mut conditions = Vec::new();
        for asserted in &*case.expressions {
            let value_node = self.alloc_node(asserted.source)?;
            let interface = self.local_expr(value_node, interface_local, interface_ty.clone());
            let condition_node = self.alloc_node(asserted.source)?;
            let condition_source = SourceRef::node(condition_node);
            if is_nil_identifier(asserted) {
                if *seen_nil {
                    return Err(Diagnostic::semantic(
                        "duplicate nil type-switch case",
                        condition_source,
                    ));
                }
                *seen_nil = true;
                conditions.push(interface_nil_test(
                    interface,
                    condition_node,
                    condition_source,
                ));
            } else {
                let (asserted_ty, condition) = self.build_interface_type_test(
                    interface,
                    asserted,
                    condition_node,
                    condition_source,
                )?;
                if !seen_types.insert(asserted_ty.clone()) {
                    return Err(Diagnostic::semantic(
                        format!("duplicate type-switch case {asserted_ty:?}"),
                        condition_source,
                    ));
                }
                conditions.push(condition);
            }
        }
        let condition = self.combine_type_switch_conditions(conditions, case.source)?;
        let body = self.lower_interface_bound_case_body(
            &case.body,
            binding,
            interface_local,
            interface_ty,
            expression_source,
        )?;
        if case.expressions.is_empty() {
            return Err(Diagnostic::backend(
                "non-default type-switch case lost its type list",
            ));
        }
        Ok(TypeSwitchBranch {
            source: case.source,
            init: None,
            condition,
            body,
        })
    }

    fn combine_type_switch_conditions(
        &mut self,
        conditions: Vec<hir::Expr>,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let mut conditions = conditions.into_iter();
        let mut condition = conditions
            .next()
            .ok_or_else(|| Diagnostic::backend("type-switch case lost its type list"))?;
        for right in conditions {
            let node = self.alloc_node(syntax_source)?;
            let effects = condition.effects.union(right.effects);
            condition = hir::Expr {
                node,
                kind: hir::ExprKind::Binary {
                    op: hir::BinaryOp::LogicalOr,
                    left: Box::new(condition),
                    right: Box::new(right),
                },
                ty: Ty::Bool,
                category: hir::ValueCategory::Value,
                effects,
                source: SourceRef::node(node),
            };
        }
        Ok(condition)
    }

    fn lower_interface_bound_case_body(
        &mut self,
        body: &BlockSyntax,
        binding: Option<&IdentSyntax>,
        interface_local: LocalId,
        interface_ty: &Ty,
        expression_source: SyntaxSource,
    ) -> Result<hir::Block, Diagnostic> {
        self.push_scope();
        let initializer = binding
            .map(|binding| {
                let local = self.alloc_local(
                    Some(binding.name.to_string()),
                    interface_ty.clone(),
                    hir::LocalKind::Variable,
                    binding.source,
                )?;
                let value_node = self.alloc_node(expression_source)?;
                let value = self.local_expr(value_node, interface_local, interface_ty.clone());
                let node = self.alloc_node(binding.source)?;
                Ok(hir::Stmt {
                    node,
                    kind: hir::StmtKind::Let {
                        destinations: vec![hir::Place::Local(local)],
                        values: vec![value],
                    },
                    source: SourceRef::node(node),
                })
            })
            .transpose()?;
        let lowered = self.lower_block(body, false);
        self.pop_scope();
        let mut lowered = lowered?;
        if let Some(initializer) = initializer {
            lowered.stmts.insert(0, initializer);
        }
        Ok(lowered)
    }
}

fn interface_nil_test(interface: hir::Expr, node: NodeId, source: SourceRef) -> hir::Expr {
    let effects = interface.effects.union(hir::Effects {
        may_call: true,
        ..hir::Effects::default()
    });
    hir::Expr {
        node,
        kind: hir::ExprKind::Call {
            callee: hir::Callee::Builtin(hir::Builtin::InterfaceIsNil),
            args: vec![interface],
        },
        ty: Ty::Bool,
        category: hir::ValueCategory::Value,
        effects,
        source,
    }
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == "nil"
    )
}

fn reject_type_switch_fallthrough(
    case: &SwitchCaseSyntax,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    if case.body.statements.iter().any(|statement| {
        matches!(
            statement.kind,
            StmtSyntaxKind::Branch {
                token: Token::FALLTHROUGH,
                ..
            }
        )
    }) {
        return Err(Diagnostic::semantic(
            "fallthrough is not permitted in a type switch",
            source,
        ));
    }
    Ok(())
}
