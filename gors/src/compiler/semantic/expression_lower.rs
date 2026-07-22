//! Typed expression and direct-call lowering.

use crate::ast;
use crate::token::Token;

use super::FunctionLowerer;
use super::expressions::*;
use super::positions::expr_position;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId, SourceSpan};
use crate::compiler::types::{ConstValue, IntTy, Ty, UntypedTy};

impl FunctionLowerer<'_> {
    pub(super) fn local_expr(
        &self,
        node: NodeId,
        local: LocalId,
        ty: Ty,
        span: SourceSpan,
    ) -> hir::Expr {
        hir::Expr {
            node,
            kind: hir::ExprKind::Local(local),
            ty,
            category: hir::ValueCategory::Place,
            effects: hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            span,
        }
    }

    pub(super) fn lower_expr(
        &mut self,
        expr: &ast::Expr<'_>,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        self.lower_expr_inner(expr, expected, false)
    }

    pub(super) fn lower_expr_inner(
        &mut self,
        expr: &ast::Expr<'_>,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let span = self.file.span(&expr_position(expr));
        let node = self.alloc_node()?;
        let mut lowered = match expr {
            ast::Expr::BasicLit(literal) => {
                if literal.kind == Token::FLOAT {
                    return Err(Diagnostic::unsupported(
                        "floating-point literals are outside the bootstrap bool/int/string runtime frontier",
                        span,
                    ));
                }
                let (ty, value) = self.file.eval_constant(expr)?;
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Constant(value),
                    ty,
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    span: span.clone(),
                }
            }
            ast::Expr::Ident(ident) => {
                if let Some(local) = self.lookup_local(ident.name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    self.local_expr(node, local, ty, span.clone())
                } else if let Some(constant) = self.constants.get(ident.name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalConstant(constant.id, constant.value),
                        ty: constant.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        span: span.clone(),
                    }
                } else if self.functions.contains_key(ident.name) {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "function value {} is not implemented by the HIR/MIR backend",
                            ident.name
                        ),
                        span,
                    ));
                } else if ident.name == "true" || ident.name == "false" {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(ConstValue::Bool(ident.name == "true")),
                        ty: Ty::Untyped(UntypedTy::Bool),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        span: span.clone(),
                    }
                } else if ident.name == "print" || ident.name == "println" {
                    return Err(Diagnostic::unsupported(
                        format!("builtin value {} is not implemented", ident.name),
                        span,
                    ));
                } else {
                    return Err(Diagnostic::semantic(
                        format!("undefined identifier {}", ident.name),
                        span,
                    ));
                }
            }
            ast::Expr::ParenExpr(paren) => {
                return self.lower_expr_inner(&paren.x, expected, allow_discarded_call_result);
            }
            ast::Expr::UnaryExpr(unary) => {
                let mut operand = self.lower_expr(&unary.x, expected)?;
                let operand_ty = operand.ty.default_typed();
                ensure_bootstrap_value_type(&operand_ty, &span)?;
                let op = match unary.op {
                    Token::ADD if operand_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Positive,
                    Token::SUB if operand_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Negative,
                    Token::NOT if is_bool(&operand_ty) => hir::UnaryOp::Not,
                    Token::XOR if operand_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::BitNot,
                    _ => {
                        return Err(Diagnostic::semantic(
                            format!("invalid unary {:?} operand {:?}", unary.op, operand.ty),
                            span,
                        ));
                    }
                };
                if let Some(value) = expr_constant(&operand)
                    .map(|value| fold_constant_unary(op, value, &span))
                    .transpose()?
                    .flatten()
                {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: operand.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        span: span.clone(),
                    }
                } else {
                    coerce_expr(&mut operand, &operand_ty, &span)?;
                    let effects = operand.effects;
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Unary {
                            op,
                            operand: Box::new(operand),
                        },
                        ty: operand_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        span: span.clone(),
                    }
                }
            }
            ast::Expr::BinaryExpr(binary) => {
                let mut left = self.lower_expr(&binary.x, None)?;
                let mut right = self.lower_expr(&binary.y, None)?;
                let op = lower_binary_op(binary.op).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("binary operator {:?} is not implemented", binary.op),
                        span.clone(),
                    )
                })?;
                let comparison = matches!(
                    op,
                    hir::BinaryOp::Equal
                        | hir::BinaryOp::NotEqual
                        | hir::BinaryOp::Less
                        | hir::BinaryOp::LessEqual
                        | hir::BinaryOp::Greater
                        | hir::BinaryOp::GreaterEqual
                );
                let logical = matches!(op, hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr);
                let operand_ty = common_operand_type(&left.ty, &right.ty).ok_or_else(|| {
                    Diagnostic::semantic(
                        format!(
                            "incompatible binary operands {:?} and {:?}",
                            left.ty, right.ty
                        ),
                        span.clone(),
                    )
                })?;
                coerce_expr(&mut left, &operand_ty, &span)?;
                coerce_expr(&mut right, &operand_ty, &span)?;
                validate_binary_operator(op, &operand_ty, &span)?;
                let mut effects = left.effects.union(right.effects);
                if matches!(
                    op,
                    hir::BinaryOp::Div
                        | hir::BinaryOp::Rem
                        | hir::BinaryOp::Shl
                        | hir::BinaryOp::Shr
                ) {
                    // Dynamic division/remainder can divide by zero, and a
                    // dynamic Go shift count can be negative. Exact constant
                    // cases were either folded or diagnosed above.
                    effects.may_panic = true;
                }
                if op == hir::BinaryOp::Add && operand_ty == Ty::String {
                    effects.may_allocate = true;
                }
                let result_ty = if comparison || logical {
                    Ty::Bool
                } else {
                    operand_ty
                };
                let folded = expr_constant(&left)
                    .zip(expr_constant(&right))
                    .map(|(left, right)| fold_constant_binary(op, left, right, &span))
                    .transpose()?
                    .flatten();
                if let Some(value) = folded {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: result_ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        span: span.clone(),
                    }
                } else {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Binary {
                            op,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        ty: result_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        span: span.clone(),
                    }
                }
            }
            ast::Expr::CallExpr(call) => {
                let ast::Expr::Ident(callee_ident) = call.fun.as_ref() else {
                    return Err(Diagnostic::unsupported(
                        "only direct calls are implemented by the HIR/MIR backend",
                        span,
                    ));
                };
                let raw_args = call.args.as_deref().unwrap_or_default();
                let callee_span = self.file.span(&callee_ident.name_pos);
                let (callee, params, results) = if self.lookup_local(callee_ident.name).is_some() {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "calling local value {} requires function-value HIR and is not implemented",
                            callee_ident.name
                        ),
                        callee_span,
                    ));
                } else if let Some(symbol) = self.functions.get(callee_ident.name).cloned() {
                    (
                        hir::Callee::Function(symbol.id),
                        symbol.signature.params,
                        symbol.signature.results,
                    )
                } else if self.constants.contains_key(callee_ident.name) {
                    return Err(Diagnostic::semantic(
                        format!("constant {} is not callable", callee_ident.name),
                        callee_span,
                    ));
                } else {
                    match callee_ident.name {
                        "print" => (hir::Callee::Builtin(hir::Builtin::Print), vec![], vec![]),
                        "println" => (hir::Callee::Builtin(hir::Builtin::Println), vec![], vec![]),
                        name => {
                            return Err(Diagnostic::semantic(
                                format!("undefined function {name}"),
                                callee_span,
                            ));
                        }
                    }
                };
                if !matches!(callee, hir::Callee::Builtin(_)) && raw_args.len() != params.len() {
                    return Err(Diagnostic::semantic(
                        format!(
                            "call has {} arguments; expected {}",
                            raw_args.len(),
                            params.len()
                        ),
                        span,
                    ));
                }
                let args = if matches!(callee, hir::Callee::Builtin(_)) {
                    raw_args
                        .iter()
                        .map(|arg| self.lower_expr(arg, None).and_then(default_expr_type))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    raw_args
                        .iter()
                        .zip(&params)
                        .map(|(arg, expected)| self.lower_expr(arg, Some(expected)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                let ty = match results.as_slice() {
                    [] => Ty::Unit,
                    [single] => single.clone(),
                    _ => {
                        return Err(Diagnostic::unsupported(
                            "multiple-result calls require explicit expression-arity HIR",
                            span,
                        ));
                    }
                };
                if ty == Ty::Unit && !allow_discarded_call_result {
                    return Err(Diagnostic::unsupported(
                        "a no-result call cannot be used as a value",
                        span,
                    ));
                }
                let effects = args.iter().fold(
                    hir::Effects {
                        may_read: false,
                        may_call: true,
                        may_allocate: true,
                        may_block: true,
                        may_panic: true,
                        may_write: true,
                    },
                    |effects, arg| effects.union(arg.effects),
                );
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call { callee, args },
                    ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    span: span.clone(),
                }
            }
            _ => {
                return Err(Diagnostic::unsupported(
                    format!("expression {expr:?} is not implemented by the HIR/MIR backend"),
                    span,
                ));
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, &span)?;
        }
        Ok(lowered)
    }
}
