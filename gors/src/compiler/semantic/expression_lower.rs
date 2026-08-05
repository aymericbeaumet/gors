//! Typed expression and direct-call lowering over owned structural syntax.

use crate::token::Token;

use super::FunctionLowerer;
use super::eval_constant;
use super::expressions::*;
use super::lower_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Ty, UntypedTy};

impl FunctionLowerer {
    pub(super) fn local_expr(&self, node: NodeId, local: LocalId, ty: Ty) -> hir::Expr {
        hir::Expr {
            node,
            kind: hir::ExprKind::Local(local),
            ty,
            category: hir::ValueCategory::Place,
            effects: hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            source: SourceRef::node(node),
        }
    }

    pub(super) fn lower_expr(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        self.lower_expr_inner(expr, expected, false)
    }

    pub(super) fn lower_expr_inner(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let node = self.alloc_node(expr.source)?;
        let source = SourceRef::node(node);
        let mut lowered = match &expr.kind {
            ExprSyntaxKind::Literal { .. } => {
                let (ty, value) = eval_constant(expr, &self.constants, source, 0)?;
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Constant(value),
                    ty,
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    source,
                }
            }
            ExprSyntaxKind::Ident(ident) => {
                let name = ident.name.as_ref();
                if let Some(local) = self.lookup_local(name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    self.local_expr(node, local, ty)
                } else if let Some(constant) = self.constants.get(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalConstant(constant.id, constant.value),
                        ty: constant.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if self.functions.contains_key(name) {
                    return Err(Diagnostic::unsupported(
                        format!("function value {name} is not implemented by the HIR/MIR backend"),
                        source,
                    ));
                } else if matches!(name, "true" | "false") {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(ConstValue::Bool(name == "true")),
                        ty: Ty::Untyped(UntypedTy::Bool),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if matches!(name, "print" | "println") {
                    return Err(Diagnostic::unsupported(
                        format!("builtin value {name} is not implemented"),
                        source,
                    ));
                } else {
                    return Err(Diagnostic::semantic(
                        format!("undefined identifier {name}"),
                        source,
                    ));
                }
            }
            ExprSyntaxKind::Paren(expression) => {
                return self.lower_expr_inner(expression, expected, allow_discarded_call_result);
            }
            ExprSyntaxKind::Unary { token, expression } => {
                let mut operand = self.lower_expr(expression, expected)?;
                let operand_ty = operand.ty.default_typed();
                ensure_bootstrap_value_type(&operand_ty, source)?;
                let operator_ty = operand_ty.underlying();
                let op = match *token {
                    Token::ADD if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Positive,
                    Token::ADD
                        if matches!(
                            operator_ty,
                            Ty::Float(FloatTy::Float64) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Positive
                    }
                    Token::SUB if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::Negative,
                    Token::SUB
                        if matches!(
                            operator_ty,
                            Ty::Float(FloatTy::Float64) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Negative
                    }
                    Token::NOT if is_bool(&operand_ty) => hir::UnaryOp::Not,
                    Token::XOR if *operator_ty == Ty::Int(IntTy::Int) => hir::UnaryOp::BitNot,
                    _ => {
                        return Err(Diagnostic::semantic(
                            format!("invalid unary {token:?} operand {:?}", operand.ty),
                            source,
                        ));
                    }
                };
                if let Some(value) = expr_constant(&operand)
                    .map(|value| fold_constant_unary(op, value, source))
                    .transpose()?
                    .flatten()
                {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: operand.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else {
                    coerce_expr(&mut operand, &operand_ty, source)?;
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
                        source,
                    }
                }
            }
            ExprSyntaxKind::Binary { left, token, right } => {
                let mut left = self.lower_expr(left, None)?;
                let mut right = self.lower_expr(right, None)?;
                let op = lower_binary_op(*token).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("binary operator {token:?} is not implemented"),
                        source,
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
                        source,
                    )
                })?;
                coerce_expr(&mut left, &operand_ty, source)?;
                coerce_expr(&mut right, &operand_ty, source)?;
                validate_binary_operator(op, &operand_ty, source)?;
                let mut effects = left.effects.union(right.effects);
                if matches!(
                    op,
                    hir::BinaryOp::Div
                        | hir::BinaryOp::Rem
                        | hir::BinaryOp::Shl
                        | hir::BinaryOp::Shr
                ) {
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
                    .map(|(left, right)| fold_constant_binary(op, left, right, source))
                    .transpose()?
                    .flatten();
                if let Some(value) = folded {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: result_ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
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
                        source,
                    }
                }
            }
            ExprSyntaxKind::Call { callee, arguments } => {
                let ExprSyntaxKind::Ident(callee_ident) = &callee.kind else {
                    return Err(Diagnostic::unsupported(
                        "only direct calls are implemented by the HIR/MIR backend",
                        source,
                    ));
                };
                let name = callee_ident.name.as_ref();
                if self.type_aliases.contains_key(name)
                    || matches!(name, "bool" | "string" | "int" | "float64" | "complex128")
                {
                    let [argument] = arguments.as_ref() else {
                        return Err(Diagnostic::semantic(
                            format!("conversion to {name} requires exactly one argument"),
                            source,
                        ));
                    };
                    let target = lower_type(callee, &self.type_aliases, source)?;
                    let mut argument = self.lower_expr(argument, None)?;
                    if is_assignable(&argument.ty, &target) {
                        coerce_expr(&mut argument, &target, source)?;
                    } else if argument.ty.underlying() == target.underlying() {
                        let effects = argument.effects;
                        return Ok(hir::Expr {
                            node,
                            kind: hir::ExprKind::Conversion {
                                value: Box::new(argument),
                            },
                            ty: target,
                            category: hir::ValueCategory::Value,
                            effects,
                            source,
                        });
                    } else {
                        return Err(Diagnostic::unsupported(
                            format!(
                                "conversion from {:?} to {name} requires a representation change",
                                argument.ty
                            ),
                            source,
                        ));
                    }
                    return Ok(argument);
                }
                if matches!(name, "real" | "imag") {
                    let [argument] = arguments.as_ref() else {
                        return Err(Diagnostic::semantic(
                            format!("call to {name} requires exactly one argument"),
                            source,
                        ));
                    };
                    let argument = self.lower_expr(argument, None)?;
                    let Some(ConstValue::Complex { real, imag }) = expr_constant(&argument) else {
                        return Err(Diagnostic::unsupported(
                            format!("{name} of a non-constant complex value is not implemented"),
                            source,
                        ));
                    };
                    let mut component = hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(ConstValue::Float(if name == "real" {
                            real.clone()
                        } else {
                            imag.clone()
                        })),
                        ty: Ty::Untyped(UntypedTy::Float),
                        category: hir::ValueCategory::Constant,
                        effects: argument.effects,
                        source,
                    };
                    if let Some(expected) = expected {
                        coerce_expr(&mut component, expected, source)?;
                    }
                    return Ok(component);
                }
                let (callee, params, results) = if self.lookup_local(name).is_some() {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "calling local value {name} requires function-value HIR and is not implemented"
                        ),
                        source,
                    ));
                } else if let Some(symbol) = self.functions.get(name).cloned() {
                    (
                        hir::Callee::Function(symbol.id),
                        symbol.signature.params,
                        symbol.signature.results,
                    )
                } else if self.constants.contains_key(name) {
                    return Err(Diagnostic::semantic(
                        format!("constant {name} is not callable"),
                        source,
                    ));
                } else {
                    match name {
                        "print" => (hir::Callee::Builtin(hir::Builtin::Print), vec![], vec![]),
                        "println" => (hir::Callee::Builtin(hir::Builtin::Println), vec![], vec![]),
                        "panic" => (hir::Callee::Builtin(hir::Builtin::Panic), vec![], vec![]),
                        name => {
                            return Err(Diagnostic::semantic(
                                format!("undefined function {name}"),
                                source,
                            ));
                        }
                    }
                };
                match callee {
                    hir::Callee::Function(_) if arguments.len() != params.len() => {
                        return Err(Diagnostic::semantic(
                            format!(
                                "call has {} arguments; expected {}",
                                arguments.len(),
                                params.len()
                            ),
                            source,
                        ));
                    }
                    hir::Callee::Builtin(hir::Builtin::Panic) if arguments.len() != 1 => {
                        return Err(Diagnostic::semantic(
                            format!(
                                "call to panic has {} arguments; expected 1",
                                arguments.len()
                            ),
                            source,
                        ));
                    }
                    _ => {}
                }
                let args = if matches!(callee, hir::Callee::Builtin(_)) {
                    arguments
                        .iter()
                        .map(|argument| {
                            self.lower_expr(argument, None).and_then(|expression| {
                                let expression_source = expression.source;
                                default_expr_type(expression, expression_source)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    arguments
                        .iter()
                        .zip(&params)
                        .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
                        .collect::<Result<Vec<_>, _>>()?
                };
                let ty = match results.as_slice() {
                    [] => Ty::Unit,
                    [single] => single.clone(),
                    _ => {
                        return Err(Diagnostic::unsupported(
                            "multiple-result calls require explicit expression-arity HIR",
                            source,
                        ));
                    }
                };
                if ty == Ty::Unit && !allow_discarded_call_result {
                    return Err(Diagnostic::unsupported(
                        "a no-result call cannot be used as a value",
                        source,
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
                    |effects, argument| effects.union(argument.effects),
                );
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call { callee, args },
                    ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::Selector { .. } => {
                return Err(Diagnostic::unsupported(
                    "selector resolution is not implemented by the HIR/MIR backend",
                    source,
                ));
            }
            ExprSyntaxKind::Unsupported(kind) => {
                return Err(Diagnostic::unsupported(
                    format!("expression {kind} is not implemented by the HIR/MIR backend"),
                    source,
                ));
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }
}
