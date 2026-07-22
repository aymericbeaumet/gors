//! Exact constants, operator typing, coercion, and literal decoding.

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::token::Token;

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::SourceSpan;
use crate::compiler::types::{ConstValue, IntTy, Ty, UntypedTy};

pub(super) fn default_expr_type(mut expr: hir::Expr) -> Result<hir::Expr, Diagnostic> {
    let ty = expr.ty.default_typed();
    ensure_bootstrap_value_type(&ty, &expr.span)?;
    let span = expr.span.clone();
    coerce_expr(&mut expr, &ty, &span)?;
    Ok(expr)
}

pub(super) fn expr_constant(expr: &hir::Expr) -> Option<&ConstValue> {
    match &expr.kind {
        hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) => Some(value),
        _ => None,
    }
}

pub(super) fn fold_constant_unary(
    op: hir::UnaryOp,
    value: &ConstValue,
    span: &SourceSpan,
) -> Result<Option<ConstValue>, Diagnostic> {
    let folded = match (op, value) {
        (hir::UnaryOp::Positive, ConstValue::Int(_)) => value.clone(),
        (hir::UnaryOp::Negative, ConstValue::Int(value)) => {
            let value = BigInt::parse_bytes(value.as_bytes(), 10).ok_or_else(|| {
                Diagnostic::semantic("invalid exact integer constant", span.clone())
            })?;
            ConstValue::Int((-value).to_string())
        }
        (hir::UnaryOp::Not, ConstValue::Bool(value)) => ConstValue::Bool(!value),
        (hir::UnaryOp::BitNot, ConstValue::Int(value)) => {
            let value = BigInt::parse_bytes(value.as_bytes(), 10).ok_or_else(|| {
                Diagnostic::semantic("invalid exact integer constant", span.clone())
            })?;
            ConstValue::Int((!value).to_string())
        }
        _ => return Ok(None),
    };
    Ok(Some(folded))
}

pub(super) fn fold_constant_binary(
    op: hir::BinaryOp,
    left: &ConstValue,
    right: &ConstValue,
    span: &SourceSpan,
) -> Result<Option<ConstValue>, Diagnostic> {
    let folded = match (left, right) {
        (ConstValue::Int(left), ConstValue::Int(right)) => {
            let left = BigInt::parse_bytes(left.as_bytes(), 10).ok_or_else(|| {
                Diagnostic::semantic("invalid exact integer constant", span.clone())
            })?;
            let right = BigInt::parse_bytes(right.as_bytes(), 10).ok_or_else(|| {
                Diagnostic::semantic("invalid exact integer constant", span.clone())
            })?;
            match op {
                hir::BinaryOp::Add => ConstValue::Int((left + right).to_string()),
                hir::BinaryOp::Sub => ConstValue::Int((left - right).to_string()),
                hir::BinaryOp::Mul => ConstValue::Int((left * right).to_string()),
                hir::BinaryOp::Div | hir::BinaryOp::Rem if right.is_zero() => {
                    return Err(Diagnostic::semantic("division by zero", span.clone()));
                }
                hir::BinaryOp::Div => ConstValue::Int((left / right).to_string()),
                hir::BinaryOp::Rem => ConstValue::Int((left % right).to_string()),
                hir::BinaryOp::BitAnd => ConstValue::Int((left & right).to_string()),
                hir::BinaryOp::BitOr => ConstValue::Int((left | right).to_string()),
                hir::BinaryOp::BitXor => ConstValue::Int((left ^ right).to_string()),
                hir::BinaryOp::AndNot => ConstValue::Int((left & !right).to_string()),
                hir::BinaryOp::Shl | hir::BinaryOp::Shr => {
                    let shift = right.to_usize().ok_or_else(|| {
                        Diagnostic::semantic(
                            "shift count must be a non-negative integer",
                            span.clone(),
                        )
                    })?;
                    if shift > 4096 {
                        return Err(Diagnostic::unsupported(
                            "constant shifts larger than 4096 bits are not implemented",
                            span.clone(),
                        ));
                    }
                    let value = if op == hir::BinaryOp::Shl {
                        left << shift
                    } else {
                        left >> shift
                    };
                    ConstValue::Int(value.to_string())
                }
                hir::BinaryOp::Equal => ConstValue::Bool(left == right),
                hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
                hir::BinaryOp::Less => ConstValue::Bool(left < right),
                hir::BinaryOp::LessEqual => ConstValue::Bool(left <= right),
                hir::BinaryOp::Greater => ConstValue::Bool(left > right),
                hir::BinaryOp::GreaterEqual => ConstValue::Bool(left >= right),
                hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => return Ok(None),
            }
        }
        (ConstValue::Bool(left), ConstValue::Bool(right)) => match op {
            hir::BinaryOp::Equal => ConstValue::Bool(left == right),
            hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
            hir::BinaryOp::LogicalAnd => ConstValue::Bool(*left && *right),
            hir::BinaryOp::LogicalOr => ConstValue::Bool(*left || *right),
            _ => return Ok(None),
        },
        (ConstValue::String(left), ConstValue::String(right)) => match op {
            hir::BinaryOp::Add => {
                let mut result = left.clone();
                result.extend_from_slice(right);
                ConstValue::String(result)
            }
            hir::BinaryOp::Equal => ConstValue::Bool(left == right),
            hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
            hir::BinaryOp::Less => ConstValue::Bool(left < right),
            hir::BinaryOp::LessEqual => ConstValue::Bool(left <= right),
            hir::BinaryOp::Greater => ConstValue::Bool(left > right),
            hir::BinaryOp::GreaterEqual => ConstValue::Bool(left >= right),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(Some(folded))
}

pub(super) fn coerce_expr(
    expr: &mut hir::Expr,
    expected: &Ty,
    span: &SourceSpan,
) -> Result<(), Diagnostic> {
    if !is_assignable(&expr.ty, expected) {
        return Err(Diagnostic::semantic(
            format!("cannot use {:?} as {expected:?}", expr.ty),
            span.clone(),
        ));
    }
    if let hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) = &expr.kind
        && !value.is_representable_as(expected)
    {
        return Err(Diagnostic::semantic(
            format!("constant is not representable as {expected:?}"),
            span.clone(),
        ));
    }
    expr.ty = expected.clone();
    Ok(())
}

pub(super) fn is_assignable(actual: &Ty, expected: &Ty) -> bool {
    if actual == expected {
        return true;
    }
    matches!(
        (actual, expected),
        (Ty::Untyped(UntypedTy::Bool), Ty::Bool)
            | (
                Ty::Untyped(UntypedTy::Int),
                Ty::Int(_) | Ty::Uint(_) | Ty::Float(_)
            )
            | (Ty::Untyped(UntypedTy::Float), Ty::Float(_))
            | (Ty::Untyped(UntypedTy::String), Ty::String)
    )
}

pub(super) fn common_operand_type(left: &Ty, right: &Ty) -> Option<Ty> {
    if left == right {
        return Some(left.default_typed());
    }
    if is_assignable(left, right) {
        return Some(right.clone());
    }
    if is_assignable(right, left) {
        return Some(left.clone());
    }
    None
}

pub(super) fn exact_common_operand_type(left: &Ty, right: &Ty) -> Option<Ty> {
    if left == right {
        return Some(left.clone());
    }
    if is_assignable(left, right) {
        return Some(right.clone());
    }
    if is_assignable(right, left) {
        return Some(left.clone());
    }
    None
}

pub(super) fn is_bool(ty: &Ty) -> bool {
    matches!(ty, Ty::Bool | Ty::Untyped(UntypedTy::Bool))
}

pub(super) fn ensure_bootstrap_value_type(ty: &Ty, span: &SourceSpan) -> Result<(), Diagnostic> {
    if ty.is_bootstrap_value() {
        Ok(())
    } else {
        Err(Diagnostic::unsupported(
            format!("type {ty:?} is outside the bootstrap bool/int/string runtime frontier"),
            span.clone(),
        ))
    }
}

pub(super) fn validate_binary_operator(
    op: hir::BinaryOp,
    ty: &Ty,
    span: &SourceSpan,
) -> Result<(), Diagnostic> {
    let valid = match op {
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => *ty == Ty::Bool,
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            matches!(ty, Ty::Bool | Ty::Int(IntTy::Int) | Ty::String)
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => matches!(ty, Ty::Int(IntTy::Int) | Ty::String),
        hir::BinaryOp::Add => matches!(ty, Ty::Int(IntTy::Int) | Ty::String),
        hir::BinaryOp::Sub
        | hir::BinaryOp::Mul
        | hir::BinaryOp::Div
        | hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => *ty == Ty::Int(IntTy::Int),
    };
    if valid {
        Ok(())
    } else {
        Err(Diagnostic::semantic(
            format!("operator {op:?} is invalid for {ty:?}"),
            span.clone(),
        ))
    }
}

pub(super) fn assignment_binary_op(op: hir::AssignOp) -> hir::BinaryOp {
    match op {
        hir::AssignOp::Set | hir::AssignOp::Add => hir::BinaryOp::Add,
        hir::AssignOp::Sub => hir::BinaryOp::Sub,
        hir::AssignOp::Mul => hir::BinaryOp::Mul,
        hir::AssignOp::Div => hir::BinaryOp::Div,
        hir::AssignOp::Rem => hir::BinaryOp::Rem,
        hir::AssignOp::BitAnd => hir::BinaryOp::BitAnd,
        hir::AssignOp::BitOr => hir::BinaryOp::BitOr,
        hir::AssignOp::BitXor => hir::BinaryOp::BitXor,
        hir::AssignOp::Shl => hir::BinaryOp::Shl,
        hir::AssignOp::Shr => hir::BinaryOp::Shr,
        hir::AssignOp::AndNot => hir::BinaryOp::AndNot,
    }
}

pub(super) fn lower_binary_op(token: Token) -> Option<hir::BinaryOp> {
    Some(match token {
        Token::ADD => hir::BinaryOp::Add,
        Token::SUB => hir::BinaryOp::Sub,
        Token::MUL => hir::BinaryOp::Mul,
        Token::QUO => hir::BinaryOp::Div,
        Token::REM => hir::BinaryOp::Rem,
        Token::AND => hir::BinaryOp::BitAnd,
        Token::OR => hir::BinaryOp::BitOr,
        Token::XOR => hir::BinaryOp::BitXor,
        Token::SHL => hir::BinaryOp::Shl,
        Token::SHR => hir::BinaryOp::Shr,
        Token::AND_NOT => hir::BinaryOp::AndNot,
        Token::EQL => hir::BinaryOp::Equal,
        Token::NEQ => hir::BinaryOp::NotEqual,
        Token::LSS => hir::BinaryOp::Less,
        Token::LEQ => hir::BinaryOp::LessEqual,
        Token::GTR => hir::BinaryOp::Greater,
        Token::GEQ => hir::BinaryOp::GreaterEqual,
        Token::LAND => hir::BinaryOp::LogicalAnd,
        Token::LOR => hir::BinaryOp::LogicalOr,
        _ => return None,
    })
}

pub(super) fn parse_go_integer(value: &str) -> Option<String> {
    let cleaned = value.replace('_', "");
    let (radix, digits) = if let Some(rest) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        (2, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        (16, rest)
    } else if cleaned.len() > 1 && cleaned.starts_with('0') {
        (8, cleaned.trim_start_matches('0'))
    } else {
        (10, cleaned.as_str())
    };
    BigInt::parse_bytes(
        if digits.is_empty() {
            b"0"
        } else {
            digits.as_bytes()
        },
        radix,
    )
    .map(|value| value.to_string())
}

pub(super) fn parse_go_string(value: &str) -> Option<Vec<u8>> {
    if value.len() < 2 {
        return None;
    }
    let delimiter = value.as_bytes().first().copied()?;
    if value.as_bytes().last().copied()? != delimiter {
        return None;
    }
    let inner = value.get(1..value.len().checked_sub(1)?)?;
    if delimiter == b'`' {
        return Some(inner.bytes().filter(|byte| *byte != b'\r').collect());
    }
    (delimiter == b'"')
        .then(|| parse_interpreted_bytes(inner))
        .flatten()
}

pub(super) fn parse_go_rune(value: &str) -> Option<u32> {
    if value.len() < 2 || !value.starts_with('\'') || !value.ends_with('\'') {
        return None;
    }
    let bytes = parse_interpreted_bytes(&value[1..value.len() - 1])?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut chars = text.chars();
    let value = chars.next()? as u32;
    chars.next().is_none().then_some(value)
}

fn parse_interpreted_bytes(value: &str) -> Option<Vec<u8>> {
    let mut result = Vec::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut bytes = [0; 4];
            result.extend_from_slice(ch.encode_utf8(&mut bytes).as_bytes());
            continue;
        }
        match chars.next()? {
            'a' => result.push(0x07),
            'b' => result.push(0x08),
            'f' => result.push(0x0c),
            'n' => result.push(b'\n'),
            'r' => result.push(b'\r'),
            't' => result.push(b'\t'),
            'v' => result.push(0x0b),
            '\\' => result.push(b'\\'),
            '\'' => result.push(b'\''),
            '"' => result.push(b'"'),
            'x' => result.push(parse_radix_escape(&mut chars, 2, 16)? as u8),
            'u' => push_rune_bytes(&mut result, parse_radix_escape(&mut chars, 4, 16)?)?,
            'U' => push_rune_bytes(&mut result, parse_radix_escape(&mut chars, 8, 16)?)?,
            first @ '0'..='7' => {
                let mut digits = String::from(first);
                digits.extend(chars.by_ref().take(2));
                if digits.len() != 3 {
                    return None;
                }
                result.push(u8::from_str_radix(&digits, 8).ok()?);
            }
            _ => return None,
        }
    }
    Some(result)
}

fn parse_radix_escape(
    chars: &mut impl Iterator<Item = char>,
    count: usize,
    radix: u32,
) -> Option<u32> {
    let digits = chars.take(count).collect::<String>();
    (digits.len() == count)
        .then(|| u32::from_str_radix(&digits, radix).ok())
        .flatten()
}

fn push_rune_bytes(result: &mut Vec<u8>, value: u32) -> Option<()> {
    let value = char::from_u32(value)?;
    let mut bytes = [0; 4];
    result.extend_from_slice(value.encode_utf8(&mut bytes).as_bytes());
    Some(())
}
