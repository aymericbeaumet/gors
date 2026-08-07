//! Exact constants, operator typing, coercion, and literal decoding.

use num_bigint::BigInt;

pub(super) use super::constant_ops::{
    constant_shift_integer_operand, fold_constant_binary, fold_constant_unary,
    fold_untyped_constant_shift,
};

use crate::token::Token;

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ComplexTy, ConstValue, ExactNumber, FloatTy, Ty, UntypedTy};

pub(super) fn default_expr_type(
    mut expr: hir::Expr,
    source: SourceRef,
) -> Result<hir::Expr, Diagnostic> {
    let ty = expr.ty.default_typed();
    ensure_bootstrap_value_type(&ty, source)?;
    coerce_expr(&mut expr, &ty, source)?;
    Ok(expr)
}

pub(super) fn expr_constant(expr: &hir::Expr) -> Option<&ConstValue> {
    match &expr.kind {
        hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) => Some(value),
        _ => None,
    }
}

pub(super) fn normalize_constant_for_type(
    value: ConstValue,
    ty: &Ty,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    if !value.is_representable_as(ty) {
        return Err(Diagnostic::semantic(
            format!("constant result is not representable as {ty:?}"),
            source,
        ));
    }
    Ok(value.normalized_for(ty))
}

pub(super) fn coerce_expr(
    expr: &mut hir::Expr,
    expected: &Ty,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    if !is_assignable(&expr.ty, expected) {
        return Err(Diagnostic::semantic(
            format!("cannot use {:?} as {expected:?}", expr.ty),
            source,
        ));
    }
    if let hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value) = &mut expr.kind
    {
        *value = normalize_constant_for_type(value.clone(), expected, source)?;
    }
    if matches!(expr.ty, Ty::Untyped(_))
        && expected.is_integer()
        && let hir::ExprKind::Binary {
            op: hir::BinaryOp::Shl | hir::BinaryOp::Shr,
            left,
            ..
        } = &mut expr.kind
    {
        coerce_expr(left, expected, source)?;
    }
    let representation_preserving_conversion = expr.ty != *expected
        && (expr.ty.underlying() == expected.underlying()
            || matches!(
                (expr.ty.underlying(), expected.underlying()),
                (Ty::Channel(_, actual), Ty::Channel(_, expected)) if actual == expected
            ));
    if representation_preserving_conversion
        && !matches!(
            expr.kind,
            hir::ExprKind::Constant(_) | hir::ExprKind::GlobalConstant(_, _)
        )
    {
        let value = expr.clone();
        expr.kind = hir::ExprKind::Conversion {
            value: Box::new(value),
        };
    }
    expr.ty = expected.clone();
    Ok(())
}

pub(super) fn is_assignable(actual: &Ty, expected: &Ty) -> bool {
    if same_semantic_type(actual, expected) {
        return true;
    }
    if actual.underlying() == expected.underlying()
        && (!is_defined_type(actual) || !is_defined_type(expected))
    {
        return true;
    }
    if let Ty::Named { underlying, .. } | Ty::LocalNamed { underlying, .. } = expected
        && matches!(actual, Ty::Untyped(_))
    {
        return is_assignable(actual, underlying);
    }
    if let (
        Ty::Channel(actual_direction, actual_element),
        Ty::Channel(expected_direction, expected_element),
    ) = (actual.underlying(), expected.underlying())
    {
        return actual_element == expected_element
            && (*actual_direction == *expected_direction
                || *actual_direction == crate::compiler::types::ChannelDir::SendReceive);
    }
    matches!(
        (actual, expected),
        (Ty::Untyped(UntypedTy::Bool), Ty::Bool)
            | (
                Ty::Untyped(UntypedTy::Int),
                Ty::Untyped(UntypedTy::Rune)
                    | Ty::Untyped(UntypedTy::Float)
                    | Ty::Untyped(UntypedTy::Complex)
                    | Ty::Int(_)
                    | Ty::Uint(_)
                    | Ty::Float(_)
                    | Ty::Complex(_)
            )
            | (
                Ty::Untyped(UntypedTy::Rune),
                Ty::Untyped(UntypedTy::Float | UntypedTy::Complex)
                    | Ty::Int(_)
                    | Ty::Uint(_)
                    | Ty::Float(_)
                    | Ty::Complex(_)
            )
            | (
                Ty::Untyped(UntypedTy::Float),
                Ty::Untyped(UntypedTy::Complex)
                    | Ty::Int(_)
                    | Ty::Uint(_)
                    | Ty::Float(_)
                    | Ty::Complex(_)
            )
            | (Ty::Untyped(UntypedTy::Complex), Ty::Complex(_))
            | (Ty::Untyped(UntypedTy::String), Ty::String)
    )
}

fn is_defined_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Named { .. } | Ty::LocalNamed { .. })
}

fn same_semantic_type(left: &Ty, right: &Ty) -> bool {
    if left == right {
        return true;
    }
    match (left, right) {
        (
            Ty::Named {
                definition: left, ..
            }
            | Ty::NamedRef { definition: left },
            Ty::Named {
                definition: right, ..
            }
            | Ty::NamedRef { definition: right },
        ) => left == right,
        (Ty::Pointer(left), Ty::Pointer(right)) | (Ty::Slice(left), Ty::Slice(right)) => {
            same_semantic_type(left, right)
        }
        (Ty::Array(left_length, left), Ty::Array(right_length, right)) => {
            left_length == right_length && same_semantic_type(left, right)
        }
        (Ty::Map(left_key, left_value), Ty::Map(right_key, right_value)) => {
            same_semantic_type(left_key, right_key) && same_semantic_type(left_value, right_value)
        }
        (Ty::Channel(left_direction, left), Ty::Channel(right_direction, right)) => {
            left_direction == right_direction && same_semantic_type(left, right)
        }
        _ => false,
    }
}

pub(super) fn common_operand_type(left: &Ty, right: &Ty) -> Option<Ty> {
    if left == right {
        return Some(left.default_typed());
    }
    if is_assignable(left, right) {
        return Some(right.default_typed());
    }
    if is_assignable(right, left) {
        return Some(left.default_typed());
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
    matches!(ty.underlying(), Ty::Bool | Ty::Untyped(UntypedTy::Bool))
}

pub(super) fn ensure_bootstrap_value_type(ty: &Ty, source: SourceRef) -> Result<(), Diagnostic> {
    if ty.is_bootstrap_value() {
        Ok(())
    } else {
        Err(Diagnostic::unsupported(
            format!("values of type {ty:?} are not yet supported"),
            source,
        ))
    }
}

pub(super) fn validate_binary_operator(
    op: hir::BinaryOp,
    ty: &Ty,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let ty = ty.underlying();
    let valid = match op {
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => *ty == Ty::Bool,
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            matches!(
                ty,
                Ty::Bool
                    | Ty::Int(_)
                    | Ty::Uint(_)
                    | Ty::Float(_)
                    | Ty::Complex(ComplexTy::Complex128)
                    | Ty::String
            ) || ty.is_bootstrap_comparable_aggregate()
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => {
            matches!(ty, Ty::Int(_) | Ty::Uint(_) | Ty::Float(_) | Ty::String)
        }
        hir::BinaryOp::Add => matches!(
            ty,
            Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(FloatTy::Float64)
                | Ty::Complex(ComplexTy::Complex128)
                | Ty::String
        ),
        hir::BinaryOp::Sub | hir::BinaryOp::Mul => matches!(
            ty,
            Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(FloatTy::Float64)
                | Ty::Complex(ComplexTy::Complex128)
        ),
        hir::BinaryOp::Div => matches!(
            ty,
            Ty::Int(_)
                | Ty::Uint(_)
                | Ty::Float(FloatTy::Float64)
                | Ty::Complex(ComplexTy::Complex128)
        ),
        hir::BinaryOp::Min | hir::BinaryOp::Max => {
            matches!(ty, Ty::Int(_) | Ty::Uint(_) | Ty::Float(_))
        }
        hir::BinaryOp::Complex => *ty == Ty::Float(FloatTy::Float64),
        hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::AndNot => matches!(ty, Ty::Int(_) | Ty::Uint(_)),
        hir::BinaryOp::Rem | hir::BinaryOp::Shl | hir::BinaryOp::Shr => {
            matches!(ty, Ty::Int(_) | Ty::Uint(_))
        }
    };
    if valid {
        Ok(())
    } else {
        Err(Diagnostic::semantic(
            format!("operator {op:?} is invalid for {ty:?}"),
            source,
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

/// Exact imaginary component under Go's leading-zero compatibility rule.
pub(super) fn parse_go_imaginary(value: &str) -> Option<ExactNumber> {
    let component = value.strip_suffix('i')?;
    let cleaned = component.replace('_', "");
    if component
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'_')
    {
        return BigInt::parse_bytes(cleaned.as_bytes(), 10)
            .and_then(|value| ExactNumber::from_integer_spelling(&value.to_string()));
    }
    parse_go_integer(component)
        .and_then(|value| ExactNumber::from_integer_spelling(&value))
        .or_else(|| ExactNumber::from_spelling(&cleaned))
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
    let inner = &value[1..value.len() - 1];
    if let Some(escaped) = inner.strip_prefix('\\') {
        return match escaped {
            "a" => Some(0x07),
            "b" => Some(0x08),
            "f" => Some(0x0c),
            "n" => Some(u32::from(b'\n')),
            "r" => Some(u32::from(b'\r')),
            "t" => Some(u32::from(b'\t')),
            "v" => Some(0x0b),
            "\\" => Some(u32::from(b'\\')),
            "'" => Some(u32::from(b'\'')),
            "\"" => Some(u32::from(b'"')),
            _ if escaped.len() == 3 && escaped.starts_with('x') => {
                u32::from_str_radix(&escaped[1..], 16).ok()
            }
            _ if escaped.len() == 5 && escaped.starts_with('u') => {
                parse_unicode_rune_escape(&escaped[1..])
            }
            _ if escaped.len() == 9 && escaped.starts_with('U') => {
                parse_unicode_rune_escape(&escaped[1..])
            }
            _ if escaped.len() == 3 && escaped.bytes().all(|byte| matches!(byte, b'0'..=b'7')) => {
                u32::from_str_radix(escaped, 8).ok()
            }
            _ => None,
        };
    }
    let mut chars = inner.chars();
    let value = chars.next()? as u32;
    chars.next().is_none().then_some(value)
}

fn parse_unicode_rune_escape(digits: &str) -> Option<u32> {
    let value = u32::from_str_radix(digits, 16).ok()?;
    char::from_u32(value).map(u32::from)
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

#[cfg(test)]
mod rune_literal_tests {
    use super::parse_go_rune;

    #[test]
    fn rune_escapes_decode_to_code_points_instead_of_utf8_bytes() {
        assert_eq!(parse_go_rune(r"'\a'"), Some(7));
        assert_eq!(parse_go_rune(r"'\377'"), Some(255));
        assert_eq!(parse_go_rune(r"'\xff'"), Some(255));
        assert_eq!(parse_go_rune(r#"'\"'"#), Some(u32::from(b'"')));
        assert_eq!(parse_go_rune(r"'\u12e4'"), Some(0x12e4));
        assert_eq!(parse_go_rune(r"'\U00101234'"), Some(0x10_1234));
        assert_eq!(parse_go_rune("'ä'"), Some(0xe4));
        assert_eq!(parse_go_rune("'本'"), Some(0x672c));
    }

    #[test]
    fn rune_decoder_rejects_multiple_values_and_invalid_unicode() {
        assert_eq!(parse_go_rune("'ab'"), None);
        assert_eq!(parse_go_rune(r"'\uD800'"), None);
        assert_eq!(parse_go_rune(r"'\U00110000'"), None);
    }
}

#[cfg(test)]
mod imaginary_literal_tests {
    use super::parse_go_imaginary;

    #[test]
    fn decimal_only_imaginary_components_ignore_legacy_octal_rules() {
        assert_eq!(
            parse_go_imaginary("0123i")
                .map(|v| v.to_string())
                .as_deref(),
            Some("123")
        );
        assert_eq!(
            parse_go_imaginary("0_123i")
                .map(|v| v.to_string())
                .as_deref(),
            Some("123")
        );
        assert_eq!(
            parse_go_imaginary("0o123i")
                .map(|v| v.to_string())
                .as_deref(),
            Some("83")
        );
        assert_eq!(
            parse_go_imaginary("0xabci")
                .map(|v| v.to_string())
                .as_deref(),
            Some("2748")
        );
        assert_eq!(
            parse_go_imaginary("0x1p-2i")
                .map(|v| v.to_string())
                .as_deref(),
            Some("1/4")
        );
    }
}
