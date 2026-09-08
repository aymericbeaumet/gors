//! Go `len`/`cap` constness and representation-neutral operand classification.

use super::FunctionLowerer;
use super::channels::channel_parts;
use super::expressions::{
    coerce_expr, exact_common_operand_type, lower_binary_op, validate_binary_operator,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, IntTy, Ty, UintTy, UntypedTy};
use crate::token::Token;

mod check_only;
mod package_constants;

pub(super) use package_constants::{eval_package_constant, eval_package_constant_with_variables};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LengthCapacityOp {
    Len,
    Cap,
}

impl LengthCapacityOp {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Len => "len",
            Self::Cap => "cap",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LengthCapacityClass {
    Constant(u64),
    Runtime,
}

/// Classify a fully type-checked operand before any executable representation
/// is selected. `contains_call_or_receive` describes source semantics, not HIR
/// effects: a pure ordinary call still makes an array length non-constant.
pub(super) fn classify(
    operation: LengthCapacityOp,
    ty: &Ty,
    constant: Option<&ConstValue>,
    contains_call_or_receive: bool,
    source: SourceRef,
) -> Result<LengthCapacityClass, Diagnostic> {
    let underlying = ty.underlying();
    if matches!(underlying, Ty::String | Ty::Untyped(UntypedTy::String)) {
        if operation == LengthCapacityOp::Cap {
            return Err(invalid_operand(operation, ty, source));
        }
        return Ok(match constant {
            Some(ConstValue::String(bytes)) => LengthCapacityClass::Constant(
                u64::try_from(bytes.len())
                    .map_err(|_| Diagnostic::backend("constant string length exceeds u64"))?,
            ),
            _ => LengthCapacityClass::Runtime,
        });
    }

    if let Some(length) = array_or_pointer_length(underlying) {
        return Ok(if contains_call_or_receive {
            LengthCapacityClass::Runtime
        } else {
            LengthCapacityClass::Constant(length)
        });
    }

    let valid_runtime = match underlying {
        Ty::Slice(_) | Ty::Channel(_, _) => true,
        Ty::Map(_, _) => operation == LengthCapacityOp::Len,
        _ => false,
    };
    if valid_runtime {
        Ok(LengthCapacityClass::Runtime)
    } else {
        Err(invalid_operand(operation, ty, source))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CheckedOperand {
    pub(super) ty: Ty,
    pub(super) constant: Option<ConstValue>,
    pub(super) contains_call_or_receive: bool,
}

impl FunctionLowerer {
    pub(super) fn lower_length_capacity_builtin(
        &mut self,
        operation: LengthCapacityOp,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [operand_syntax] = arguments else {
            return Err(Diagnostic::semantic(
                format!("{} requires exactly one argument", operation.name()),
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic(
                format!("{} does not accept ...", operation.name()),
                source,
            ));
        }

        let checked = self.check_length_capacity_operand(operand_syntax, source, None);
        if let Ok(checked) = &checked
            && let LengthCapacityClass::Constant(length) = classify(
                operation,
                &checked.ty,
                checked.constant.as_ref(),
                checked.contains_call_or_receive,
                source,
            )?
        {
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Int(length.to_string())),
                ty: Ty::Untyped(UntypedTy::Int),
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }

        let operand = self.lower_expr(operand_syntax, None)?;
        let contains_call_or_receive = match &checked {
            Ok(checked) => checked.contains_call_or_receive,
            Err(check_error) => {
                let contains = self.source_contains_call_or_receive(operand_syntax);
                if array_or_pointer_length(&operand.ty).is_some() && !contains {
                    return Err(check_error.clone());
                }
                contains
            }
        };
        let classification = classify(
            operation,
            &operand.ty,
            super::expressions::expr_constant(&operand),
            contains_call_or_receive,
            source,
        )?;
        if matches!(classification, LengthCapacityClass::Constant(_)) {
            return Err(Diagnostic::unsupported(
                "constant array len/cap requires a check-only operand representation",
                source,
            ));
        }
        if let Some(length) = array_or_pointer_length(&operand.ty) {
            return self.lower_array_len(operand, length, node, source, expected);
        }

        let builtin = runtime_builtin(operation, &operand.ty, source)?;
        let mut effects = operand.effects;
        effects.may_call = true;
        if matches!(operand.ty.underlying(), Ty::Map(_, _)) {
            effects.may_read = true;
        }
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args: vec![operand],
            },
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    /// Type-check the operand facts needed by Go's special array len/cap rule
    /// without constructing executable HIR. Suppressed operands may use types
    /// that deliberately have no executable representation.
    pub(super) fn check_length_capacity_operand(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<CheckedOperand, Diagnostic> {
        match &expression.kind {
            ExprSyntaxKind::Literal { .. } => {
                let (ty, value) = self.eval_constant_expression(expression, source, iota)?;
                Ok(CheckedOperand {
                    ty,
                    constant: Some(value),
                    contains_call_or_receive: false,
                })
            }
            ExprSyntaxKind::Ident(identifier) => {
                self.check_length_capacity_identifier(identifier.name.as_ref(), source, iota)
            }
            ExprSyntaxKind::Paren(inner) => self.check_length_capacity_operand(inner, source, iota),
            ExprSyntaxKind::Unary {
                token: Token::ARROW,
                expression,
            } => {
                let channel = self.check_length_capacity_operand(expression, source, iota)?;
                let Ty::Channel(direction, element) = channel.ty.underlying() else {
                    return Err(Diagnostic::semantic(
                        "receive requires a channel operand",
                        source,
                    ));
                };
                if !direction.can_receive() {
                    return Err(Diagnostic::semantic(
                        "cannot receive from a send-only channel",
                        source,
                    ));
                }
                Ok(CheckedOperand {
                    ty: element.as_ref().clone(),
                    constant: None,
                    contains_call_or_receive: true,
                })
            }
            ExprSyntaxKind::Unary {
                token,
                expression: inner,
            } => {
                let operand = self.check_length_capacity_operand(inner, source, iota)?;
                let ty = match token {
                    Token::AND => {
                        if !source_is_addressable(inner, &|name| {
                            self.lookup_local(name).is_some() || self.variables.contains_key(name)
                        }) {
                            return Err(Diagnostic::semantic(
                                "cannot take the address of this expression",
                                source,
                            ));
                        }
                        Ty::Pointer(Box::new(operand.ty.clone()))
                    }
                    Token::MUL => {
                        let Ty::Pointer(element) = operand.ty.underlying() else {
                            return Err(Diagnostic::semantic(
                                "indirection requires a pointer operand",
                                source,
                            ));
                        };
                        element.as_ref().clone()
                    }
                    Token::ADD | Token::SUB if operand.ty.default_typed().is_numeric() => {
                        operand.ty.clone()
                    }
                    Token::NOT if operand.ty.default_typed().underlying() == &Ty::Bool => {
                        operand.ty.clone()
                    }
                    Token::XOR if operand.ty.default_typed().is_integer() => operand.ty.clone(),
                    _ => {
                        return Err(Diagnostic::semantic(
                            format!("invalid unary {token:?} operand {:?}", operand.ty),
                            source,
                        ));
                    }
                };
                let constant =
                    if operand.constant.is_some() && !matches!(token, Token::AND | Token::MUL) {
                        Some(self.eval_constant_expression(expression, source, iota)?.1)
                    } else {
                        None
                    };
                Ok(CheckedOperand {
                    ty,
                    constant,
                    contains_call_or_receive: operand.contains_call_or_receive,
                })
            }
            ExprSyntaxKind::Binary { left, token, right } => {
                let left = self.check_length_capacity_operand(left, source, iota)?;
                let right = self.check_length_capacity_operand(right, source, iota)?;
                let op = lower_binary_op(*token).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("binary operator {token:?} is not implemented"),
                        source,
                    )
                })?;
                let both_constant = left.constant.is_some() && right.constant.is_some();
                let operand_ty =
                    exact_common_operand_type(&left.ty, &right.ty).ok_or_else(|| {
                        Diagnostic::semantic(
                            format!("incompatible operands {:?} and {:?}", left.ty, right.ty),
                            source,
                        )
                    })?;
                if both_constant {
                    super::validate_constant_binary_operator(op, &operand_ty, source)?;
                } else {
                    validate_binary_operator(op, &operand_ty.default_typed(), source)?;
                }
                let comparison_or_logical = matches!(
                    op,
                    hir::BinaryOp::Equal
                        | hir::BinaryOp::NotEqual
                        | hir::BinaryOp::Less
                        | hir::BinaryOp::LessEqual
                        | hir::BinaryOp::Greater
                        | hir::BinaryOp::GreaterEqual
                        | hir::BinaryOp::LogicalAnd
                        | hir::BinaryOp::LogicalOr
                );
                let (ty, constant) = if both_constant {
                    let (ty, value) = self.eval_constant_expression(expression, source, iota)?;
                    (ty, Some(value))
                } else if comparison_or_logical {
                    (Ty::Bool, None)
                } else {
                    (operand_ty.default_typed(), None)
                };
                Ok(CheckedOperand {
                    ty,
                    constant,
                    contains_call_or_receive: left.contains_call_or_receive
                        || right.contains_call_or_receive,
                })
            }
            ExprSyntaxKind::Call {
                callee,
                arguments,
                spread,
            } => self
                .check_length_capacity_call(callee, arguments, *spread, expression, source, iota),
            ExprSyntaxKind::CompositeLiteral {
                ty: Some(ty),
                elements,
            } => self.check_length_capacity_composite(ty, elements, source, iota),
            _ => Err(Diagnostic::unsupported(
                "this len/cap operand form has no check-only semantic representation",
                source,
            )),
        }
    }
}

/// Addressability shapes used by the representation-free `len`/`cap`
/// checker. Name resolution remains caller-owned because function and package
/// constant queries have deliberately different symbol environments.
fn source_is_addressable(
    expression: &ExprSyntax,
    identifier_is_variable: &impl Fn(&str) -> bool,
) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => source_is_addressable(inner, identifier_is_variable),
        ExprSyntaxKind::Ident(identifier) => identifier_is_variable(identifier.name.as_ref()),
        ExprSyntaxKind::Unary {
            token: Token::MUL, ..
        }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Index { .. }
        | ExprSyntaxKind::Selector { .. } => true,
        _ => false,
    }
}

fn runtime_builtin(
    operation: LengthCapacityOp,
    ty: &Ty,
    source: SourceRef,
) -> Result<hir::Builtin, Diagnostic> {
    if let Some((_, _, representation)) = channel_parts(ty) {
        return Ok(match operation {
            LengthCapacityOp::Len => representation.builtins().len,
            LengthCapacityOp::Cap => representation.builtins().cap,
        });
    }
    let builtin = match (operation, ty.underlying()) {
        (LengthCapacityOp::Len, Ty::String) => hir::Builtin::StringLen,
        (LengthCapacityOp::Len, Ty::Map(key, element))
            if key.underlying() == &Ty::String && element.underlying() == &Ty::Int(IntTy::Int) =>
        {
            hir::Builtin::MapStringI64Len
        }
        (LengthCapacityOp::Len, Ty::Map(key, element))
            if key.underlying() == &Ty::Int(IntTy::Int) && element.underlying() == &Ty::String =>
        {
            hir::Builtin::MapI64GoStringLen
        }
        (LengthCapacityOp::Len, Ty::Map(key, element))
            if key.underlying() == &Ty::String
                && element.bootstrap_i64_struct_fields().is_some() =>
        {
            hir::Builtin::AggregateMapLen
        }
        (LengthCapacityOp::Len, Ty::Slice(element)) if element.uses_i64_slice_carrier() => {
            hir::Builtin::SliceI64Len
        }
        (LengthCapacityOp::Cap, Ty::Slice(element)) if element.uses_i64_slice_carrier() => {
            hir::Builtin::SliceI64Cap
        }
        (LengthCapacityOp::Len, Ty::Slice(element))
            if element.underlying() == &Ty::Uint(UintTy::Uint8) =>
        {
            hir::Builtin::SliceU8Len
        }
        // Every aggregate element shares the tagged interface-slice
        // representation, so its length is the same runtime operation.
        (LengthCapacityOp::Len, Ty::Slice(element))
            if element.bootstrap_i64_struct_fields().is_some()
                || matches!(element.underlying(), Ty::Interface(_))
                || element.interface_aggregate_struct_fields().is_some() =>
        {
            hir::Builtin::AggregateSliceLen
        }
        (LengthCapacityOp::Len, Ty::Slice(element)) if element.underlying() == &Ty::String => {
            hir::Builtin::SliceGoStringLen
        }
        (LengthCapacityOp::Cap, Ty::Slice(element)) if element.underlying() == &Ty::String => {
            hir::Builtin::SliceGoStringCap
        }
        _ => {
            return Err(Diagnostic::unsupported(
                format!(
                    "{} has no executable representation for {ty:?}",
                    operation.name()
                ),
                source,
            ));
        }
    };
    Ok(builtin)
}

fn is_nilable(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Pointer(_)
            | Ty::Slice(_)
            | Ty::Map(_, _)
            | Ty::Channel(_, _)
            | Ty::Function(_)
            | Ty::Interface(_)
    )
}

pub(super) fn array_or_pointer_length(ty: &Ty) -> Option<u64> {
    match ty.underlying() {
        Ty::Array(length, _) => Some(*length),
        Ty::Pointer(element) => match element.underlying() {
            Ty::Array(length, _) => Some(*length),
            _ => None,
        },
        _ => None,
    }
}

fn invalid_operand(operation: LengthCapacityOp, ty: &Ty, source: SourceRef) -> Diagnostic {
    Diagnostic::semantic(
        format!(
            "invalid argument: {} is not defined for {ty:?}",
            operation.name()
        ),
        source,
    )
}
