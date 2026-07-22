use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::token;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::constant_int::ExactInt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoType {
    Bool,
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uintptr,
    Float32,
    Float64,
    Complex64,
    Complex128,
    String,
    Slice(Box<GoType>),
    Map(Box<GoType>, Box<GoType>),
    Pointer(Box<GoType>),
    Array(Box<GoType>),
    Chan {
        elem: Box<GoType>,
        direction: GoChannelDirection,
    },
    Func {
        params: Vec<GoType>,
        results: Vec<GoType>,
        variadic_start: Option<usize>,
    },
    Named(std::string::String),
    Instantiated {
        name: std::string::String,
        args: Vec<GoType>,
    },
    Interface(std::string::String),
    Any,
    Error,
    Unit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoChannelDirection {
    Bidirectional,
    Send,
    Receive,
}

impl GoChannelDirection {
    pub fn from_ast_dir(dir: u8) -> Self {
        match dir {
            1 => Self::Send,
            2 => Self::Receive,
            _ => Self::Bidirectional,
        }
    }

    pub fn can_send(self) -> bool {
        matches!(self, Self::Bidirectional | Self::Send)
    }

    pub fn can_receive(self) -> bool {
        matches!(self, Self::Bidirectional | Self::Receive)
    }
}

impl GoType {
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            GoType::Int
                | GoType::Int8
                | GoType::Int16
                | GoType::Int32
                | GoType::Int64
                | GoType::Uint
                | GoType::Uint8
                | GoType::Uint16
                | GoType::Uint32
                | GoType::Uint64
                | GoType::Uintptr
        )
    }

    pub fn is_signed_int(&self) -> bool {
        matches!(
            self,
            GoType::Int | GoType::Int8 | GoType::Int16 | GoType::Int32 | GoType::Int64
        )
    }

    pub fn is_unsigned_int(&self) -> bool {
        matches!(
            self,
            GoType::Uint
                | GoType::Uint8
                | GoType::Uint16
                | GoType::Uint32
                | GoType::Uint64
                | GoType::Uintptr
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, GoType::Float32 | GoType::Float64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_string(&self) -> bool {
        matches!(self, GoType::String)
    }

    pub fn is_interface(&self) -> bool {
        matches!(self, GoType::Any | GoType::Error | GoType::Interface(_))
    }

    /// Returns the Rust type that len() returns for this type.
    pub fn len_type(&self) -> GoType {
        GoType::Int
    }

    pub fn from_expr(expr: &ast::Expr) -> GoType {
        match expr {
            ast::Expr::ParenExpr(paren) => GoType::from_expr(&paren.x),
            ast::Expr::Ident(id) => GoType::from_name(id.name),
            ast::Expr::StarExpr(star) => GoType::Pointer(Box::new(GoType::from_expr(&star.x))),
            ast::Expr::ArrayType(arr) => {
                let elem = arr.elt.as_ref();
                let elem_type = GoType::from_expr(elem);
                if arr.len.is_some() {
                    GoType::Array(Box::new(elem_type))
                } else {
                    GoType::Slice(Box::new(elem_type))
                }
            }
            ast::Expr::MapType(map) => GoType::Map(
                Box::new(GoType::from_expr(&map.key)),
                Box::new(GoType::from_expr(&map.value)),
            ),
            ast::Expr::ChanType(chan) => GoType::Chan {
                elem: Box::new(GoType::from_expr(&chan.value)),
                direction: GoChannelDirection::from_ast_dir(chan.dir),
            },
            ast::Expr::InterfaceType(_) => GoType::Any,
            ast::Expr::StructType(struct_type)
                if struct_type
                    .fields
                    .as_ref()
                    .is_none_or(|fields| fields.list.is_empty()) =>
            {
                GoType::Unit
            }
            ast::Expr::Ellipsis(e) => {
                if let Some(elt) = &e.elt {
                    GoType::Slice(Box::new(GoType::from_expr(elt)))
                } else {
                    GoType::Slice(Box::new(GoType::Any))
                }
            }
            ast::Expr::IndexExpr(index) => instantiate_named_type(
                GoType::from_expr(&index.x),
                vec![GoType::from_expr(&index.index)],
            ),
            ast::Expr::IndexListExpr(index) => instantiate_named_type(
                GoType::from_expr(&index.x),
                index.indices.iter().map(GoType::from_expr).collect(),
            ),
            ast::Expr::SelectorExpr(sel) => {
                if let ast::Expr::Ident(pkg) = &*sel.x {
                    GoType::Named(format!("{}.{}", pkg.name, sel.sel.name))
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::FuncType(ft) => GoType::from_func_type(ft),
            _ => GoType::Unknown,
        }
    }

    fn from_func_type(ft: &ast::FuncType) -> GoType {
        let mut params = Vec::new();
        let mut variadic_start = None;
        for f in &ft.params.list {
            let ty = f
                .type_
                .as_ref()
                .map(GoType::from_expr)
                .unwrap_or(GoType::Unknown);
            let count = f.names.as_ref().map_or(1, |n| n.len());
            if matches!(f.type_, Some(ast::Expr::Ellipsis(_))) && variadic_start.is_none() {
                variadic_start = Some(params.len());
            }
            params.extend(std::iter::repeat_n(ty, count));
        }
        let results: Vec<GoType> = ft
            .results
            .as_ref()
            .map(|r| {
                r.list
                    .iter()
                    .flat_map(|f| {
                        let ty = f
                            .type_
                            .as_ref()
                            .map(GoType::from_expr)
                            .unwrap_or(GoType::Unknown);
                        let count = f.names.as_ref().map_or(1, |n| n.len());
                        std::iter::repeat_n(ty, count)
                    })
                    .collect()
            })
            .unwrap_or_default();
        GoType::Func {
            params,
            results,
            variadic_start,
        }
    }

    pub fn from_name(name: &str) -> GoType {
        match name {
            "bool" => GoType::Bool,
            "int" => GoType::Int,
            "int8" => GoType::Int8,
            "int16" => GoType::Int16,
            "int32" => GoType::Int32,
            "rune" => GoType::Int32,
            "int64" => GoType::Int64,
            "uint" => GoType::Uint,
            "uint8" | "byte" => GoType::Uint8,
            "uint16" => GoType::Uint16,
            "uint32" => GoType::Uint32,
            "uint64" => GoType::Uint64,
            "uintptr" => GoType::Uintptr,
            "float32" => GoType::Float32,
            "float64" => GoType::Float64,
            "complex64" => GoType::Complex64,
            "complex128" => GoType::Complex128,
            "string" => GoType::String,
            "error" => GoType::Error,
            "any" => GoType::Any,
            _ => GoType::Named(name.to_string()),
        }
    }

    /// Infer the type of a Go expression given the current type environment.
    pub fn infer_expr(expr: &ast::Expr, env: &TypeEnv) -> GoType {
        match expr {
            ast::Expr::BasicLit(lit) => match lit.kind {
                token::Token::INT => GoType::Int,
                token::Token::FLOAT => GoType::Float64,
                token::Token::IMAG => GoType::Complex128,
                token::Token::STRING => GoType::String,
                token::Token::CHAR => GoType::Int32,
                _ => GoType::Unknown,
            },
            ast::Expr::Ident(id) => match id.name {
                "true" | "false" => GoType::Bool,
                "nil" => GoType::Any,
                name => {
                    super::selector_semantics::declared_value_type(name, env).unwrap_or_else(|| {
                        if name == "iota" {
                            GoType::Int
                        } else if env.has_func(name) {
                            GoType::Func {
                                params: env.get_func_params(name),
                                results: env.get_func_returns(name),
                                variadic_start: env.get_func_variadic_start(name),
                            }
                        } else {
                            GoType::Unknown
                        }
                    })
                }
            },
            ast::Expr::FuncLit(func_lit) => GoType::from_func_type(&func_lit.type_),
            ast::Expr::UnaryExpr(u) if u.op == token::Token::AND => {
                GoType::Pointer(Box::new(GoType::infer_expr(&u.x, env)))
            }
            ast::Expr::UnaryExpr(u) if u.op == token::Token::ARROW => {
                let operand = GoType::infer_expr(&u.x, env);
                match env.resolve_alias(&operand) {
                    GoType::Chan { elem, .. } => *elem,
                    GoType::Unknown | GoType::Named(_) => GoType::Unknown,
                    _ => GoType::Unknown,
                }
            }
            ast::Expr::UnaryExpr(u) => GoType::infer_expr(&u.x, env),
            ast::Expr::BinaryExpr(bin) => {
                let left = GoType::infer_expr(&bin.x, env);
                let right = GoType::infer_expr(&bin.y, env);
                match bin.op {
                    // Comparison operators produce bool
                    token::Token::EQL
                    | token::Token::NEQ
                    | token::Token::LSS
                    | token::Token::GTR
                    | token::Token::LEQ
                    | token::Token::GEQ
                    | token::Token::LAND
                    | token::Token::LOR => GoType::Bool,
                    token::Token::ADD
                    | token::Token::SUB
                    | token::Token::MUL
                    | token::Token::QUO
                        if matches!(left, GoType::Complex128)
                            || matches!(right, GoType::Complex128) =>
                    {
                        GoType::Complex128
                    }
                    token::Token::ADD
                    | token::Token::SUB
                    | token::Token::MUL
                    | token::Token::QUO
                        if matches!(left, GoType::Complex64)
                            || matches!(right, GoType::Complex64) =>
                    {
                        GoType::Complex64
                    }
                    token::Token::ADD
                    | token::Token::SUB
                    | token::Token::MUL
                    | token::Token::QUO
                    | token::Token::REM
                    | token::Token::AND
                    | token::Token::OR
                    | token::Token::XOR
                    | token::Token::AND_NOT
                        if expr_is_untyped_constant_for_inference(&bin.x, env)
                            && !expr_is_untyped_constant_for_inference(&bin.y, env) =>
                    {
                        right
                    }
                    // Arithmetic preserves the type of the left operand
                    _ => left,
                }
            }
            ast::Expr::CallExpr(call) => {
                if let Some(result) =
                    func_call_result_from_callee_type(GoType::infer_expr(&call.fun, env), env)
                {
                    return result;
                }

                // For function calls, return the first result type
                match &*call.fun {
                    ast::Expr::Ident(id) => {
                        if let Some(var_ty) = env.get_var(id.name) {
                            return match var_ty {
                                GoType::Func { results, .. } => {
                                    results.first().cloned().unwrap_or(GoType::Unknown)
                                }
                                _ => GoType::Unknown,
                            };
                        }
                        if env.has_func(id.name) {
                            return env.get_func_return(id.name);
                        }
                        if env.get_type_kind(id.name).is_some() {
                            return GoType::from_name(id.name);
                        }

                        // Builtin functions
                        match id.name {
                            "len" | "cap" => GoType::Int,
                            "make" => {
                                // infer from first arg which is the type
                                call.args
                                    .as_ref()
                                    .and_then(|a| a.first())
                                    .map(|e| GoType::from_expr(e))
                                    .unwrap_or(GoType::Unknown)
                            }
                            "new" => {
                                let inner = call
                                    .args
                                    .as_ref()
                                    .and_then(|a| a.first())
                                    .map(|e| {
                                        if new_arg_is_type(e, env) {
                                            GoType::from_expr(e)
                                        } else {
                                            GoType::infer_expr(e, env)
                                        }
                                    })
                                    .unwrap_or(GoType::Unknown);
                                GoType::Pointer(Box::new(inner))
                            }
                            "append" => call
                                .args
                                .as_ref()
                                .and_then(|a| a.first())
                                .map(|e| GoType::infer_expr(e, env))
                                .unwrap_or(GoType::Unknown),
                            "max" | "min" => call
                                .args
                                .as_ref()
                                .and_then(|args| {
                                    args.iter()
                                        .find(|arg| {
                                            !expr_is_untyped_constant_for_inference(arg, env)
                                        })
                                        .or_else(|| args.first())
                                })
                                .map(|expr| GoType::infer_expr(expr, env))
                                .unwrap_or(GoType::Unknown),
                            "string" => GoType::String,
                            "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8"
                            | "uint16" | "uint32" | "uint64" | "uintptr" | "float32"
                            | "float64" | "complex64" | "complex128" | "byte" | "rune" | "bool" => {
                                GoType::from_name(id.name)
                            }
                            "complex" => GoType::Complex128,
                            "recover" => GoType::Any,
                            "real" | "imag" => call
                                .args
                                .as_ref()
                                .and_then(|a| a.first())
                                .map(|expr| match GoType::infer_expr(expr, env) {
                                    GoType::Complex64 => GoType::Float32,
                                    GoType::Complex128 => GoType::Float64,
                                    _ => GoType::Float64,
                                })
                                .unwrap_or(GoType::Float64),
                            _ => GoType::Unknown,
                        }
                    }
                    ast::Expr::SelectorExpr(sel) => {
                        if super::ast_inspect::selector_is_unsafe_constant(sel) {
                            return GoType::Uintptr;
                        }
                        if let Some(key) = super::selector_semantics::qualified_member_key(sel) {
                            if env.get_type_kind(&key).is_some() {
                                return GoType::from_expr(&call.fun);
                            }
                            let package_return = env.get_func_return(&key);
                            if !matches!(package_return, GoType::Unknown) {
                                return package_return;
                            }
                        }
                        let receiver_type = GoType::infer_expr(&sel.x, env);
                        method_return_from_receiver_type(receiver_type, sel.sel.name, env)
                    }
                    other => {
                        let converted = GoType::from_expr(other);
                        if matches!(converted, GoType::Unknown) {
                            GoType::Unknown
                        } else {
                            converted
                        }
                    }
                }
            }
            ast::Expr::IndexExpr(index) => {
                let container = GoType::infer_expr(&index.x, env);
                let resolved = match env.resolve_alias_outer(&container) {
                    GoType::Pointer(inner) => match env.resolve_alias_outer(&inner) {
                        GoType::Array(elem) => GoType::Array(elem),
                        _ => GoType::Pointer(inner),
                    },
                    other => other,
                };
                match resolved {
                    GoType::String => GoType::Uint8,
                    GoType::Slice(elem) | GoType::Array(elem) => *elem,
                    GoType::Map(_, value) => *value,
                    _ => GoType::Unknown,
                }
            }
            ast::Expr::SelectorExpr(sel) => {
                let base_is_declared_value =
                    super::selector_semantics::selector_base_is_declared_value(sel, env);
                if !base_is_declared_value && let Some(func) = type_method_expression_func(sel, env)
                {
                    return func;
                }
                let base_type = GoType::infer_expr(&sel.x, env);
                if !matches!(base_type, GoType::Unknown) {
                    let field_type =
                        field_type_from_receiver_type(base_type.clone(), sel.sel.name, env);
                    if !matches!(field_type, GoType::Unknown) {
                        return field_type;
                    }
                    let method_type = method_func_from_receiver_type(base_type, sel.sel.name, env);
                    if !matches!(method_type, GoType::Unknown) {
                        return method_type;
                    }
                }
                if base_is_declared_value {
                    return GoType::Unknown;
                }
                if let Some(package_key) = super::selector_semantics::qualified_member_key(sel) {
                    if let Some(ty) =
                        super::selector_semantics::declared_value_type(&package_key, env)
                    {
                        return ty;
                    }
                    if env.has_func(&package_key) {
                        return GoType::Func {
                            params: env.get_func_params(&package_key),
                            results: env.get_func_returns(&package_key),
                            variadic_start: env.get_func_variadic_start(&package_key),
                        };
                    }
                }
                GoType::Unknown
            }
            ast::Expr::TypeAssertExpr(ta) => {
                if let Some(type_expr) = &ta.type_ {
                    go_type_from_asserted_type_expr(type_expr, env)
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::CompositeLit(cl) => {
                if let Some(type_expr) = &cl.type_ {
                    GoType::from_expr(type_expr)
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::SliceExpr(slice) => {
                let base = resolve_true_aliases_preserving_defined_type(
                    GoType::infer_expr(&slice.x, env),
                    env,
                );
                if matches!(base, GoType::Named(_) | GoType::Instantiated { .. })
                    && matches!(env.resolve_alias(&base), GoType::String | GoType::Slice(_))
                {
                    return base;
                }
                match env.resolve_alias(&base) {
                    GoType::String => GoType::String,
                    GoType::Slice(elem) => GoType::Slice(elem),
                    GoType::Array(elem) => GoType::Slice(elem),
                    GoType::Pointer(inner) => match env.resolve_alias(&inner) {
                        GoType::Array(elem) => GoType::Slice(elem),
                        _ => GoType::Pointer(inner),
                    },
                    GoType::Named(name) => GoType::Named(name),
                    other => other,
                }
            }
            ast::Expr::StarExpr(star) => {
                let inner = GoType::infer_expr(&star.x, env);
                if let GoType::Pointer(inner_type) = inner {
                    *inner_type
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::ParenExpr(p) => GoType::infer_expr(&p.x, env),
            _ => GoType::Unknown,
        }
    }
}

pub(super) fn resolve_true_aliases_preserving_defined_type(
    mut ty: GoType,
    env: &TypeEnv,
) -> GoType {
    loop {
        let name = match &ty {
            GoType::Named(name) | GoType::Instantiated { name, .. } => name,
            _ => return ty,
        };
        if !env.is_type_alias(name) {
            return ty;
        }
        let resolved = env.resolve_alias_outer(&ty);
        if resolved == ty {
            return ty;
        }
        ty = resolved;
    }
}

fn func_call_result_from_callee_type(ty: GoType, env: &TypeEnv) -> Option<GoType> {
    let GoType::Func { results, .. } = env.resolve_alias(&ty) else {
        return None;
    };
    Some(results.first().cloned().unwrap_or(GoType::Unknown))
}

pub(super) fn expr_is_untyped_constant_for_inference(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(_) => true,
        ast::Expr::Ident(ident) if matches!(ident.name, "true" | "false" | "iota") => true,
        ast::Expr::Ident(ident) => {
            env.is_const(ident.name) && !const_name_has_named_type(ident.name, env)
        }
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return false;
            };
            let name = format!("{}.{}", pkg.name, selector.sel.name);
            env.is_const(&name) && !const_name_has_named_type(&name, env)
        }
        ast::Expr::UnaryExpr(unary)
            if matches!(
                unary.op,
                token::Token::ADD | token::Token::SUB | token::Token::NOT | token::Token::XOR
            ) =>
        {
            expr_is_untyped_constant_for_inference(&unary.x, env)
        }
        ast::Expr::BinaryExpr(binary) => {
            expr_is_untyped_constant_for_inference(&binary.x, env)
                && expr_is_untyped_constant_for_inference(&binary.y, env)
        }
        _ => false,
    }
}

fn const_name_has_named_type(name: &str, env: &TypeEnv) -> bool {
    matches!(
        super::selector_semantics::declared_value_type(name, env),
        Some(GoType::Named(_))
    )
}

pub(super) fn const_integer_value_exact(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
    iota_value: Option<i64>,
) -> Option<ExactInt> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            ExactInt::parse_go_literal(lit.value)
        }
        ast::Expr::BasicLit(lit)
            if lit.kind == token::Token::FLOAT && decimal_float_literal_is_integer(lit.value) =>
        {
            parse_decimal_float_integer_i128(lit.value).map(ExactInt::from_i128)
        }
        ast::Expr::Ident(ident) if ident.name == "iota" => {
            iota_value.map(|value| ExactInt::from_i128(value.into()))
        }
        ast::Expr::Ident(ident) => env.get_const_integer_exact_value(ident.name),
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(package) = selector.x.as_ref() else {
                return None;
            };
            env.get_const_integer_exact_value(&format!("{}.{}", package.name, selector.sel.name))
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            const_integer_value_exact(&unary.x, env, iota_value)
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::SUB => {
            const_integer_value_exact(&unary.x, env, iota_value).map(|value| value.neg())
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::XOR => {
            const_integer_value_exact(&unary.x, env, iota_value).map(|value| value.bit_not())
        }
        ast::Expr::BinaryExpr(binary) => {
            let lhs = const_integer_value_exact(&binary.x, env, iota_value)?;
            let rhs = const_integer_value_exact(&binary.y, env, iota_value)?;
            match binary.op {
                token::Token::ADD => Some(lhs.add(&rhs)),
                token::Token::SUB => Some(lhs.sub(&rhs)),
                token::Token::MUL => Some(lhs.mul(&rhs)),
                token::Token::QUO => lhs.div(&rhs),
                token::Token::REM => lhs.rem(&rhs),
                token::Token::SHL => lhs.shl(&rhs),
                token::Token::SHR => lhs.shr(&rhs),
                token::Token::AND => Some(lhs.bit_and(&rhs)),
                token::Token::AND_NOT => Some(lhs.bit_and_not(&rhs)),
                token::Token::OR => Some(lhs.bit_or(&rhs)),
                token::Token::XOR => Some(lhs.bit_xor(&rhs)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn decimal_float_literal_is_integer(value: &str) -> bool {
    parse_decimal_float_integer_i128(value).is_some()
}

fn parse_decimal_float_integer_i128(value: &str) -> Option<i128> {
    let value = value.replace('_', "").to_ascii_lowercase();
    if value.starts_with("0x") || value.contains('p') {
        return None;
    }

    let (mantissa, exponent) = value
        .split_once('e')
        .map_or((value.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().ok().unwrap_or(0))
        });
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.strip_prefix(['+', '-']).unwrap_or(mantissa);
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int_part}{frac_part}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let shift = exponent - frac_part.len() as i32;
    let normalized = if shift >= 0 {
        let mut digits = digits;
        digits.extend(std::iter::repeat_n('0', shift as usize));
        digits
    } else {
        let trim = (-shift) as usize;
        if digits.bytes().rev().take(trim).any(|byte| byte != b'0') {
            return None;
        }
        digits[..digits.len().saturating_sub(trim)].to_string()
    };
    let parsed = normalized.parse::<i128>().ok()?;
    if negative {
        parsed.checked_neg()
    } else {
        Some(parsed)
    }
}

fn new_arg_is_type(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) => {
            !matches!(ident.name, "true" | "false" | "nil")
                && env.get_var(ident.name).is_none()
                && !env.has_func(ident.name)
                && !env.is_const(ident.name)
                && (super::predeclared::is_type_name(ident.name)
                    || env.get_type_kind(ident.name).is_some())
        }
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return false;
            };
            if super::ast_inspect::selector_is_unsafe_pointer(selector) {
                return true;
            }
            let key = format!("{}.{}", pkg.name, selector.sel.name);
            env.get_type_kind(&key).is_some()
                && env.get_var(&key).is_none()
                && !env.has_func(&key)
                && !env.is_const(&key)
        }
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::StarExpr(star) => new_arg_is_type(&star.x, env),
        ast::Expr::IndexExpr(index) => {
            type_name(&index.x).is_some_and(|name| env.get_type_kind(&name).is_some())
        }
        ast::Expr::IndexListExpr(index) => {
            type_name(&index.x).is_some_and(|name| env.get_type_kind(&name).is_some())
        }
        _ => false,
    }
}

fn type_name(expr: &ast::Expr<'_>) -> Option<String> {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => {
            super::selector_semantics::qualified_member_key(selector)
        }
        _ => None,
    }
}

fn unparen_expr<'a>(expr: &'a ast::Expr<'a>) -> &'a ast::Expr<'a> {
    match expr {
        ast::Expr::ParenExpr(paren) => unparen_expr(&paren.x),
        _ => expr,
    }
}

fn instantiate_named_type(base: GoType, args: Vec<GoType>) -> GoType {
    match base {
        GoType::Named(name) | GoType::Interface(name) => GoType::Instantiated { name, args },
        _ => base,
    }
}

fn substitute_receiver_type_params(
    env: &TypeEnv,
    receiver_name: &str,
    receiver_args: &[GoType],
    ty: GoType,
) -> GoType {
    let type_params = env.get_type_param_names(receiver_name);
    if type_params.len() != receiver_args.len() {
        return ty;
    }
    let substitutions = type_params
        .into_iter()
        .zip(receiver_args.iter().cloned())
        .collect::<HashMap<_, _>>();
    substitute_type_params(ty, &substitutions)
}

fn substitute_receiver_type_param_vec(
    env: &TypeEnv,
    receiver_name: &str,
    receiver_args: &[GoType],
    tys: Vec<GoType>,
) -> Vec<GoType> {
    tys.into_iter()
        .map(|ty| substitute_receiver_type_params(env, receiver_name, receiver_args, ty))
        .collect()
}

fn field_type_from_receiver_type(receiver_type: GoType, field: &str, env: &TypeEnv) -> GoType {
    match super::selector_semantics::resolve_selector(&receiver_type, field, false, env) {
        super::selector_semantics::SelectorResolution::Found(
            super::selector_semantics::ResolvedSelector {
                member: super::selector_semantics::SelectorMember::Field(field),
                ..
            },
        ) => field.ty,
        _ => GoType::Unknown,
    }
}

fn method_return_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    let Some(method) = resolved_selector_method(&receiver_type, method, true, env) else {
        return GoType::Unknown;
    };
    resolved_method_signature_types(&method, env.get_func_returns(&method.key), env)
        .into_iter()
        .next()
        .unwrap_or(GoType::Unknown)
}

fn method_func_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    let Some(method) = resolved_selector_method(&receiver_type, method, true, env) else {
        return GoType::Unknown;
    };
    GoType::Func {
        params: resolved_method_signature_types(&method, env.get_func_params(&method.key), env),
        results: resolved_method_signature_types(&method, env.get_func_returns(&method.key), env),
        variadic_start: env.get_func_variadic_start(&method.key),
    }
}

fn resolved_selector_method(
    receiver_type: &GoType,
    method_name: &str,
    include_pointer_receiver_methods: bool,
    env: &TypeEnv,
) -> Option<super::selector_semantics::SelectorMethod> {
    match super::selector_semantics::resolve_selector(
        receiver_type,
        method_name,
        include_pointer_receiver_methods,
        env,
    ) {
        super::selector_semantics::SelectorResolution::Found(
            super::selector_semantics::ResolvedSelector {
                member: super::selector_semantics::SelectorMember::Method(method),
                ..
            },
        ) => Some(method),
        _ => None,
    }
}

fn resolved_method_signature_types(
    method: &super::selector_semantics::SelectorMethod,
    types: Vec<GoType>,
    env: &TypeEnv,
) -> Vec<GoType> {
    match &method.receiver {
        GoType::Instantiated { name, args } => {
            substitute_receiver_type_param_vec(env, name, args, types)
        }
        _ => types,
    }
}

fn type_method_expression_func(sel: &ast::SelectorExpr, env: &TypeEnv) -> Option<GoType> {
    let method = super::method_expressions::for_selector(sel, env)?;
    Some(GoType::Func {
        params: method.params_with_receiver(env),
        results: method.returns(env),
        variadic_start: method.variadic_start_with_receiver(env),
    })
}

/// Type environment for tracking Go types during compilation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeEnv {
    /// Variable name → Go type (current scope)
    vars: HashMap<std::string::String, GoType>,
    /// Function/method name → return types
    funcs: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → parameter types
    func_params: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → interface parameter indices that need owned values.
    owned_interface_params: HashMap<std::string::String, HashSet<usize>>,
    /// Function/method name → slice parameter indices that must alias caller storage.
    borrowed_slice_params: HashMap<std::string::String, HashSet<usize>>,
    /// Method names declared with pointer receivers.
    pointer_receiver_methods: HashSet<std::string::String>,
    /// Function/method name → type parameter name → accepted constraint terms.
    func_type_param_constraints:
        HashMap<std::string::String, HashMap<std::string::String, Vec<GoType>>>,
    /// Type parameter constraints currently in lexical scope while validating/lowering a body.
    scoped_type_param_constraints: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → index where a variadic parameter starts
    func_variadic_start: HashMap<std::string::String, usize>,
    /// Function/method name → exact container shapes retained from the Go AST.
    ///
    /// `GoType::Array` intentionally erases fixed-array lengths for ordinary
    /// inference. Interface ABI reconstruction cannot: `[2]byte` and
    /// `[3]byte` are different method signatures, including after package
    /// facts are serialized or merged under an import name.
    #[serde(default)]
    func_signature_shapes: HashMap<std::string::String, FunctionSignatureShapes>,
    /// Function/method name → named interfaces asserted in the function body.
    func_interface_assertions: HashMap<std::string::String, Vec<std::string::String>>,
    /// Package-level variable name → interfaces asserted while initializing it.
    #[serde(default)]
    top_level_interface_assertions: HashMap<std::string::String, Vec<std::string::String>>,
    /// Type name → kind (struct, interface, alias)
    type_kinds: HashMap<std::string::String, TypeKind>,
    /// Type name → declared type parameter count
    type_param_counts: HashMap<std::string::String, usize>,
    /// Type name → declared type parameter names in source order.
    type_param_names: HashMap<std::string::String, Vec<std::string::String>>,
    /// Type names declared with alias syntax.
    type_aliases: HashSet<std::string::String>,
    /// Alternate qualified spellings of a declared type → its stable package identity.
    ///
    /// This is deliberately separate from `type_aliases`: `fs.File` and
    /// `io__fs.File` denote the same Go declaration after import-path module
    /// rewriting, but neither is a Go type alias for the other.
    #[serde(default)]
    canonical_declared_type_identities: HashMap<std::string::String, std::string::String>,
    /// Alias declarations whose right side is an instantiated generic type.
    instantiated_type_aliases: HashSet<std::string::String>,
    /// Alias name → direct alias target after ignoring pointer indirections.
    type_alias_targets: HashMap<std::string::String, std::string::String>,
    /// Interface name → required method names
    interface_methods: HashMap<std::string::String, Vec<std::string::String>>,
    /// Interface name → embedded interface names that contribute promoted methods.
    interface_embedded: HashMap<std::string::String, Vec<std::string::String>>,
    /// Interface name → type-set terms used when the interface is a constraint.
    interface_type_terms: HashMap<std::string::String, Vec<GoType>>,
    /// Struct name → field types
    struct_fields: HashMap<std::string::String, Vec<(std::string::String, GoType)>>,
    /// Struct name → fields declared as embedded fields.
    struct_embedded_fields: HashMap<std::string::String, HashSet<std::string::String>>,
    /// Struct name → array field lengths
    struct_field_array_lengths: HashMap<std::string::String, HashMap<std::string::String, i128>>,
    /// Package-level string constants emitted as owned-String functions.
    string_consts: HashSet<std::string::String>,
    top_level_vars: HashSet<std::string::String>,
    top_level_var_types: HashMap<std::string::String, GoType>,
    /// Package-level variables whose generated static includes an outer
    /// `Arc<Mutex<_>>` value cell.
    ///
    /// This is a representation fact, not merely a lowering-local mutation
    /// hint: downstream packages must use the same read path as the package
    /// that owns the static. Keep it serialized and qualify it through package
    /// merges alongside `top_level_var_types`.
    #[serde(default)]
    mutable_top_level_vars: HashSet<std::string::String>,
    consts: HashSet<std::string::String>,
    const_types: HashMap<std::string::String, GoType>,
    const_integer_values: HashMap<std::string::String, i128>,
    /// Exact decimal values for integer constants that participate in folding.
    ///
    /// Decimal strings keep the serialized type-environment format independent
    /// of the compiler's bigint implementation.
    #[serde(default)]
    const_integer_exact_values: HashMap<std::string::String, std::string::String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Interface,
    TypeParam,
    Alias(GoType),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum SignatureTypeShape {
    Other,
    Pointer(Box<SignatureTypeShape>),
    Slice(Box<SignatureTypeShape>),
    Array {
        length: Option<std::string::String>,
        elem: Box<SignatureTypeShape>,
    },
    Map {
        key: Box<SignatureTypeShape>,
        value: Box<SignatureTypeShape>,
    },
    Chan(Box<SignatureTypeShape>),
    Func {
        params: Vec<SignatureTypeShape>,
        results: Vec<SignatureTypeShape>,
    },
    Instantiated(Vec<SignatureTypeShape>),
}

impl SignatureTypeShape {
    fn from_expr(expr: &ast::Expr<'_>, env: &TypeEnv) -> Self {
        match expr {
            ast::Expr::ParenExpr(paren) => Self::from_expr(&paren.x, env),
            ast::Expr::StarExpr(star) => Self::Pointer(Box::new(Self::from_expr(&star.x, env))),
            ast::Expr::ArrayType(array) => {
                let elem = Box::new(Self::from_expr(&array.elt, env));
                match array.len.as_deref() {
                    Some(length) => Self::Array {
                        length: const_integer_value_exact(length, env, None)
                            .map(|length| length.decimal_string()),
                        elem,
                    },
                    None => Self::Slice(elem),
                }
            }
            ast::Expr::MapType(map) => Self::Map {
                key: Box::new(Self::from_expr(&map.key, env)),
                value: Box::new(Self::from_expr(&map.value, env)),
            },
            ast::Expr::ChanType(chan) => Self::Chan(Box::new(Self::from_expr(&chan.value, env))),
            ast::Expr::FuncType(func) => {
                let shapes = function_signature_shapes(func, env);
                Self::Func {
                    params: shapes.params,
                    results: shapes.results,
                }
            }
            ast::Expr::Ellipsis(ellipsis) => Self::Slice(Box::new(
                ellipsis
                    .elt
                    .as_ref()
                    .map(|elem| Self::from_expr(elem, env))
                    .unwrap_or(Self::Other),
            )),
            ast::Expr::IndexExpr(index) => {
                Self::Instantiated(vec![Self::from_expr(&index.index, env)])
            }
            ast::Expr::IndexListExpr(index) => Self::Instantiated(
                index
                    .indices
                    .iter()
                    .map(|arg| Self::from_expr(arg, env))
                    .collect(),
            ),
            _ => Self::Other,
        }
    }

    fn contains_fixed_array(&self) -> bool {
        match self {
            Self::Array { .. } => true,
            Self::Pointer(inner) | Self::Slice(inner) | Self::Chan(inner) => {
                inner.contains_fixed_array()
            }
            Self::Map { key, value } => key.contains_fixed_array() || value.contains_fixed_array(),
            Self::Func { params, results } => {
                params.iter().chain(results).any(Self::contains_fixed_array)
            }
            Self::Instantiated(args) => args.iter().any(Self::contains_fixed_array),
            Self::Other => false,
        }
    }

    fn has_exact_fixed_array_lengths(&self) -> bool {
        match self {
            Self::Array { length, elem } => {
                length.is_some() && elem.has_exact_fixed_array_lengths()
            }
            Self::Pointer(inner) | Self::Slice(inner) | Self::Chan(inner) => {
                inner.has_exact_fixed_array_lengths()
            }
            Self::Map { key, value } => {
                key.has_exact_fixed_array_lengths() && value.has_exact_fixed_array_lengths()
            }
            Self::Func { params, results } => params
                .iter()
                .chain(results)
                .all(Self::has_exact_fixed_array_lengths),
            Self::Instantiated(args) => args.iter().all(Self::has_exact_fixed_array_lengths),
            Self::Other => true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct FunctionSignatureShapes {
    pub(crate) params: Vec<SignatureTypeShape>,
    pub(crate) results: Vec<SignatureTypeShape>,
}

#[derive(Debug, Clone)]
struct InterfaceMethodSignature {
    name: std::string::String,
    params: Vec<GoType>,
    results: Vec<GoType>,
    variadic_start: Option<usize>,
    shapes: FunctionSignatureShapes,
}

#[derive(Debug, Clone)]
struct AnonymousInterfaceFacts {
    name: std::string::String,
    methods: Vec<InterfaceMethodSignature>,
    embedded: Vec<std::string::String>,
}

const ANONYMOUS_INTERFACE_PREFIX: &str = "__gors_anonymous_interface_";

#[derive(Debug, Clone, PartialEq)]
struct MethodSignature {
    params: Vec<GoType>,
    results: Vec<GoType>,
    variadic_start: Option<usize>,
    shapes: Option<FunctionSignatureShapes>,
    owned_interface_params: HashSet<usize>,
    borrowed_slice_params: HashSet<usize>,
}

fn go_type_contains_fixed_array(ty: &GoType) -> bool {
    match ty {
        GoType::Array(_) => true,
        GoType::Pointer(inner) | GoType::Slice(inner) => go_type_contains_fixed_array(inner),
        GoType::Map(key, value) => {
            go_type_contains_fixed_array(key) || go_type_contains_fixed_array(value)
        }
        GoType::Chan { elem, .. } => go_type_contains_fixed_array(elem),
        GoType::Func {
            params, results, ..
        } => params
            .iter()
            .chain(results)
            .any(go_type_contains_fixed_array),
        GoType::Instantiated { args, .. } => args.iter().any(go_type_contains_fixed_array),
        _ => false,
    }
}

fn borrowed_slice_indices_from_params(params: &[GoType]) -> HashSet<usize> {
    params
        .iter()
        .enumerate()
        .filter_map(|(index, ty)| matches!(ty, GoType::Slice(_)).then_some(index))
        .collect()
}

fn interface_method_names(expr: &ast::Expr) -> Vec<std::string::String> {
    let ast::Expr::InterfaceType(interface) = expr else {
        return Vec::new();
    };
    interface
        .methods
        .as_ref()
        .map(|methods| {
            methods
                .list
                .iter()
                .filter_map(|field| field.names.as_ref())
                .flat_map(|names| names.iter().map(|name| name.name.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn field_list_signature_shapes(
    fields: &ast::FieldList<'_>,
    env: &TypeEnv,
) -> Vec<SignatureTypeShape> {
    fields
        .list
        .iter()
        .flat_map(|field| {
            let shape = field
                .type_
                .as_ref()
                .map(|ty| SignatureTypeShape::from_expr(ty, env))
                .unwrap_or(SignatureTypeShape::Other);
            std::iter::repeat_n(shape, field.names.as_ref().map_or(1, Vec::len))
        })
        .collect()
}

fn function_signature_shapes(
    func_type: &ast::FuncType<'_>,
    env: &TypeEnv,
) -> FunctionSignatureShapes {
    FunctionSignatureShapes {
        params: field_list_signature_shapes(&func_type.params, env),
        results: func_type
            .results
            .as_ref()
            .map(|results| field_list_signature_shapes(results, env))
            .unwrap_or_default(),
    }
}

fn interface_method_signatures(expr: &ast::Expr, env: &TypeEnv) -> Vec<InterfaceMethodSignature> {
    let ast::Expr::InterfaceType(interface) = expr else {
        return Vec::new();
    };
    interface_method_signatures_from_interface(interface, env)
}

fn interface_method_signatures_from_interface(
    interface: &ast::InterfaceType<'_>,
    env: &TypeEnv,
) -> Vec<InterfaceMethodSignature> {
    interface
        .methods
        .as_ref()
        .map(|methods| {
            methods
                .list
                .iter()
                .filter_map(|field| {
                    let names = field.names.as_ref()?;
                    let ast::Expr::FuncType(func_type) = field.type_.as_ref()? else {
                        return None;
                    };
                    let GoType::Func {
                        params,
                        results,
                        variadic_start,
                    } = GoType::from_func_type(func_type)
                    else {
                        return None;
                    };
                    let shapes = function_signature_shapes(func_type, env);
                    Some(names.iter().map(move |name| InterfaceMethodSignature {
                        name: name.name.to_string(),
                        params: params.clone(),
                        results: results.clone(),
                        variadic_start,
                        shapes: shapes.clone(),
                    }))
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default()
}

fn anonymous_interface_facts(
    interface: &ast::InterfaceType<'_>,
    env: &TypeEnv,
) -> Option<AnonymousInterfaceFacts> {
    let mut methods = interface_method_signatures_from_interface(interface, env);
    methods.sort_by_cached_key(|method| {
        format!(
            "{}:{:?}:{:?}:{:?}:{:?}",
            method.name, method.params, method.results, method.variadic_start, method.shapes,
        )
    });
    let mut embedded = interface
        .methods
        .as_ref()
        .into_iter()
        .flat_map(|methods| &methods.list)
        .filter(|field| field.names.as_ref().is_none_or(Vec::is_empty))
        .filter_map(|field| field.type_.as_ref())
        .filter_map(embedded_interface_name)
        .collect::<Vec<_>>();
    if methods.is_empty() && embedded.is_empty() {
        return None;
    }

    let mut signature_parts = methods
        .iter()
        .map(|method| {
            format!(
                "method:{}:{:?}:{:?}:{:?}:{:?}",
                method.name, method.params, method.results, method.variadic_start, method.shapes,
            )
        })
        .collect::<Vec<_>>();
    embedded.sort();
    signature_parts.extend(
        embedded
            .iter()
            .map(|embedded| format!("embedded:{embedded}")),
    );
    signature_parts.sort();

    let mut hash = Sha256::new();
    for part in signature_parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    let digest = hash.finalize();
    let suffix = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<std::string::String>();
    Some(AnonymousInterfaceFacts {
        name: format!("{ANONYMOUS_INTERFACE_PREFIX}{suffix}"),
        methods,
        embedded,
    })
}

pub(crate) fn anonymous_interface_name(
    interface: &ast::InterfaceType<'_>,
    env: &TypeEnv,
) -> Option<std::string::String> {
    anonymous_interface_facts(interface, env).map(|facts| facts.name)
}

fn go_type_from_asserted_type_expr(type_expr: &ast::Expr<'_>, env: &TypeEnv) -> GoType {
    match type_expr {
        ast::Expr::ParenExpr(paren) => go_type_from_asserted_type_expr(&paren.x, env),
        ast::Expr::InterfaceType(interface) => anonymous_interface_name(interface, env)
            .map(GoType::Interface)
            .unwrap_or(GoType::Any),
        other => GoType::from_expr(other),
    }
}

pub(crate) fn is_anonymous_interface_name(name: &str) -> bool {
    name.starts_with(ANONYMOUS_INTERFACE_PREFIX)
}

pub(crate) fn anonymous_interface_assertion_names_in_file(
    file: &ast::File<'_>,
    env: &TypeEnv,
) -> Vec<std::string::String> {
    let facts = interface_assertion_facts_in_file(file, env);
    let mut names = facts
        .sorted_names()
        .into_iter()
        .filter(|name| is_anonymous_interface_name(name))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn interface_assertion_facts_in_file(
    file: &ast::File<'_>,
    env: &TypeEnv,
) -> InterfaceAssertionFacts {
    let mut facts = InterfaceAssertionFacts::new(env);
    for decl in &file.decls {
        match decl {
            ast::Decl::FuncDecl(func) => {
                if let Some(body) = &func.body {
                    collect_interface_assertion_names_from_block(body, &mut facts);
                }
            }
            ast::Decl::GenDecl(general) => {
                for spec in &general.specs {
                    let ast::Spec::ValueSpec(value) = spec else {
                        continue;
                    };
                    collect_interface_assertion_facts_from_value_spec(value, &mut facts);
                }
            }
        }
    }
    facts
}

fn signature_shape_updates(
    files: &[&ast::File<'_>],
    env: &TypeEnv,
) -> Vec<(std::string::String, FunctionSignatureShapes)> {
    let mut updates = Vec::new();
    for file in files {
        for decl in &file.decls {
            match decl {
                ast::Decl::FuncDecl(func) => {
                    let key = func
                        .recv
                        .as_ref()
                        .and_then(|receiver| receiver.list.first())
                        .and_then(|field| field.type_.as_ref())
                        .map(|receiver| {
                            format!("{}.{}", extract_type_name(receiver), func.name.name)
                        })
                        .unwrap_or_else(|| func.name.name.to_string());
                    updates.push((key, function_signature_shapes(&func.type_, env)));
                }
                ast::Decl::GenDecl(general) => {
                    for spec in &general.specs {
                        let ast::Spec::TypeSpec(type_spec) = spec else {
                            continue;
                        };
                        let Some(type_name) = type_spec.name.as_ref() else {
                            continue;
                        };
                        updates.extend(
                            interface_method_signatures(&type_spec.type_, env)
                                .into_iter()
                                .map(|signature| {
                                    (
                                        format!("{}.{}", type_name.name, signature.name),
                                        signature.shapes,
                                    )
                                }),
                        );
                    }
                }
            }
        }
    }
    updates
}

struct InterfaceAssertionFacts {
    env: TypeEnv,
    names: Vec<std::string::String>,
    anonymous: HashMap<std::string::String, AnonymousInterfaceFacts>,
}

impl InterfaceAssertionFacts {
    fn new(env: &TypeEnv) -> Self {
        Self {
            env: env.clone(),
            names: Vec::new(),
            anonymous: HashMap::new(),
        }
    }

    fn discover_anonymous(&mut self, interface: &ast::InterfaceType<'_>) -> Option<String> {
        let Some(facts) = anonymous_interface_facts(interface, &self.env) else {
            return None;
        };
        let name = facts.name.clone();
        self.anonymous.entry(name.clone()).or_insert(facts);
        Some(name)
    }

    fn record_asserted_type(&mut self, type_expr: &ast::Expr<'_>) {
        match type_expr {
            ast::Expr::ParenExpr(paren) => self.record_asserted_type(&paren.x),
            ast::Expr::InterfaceType(interface) => {
                if let Some(name) = self.discover_anonymous(interface) {
                    self.names.push(name);
                }
            }
            other => {
                if let Some(name) = named_assertion_type(other) {
                    self.names.push(name);
                }
            }
        }
    }

    fn sorted_names(&self) -> Vec<std::string::String> {
        let mut names = self.names.clone();
        names.sort();
        names.dedup();
        names
    }
}

fn collect_interface_assertion_facts_from_value_spec(
    value: &ast::ValueSpec<'_>,
    out: &mut InterfaceAssertionFacts,
) {
    if let Some(type_) = &value.type_ {
        collect_interface_assertion_names_from_expr(type_, out);
    }
    if let Some(values) = &value.values {
        for expr in values {
            collect_interface_assertion_names_from_expr(expr, out);
        }
    }
}

fn interface_assertion_facts_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> InterfaceAssertionFacts {
    let mut facts = InterfaceAssertionFacts::new(env);
    collect_interface_assertion_names_from_block(block, &mut facts);
    facts
}

fn collect_interface_assertion_names_from_block(
    block: &ast::BlockStmt<'_>,
    out: &mut InterfaceAssertionFacts,
) {
    for stmt in &block.list {
        collect_interface_assertion_names_from_stmt(stmt, out);
    }
}

fn collect_interface_assertion_names_from_stmt(
    stmt: &ast::Stmt<'_>,
    out: &mut InterfaceAssertionFacts,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_interface_assertion_names_from_expr(expr, out);
            }
        }
        ast::Stmt::BlockStmt(block) => collect_interface_assertion_names_from_block(block, out),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
        ast::Stmt::CaseClause(case) => {
            if let Some(list) = &case.list {
                for expr in list {
                    collect_interface_assertion_names_from_expr(expr, out);
                }
            }
            for stmt in &case.body {
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
            for stmt in &comm.body {
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_interface_assertion_names_from_expr(expr, out);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer) => {
            collect_interface_assertion_names_from_expr(&defer.call.fun, out);
            if let Some(args) = &defer.call.args {
                for arg in args {
                    collect_interface_assertion_names_from_expr(arg, out);
                }
            }
        }
        ast::Stmt::ExprStmt(expr) => collect_interface_assertion_names_from_expr(&expr.x, out),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_interface_assertion_names_from_stmt(init, out);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_interface_assertion_names_from_expr(cond, out);
            }
            if let Some(post) = &for_stmt.post {
                collect_interface_assertion_names_from_stmt(post, out);
            }
            collect_interface_assertion_names_from_block(&for_stmt.body, out);
        }
        ast::Stmt::GoStmt(go) => {
            collect_interface_assertion_names_from_expr(&go.call.fun, out);
            if let Some(args) = &go.call.args {
                for arg in args {
                    collect_interface_assertion_names_from_expr(arg, out);
                }
            }
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_interface_assertion_names_from_stmt(init, out);
            }
            collect_interface_assertion_names_from_expr(&if_stmt.cond, out);
            collect_interface_assertion_names_from_block(&if_stmt.body, out);
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_interface_assertion_names_from_stmt(else_stmt, out);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            collect_interface_assertion_names_from_expr(&inc_dec.x, out);
        }
        ast::Stmt::LabeledStmt(labeled) => {
            collect_interface_assertion_names_from_stmt(&labeled.stmt, out);
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_interface_assertion_names_from_expr(key, out);
            }
            if let Some(value) = &range.value {
                collect_interface_assertion_names_from_expr(value, out);
            }
            collect_interface_assertion_names_from_expr(&range.x, out);
            collect_interface_assertion_names_from_block(&range.body, out);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_interface_assertion_names_from_expr(expr, out);
            }
        }
        ast::Stmt::SelectStmt(select) => {
            for stmt in &select.body.list {
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_interface_assertion_names_from_expr(&send.chan, out);
            collect_interface_assertion_names_from_expr(&send.value, out);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_interface_assertion_names_from_stmt(init, out);
            }
            if let Some(tag) = &switch.tag {
                collect_interface_assertion_names_from_expr(tag, out);
            }
            for stmt in &switch.body.list {
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_interface_assertion_names_from_stmt(init, out);
            }
            collect_interface_assertion_names_from_stmt(&type_switch.assign, out);
            for stmt in &type_switch.body.list {
                let ast::Stmt::CaseClause(case) = stmt else {
                    collect_interface_assertion_names_from_stmt(stmt, out);
                    continue;
                };
                if let Some(types) = &case.list {
                    for type_expr in types {
                        out.record_asserted_type(type_expr);
                        collect_interface_assertion_names_from_expr(type_expr, out);
                    }
                }
                for stmt in &case.body {
                    collect_interface_assertion_names_from_stmt(stmt, out);
                }
            }
        }
    }
}

fn collect_interface_assertion_names_from_expr(
    expr: &ast::Expr<'_>,
    out: &mut InterfaceAssertionFacts,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_interface_assertion_names_from_expr(len, out);
            }
            collect_interface_assertion_names_from_expr(&array.elt, out);
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => {}
        ast::Expr::BinaryExpr(binary) => {
            collect_interface_assertion_names_from_expr(&binary.x, out);
            collect_interface_assertion_names_from_expr(&binary.y, out);
        }
        ast::Expr::CallExpr(call) => {
            collect_interface_assertion_names_from_expr(&call.fun, out);
            if let Some(args) = &call.args {
                for arg in args {
                    collect_interface_assertion_names_from_expr(arg, out);
                }
            }
        }
        ast::Expr::ChanType(chan) => {
            collect_interface_assertion_names_from_expr(&chan.value, out);
        }
        ast::Expr::CompositeLit(lit) => {
            if let Some(type_) = &lit.type_ {
                collect_interface_assertion_names_from_expr(type_, out);
            }
            if let Some(elts) = &lit.elts {
                for elt in elts {
                    collect_interface_assertion_names_from_expr(elt, out);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_interface_assertion_names_from_expr(elt, out);
            }
        }
        ast::Expr::FuncLit(func) => collect_interface_assertion_names_from_block(&func.body, out),
        ast::Expr::FuncType(func) => {
            for field in &func.params.list {
                if let Some(type_) = &field.type_ {
                    collect_interface_assertion_names_from_expr(type_, out);
                }
            }
            if let Some(results) = &func.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_ {
                        collect_interface_assertion_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::IndexExpr(index) => {
            collect_interface_assertion_names_from_expr(&index.x, out);
            collect_interface_assertion_names_from_expr(&index.index, out);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_interface_assertion_names_from_expr(&index.x, out);
            for expr in &index.indices {
                collect_interface_assertion_names_from_expr(expr, out);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            out.discover_anonymous(interface);
            if let Some(methods) = &interface.methods {
                for field in &methods.list {
                    if let Some(type_) = &field.type_ {
                        collect_interface_assertion_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::KeyValueExpr(key_value) => {
            collect_interface_assertion_names_from_expr(&key_value.key, out);
            collect_interface_assertion_names_from_expr(&key_value.value, out);
        }
        ast::Expr::MapType(map) => {
            collect_interface_assertion_names_from_expr(&map.key, out);
            collect_interface_assertion_names_from_expr(&map.value, out);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_interface_assertion_names_from_expr(&paren.x, out);
        }
        ast::Expr::SelectorExpr(selector) => {
            collect_interface_assertion_names_from_expr(&selector.x, out);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_interface_assertion_names_from_expr(&slice.x, out);
            if let Some(low) = &slice.low {
                collect_interface_assertion_names_from_expr(low, out);
            }
            if let Some(high) = &slice.high {
                collect_interface_assertion_names_from_expr(high, out);
            }
            if let Some(max) = &slice.max {
                collect_interface_assertion_names_from_expr(max, out);
            }
        }
        ast::Expr::StarExpr(star) => {
            collect_interface_assertion_names_from_expr(&star.x, out);
        }
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                for field in &fields.list {
                    if let Some(type_) = &field.type_ {
                        collect_interface_assertion_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_interface_assertion_names_from_expr(&assert.x, out);
            if let Some(type_) = &assert.type_ {
                out.record_asserted_type(type_);
                collect_interface_assertion_names_from_expr(type_, out);
            }
        }
        ast::Expr::UnaryExpr(unary) => {
            collect_interface_assertion_names_from_expr(&unary.x, out);
        }
    }
}

fn named_assertion_type(expr: &ast::Expr<'_>) -> Option<std::string::String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = &*selector.x else {
                return None;
            };
            Some(format!("{}.{}", pkg.name, selector.sel.name))
        }
        ast::Expr::ParenExpr(paren) => named_assertion_type(&paren.x),
        ast::Expr::StarExpr(star) => named_assertion_type(&star.x),
        ast::Expr::IndexExpr(index) => named_assertion_type(&index.x),
        ast::Expr::IndexListExpr(index) => named_assertion_type(&index.x),
        _ => None,
    }
}

fn owned_interface_param_indices(
    param_types: &[GoType],
    fields: &ast::FieldList<'_>,
    body: &ast::BlockStmt<'_>,
    mut is_named_interface: impl FnMut(&str) -> bool,
) -> HashSet<usize> {
    let mut assigned = assigned_ident_names_in_block(body);
    assigned.extend(stored_or_returned_ident_names_in_block(body));
    if assigned.is_empty() {
        return HashSet::new();
    }

    let mut owned = HashSet::new();
    let mut index = 0usize;
    for field in &fields.list {
        if let Some(names) = &field.names {
            for name in names {
                let needs_owned = assigned.contains(name.name)
                    && param_types.get(index).is_some_and(|ty| match ty {
                        GoType::Interface(_) => true,
                        GoType::Named(name) => is_named_interface(name),
                        _ => false,
                    });
                if needs_owned {
                    owned.insert(index);
                }
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    owned
}

fn stored_or_returned_ident_names_in_block(
    block: &ast::BlockStmt<'_>,
) -> HashSet<std::string::String> {
    let mut names = HashSet::new();
    for stmt in &block.list {
        collect_stored_or_returned_ident_names_from_stmt(stmt, &mut names);
    }
    names
}

fn collect_stored_or_returned_ident_names_from_stmt(
    stmt: &ast::Stmt<'_>,
    out: &mut HashSet<std::string::String>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for rhs in &assign.rhs {
                collect_stored_ident_names_from_expr(rhs, false, out);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            for stmt in &block.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::CaseClause(case) => {
            for stmt in &case.body {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
            for stmt in &comm.body {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_stored_ident_names_from_expr(expr, false, out);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer) => collect_stored_ident_names_from_call(&defer.call, out),
        ast::Stmt::ExprStmt(expr) => collect_stored_ident_names_from_expr(&expr.x, false, out),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_stored_or_returned_ident_names_from_stmt(init, out);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_stored_ident_names_from_expr(cond, false, out);
            }
            if let Some(post) = &for_stmt.post {
                collect_stored_or_returned_ident_names_from_stmt(post, out);
            }
            for stmt in &for_stmt.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::GoStmt(go) => collect_stored_ident_names_from_call(&go.call, out),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_stored_or_returned_ident_names_from_stmt(init, out);
            }
            collect_stored_ident_names_from_expr(&if_stmt.cond, false, out);
            for stmt in &if_stmt.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_stored_or_returned_ident_names_from_stmt(else_stmt, out);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => {
            collect_stored_or_returned_ident_names_from_stmt(&labeled.stmt, out)
        }
        ast::Stmt::RangeStmt(range) => {
            collect_stored_ident_names_from_expr(&range.x, false, out);
            for stmt in &range.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_stored_ident_names_from_expr(expr, true, out);
            }
        }
        ast::Stmt::SelectStmt(select) => {
            for stmt in &select.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_stored_ident_names_from_expr(&send.chan, false, out);
            collect_stored_ident_names_from_expr(&send.value, false, out);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_stored_or_returned_ident_names_from_stmt(init, out);
            }
            if let Some(tag) = &switch.tag {
                collect_stored_ident_names_from_expr(tag, false, out);
            }
            for stmt in &switch.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_stored_or_returned_ident_names_from_stmt(init, out);
            }
            collect_stored_or_returned_ident_names_from_stmt(&type_switch.assign, out);
            for stmt in &type_switch.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) | ast::Stmt::IncDecStmt(_) => {}
    }
}

fn collect_stored_ident_names_from_expr(
    expr: &ast::Expr<'_>,
    storage_position: bool,
    out: &mut HashSet<std::string::String>,
) {
    match expr {
        ast::Expr::Ident(ident) if storage_position && ident.name != "_" => {
            out.insert(ident.name.to_string());
        }
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_stored_ident_names_from_expr(len, false, out);
            }
            collect_stored_ident_names_from_expr(&array.elt, false, out);
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => {}
        ast::Expr::BinaryExpr(binary) => {
            collect_stored_ident_names_from_expr(&binary.x, false, out);
            collect_stored_ident_names_from_expr(&binary.y, false, out);
        }
        ast::Expr::CallExpr(call) => collect_stored_ident_names_from_call(call, out),
        ast::Expr::ChanType(chan) => collect_stored_ident_names_from_expr(&chan.value, false, out),
        ast::Expr::CompositeLit(lit) => {
            if let Some(type_) = &lit.type_ {
                collect_stored_ident_names_from_expr(type_, false, out);
            }
            if let Some(elts) = &lit.elts {
                for elt in elts {
                    collect_stored_ident_names_from_expr(elt, true, out);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_stored_ident_names_from_expr(elt, storage_position, out);
            }
        }
        ast::Expr::FuncLit(func) => {
            for stmt in &func.body.list {
                collect_stored_or_returned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Expr::FuncType(func) => {
            for field in &func.params.list {
                if let Some(type_) = &field.type_ {
                    collect_stored_ident_names_from_expr(type_, false, out);
                }
            }
            if let Some(results) = &func.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_ {
                        collect_stored_ident_names_from_expr(type_, false, out);
                    }
                }
            }
        }
        ast::Expr::IndexExpr(index) => {
            collect_stored_ident_names_from_expr(&index.x, false, out);
            collect_stored_ident_names_from_expr(&index.index, false, out);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_stored_ident_names_from_expr(&index.x, false, out);
            for expr in &index.indices {
                collect_stored_ident_names_from_expr(expr, false, out);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                for field in &methods.list {
                    if let Some(type_) = &field.type_ {
                        collect_stored_ident_names_from_expr(type_, false, out);
                    }
                }
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_stored_ident_names_from_expr(&kv.key, false, out);
            collect_stored_ident_names_from_expr(&kv.value, true, out);
        }
        ast::Expr::MapType(map) => {
            collect_stored_ident_names_from_expr(&map.key, false, out);
            collect_stored_ident_names_from_expr(&map.value, false, out);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_stored_ident_names_from_expr(&paren.x, storage_position, out)
        }
        ast::Expr::SelectorExpr(selector) => {
            collect_stored_ident_names_from_expr(&selector.x, false, out);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_stored_ident_names_from_expr(&slice.x, false, out);
            if let Some(low) = &slice.low {
                collect_stored_ident_names_from_expr(low, false, out);
            }
            if let Some(high) = &slice.high {
                collect_stored_ident_names_from_expr(high, false, out);
            }
            if let Some(max) = &slice.max {
                collect_stored_ident_names_from_expr(max, false, out);
            }
        }
        ast::Expr::StarExpr(star) => {
            collect_stored_ident_names_from_expr(&star.x, storage_position, out)
        }
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                for field in &fields.list {
                    if let Some(type_) = &field.type_ {
                        collect_stored_ident_names_from_expr(type_, false, out);
                    }
                }
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_stored_ident_names_from_expr(&assert.x, storage_position, out);
            if let Some(type_) = &assert.type_ {
                collect_stored_ident_names_from_expr(type_, false, out);
            }
        }
        ast::Expr::UnaryExpr(unary) => {
            collect_stored_ident_names_from_expr(&unary.x, storage_position, out)
        }
    }
}

fn collect_stored_ident_names_from_call(
    call: &ast::CallExpr<'_>,
    out: &mut HashSet<std::string::String>,
) {
    collect_stored_ident_names_from_expr(&call.fun, false, out);
    if let Some(args) = &call.args {
        for arg in args {
            collect_stored_ident_names_from_expr(arg, false, out);
        }
    }
}

fn assigned_ident_names_in_block(block: &ast::BlockStmt<'_>) -> HashSet<std::string::String> {
    let mut names = HashSet::new();
    for stmt in &block.list {
        collect_assigned_ident_names_from_stmt(stmt, &mut names);
    }
    names
}

fn collect_assigned_ident_names_from_stmt(
    stmt: &ast::Stmt<'_>,
    out: &mut HashSet<std::string::String>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for lhs in &assign.lhs {
                collect_assigned_ident_names_from_lhs(lhs, out);
            }
            for rhs in &assign.rhs {
                collect_assigned_ident_names_from_expr(rhs, out);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            for stmt in &block.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
        ast::Stmt::CaseClause(case) => {
            for stmt in &case.body {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
            for stmt in &comm.body {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_assigned_ident_names_from_expr(expr, out);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer) => collect_assigned_ident_names_from_call(&defer.call, out),
        ast::Stmt::ExprStmt(expr) => collect_assigned_ident_names_from_expr(&expr.x, out),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_assigned_ident_names_from_stmt(init, out);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_assigned_ident_names_from_expr(cond, out);
            }
            if let Some(post) = &for_stmt.post {
                collect_assigned_ident_names_from_stmt(post, out);
            }
            for stmt in &for_stmt.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::GoStmt(go) => collect_assigned_ident_names_from_call(&go.call, out),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_assigned_ident_names_from_stmt(init, out);
            }
            collect_assigned_ident_names_from_expr(&if_stmt.cond, out);
            for stmt in &if_stmt.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_assigned_ident_names_from_stmt(else_stmt, out);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => collect_assigned_ident_names_from_lhs(&inc_dec.x, out),
        ast::Stmt::LabeledStmt(labeled) => {
            collect_assigned_ident_names_from_stmt(&labeled.stmt, out)
        }
        ast::Stmt::RangeStmt(range) => {
            if matches!(range.tok, Some(token::Token::ASSIGN | token::Token::DEFINE)) {
                if let Some(key) = &range.key {
                    collect_assigned_ident_names_from_lhs(key, out);
                }
                if let Some(value) = &range.value {
                    collect_assigned_ident_names_from_lhs(value, out);
                }
            }
            collect_assigned_ident_names_from_expr(&range.x, out);
            for stmt in &range.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_assigned_ident_names_from_expr(expr, out);
            }
        }
        ast::Stmt::SelectStmt(select) => {
            for stmt in &select.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_assigned_ident_names_from_expr(&send.chan, out);
            collect_assigned_ident_names_from_expr(&send.value, out);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_assigned_ident_names_from_stmt(init, out);
            }
            if let Some(tag) = &switch.tag {
                collect_assigned_ident_names_from_expr(tag, out);
            }
            for stmt in &switch.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_assigned_ident_names_from_stmt(init, out);
            }
            collect_assigned_ident_names_from_stmt(&type_switch.assign, out);
            for stmt in &type_switch.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
    }
}

fn collect_assigned_ident_names_from_lhs(
    expr: &ast::Expr<'_>,
    out: &mut HashSet<std::string::String>,
) {
    match expr {
        ast::Expr::Ident(ident) if ident.name != "_" => {
            out.insert(ident.name.to_string());
        }
        ast::Expr::ParenExpr(paren) => collect_assigned_ident_names_from_lhs(&paren.x, out),
        _ => {}
    }
}

fn collect_assigned_ident_names_from_expr(
    expr: &ast::Expr<'_>,
    out: &mut HashSet<std::string::String>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_assigned_ident_names_from_expr(len, out);
            }
            collect_assigned_ident_names_from_expr(&array.elt, out);
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => {}
        ast::Expr::BinaryExpr(binary) => {
            collect_assigned_ident_names_from_expr(&binary.x, out);
            collect_assigned_ident_names_from_expr(&binary.y, out);
        }
        ast::Expr::CallExpr(call) => collect_assigned_ident_names_from_call(call, out),
        ast::Expr::ChanType(chan) => collect_assigned_ident_names_from_expr(&chan.value, out),
        ast::Expr::CompositeLit(lit) => {
            if let Some(type_) = &lit.type_ {
                collect_assigned_ident_names_from_expr(type_, out);
            }
            if let Some(elts) = &lit.elts {
                for elt in elts {
                    collect_assigned_ident_names_from_expr(elt, out);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_assigned_ident_names_from_expr(elt, out);
            }
        }
        ast::Expr::FuncLit(func) => {
            for stmt in &func.body.list {
                collect_assigned_ident_names_from_stmt(stmt, out);
            }
        }
        ast::Expr::FuncType(func) => {
            for field in &func.params.list {
                if let Some(type_) = &field.type_ {
                    collect_assigned_ident_names_from_expr(type_, out);
                }
            }
            if let Some(results) = &func.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_ {
                        collect_assigned_ident_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::IndexExpr(index) => {
            collect_assigned_ident_names_from_expr(&index.x, out);
            collect_assigned_ident_names_from_expr(&index.index, out);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_assigned_ident_names_from_expr(&index.x, out);
            for expr in &index.indices {
                collect_assigned_ident_names_from_expr(expr, out);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                for field in &methods.list {
                    if let Some(type_) = &field.type_ {
                        collect_assigned_ident_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_assigned_ident_names_from_expr(&kv.key, out);
            collect_assigned_ident_names_from_expr(&kv.value, out);
        }
        ast::Expr::MapType(map) => {
            collect_assigned_ident_names_from_expr(&map.key, out);
            collect_assigned_ident_names_from_expr(&map.value, out);
        }
        ast::Expr::ParenExpr(paren) => collect_assigned_ident_names_from_expr(&paren.x, out),
        ast::Expr::SelectorExpr(selector) => {
            collect_assigned_ident_names_from_expr(&selector.x, out);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_assigned_ident_names_from_expr(&slice.x, out);
            if let Some(low) = &slice.low {
                collect_assigned_ident_names_from_expr(low, out);
            }
            if let Some(high) = &slice.high {
                collect_assigned_ident_names_from_expr(high, out);
            }
            if let Some(max) = &slice.max {
                collect_assigned_ident_names_from_expr(max, out);
            }
        }
        ast::Expr::StarExpr(star) => collect_assigned_ident_names_from_expr(&star.x, out),
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                for field in &fields.list {
                    if let Some(type_) = &field.type_ {
                        collect_assigned_ident_names_from_expr(type_, out);
                    }
                }
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_assigned_ident_names_from_expr(&assert.x, out);
            if let Some(type_) = &assert.type_ {
                collect_assigned_ident_names_from_expr(type_, out);
            }
        }
        ast::Expr::UnaryExpr(unary) => collect_assigned_ident_names_from_expr(&unary.x, out),
    }
}

fn collect_assigned_ident_names_from_call(
    call: &ast::CallExpr<'_>,
    out: &mut HashSet<std::string::String>,
) {
    collect_assigned_ident_names_from_expr(&call.fun, out);
    if let Some(args) = &call.args {
        for arg in args {
            collect_assigned_ident_names_from_expr(arg, out);
        }
    }
}

fn embedded_interface_name(expr: &ast::Expr) -> Option<std::string::String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::SelectorExpr(sel) => match &*sel.x {
            ast::Expr::Ident(pkg) => Some(format!("{}.{}", pkg.name, sel.sel.name)),
            _ => None,
        },
        ast::Expr::ParenExpr(paren) => embedded_interface_name(&paren.x),
        ast::Expr::IndexExpr(index) => embedded_interface_name(&index.x),
        ast::Expr::IndexListExpr(index) => embedded_interface_name(&index.x),
        _ => None,
    }
}

fn interface_embedded_names(expr: &ast::Expr) -> Vec<std::string::String> {
    let ast::Expr::InterfaceType(interface) = expr else {
        return Vec::new();
    };
    interface
        .methods
        .as_ref()
        .map(|methods| {
            methods
                .list
                .iter()
                .filter(|field| field.names.as_ref().is_none_or(Vec::is_empty))
                .filter_map(|field| field.type_.as_ref())
                .filter_map(embedded_interface_name)
                .collect()
        })
        .unwrap_or_default()
}

fn interface_constraint_terms(expr: &ast::Expr) -> Vec<GoType> {
    let ast::Expr::InterfaceType(interface) = expr else {
        return Vec::new();
    };
    interface
        .methods
        .as_ref()
        .map(|methods| {
            methods
                .list
                .iter()
                .filter(|field| field.names.as_ref().is_none_or(Vec::is_empty))
                .filter_map(|field| field.type_.as_ref())
                .flat_map(constraint_type_terms)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn type_parameter_names(
    type_params: Option<&ast::FieldList<'_>>,
) -> Vec<std::string::String> {
    type_params
        .map(|fields| {
            fields
                .list
                .iter()
                .filter_map(|field| field.names.as_ref())
                .flat_map(|names| names.iter().map(|name| name.name.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn anonymous_top_level_struct_type_name(var_name: &str) -> std::string::String {
    format!("__gors_anon_var_{var_name}")
}

fn type_param_constraints(
    type_params: Option<&ast::FieldList<'_>>,
) -> HashMap<std::string::String, Vec<GoType>> {
    let mut constraints = HashMap::new();
    let Some(type_params) = type_params else {
        return constraints;
    };
    let type_param_names: HashSet<_> = type_params
        .list
        .iter()
        .filter_map(|field| field.names.as_ref())
        .flat_map(|names| names.iter().map(|name| name.name.to_string()))
        .collect();
    for field in &type_params.list {
        let Some(names) = &field.names else {
            continue;
        };
        let terms: Vec<_> = field
            .type_
            .as_ref()
            .map(constraint_type_terms)
            .unwrap_or_default()
            .into_iter()
            .map(|term| erase_type_param_mentions(term, &type_param_names))
            .filter(|term| !matches!(term, GoType::Unknown | GoType::Any))
            .collect();
        if terms.is_empty() {
            continue;
        }
        for name in names {
            constraints.insert(name.name.to_string(), terms.clone());
        }
    }
    constraints
}

fn erase_type_param_mentions(ty: GoType, names: &HashSet<std::string::String>) -> GoType {
    match ty {
        GoType::Named(name) if names.contains(&name) => GoType::Unknown,
        GoType::Instantiated { name, args } if names.contains(&name) => {
            let _ = args;
            GoType::Unknown
        }
        GoType::Instantiated { name, args } => GoType::Instantiated {
            name,
            args: args
                .into_iter()
                .map(|ty| erase_type_param_mentions(ty, names))
                .collect(),
        },
        GoType::Slice(elem) => GoType::Slice(Box::new(erase_type_param_mentions(*elem, names))),
        GoType::Pointer(elem) => GoType::Pointer(Box::new(erase_type_param_mentions(*elem, names))),
        GoType::Array(elem) => GoType::Array(Box::new(erase_type_param_mentions(*elem, names))),
        GoType::Map(key, value) => GoType::Map(
            Box::new(erase_type_param_mentions(*key, names)),
            Box::new(erase_type_param_mentions(*value, names)),
        ),
        GoType::Chan { elem, direction } => GoType::Chan {
            elem: Box::new(erase_type_param_mentions(*elem, names)),
            direction,
        },
        GoType::Func {
            params,
            results,
            variadic_start,
        } => GoType::Func {
            params: params
                .into_iter()
                .map(|ty| erase_type_param_mentions(ty, names))
                .collect(),
            results: results
                .into_iter()
                .map(|ty| erase_type_param_mentions(ty, names))
                .collect(),
            variadic_start,
        },
        other => other,
    }
}

fn substitute_type_params(
    ty: GoType,
    substitutions: &HashMap<std::string::String, GoType>,
) -> GoType {
    match ty {
        GoType::Named(name) => substitutions
            .get(&name)
            .cloned()
            .unwrap_or(GoType::Named(name)),
        GoType::Instantiated { name, args } => GoType::Instantiated {
            name,
            args: args
                .into_iter()
                .map(|ty| substitute_type_params(ty, substitutions))
                .collect(),
        },
        GoType::Slice(elem) => {
            GoType::Slice(Box::new(substitute_type_params(*elem, substitutions)))
        }
        GoType::Pointer(elem) => {
            GoType::Pointer(Box::new(substitute_type_params(*elem, substitutions)))
        }
        GoType::Array(elem) => {
            GoType::Array(Box::new(substitute_type_params(*elem, substitutions)))
        }
        GoType::Map(key, value) => GoType::Map(
            Box::new(substitute_type_params(*key, substitutions)),
            Box::new(substitute_type_params(*value, substitutions)),
        ),
        GoType::Chan { elem, direction } => GoType::Chan {
            elem: Box::new(substitute_type_params(*elem, substitutions)),
            direction,
        },
        GoType::Func {
            params,
            results,
            variadic_start,
        } => GoType::Func {
            params: params
                .into_iter()
                .map(|ty| substitute_type_params(ty, substitutions))
                .collect(),
            results: results
                .into_iter()
                .map(|ty| substitute_type_params(ty, substitutions))
                .collect(),
            variadic_start,
        },
        other => other,
    }
}

fn go_type_contains_unknown(ty: &GoType) -> bool {
    match ty {
        GoType::Unknown => true,
        GoType::Slice(inner) | GoType::Pointer(inner) | GoType::Array(inner) => {
            go_type_contains_unknown(inner)
        }
        GoType::Map(key, value) => go_type_contains_unknown(key) || go_type_contains_unknown(value),
        GoType::Chan { elem, .. } => go_type_contains_unknown(elem),
        GoType::Func {
            params, results, ..
        } => {
            params.iter().any(go_type_contains_unknown)
                || results.iter().any(go_type_contains_unknown)
        }
        GoType::Instantiated { args, .. } => args.iter().any(go_type_contains_unknown),
        _ => false,
    }
}

fn constraint_type_terms(expr: &ast::Expr<'_>) -> Vec<GoType> {
    match expr {
        ast::Expr::BinaryExpr(binary) if binary.op == token::Token::OR => {
            let mut terms = constraint_type_terms(&binary.x);
            terms.extend(constraint_type_terms(&binary.y));
            terms
        }
        ast::Expr::ParenExpr(paren) => constraint_type_terms(&paren.x),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::TILDE => {
            constraint_type_terms(&unary.x)
        }
        ast::Expr::Ident(ident) if ident.name == "any" => Vec::new(),
        other => {
            let ty = GoType::from_expr(other);
            if matches!(ty, GoType::Unknown | GoType::Any) {
                Vec::new()
            } else {
                vec![ty]
            }
        }
    }
}

fn type_expr_is_instantiated(expr: &ast::Expr<'_>) -> bool {
    match expr {
        ast::Expr::IndexExpr(_) | ast::Expr::IndexListExpr(_) => true,
        ast::Expr::ParenExpr(paren) => type_expr_is_instantiated(&paren.x),
        ast::Expr::StarExpr(star) => type_expr_is_instantiated(&star.x),
        _ => false,
    }
}

fn alias_target_name(expr: &ast::Expr<'_>) -> Option<std::string::String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::IndexExpr(index) => alias_target_name(&index.x),
        ast::Expr::IndexListExpr(index) => alias_target_name(&index.x),
        ast::Expr::ParenExpr(paren) => alias_target_name(&paren.x),
        ast::Expr::SelectorExpr(selector) => Some(selector.sel.name.to_string()),
        ast::Expr::StarExpr(star) => alias_target_name(&star.x),
        _ => None,
    }
}

fn qualify_package_constraint_map(
    package_name: &str,
    constraints: &HashMap<std::string::String, Vec<GoType>>,
    package_env: &TypeEnv,
) -> HashMap<std::string::String, Vec<GoType>> {
    constraints
        .iter()
        .map(|(name, terms)| {
            (
                name.clone(),
                terms
                    .iter()
                    .map(|term| qualify_package_type(package_name, term, package_env))
                    .collect(),
            )
        })
        .collect()
}

struct StructFieldFacts {
    fields: Vec<(std::string::String, GoType)>,
    embedded_fields: HashSet<std::string::String>,
    array_lengths: Vec<(std::string::String, i128)>,
}

fn collect_struct_field_facts(struct_type: &ast::StructType<'_>) -> StructFieldFacts {
    let mut fields = vec![];
    let mut embedded_fields = HashSet::new();
    let mut array_lengths = vec![];
    if let Some(ref field_list) = struct_type.fields {
        for field in &field_list.list {
            let array_len = field.type_.as_ref().and_then(array_type_len_value);
            let ty = field
                .type_
                .as_ref()
                .map(GoType::from_expr)
                .unwrap_or(GoType::Unknown);
            if let Some(ref names) = field.names {
                for field_name in names {
                    fields.push((field_name.name.to_string(), ty.clone()));
                    if let Some(len) = array_len {
                        array_lengths.push((field_name.name.to_string(), len));
                    }
                }
            } else if let Some(type_expr) = &field.type_
                && let Some(field_name) = embedded_field_name(type_expr)
            {
                embedded_fields.insert(field_name.clone());
                fields.push((field_name, ty.clone()));
            }
        }
    }
    StructFieldFacts {
        fields,
        embedded_fields,
        array_lengths,
    }
}

fn non_empty_struct_type<'a>(expr: &'a ast::Expr<'a>) -> Option<&'a ast::StructType<'a>> {
    match expr {
        ast::Expr::ParenExpr(paren) => non_empty_struct_type(&paren.x),
        ast::Expr::StructType(struct_type)
            if struct_type
                .fields
                .as_ref()
                .is_some_and(|fields| !fields.list.is_empty()) =>
        {
            Some(struct_type)
        }
        _ => None,
    }
}

fn anonymous_value_spec_struct_type<'a>(
    vs: &'a ast::ValueSpec<'a>,
) -> Option<(std::string::String, &'a ast::StructType<'a>)> {
    let struct_type = vs.type_.as_ref().and_then(non_empty_struct_type)?;
    let first_name = vs.names.first()?;
    Some((
        anonymous_top_level_struct_type_name(first_name.name),
        struct_type,
    ))
}

fn value_spec_explicit_go_type(vs: &ast::ValueSpec<'_>) -> Option<GoType> {
    anonymous_value_spec_struct_type(vs)
        .map(|(type_name, _)| GoType::Named(type_name))
        .or_else(|| vs.type_.as_ref().map(GoType::from_expr))
}

fn qualify_package_type(package_name: &str, ty: &GoType, package_env: &TypeEnv) -> GoType {
    match ty {
        GoType::Named(name) if !name.contains('.') && package_env.get_type_kind(name).is_some() => {
            GoType::Named(format!("{package_name}.{name}"))
        }
        GoType::Instantiated { name, args } => {
            let qualified_name = if !name.contains('.') && package_env.get_type_kind(name).is_some()
            {
                format!("{package_name}.{name}")
            } else {
                name.clone()
            };
            GoType::Instantiated {
                name: qualified_name,
                args: args
                    .iter()
                    .map(|arg| qualify_package_type(package_name, arg, package_env))
                    .collect(),
            }
        }
        GoType::Pointer(inner) => GoType::Pointer(Box::new(qualify_package_type(
            package_name,
            inner,
            package_env,
        ))),
        GoType::Slice(inner) => GoType::Slice(Box::new(qualify_package_type(
            package_name,
            inner,
            package_env,
        ))),
        GoType::Array(inner) => GoType::Array(Box::new(qualify_package_type(
            package_name,
            inner,
            package_env,
        ))),
        GoType::Map(key, value) => GoType::Map(
            Box::new(qualify_package_type(package_name, key, package_env)),
            Box::new(qualify_package_type(package_name, value, package_env)),
        ),
        GoType::Chan { elem, direction } => GoType::Chan {
            elem: Box::new(qualify_package_type(package_name, elem, package_env)),
            direction: *direction,
        },
        GoType::Func {
            params,
            results,
            variadic_start,
        } => GoType::Func {
            params: params
                .iter()
                .map(|param| qualify_package_type(package_name, param, package_env))
                .collect(),
            results: results
                .iter()
                .map(|result| qualify_package_type(package_name, result, package_env))
                .collect(),
            variadic_start: *variadic_start,
        },
        _ => ty.clone(),
    }
}

fn qualify_package_interface_name(
    package_name: &str,
    name: &str,
    package_env: &TypeEnv,
) -> std::string::String {
    match qualify_package_type(package_name, &GoType::Named(name.to_string()), package_env) {
        GoType::Named(qualified) | GoType::Interface(qualified) => qualified,
        _ => name.to_string(),
    }
}

fn qualify_package_interface_names(
    package_name: &str,
    names: &[std::string::String],
    package_env: &TypeEnv,
) -> Vec<std::string::String> {
    names
        .iter()
        .map(|name| qualify_package_interface_name(package_name, name, package_env))
        .collect()
}

fn qualify_package_member_name(
    package_name: &str,
    name: &str,
    package_env: &TypeEnv,
) -> std::string::String {
    if let Some((head, _)) = name.split_once('.')
        && package_env.get_type_kind(head).is_none()
    {
        return name.to_string();
    }
    format!("{package_name}.{name}")
}

fn qualify_package_declared_type_name(
    package_name: &str,
    name: &str,
    package_env: &TypeEnv,
) -> std::string::String {
    if name.contains('.') || package_env.get_type_kind(name).is_none() {
        name.to_string()
    } else {
        format!("{package_name}.{name}")
    }
}

fn qualify_package_types(
    package_name: &str,
    types: &[GoType],
    package_env: &TypeEnv,
) -> Vec<GoType> {
    types
        .iter()
        .map(|ty| qualify_package_type(package_name, ty, package_env))
        .collect()
}

fn method_receiver_name(method_key: &str) -> Option<&str> {
    let (receiver, _) = method_key.rsplit_once('.')?;
    (!receiver.is_empty()).then_some(receiver)
}

fn qualify_package_type_kind(
    package_name: &str,
    kind: &TypeKind,
    package_env: &TypeEnv,
) -> TypeKind {
    match kind {
        TypeKind::Alias(ty) => TypeKind::Alias(qualify_package_type(package_name, ty, package_env)),
        _ => kind.clone(),
    }
}

fn canonical_import_qualified_name(
    name: &str,
    canonical_imports: &HashMap<std::string::String, std::string::String>,
) -> std::string::String {
    let Some((qualifier, member)) = name.split_once('.') else {
        return name.to_string();
    };
    canonical_imports
        .get(qualifier)
        .map(|canonical| format!("{canonical}.{member}"))
        .unwrap_or_else(|| name.to_string())
}

fn canonical_import_qualified_type(
    ty: &GoType,
    canonical_imports: &HashMap<std::string::String, std::string::String>,
) -> GoType {
    match ty {
        GoType::Named(name) => {
            GoType::Named(canonical_import_qualified_name(name, canonical_imports))
        }
        GoType::Interface(name) => {
            GoType::Interface(canonical_import_qualified_name(name, canonical_imports))
        }
        GoType::Instantiated { name, args } => GoType::Instantiated {
            name: canonical_import_qualified_name(name, canonical_imports),
            args: args
                .iter()
                .map(|arg| canonical_import_qualified_type(arg, canonical_imports))
                .collect(),
        },
        GoType::Pointer(inner) => GoType::Pointer(Box::new(canonical_import_qualified_type(
            inner,
            canonical_imports,
        ))),
        GoType::Slice(inner) => GoType::Slice(Box::new(canonical_import_qualified_type(
            inner,
            canonical_imports,
        ))),
        GoType::Array(inner) => GoType::Array(Box::new(canonical_import_qualified_type(
            inner,
            canonical_imports,
        ))),
        GoType::Map(key, value) => GoType::Map(
            Box::new(canonical_import_qualified_type(key, canonical_imports)),
            Box::new(canonical_import_qualified_type(value, canonical_imports)),
        ),
        GoType::Chan { elem, direction } => GoType::Chan {
            elem: Box::new(canonical_import_qualified_type(elem, canonical_imports)),
            direction: *direction,
        },
        GoType::Func {
            params,
            results,
            variadic_start,
        } => GoType::Func {
            params: params
                .iter()
                .map(|param| canonical_import_qualified_type(param, canonical_imports))
                .collect(),
            results: results
                .iter()
                .map(|result| canonical_import_qualified_type(result, canonical_imports))
                .collect(),
            variadic_start: *variadic_start,
        },
        _ => ty.clone(),
    }
}

fn borrowed_slice_param_indices_for_func(fd: &ast::FuncDecl, env: &TypeEnv) -> HashSet<usize> {
    let Some(body) = fd.body.as_ref() else {
        return HashSet::new();
    };
    let mut indices = HashSet::new();
    let mut index = 0usize;
    for field in &fd.type_.params.list {
        let ty = field
            .type_
            .as_ref()
            .map(GoType::from_expr)
            .unwrap_or(GoType::Unknown);
        let is_slice = matches!(env.resolve_alias(&ty), GoType::Slice(_));
        let count = field.names.as_ref().map_or(1, Vec::len);
        for offset in 0..count {
            let param_index = index + offset;
            let Some(name) = field.names.as_ref().and_then(|names| names.get(offset)) else {
                continue;
            };
            if is_slice
                && !body_reassigns_ident_breaking_slice_alias(body, name.name)
                && body_mutates_slice_param(body, name.name, env)
            {
                indices.insert(param_index);
            }
        }
        index += count;
    }
    indices
}

fn body_reassigns_ident_breaking_slice_alias(body: &ast::BlockStmt, name: &str) -> bool {
    body.list
        .iter()
        .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
}

fn stmt_reassigns_ident_breaking_slice_alias(stmt: &ast::Stmt, name: &str) -> bool {
    match stmt {
        ast::Stmt::AssignStmt(assign) => assign_reassigns_ident_breaking_slice_alias(assign, name),
        ast::Stmt::BlockStmt(block) => body_reassigns_ident_breaking_slice_alias(block, name),
        ast::Stmt::CaseClause(case) => case
            .body
            .iter()
            .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name)),
        ast::Stmt::CommClause(comm) => {
            comm.comm
                .as_ref()
                .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || comm
                    .body
                    .iter()
                    .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
        }
        ast::Stmt::ForStmt(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || for_stmt
                    .post
                    .as_ref()
                    .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || body_reassigns_ident_breaking_slice_alias(&for_stmt.body, name)
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt
                .init
                .as_ref()
                .as_ref()
                .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || body_reassigns_ident_breaking_slice_alias(&if_stmt.body, name)
                || if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
        }
        ast::Stmt::LabeledStmt(labeled) => {
            stmt_reassigns_ident_breaking_slice_alias(&labeled.stmt, name)
        }
        ast::Stmt::RangeStmt(range) => body_reassigns_ident_breaking_slice_alias(&range.body, name),
        ast::Stmt::SelectStmt(select) => select
            .body
            .list
            .iter()
            .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name)),
        ast::Stmt::SwitchStmt(switch) => {
            switch
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || switch
                    .body
                    .list
                    .iter()
                    .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            type_switch
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
                || stmt_reassigns_ident_breaking_slice_alias(&type_switch.assign, name)
                || type_switch
                    .body
                    .list
                    .iter()
                    .any(|stmt| stmt_reassigns_ident_breaking_slice_alias(stmt, name))
        }
        ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => false,
    }
}

fn assign_reassigns_ident_breaking_slice_alias(assign: &ast::AssignStmt, name: &str) -> bool {
    assign.lhs.iter().enumerate().any(|(index, lhs)| {
        if !matches!(lhs, ast::Expr::Ident(ident) if ident.name == name) {
            return false;
        }
        if !matches!(assign.tok, token::Token::ASSIGN | token::Token::DEFINE)
            || assign.lhs.len() != assign.rhs.len()
        {
            return true;
        }
        assign
            .rhs
            .get(index)
            .is_none_or(|rhs| !expr_aliases_slice_param(rhs, name))
    })
}

fn body_mutates_slice_param(body: &ast::BlockStmt, name: &str, env: &TypeEnv) -> bool {
    body.list
        .iter()
        .any(|stmt| stmt_mutates_slice_param(stmt, name, env))
}

fn stmt_mutates_slice_param(stmt: &ast::Stmt, name: &str, env: &TypeEnv) -> bool {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            assign
                .lhs
                .iter()
                .any(|lhs| lhs_mutates_slice_param(lhs, name))
                || assign
                    .rhs
                    .iter()
                    .any(|rhs| expr_mutates_slice_param(rhs, name, env))
        }
        ast::Stmt::BlockStmt(block) => body_mutates_slice_param(block, name, env),
        ast::Stmt::CaseClause(case) => case
            .body
            .iter()
            .any(|stmt| stmt_mutates_slice_param(stmt, name, env)),
        ast::Stmt::CommClause(comm) => {
            comm.comm
                .as_ref()
                .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || comm
                    .body
                    .iter()
                    .any(|stmt| stmt_mutates_slice_param(stmt, name, env))
        }
        ast::Stmt::DeclStmt(decl) => decl.decl.specs.iter().any(|spec| match spec {
            ast::Spec::ValueSpec(value) => value.values.as_ref().is_some_and(|values| {
                values
                    .iter()
                    .any(|expr| expr_mutates_slice_param(expr, name, env))
            }),
            _ => false,
        }),
        ast::Stmt::DeferStmt(defer) => call_mutates_slice_param(&defer.call, name, env),
        ast::Stmt::ExprStmt(expr) => expr_mutates_slice_param(&expr.x, name, env),
        ast::Stmt::ForStmt(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || for_stmt
                    .cond
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_slice_param(expr, name, env))
                || for_stmt
                    .post
                    .as_ref()
                    .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || body_mutates_slice_param(&for_stmt.body, name, env)
        }
        ast::Stmt::GoStmt(go) => call_mutates_slice_param(&go.call, name, env),
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt
                .init
                .as_ref()
                .as_ref()
                .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || expr_mutates_slice_param(&if_stmt.cond, name, env)
                || body_mutates_slice_param(&if_stmt.body, name, env)
                || if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
        }
        ast::Stmt::IncDecStmt(inc_dec) => lhs_mutates_slice_param(&inc_dec.x, name),
        ast::Stmt::LabeledStmt(labeled) => stmt_mutates_slice_param(&labeled.stmt, name, env),
        ast::Stmt::RangeStmt(range) => {
            expr_mutates_slice_param(&range.x, name, env)
                || body_mutates_slice_param(&range.body, name, env)
        }
        ast::Stmt::SelectStmt(select) => select
            .body
            .list
            .iter()
            .any(|stmt| stmt_mutates_slice_param(stmt, name, env)),
        ast::Stmt::SendStmt(send) => {
            expr_mutates_slice_param(&send.chan, name, env)
                || expr_mutates_slice_param(&send.value, name, env)
        }
        ast::Stmt::SwitchStmt(switch) => {
            switch
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || switch
                    .tag
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_slice_param(expr, name, env))
                || switch
                    .body
                    .list
                    .iter()
                    .any(|stmt| stmt_mutates_slice_param(stmt, name, env))
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            type_switch
                .init
                .as_ref()
                .is_some_and(|stmt| stmt_mutates_slice_param(stmt, name, env))
                || stmt_mutates_slice_param(&type_switch.assign, name, env)
                || type_switch
                    .body
                    .list
                    .iter()
                    .any(|stmt| stmt_mutates_slice_param(stmt, name, env))
        }
        ast::Stmt::ReturnStmt(ret) => ret
            .results
            .iter()
            .any(|expr| expr_mutates_slice_param(expr, name, env)),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => false,
    }
}

fn expr_mutates_slice_param(expr: &ast::Expr, name: &str, env: &TypeEnv) -> bool {
    match expr {
        ast::Expr::CallExpr(call) => call_mutates_slice_param(call, name, env),
        ast::Expr::BinaryExpr(binary) => {
            expr_mutates_slice_param(&binary.x, name, env)
                || expr_mutates_slice_param(&binary.y, name, env)
        }
        ast::Expr::CompositeLit(lit) => lit.elts.as_ref().is_some_and(|elts| {
            elts.iter().any(|elt| match elt {
                ast::Expr::KeyValueExpr(kv) => {
                    expr_mutates_slice_param(&kv.key, name, env)
                        || expr_mutates_slice_param(&kv.value, name, env)
                }
                other => expr_mutates_slice_param(other, name, env),
            })
        }),
        ast::Expr::FuncLit(func) => body_mutates_slice_param(&func.body, name, env),
        ast::Expr::IndexExpr(index) => {
            expr_mutates_slice_param(&index.x, name, env)
                || expr_mutates_slice_param(&index.index, name, env)
        }
        ast::Expr::ParenExpr(paren) => expr_mutates_slice_param(&paren.x, name, env),
        ast::Expr::SelectorExpr(selector) => expr_mutates_slice_param(&selector.x, name, env),
        ast::Expr::SliceExpr(slice) => {
            expr_mutates_slice_param(&slice.x, name, env)
                || slice
                    .low
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_slice_param(expr, name, env))
                || slice
                    .high
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_slice_param(expr, name, env))
                || slice
                    .max
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_slice_param(expr, name, env))
        }
        ast::Expr::StarExpr(star) => expr_mutates_slice_param(&star.x, name, env),
        ast::Expr::TypeAssertExpr(assert) => expr_mutates_slice_param(&assert.x, name, env),
        ast::Expr::UnaryExpr(unary) => expr_mutates_slice_param(&unary.x, name, env),
        _ => false,
    }
}

fn call_mutates_slice_param(call: &ast::CallExpr, name: &str, env: &TypeEnv) -> bool {
    if call_is_builtin_write_into_slice_param(call, name) {
        return true;
    }
    let args_mutate = call.args.as_ref().is_some_and(|args| {
        let target = call_target_key_for_slice_mutation(&call.fun, env);
        args.iter().enumerate().any(|(index, arg)| {
            expr_aliases_slice_param(arg, name)
                && target
                    .as_ref()
                    .is_some_and(|target| env.func_param_needs_borrowed_slice(target, index))
        }) || args
            .iter()
            .any(|arg| expr_mutates_slice_param(arg, name, env))
    });
    args_mutate || expr_mutates_slice_param(&call.fun, name, env)
}

fn call_is_builtin_write_into_slice_param(call: &ast::CallExpr, name: &str) -> bool {
    let ast::Expr::Ident(ident) = call.fun.as_ref() else {
        return false;
    };
    match ident.name {
        "copy" => call
            .args
            .as_ref()
            .and_then(|args| args.first())
            .is_some_and(|arg| expr_aliases_slice_param(arg, name)),
        "clear" => call.args.as_ref().is_some_and(|args| {
            args.first()
                .is_some_and(|arg| expr_aliases_slice_param(arg, name))
        }),
        _ => false,
    }
}

fn call_target_key_for_slice_mutation(fun: &ast::Expr, env: &TypeEnv) -> Option<String> {
    match fun {
        ast::Expr::Ident(ident) => env.has_func(ident.name).then(|| ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => {
            if let Some(package_key) = super::selector_semantics::qualified_member_key(selector) {
                if env.has_func(&package_key) {
                    return Some(package_key);
                }
            }
            let receiver = GoType::infer_expr(&selector.x, env);
            receiver_method_key_for_slice_mutation(receiver, selector.sel.name, env)
        }
        ast::Expr::IndexExpr(index) => call_target_key_for_slice_mutation(&index.x, env),
        ast::Expr::IndexListExpr(index) => call_target_key_for_slice_mutation(&index.x, env),
        ast::Expr::ParenExpr(paren) => call_target_key_for_slice_mutation(&paren.x, env),
        _ => None,
    }
}

fn receiver_method_key_for_slice_mutation(
    ty: GoType,
    method: &str,
    env: &TypeEnv,
) -> Option<String> {
    match ty {
        GoType::Named(name) | GoType::Interface(name) => {
            if let Some(method_key) = env.get_method_func_key(&name, method) {
                return Some(method_key);
            }
            match env.resolve_alias(&GoType::Named(name)) {
                GoType::Named(alias_name) | GoType::Interface(alias_name) => {
                    env.get_method_func_key(&alias_name, method)
                }
                GoType::Pointer(inner) => {
                    receiver_method_key_for_slice_mutation(*inner, method, env)
                }
                _ => None,
            }
        }
        GoType::Pointer(inner) => receiver_method_key_for_slice_mutation(*inner, method, env),
        other => match env.resolve_alias(&other) {
            GoType::Named(name) | GoType::Interface(name) => env.get_method_func_key(&name, method),
            GoType::Pointer(inner) => receiver_method_key_for_slice_mutation(*inner, method, env),
            _ => None,
        },
    }
}

fn lhs_mutates_slice_param(expr: &ast::Expr, name: &str) -> bool {
    match expr {
        ast::Expr::IndexExpr(index) => expr_base_aliases_slice_param(&index.x, name),
        ast::Expr::ParenExpr(paren) => lhs_mutates_slice_param(&paren.x, name),
        ast::Expr::SelectorExpr(selector) => lhs_mutates_slice_param(&selector.x, name),
        ast::Expr::StarExpr(star) => lhs_mutates_slice_param(&star.x, name),
        _ => false,
    }
}

fn expr_aliases_slice_param(expr: &ast::Expr, name: &str) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident.name == name,
        ast::Expr::ParenExpr(paren) => expr_aliases_slice_param(&paren.x, name),
        ast::Expr::SliceExpr(slice) => expr_aliases_slice_param(&slice.x, name),
        _ => false,
    }
}

fn expr_base_aliases_slice_param(expr: &ast::Expr, name: &str) -> bool {
    match expr {
        ast::Expr::Ident(ident) => ident.name == name,
        ast::Expr::IndexExpr(index) => expr_base_aliases_slice_param(&index.x, name),
        ast::Expr::ParenExpr(paren) => expr_base_aliases_slice_param(&paren.x, name),
        ast::Expr::SelectorExpr(selector) => expr_base_aliases_slice_param(&selector.x, name),
        ast::Expr::SliceExpr(slice) => expr_base_aliases_slice_param(&slice.x, name),
        ast::Expr::StarExpr(star) => expr_base_aliases_slice_param(&star.x, name),
        _ => false,
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_var(&mut self, name: &str, ty: GoType) {
        self.vars.insert(name.to_string(), ty);
    }

    pub fn set_top_level_var(&mut self, name: &str, ty: GoType) {
        self.top_level_vars.insert(name.to_string());
        self.top_level_var_types.insert(name.to_string(), ty);
    }

    pub fn is_top_level_var(&self, name: &str) -> bool {
        self.top_level_vars.contains(name)
    }

    pub fn get_top_level_var(&self, name: &str) -> Option<GoType> {
        self.top_level_var_types.get(name).cloned()
    }

    pub(crate) fn set_package_mutable_top_level_vars(
        &mut self,
        names: HashSet<std::string::String>,
    ) {
        self.mutable_top_level_vars
            .retain(|name| name.contains('.'));
        self.mutable_top_level_vars.extend(names);
    }

    pub(crate) fn is_mutable_top_level_var(&self, name: &str) -> bool {
        self.mutable_top_level_vars.contains(name)
    }

    pub fn top_level_var_types_snapshot(&self) -> Vec<(std::string::String, GoType)> {
        let mut snapshot = self
            .top_level_var_types
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect::<Vec<_>>();
        snapshot.sort_by(|(left, _), (right, _)| left.cmp(right));
        snapshot
    }

    pub(super) fn top_level_rust_item_names(&self) -> HashSet<std::string::String> {
        self.top_level_vars
            .iter()
            .chain(self.consts.iter())
            .chain(self.funcs.keys())
            .chain(self.type_kinds.keys())
            .filter(|name| !name.contains('.'))
            .cloned()
            .collect()
    }

    pub fn get_var(&self, name: &str) -> Option<GoType> {
        self.vars.get(name).cloned()
    }

    pub fn retain_package_value_bindings(&mut self) {
        self.vars
            .retain(|name, _| self.top_level_vars.contains(name) || self.consts.contains(name));
    }

    pub fn set_func(&mut self, name: &str, returns: Vec<GoType>) {
        self.funcs.insert(name.to_string(), returns);
    }

    pub fn set_func_params(&mut self, name: &str, params: Vec<GoType>) {
        self.func_params.insert(name.to_string(), params);
    }

    pub fn set_owned_interface_params(&mut self, name: &str, params: HashSet<usize>) {
        if params.is_empty() {
            self.owned_interface_params.remove(name);
        } else {
            self.owned_interface_params.insert(name.to_string(), params);
        }
    }

    pub fn set_borrowed_slice_params(&mut self, name: &str, params: HashSet<usize>) {
        if params.is_empty() {
            self.borrowed_slice_params.remove(name);
        } else {
            self.borrowed_slice_params.insert(name.to_string(), params);
        }
    }

    pub fn func_param_needs_borrowed_slice(&self, name: &str, index: usize) -> bool {
        self.borrowed_slice_params
            .get(name)
            .is_some_and(|indices| indices.contains(&index))
    }

    pub fn func_param_needs_owned_interface(&self, name: &str, index: usize) -> bool {
        self.owned_interface_params
            .get(name)
            .is_some_and(|indices| indices.contains(&index))
    }

    pub fn set_func_variadic_start(&mut self, name: &str, start: usize) {
        self.func_variadic_start.insert(name.to_string(), start);
    }

    pub fn get_func_variadic_start(&self, name: &str) -> Option<usize> {
        self.func_variadic_start.get(name).copied()
    }

    pub(crate) fn set_func_signature_shapes(
        &mut self,
        name: &str,
        shapes: FunctionSignatureShapes,
    ) {
        self.func_signature_shapes.insert(name.to_string(), shapes);
    }

    pub(crate) fn get_func_signature_shapes(&self, name: &str) -> Option<FunctionSignatureShapes> {
        self.func_signature_shapes.get(name).cloned()
    }

    pub fn set_func_interface_assertions(
        &mut self,
        name: &str,
        mut assertions: Vec<std::string::String>,
    ) {
        assertions.sort();
        assertions.dedup();
        if assertions.is_empty() {
            self.func_interface_assertions.remove(name);
        } else {
            self.func_interface_assertions
                .insert(name.to_string(), assertions);
        }
    }

    pub fn get_func_interface_assertions(&self, name: &str) -> Vec<std::string::String> {
        self.func_interface_assertions
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    fn set_top_level_interface_assertions(
        &mut self,
        name: &str,
        mut assertions: Vec<std::string::String>,
    ) {
        assertions.sort();
        assertions.dedup();
        if assertions.is_empty() {
            self.top_level_interface_assertions.remove(name);
        } else {
            self.top_level_interface_assertions
                .insert(name.to_string(), assertions);
        }
    }

    pub fn interface_assertion_names(&self) -> Vec<std::string::String> {
        let mut names = self
            .func_interface_assertions
            .values()
            .chain(self.top_level_interface_assertions.values())
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    pub fn get_func_params(&self, name: &str) -> Vec<GoType> {
        self.func_params.get(name).cloned().unwrap_or_default()
    }

    pub fn set_func_type_param_constraints(
        &mut self,
        name: &str,
        constraints: HashMap<std::string::String, Vec<GoType>>,
    ) {
        if !constraints.is_empty() {
            self.func_type_param_constraints
                .insert(name.to_string(), constraints);
        }
    }

    pub fn get_func_type_param_constraint(
        &self,
        func_name: &str,
        type_param: &str,
    ) -> Option<Vec<GoType>> {
        self.func_type_param_constraints
            .get(func_name)
            .and_then(|constraints| constraints.get(type_param))
            .cloned()
    }

    fn extend_scoped_type_param_constraints(
        &mut self,
        constraints: HashMap<std::string::String, Vec<GoType>>,
    ) {
        self.scoped_type_param_constraints.extend(constraints);
    }

    pub(crate) fn extend_scoped_type_param_constraints_from_fields(
        &mut self,
        type_params: Option<&ast::FieldList<'_>>,
    ) {
        self.extend_scoped_type_param_constraints(type_param_constraints(type_params));
    }

    pub fn get_type_param_constraints(&self, type_param: &str) -> Option<Vec<GoType>> {
        if let Some(constraints) = self.scoped_type_param_constraints.get(type_param) {
            return Some(constraints.clone());
        }
        self.func_type_param_constraints
            .values()
            .find_map(|constraints| constraints.get(type_param))
            .cloned()
    }

    pub fn has_func(&self, name: &str) -> bool {
        self.funcs.contains_key(name) || self.func_params.contains_key(name)
    }

    pub fn set_pointer_receiver_method(&mut self, name: &str) {
        self.pointer_receiver_methods.insert(name.to_string());
    }

    pub fn method_has_pointer_receiver(&self, name: &str) -> bool {
        self.pointer_receiver_methods.contains(name)
    }

    pub fn has_value_method(&self, name: &str) -> bool {
        self.has_func(name) && !self.method_has_pointer_receiver(name)
    }

    pub fn get_func_return(&self, name: &str) -> GoType {
        self.funcs
            .get(name)
            .and_then(|r| r.first().cloned())
            .unwrap_or(GoType::Unknown)
    }

    pub fn get_func_returns(&self, name: &str) -> Vec<GoType> {
        self.funcs.get(name).cloned().unwrap_or_default()
    }

    pub fn has_method_func(&self, receiver: &str, method: &str) -> bool {
        self.method_func_key(receiver, method).is_some()
    }

    pub fn get_method_func_key(&self, receiver: &str, method: &str) -> Option<std::string::String> {
        self.method_func_key(receiver, method)
    }

    pub fn get_method_return(&self, receiver: &str, method: &str) -> GoType {
        self.method_func_key(receiver, method)
            .map(|key| self.get_func_return(&key))
            .unwrap_or(GoType::Unknown)
    }

    pub fn get_method_returns(&self, receiver: &str, method: &str) -> Vec<GoType> {
        self.method_func_key(receiver, method)
            .map(|key| self.get_func_returns(&key))
            .unwrap_or_default()
    }

    pub fn get_method_params(&self, receiver: &str, method: &str) -> Vec<GoType> {
        self.method_func_key(receiver, method)
            .map(|key| self.get_func_params(&key))
            .unwrap_or_default()
    }

    pub fn get_method_variadic_start(&self, receiver: &str, method: &str) -> Option<usize> {
        self.method_func_key(receiver, method)
            .and_then(|key| self.get_func_variadic_start(&key))
    }

    pub fn set_type_kind(&mut self, name: &str, kind: TypeKind) {
        self.type_kinds.insert(name.to_string(), kind);
    }

    /// Replace a source interface declaration with a concrete generated
    /// representation.
    ///
    /// Runtime primitives can intentionally expose a concrete Rust handle for
    /// a Go interface. Updating only `type_kinds` is insufficient because
    /// `is_interface` also consults the recorded method-set tables. Clear the
    /// interface-only declaration facts while retaining the ordinary method
    /// signatures that describe the concrete handle's ABI.
    pub(crate) fn replace_interface_with_concrete_type(&mut self, name: &str, kind: TypeKind) {
        debug_assert!(!matches!(kind, TypeKind::Interface));
        self.set_type_kind(name, kind);
        self.interface_methods.remove(name);
        self.interface_embedded.remove(name);
        self.interface_type_terms.remove(name);
    }

    pub fn remove_type_kind(&mut self, name: &str) {
        self.type_kinds.remove(name);
    }

    pub fn get_type_kind(&self, name: &str) -> Option<&TypeKind> {
        self.type_kinds.get(name)
    }

    pub fn struct_type_names(&self) -> Vec<std::string::String> {
        let mut names = self
            .type_kinds
            .iter()
            .filter_map(|(name, kind)| matches!(kind, TypeKind::Struct).then_some(name.clone()))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    /// Names introduced by defined-type declarations that can carry methods.
    ///
    /// `TypeKind::Alias` describes the underlying shape of both Go defined
    /// types such as `type Bytes []byte` and true aliases such as
    /// `type Bytes = []byte`. The separate `type_aliases` set distinguishes
    /// those declarations. Interface implementor discovery must consider the
    /// former, while excluding aliases, interfaces, and type parameters.
    pub fn concrete_named_type_names(&self) -> Vec<std::string::String> {
        let mut names = self
            .type_kinds
            .iter()
            .filter_map(|(name, kind)| {
                (!matches!(kind, TypeKind::Interface | TypeKind::TypeParam)
                    && !self.is_type_alias(name))
                .then_some(name.clone())
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub(super) fn declared_type_names(&self) -> Vec<std::string::String> {
        let mut names = self.type_kinds.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn interface_names(&self) -> Vec<std::string::String> {
        let mut names = self.interface_methods.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn set_type_param_count(&mut self, name: &str, count: usize) {
        self.type_param_counts.insert(name.to_string(), count);
    }

    pub fn get_type_param_count(&self, name: &str) -> Option<usize> {
        self.type_param_counts.get(name).copied()
    }

    pub fn set_type_param_names(&mut self, name: &str, names: Vec<std::string::String>) {
        self.type_param_names.insert(name.to_string(), names);
    }

    pub fn get_type_param_names(&self, name: &str) -> Vec<std::string::String> {
        self.type_param_names.get(name).cloned().unwrap_or_default()
    }

    pub fn set_type_alias(
        &mut self,
        name: &str,
        target: Option<std::string::String>,
        instantiated: bool,
    ) {
        self.type_aliases.insert(name.to_string());
        if let Some(target) = target {
            self.type_alias_targets.insert(name.to_string(), target);
        }
        if instantiated {
            self.instantiated_type_aliases.insert(name.to_string());
        }
    }

    pub fn is_type_alias(&self, name: &str) -> bool {
        self.type_aliases.contains(name)
    }

    /// Record the stable generated-module identity of this package's declared types.
    ///
    /// Package facts keep their own declarations unqualified. When those facts
    /// are merged under a file-local import name, `merge_package` carries this
    /// identity along so method signatures can compare exact Go declarations
    /// even when one side has already been canonicalized for Rust module output.
    pub(crate) fn record_canonical_package_identity(&mut self, module_name: &str) {
        let identities = self
            .type_kinds
            .keys()
            .filter(|name| !name.contains('.') && !self.type_aliases.contains(*name))
            .map(|name| (name.clone(), format!("{module_name}.{name}")))
            .collect::<Vec<_>>();
        for (name, canonical) in identities {
            if name != canonical {
                self.canonical_declared_type_identities
                    .insert(name, canonical);
            }
        }
    }

    pub fn alias_denotes_instantiated_generic(&self, name: &str) -> bool {
        let mut current = name;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.to_string()) {
                return false;
            }
            if self.instantiated_type_aliases.contains(current) {
                return true;
            }
            let Some(next) = self.type_alias_targets.get(current) else {
                return false;
            };
            if !self.type_aliases.contains(next) {
                return false;
            }
            current = next.as_str();
        }
    }

    pub fn is_interface(&self, name: &str) -> bool {
        matches!(self.type_kinds.get(name), Some(TypeKind::Interface))
            || self.interface_methods.contains_key(name)
    }

    pub fn set_interface_methods(&mut self, name: &str, methods: Vec<std::string::String>) {
        self.interface_methods.insert(name.to_string(), methods);
    }

    pub fn set_interface_embedded(&mut self, name: &str, embedded: Vec<std::string::String>) {
        if embedded.is_empty() {
            self.interface_embedded.remove(name);
        } else {
            self.interface_embedded.insert(name.to_string(), embedded);
        }
    }

    pub fn get_interface_methods(&self, name: &str) -> Option<Vec<std::string::String>> {
        self.interface_methods.get(name)?;
        let mut methods = Vec::new();
        self.collect_interface_methods(name, &mut HashSet::new(), &mut methods);
        Some(methods)
    }

    pub fn get_interface_direct_methods(&self, name: &str) -> Option<Vec<std::string::String>> {
        self.interface_methods.get(name).cloned()
    }

    pub fn get_interface_direct_embedded_interfaces(&self, name: &str) -> Vec<std::string::String> {
        self.interface_embedded
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_interface_embedded_interfaces(&self, name: &str) -> Vec<std::string::String> {
        let mut out = Vec::new();
        self.collect_interface_embedded_interfaces(name, &mut HashSet::new(), &mut out);
        out
    }

    pub fn set_interface_type_terms(&mut self, name: &str, terms: Vec<GoType>) {
        if !terms.is_empty() {
            self.interface_type_terms.insert(name.to_string(), terms);
        }
    }

    pub fn get_interface_type_terms(&self, name: &str) -> Vec<GoType> {
        self.interface_type_terms
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn interface_method_sets(&self) -> Vec<(std::string::String, Vec<std::string::String>)> {
        self.interface_methods
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    self.get_interface_methods(name).unwrap_or_default(),
                )
            })
            .collect()
    }

    pub fn exported_names(&self) -> Vec<std::string::String> {
        let mut names: Vec<_> = self
            .funcs
            .keys()
            .filter(|name| !name.contains('.') && go_name_is_exported(name))
            .chain(
                self.top_level_vars
                    .iter()
                    .filter(|name| go_name_is_exported(name)),
            )
            .chain(self.consts.iter().filter(|name| go_name_is_exported(name)))
            .chain(
                self.type_kinds
                    .keys()
                    .filter(|name| go_name_is_exported(name)),
            )
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn interface_implementors(&self, name: &str) -> Vec<std::string::String> {
        let Some(required_methods) = self.get_interface_methods(name) else {
            return Vec::new();
        };
        if required_methods.is_empty() {
            return Vec::new();
        }
        let mut implementors: Vec<_> = self
            .concrete_named_type_names()
            .into_iter()
            .filter(|type_name| self.named_type_implements_interface(type_name, name, false))
            .collect();
        implementors.sort();
        implementors
    }

    pub fn interface_pointer_implementors(&self, name: &str) -> Vec<std::string::String> {
        let Some(required_methods) = self.get_interface_methods(name) else {
            return Vec::new();
        };
        if required_methods.is_empty() {
            return Vec::new();
        }
        let mut implementors: Vec<_> = self
            .concrete_named_type_names()
            .into_iter()
            .filter(|type_name| self.named_type_implements_interface(type_name, name, true))
            .collect();
        implementors.sort();
        implementors
    }

    pub fn named_type_implements_interface(
        &self,
        type_name: &str,
        interface_name: &str,
        include_pointer_receiver_methods: bool,
    ) -> bool {
        self.get_interface_methods(interface_name)
            .is_some_and(|methods| {
                methods.iter().all(|method| {
                    self.named_type_method_satisfies_interface_method(
                        type_name,
                        interface_name,
                        method,
                        include_pointer_receiver_methods,
                    )
                })
            })
    }

    /// Reports whether every method required by `expected_interface` is present
    /// on `source_interface` with the same Go signature and generated interface
    /// ABI. Interface-to-interface bridges must use this method rather than
    /// comparing method names: Rust trait adapters cannot repair a disagreement
    /// in parameters, results, variadics, fixed-array lengths, or ownership.
    pub fn interface_implements_interface(
        &self,
        source_interface: &str,
        expected_interface: &str,
    ) -> bool {
        if !self.is_interface(source_interface) || !self.is_interface(expected_interface) {
            return false;
        }
        if source_interface == expected_interface {
            return true;
        }
        self.get_interface_methods(expected_interface)
            .is_some_and(|methods| {
                methods.iter().all(|method| {
                    self.interface_method_satisfies_interface_method(
                        source_interface,
                        expected_interface,
                        method,
                    )
                })
            })
    }

    pub fn named_type_implements_methods(
        &self,
        type_name: &str,
        methods: &[std::string::String],
        include_pointer_receiver_methods: bool,
    ) -> bool {
        methods.iter().all(|method| {
            self.named_type_has_method(type_name, method, include_pointer_receiver_methods)
        })
    }

    pub fn resolved_named_method_key(
        &self,
        type_name: &str,
        method: &str,
        include_pointer_receiver_methods: bool,
    ) -> Option<std::string::String> {
        resolved_selector_method(
            &GoType::Named(type_name.to_string()),
            method,
            include_pointer_receiver_methods,
            self,
        )
        .map(|resolved| resolved.key)
    }

    fn named_type_has_method(
        &self,
        type_name: &str,
        method: &str,
        include_pointer_receiver_methods: bool,
    ) -> bool {
        resolved_selector_method(
            &GoType::Named(type_name.to_string()),
            method,
            include_pointer_receiver_methods,
            self,
        )
        .is_some()
    }

    pub(super) fn named_type_method_satisfies_interface_method(
        &self,
        type_name: &str,
        interface_name: &str,
        method: &str,
        include_pointer_receiver_methods: bool,
    ) -> bool {
        let Some(concrete_method) = resolved_selector_method(
            &GoType::Named(type_name.to_string()),
            method,
            include_pointer_receiver_methods,
            self,
        ) else {
            return false;
        };
        let Some(interface_key) = self.get_method_func_key(interface_name, method) else {
            return true;
        };
        let Some(interface_signature) = self.method_signature_for_key(&interface_key) else {
            return true;
        };
        let Some(concrete_signature) = self.method_signature_for_selector_method(&concrete_method)
        else {
            return true;
        };
        self.method_signatures_match(&interface_signature, &concrete_signature)
    }

    fn interface_method_satisfies_interface_method(
        &self,
        source_interface: &str,
        expected_interface: &str,
        method: &str,
    ) -> bool {
        let Some(expected_key) = self.get_method_func_key(expected_interface, method) else {
            return false;
        };
        let Some(source_key) = self.get_method_func_key(source_interface, method) else {
            return false;
        };
        let Some(expected_signature) = self.method_signature_for_key(&expected_key) else {
            return false;
        };
        let Some(source_signature) = self.method_signature_for_key(&source_key) else {
            return false;
        };
        self.method_signatures_match(&expected_signature, &source_signature)
            && self.method_interface_abis_match(&expected_signature, &source_signature)
    }

    fn method_signature_for_key(&self, key: &str) -> Option<MethodSignature> {
        if !self.func_params.contains_key(key) || !self.funcs.contains_key(key) {
            return None;
        }
        Some(MethodSignature {
            params: self.get_func_params(key),
            results: self.get_func_returns(key),
            variadic_start: self.get_func_variadic_start(key),
            shapes: self.get_func_signature_shapes(key),
            owned_interface_params: self
                .owned_interface_params
                .get(key)
                .cloned()
                .unwrap_or_default(),
            borrowed_slice_params: self
                .borrowed_slice_params
                .get(key)
                .cloned()
                .unwrap_or_default(),
        })
    }

    fn method_signature_for_selector_method(
        &self,
        method: &super::selector_semantics::SelectorMethod,
    ) -> Option<MethodSignature> {
        let signature = self.method_signature_for_key(&method.key)?;
        Some(MethodSignature {
            params: resolved_method_signature_types(method, signature.params, self),
            results: resolved_method_signature_types(method, signature.results, self),
            variadic_start: signature.variadic_start,
            shapes: signature.shapes,
            owned_interface_params: signature.owned_interface_params,
            borrowed_slice_params: signature.borrowed_slice_params,
        })
    }

    fn method_signatures_match(
        &self,
        expected: &MethodSignature,
        actual: &MethodSignature,
    ) -> bool {
        expected.variadic_start == actual.variadic_start
            && self.signature_type_lists_match(&expected.params, &actual.params)
            && self.signature_type_lists_match(&expected.results, &actual.results)
            && self.signature_shapes_match(
                expected.shapes.as_ref(),
                actual.shapes.as_ref(),
                expected
                    .params
                    .iter()
                    .chain(&expected.results)
                    .chain(&actual.params)
                    .chain(&actual.results)
                    .any(go_type_contains_fixed_array),
            )
    }

    fn method_interface_abis_match(
        &self,
        expected: &MethodSignature,
        actual: &MethodSignature,
    ) -> bool {
        expected.owned_interface_params == actual.owned_interface_params
            && expected.borrowed_slice_params == actual.borrowed_slice_params
    }

    fn signature_shapes_match(
        &self,
        expected: Option<&FunctionSignatureShapes>,
        actual: Option<&FunctionSignatureShapes>,
        exact_fixed_arrays_required: bool,
    ) -> bool {
        let contains_fixed_array = |shapes: &FunctionSignatureShapes| {
            shapes
                .params
                .iter()
                .chain(&shapes.results)
                .any(SignatureTypeShape::contains_fixed_array)
        };
        match (expected, actual) {
            (Some(expected), Some(actual)) if exact_fixed_arrays_required => {
                expected == actual
                    && expected
                        .params
                        .iter()
                        .chain(&expected.results)
                        .all(SignatureTypeShape::has_exact_fixed_array_lengths)
                    && actual
                        .params
                        .iter()
                        .chain(&actual.results)
                        .all(SignatureTypeShape::has_exact_fixed_array_lengths)
            }
            _ if exact_fixed_arrays_required => false,
            (Some(expected), Some(actual))
                if contains_fixed_array(expected) || contains_fixed_array(actual) =>
            {
                expected == actual
            }
            (Some(expected), None) => !contains_fixed_array(expected),
            (None, Some(actual)) => !contains_fixed_array(actual),
            _ => true,
        }
    }

    fn signature_type_lists_match(&self, expected: &[GoType], actual: &[GoType]) -> bool {
        expected.len() == actual.len()
            && expected
                .iter()
                .zip(actual)
                .all(|(expected, actual)| self.signature_types_match(expected, actual))
    }

    fn signature_types_match(&self, expected: &GoType, actual: &GoType) -> bool {
        if go_type_contains_unknown(expected) || go_type_contains_unknown(actual) {
            return true;
        }
        let expected = self.resolve_signature_aliases(expected, &mut HashSet::new());
        let actual = self.resolve_signature_aliases(actual, &mut HashSet::new());
        self.canonicalize_declared_type_identities(&expected)
            == self.canonicalize_declared_type_identities(&actual)
    }

    fn canonicalize_declared_type_identities(&self, ty: &GoType) -> GoType {
        match ty {
            GoType::Named(name) => GoType::Named(self.canonical_declared_type_name(name)),
            GoType::Interface(name) => GoType::Interface(self.canonical_declared_type_name(name)),
            GoType::Instantiated { name, args } => GoType::Instantiated {
                name: self.canonical_declared_type_name(name),
                args: args
                    .iter()
                    .map(|arg| self.canonicalize_declared_type_identities(arg))
                    .collect(),
            },
            GoType::Pointer(inner) => {
                GoType::Pointer(Box::new(self.canonicalize_declared_type_identities(inner)))
            }
            GoType::Slice(inner) => {
                GoType::Slice(Box::new(self.canonicalize_declared_type_identities(inner)))
            }
            GoType::Array(inner) => {
                GoType::Array(Box::new(self.canonicalize_declared_type_identities(inner)))
            }
            GoType::Map(key, value) => GoType::Map(
                Box::new(self.canonicalize_declared_type_identities(key)),
                Box::new(self.canonicalize_declared_type_identities(value)),
            ),
            GoType::Chan { elem, direction } => GoType::Chan {
                elem: Box::new(self.canonicalize_declared_type_identities(elem)),
                direction: *direction,
            },
            GoType::Func {
                params,
                results,
                variadic_start,
            } => GoType::Func {
                params: params
                    .iter()
                    .map(|param| self.canonicalize_declared_type_identities(param))
                    .collect(),
                results: results
                    .iter()
                    .map(|result| self.canonicalize_declared_type_identities(result))
                    .collect(),
                variadic_start: *variadic_start,
            },
            _ => ty.clone(),
        }
    }

    pub(crate) fn canonical_declared_type_name(&self, name: &str) -> std::string::String {
        let mut canonical = name.to_string();
        let mut visiting = HashSet::new();
        while visiting.insert(canonical.clone()) {
            let Some(next) = self.canonical_declared_type_identities.get(&canonical) else {
                break;
            };
            canonical.clone_from(next);
        }
        canonical
    }

    fn resolve_signature_aliases(
        &self,
        ty: &GoType,
        visiting: &mut HashSet<std::string::String>,
    ) -> GoType {
        match ty {
            GoType::Named(name) if self.is_type_alias(name) => {
                if !visiting.insert(name.clone()) {
                    return ty.clone();
                }
                let resolved = match self.type_kinds.get(name) {
                    Some(TypeKind::Alias(inner)) => self.resolve_signature_aliases(inner, visiting),
                    _ => ty.clone(),
                };
                visiting.remove(name);
                resolved
            }
            GoType::Instantiated { name, args } if self.is_type_alias(name) => {
                if !visiting.insert(name.clone()) {
                    return ty.clone();
                }
                let resolved_args = args
                    .iter()
                    .map(|arg| self.resolve_signature_aliases(arg, visiting))
                    .collect::<Vec<_>>();
                let resolved = match self.type_kinds.get(name) {
                    Some(TypeKind::Alias(inner)) => {
                        let type_params = self.get_type_param_names(name);
                        let substituted = if type_params.len() == resolved_args.len() {
                            let substitutions = type_params
                                .into_iter()
                                .zip(resolved_args)
                                .collect::<HashMap<_, _>>();
                            substitute_type_params(inner.clone(), &substitutions)
                        } else {
                            inner.clone()
                        };
                        self.resolve_signature_aliases(&substituted, visiting)
                    }
                    _ => GoType::Instantiated {
                        name: name.clone(),
                        args: resolved_args,
                    },
                };
                visiting.remove(name);
                resolved
            }
            GoType::Instantiated { name, args } => GoType::Instantiated {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.resolve_signature_aliases(arg, visiting))
                    .collect(),
            },
            GoType::Pointer(inner) => {
                GoType::Pointer(Box::new(self.resolve_signature_aliases(inner, visiting)))
            }
            GoType::Slice(inner) => {
                GoType::Slice(Box::new(self.resolve_signature_aliases(inner, visiting)))
            }
            GoType::Array(inner) => {
                GoType::Array(Box::new(self.resolve_signature_aliases(inner, visiting)))
            }
            GoType::Map(key, value) => GoType::Map(
                Box::new(self.resolve_signature_aliases(key, visiting)),
                Box::new(self.resolve_signature_aliases(value, visiting)),
            ),
            GoType::Chan { elem, direction } => GoType::Chan {
                elem: Box::new(self.resolve_signature_aliases(elem, visiting)),
                direction: *direction,
            },
            GoType::Func {
                params,
                results,
                variadic_start,
            } => GoType::Func {
                params: params
                    .iter()
                    .map(|param| self.resolve_signature_aliases(param, visiting))
                    .collect(),
                results: results
                    .iter()
                    .map(|result| self.resolve_signature_aliases(result, visiting))
                    .collect(),
                variadic_start: *variadic_start,
            },
            GoType::Interface(name) => GoType::Named(name.clone()),
            _ => ty.clone(),
        }
    }

    pub fn resolve_alias(&self, ty: &GoType) -> GoType {
        match ty {
            GoType::Named(name) => match self.type_kinds.get(name) {
                Some(TypeKind::Alias(inner)) => self.resolve_alias(inner),
                _ => ty.clone(),
            },
            GoType::Instantiated { name, args } => {
                let resolved_args = args
                    .iter()
                    .map(|arg| self.resolve_alias(arg))
                    .collect::<Vec<_>>();
                match self.type_kinds.get(name) {
                    Some(TypeKind::Alias(inner)) => {
                        let type_params = self.get_type_param_names(name);
                        if type_params.len() == resolved_args.len() {
                            let substitutions = type_params
                                .into_iter()
                                .zip(resolved_args)
                                .collect::<HashMap<_, _>>();
                            self.resolve_alias(&substitute_type_params(
                                inner.clone(),
                                &substitutions,
                            ))
                        } else {
                            self.resolve_alias(inner)
                        }
                    }
                    _ => GoType::Instantiated {
                        name: name.clone(),
                        args: resolved_args,
                    },
                }
            }
            GoType::Pointer(inner) => GoType::Pointer(Box::new(self.resolve_alias(inner))),
            GoType::Slice(inner) => GoType::Slice(Box::new(self.resolve_alias(inner))),
            GoType::Array(inner) => GoType::Array(Box::new(self.resolve_alias(inner))),
            GoType::Map(key, value) => GoType::Map(
                Box::new(self.resolve_alias(key)),
                Box::new(self.resolve_alias(value)),
            ),
            GoType::Chan { elem, direction } => GoType::Chan {
                elem: Box::new(self.resolve_alias(elem)),
                direction: *direction,
            },
            _ => ty.clone(),
        }
    }

    pub fn resolve_alias_outer(&self, ty: &GoType) -> GoType {
        match ty {
            GoType::Named(name) => match self.type_kinds.get(name) {
                Some(TypeKind::Alias(inner)) => inner.clone(),
                _ => ty.clone(),
            },
            GoType::Instantiated { name, args } => match self.type_kinds.get(name) {
                Some(TypeKind::Alias(inner)) => {
                    let type_params = self.get_type_param_names(name);
                    if type_params.len() == args.len() {
                        let substitutions = type_params
                            .into_iter()
                            .zip(args.iter().cloned())
                            .collect::<HashMap<_, _>>();
                        substitute_type_params(inner.clone(), &substitutions)
                    } else {
                        inner.clone()
                    }
                }
                _ => ty.clone(),
            },
            _ => ty.clone(),
        }
    }

    pub fn resolve_type_param_constraint(&self, ty: &GoType) -> Option<GoType> {
        let GoType::Named(name) = ty else {
            return None;
        };
        let constraints = self.get_type_param_constraints(name)?;
        let expanded = constraints
            .iter()
            .flat_map(|term| self.expanded_constraint_terms(term))
            .collect::<Vec<_>>();
        if let [single] = expanded.as_slice() {
            Some(single.clone())
        } else {
            constraints.first().cloned()
        }
    }

    pub fn expanded_constraint_terms(&self, term: &GoType) -> Vec<GoType> {
        self.expanded_constraint_terms_inner(term, &mut HashSet::new())
    }

    fn expanded_constraint_terms_inner(
        &self,
        term: &GoType,
        visiting: &mut HashSet<std::string::String>,
    ) -> Vec<GoType> {
        match self.resolve_alias(term) {
            GoType::Named(name) | GoType::Interface(name) => {
                self.expanded_interface_constraint_terms(&name, &[], term, visiting)
            }
            GoType::Instantiated { name, args } => {
                self.expanded_interface_constraint_terms(&name, &args, term, visiting)
            }
            other => vec![other],
        }
    }

    fn expanded_interface_constraint_terms(
        &self,
        name: &str,
        args: &[GoType],
        fallback: &GoType,
        visiting: &mut HashSet<std::string::String>,
    ) -> Vec<GoType> {
        let terms = self.get_interface_type_terms(name);
        if terms.is_empty() || !visiting.insert(name.to_string()) {
            return vec![self.resolve_alias(fallback)];
        }
        let type_params = self.get_type_param_names(name);
        let substitutions = (type_params.len() == args.len()).then(|| {
            type_params
                .into_iter()
                .zip(args.iter().cloned())
                .collect::<HashMap<_, _>>()
        });
        let expanded = terms
            .into_iter()
            .map(|term| match substitutions.as_ref() {
                Some(substitutions) => substitute_type_params(term, substitutions),
                None => term,
            })
            .flat_map(|term| self.expanded_constraint_terms_inner(&term, visiting))
            .collect();
        visiting.remove(name);
        expanded
    }

    pub fn resolve_alias_or_type_param_constraint(&self, ty: &GoType) -> GoType {
        let resolved = self.resolve_alias(ty);
        if matches!(resolved, GoType::Named(_)) {
            self.resolve_type_param_constraint(&resolved)
                .unwrap_or(resolved)
        } else {
            resolved
        }
    }

    pub fn set_struct_fields(&mut self, name: &str, fields: Vec<(std::string::String, GoType)>) {
        self.struct_fields.insert(name.to_string(), fields);
    }

    fn collect_interface_methods(
        &self,
        name: &str,
        visiting: &mut HashSet<std::string::String>,
        methods: &mut Vec<std::string::String>,
    ) {
        if !visiting.insert(name.to_string()) {
            return;
        }
        if let Some(explicit) = self.interface_methods.get(name) {
            for method in explicit {
                if !methods.contains(method) {
                    methods.push(method.clone());
                }
            }
        }
        if let Some(embedded) = self.interface_embedded.get(name) {
            for embedded_name in embedded {
                let Some(resolved_name) = self.resolve_embedded_interface_name(embedded_name)
                else {
                    continue;
                };
                self.collect_interface_methods(&resolved_name, visiting, methods);
            }
        }
        visiting.remove(name);
    }

    fn collect_interface_embedded_interfaces(
        &self,
        name: &str,
        visiting: &mut HashSet<std::string::String>,
        out: &mut Vec<std::string::String>,
    ) {
        if !visiting.insert(name.to_string()) {
            return;
        }
        if let Some(embedded) = self.interface_embedded.get(name) {
            for embedded_name in embedded {
                let Some(resolved_name) = self.resolve_embedded_interface_name(embedded_name)
                else {
                    continue;
                };
                if !out.contains(&resolved_name) {
                    out.push(resolved_name.clone());
                }
                self.collect_interface_embedded_interfaces(&resolved_name, visiting, out);
            }
        }
        visiting.remove(name);
    }

    fn method_func_key(&self, receiver: &str, method: &str) -> Option<std::string::String> {
        let direct = format!("{receiver}.{method}");
        if self.has_func(&direct) {
            return Some(direct);
        }
        if !self.is_interface(receiver) {
            return None;
        }
        self.interface_method_owner(receiver, method, &mut HashSet::new())
            .map(|owner| format!("{owner}.{method}"))
    }

    fn interface_method_owner(
        &self,
        interface_name: &str,
        method: &str,
        visiting: &mut HashSet<std::string::String>,
    ) -> Option<std::string::String> {
        if !visiting.insert(interface_name.to_string()) {
            return None;
        }
        let direct = format!("{interface_name}.{method}");
        if self.has_func(&direct) {
            visiting.remove(interface_name);
            return Some(interface_name.to_string());
        }
        if let Some(embedded) = self.interface_embedded.get(interface_name) {
            for embedded_name in embedded {
                let Some(resolved_name) = self.resolve_embedded_interface_name(embedded_name)
                else {
                    continue;
                };
                if let Some(owner) = self.interface_method_owner(&resolved_name, method, visiting) {
                    visiting.remove(interface_name);
                    return Some(owner);
                }
            }
        }
        visiting.remove(interface_name);
        None
    }

    fn resolve_embedded_interface_name(&self, name: &str) -> Option<std::string::String> {
        if self.is_interface(name) {
            return Some(name.to_string());
        }
        match self.type_kinds.get(name) {
            Some(TypeKind::Alias(GoType::Named(target))) if self.is_interface(target) => {
                Some(target.clone())
            }
            Some(TypeKind::Alias(GoType::Interface(target))) if self.is_interface(target) => {
                Some(target.clone())
            }
            _ => None,
        }
    }

    pub fn set_struct_embedded_fields(&mut self, name: &str, fields: HashSet<std::string::String>) {
        self.struct_embedded_fields.insert(name.to_string(), fields);
    }

    pub fn is_struct_embedded_field(&self, struct_name: &str, field_name: &str) -> bool {
        self.struct_embedded_fields
            .get(struct_name)
            .is_some_and(|fields| fields.contains(field_name))
    }

    pub fn set_struct_field_array_len(&mut self, struct_name: &str, field_name: &str, len: i128) {
        self.struct_field_array_lengths
            .entry(struct_name.to_string())
            .or_default()
            .insert(field_name.to_string(), len);
    }

    pub fn get_field_type(&self, struct_name: &str, field_name: &str) -> GoType {
        self.struct_fields
            .get(struct_name)
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|(n, _)| n == field_name)
                    .map(|(_, t)| t.clone())
            })
            .unwrap_or(GoType::Unknown)
    }

    pub fn get_struct_fields(&self, struct_name: &str) -> Vec<(std::string::String, GoType)> {
        self.struct_fields
            .get(struct_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_struct_fields_with_type_args(
        &self,
        struct_name: &str,
        type_args: &[GoType],
    ) -> Vec<(std::string::String, GoType)> {
        let fields = self.get_struct_fields(struct_name);
        if type_args.is_empty() {
            return fields;
        }
        let type_params = self.get_type_param_names(struct_name);
        if type_params.len() != type_args.len() {
            return fields;
        }
        let substitutions = type_params
            .into_iter()
            .zip(type_args.iter().cloned())
            .collect::<HashMap<_, _>>();
        fields
            .into_iter()
            .map(|(name, ty)| (name, substitute_type_params(ty, &substitutions)))
            .collect()
    }

    pub fn get_field_array_len(&self, struct_name: &str, field_name: &str) -> Option<i128> {
        self.struct_field_array_lengths
            .get(struct_name)
            .and_then(|fields| fields.get(field_name))
            .copied()
    }

    pub fn get_field_array_len_from_receiver(
        &self,
        receiver_type: &GoType,
        field_name: &str,
    ) -> Option<i128> {
        match self.resolve_alias(receiver_type) {
            GoType::Named(name) | GoType::Instantiated { name, .. } => {
                self.get_field_array_len(&name, field_name)
            }
            GoType::Pointer(inner) => self.get_field_array_len_from_receiver(&inner, field_name),
            _ => None,
        }
    }

    pub fn set_string_const(&mut self, name: &str) {
        self.string_consts.insert(name.to_string());
    }

    pub fn set_const(&mut self, name: &str) {
        self.consts.insert(name.to_string());
    }

    pub fn set_const_type(&mut self, name: &str, ty: GoType) {
        self.set_const(name);
        self.const_types.insert(name.to_string(), ty);
    }

    pub fn set_const_integer_value(&mut self, name: &str, value: i128) {
        self.set_const(name);
        self.const_integer_values.insert(name.to_string(), value);
        self.const_integer_exact_values
            .insert(name.to_string(), value.to_string());
    }

    pub fn get_const_integer_value(&self, name: &str) -> Option<i128> {
        self.const_integer_values.get(name).copied()
    }

    pub(super) fn set_const_integer_exact_value(&mut self, name: &str, value: &ExactInt) {
        self.set_const(name);
        self.const_integer_exact_values
            .insert(name.to_string(), value.decimal_string());
        if let Some(value) = value.to_i128() {
            self.const_integer_values.insert(name.to_string(), value);
        } else {
            self.const_integer_values.remove(name);
        }
    }

    pub(super) fn get_const_integer_exact_value(&self, name: &str) -> Option<ExactInt> {
        self.const_integer_exact_values
            .get(name)
            .and_then(|value| ExactInt::parse_decimal(value))
            .or_else(|| {
                self.const_integer_values
                    .get(name)
                    .copied()
                    .map(ExactInt::from_i128)
            })
    }

    pub fn is_const(&self, name: &str) -> bool {
        self.consts.contains(name)
            && self
                .const_types
                .get(name)
                .is_none_or(|const_ty| self.vars.get(name).is_none_or(|var_ty| var_ty == const_ty))
    }

    pub fn is_string_const(&self, name: &str) -> bool {
        self.string_consts.contains(name)
    }

    pub fn string_const_names(&self) -> HashSet<std::string::String> {
        self.string_consts.clone()
    }

    pub fn merge_package(&mut self, package_name: &str, package_env: &TypeEnv) {
        for (name, returns) in &package_env.funcs {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_func(
                &qualified_name,
                qualify_package_types(package_name, returns, package_env),
            );
        }
        for (name, params) in &package_env.func_params {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_func_params(
                &qualified_name,
                qualify_package_types(package_name, params, package_env),
            );
        }
        for (name, indices) in &package_env.owned_interface_params {
            self.set_owned_interface_params(
                &qualify_package_member_name(package_name, name, package_env),
                indices.clone(),
            );
        }
        for (name, indices) in &package_env.borrowed_slice_params {
            self.set_borrowed_slice_params(
                &qualify_package_member_name(package_name, name, package_env),
                indices.clone(),
            );
        }
        for name in &package_env.pointer_receiver_methods {
            self.set_pointer_receiver_method(&qualify_package_member_name(
                package_name,
                name,
                package_env,
            ));
        }
        for (name, constraints) in &package_env.func_type_param_constraints {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_func_type_param_constraints(
                &qualified_name,
                qualify_package_constraint_map(package_name, constraints, package_env),
            );
        }
        for (name, start) in &package_env.func_variadic_start {
            self.set_func_variadic_start(
                &qualify_package_member_name(package_name, name, package_env),
                *start,
            );
        }
        for (name, shapes) in &package_env.func_signature_shapes {
            self.set_func_signature_shapes(
                &qualify_package_member_name(package_name, name, package_env),
                shapes.clone(),
            );
        }
        for (name, assertions) in &package_env.func_interface_assertions {
            self.set_func_interface_assertions(
                &qualify_package_member_name(package_name, name, package_env),
                qualify_package_interface_names(package_name, assertions, package_env),
            );
        }
        for (name, assertions) in &package_env.top_level_interface_assertions {
            self.set_top_level_interface_assertions(
                &qualify_package_member_name(package_name, name, package_env),
                qualify_package_interface_names(package_name, assertions, package_env),
            );
        }
        for (name, kind) in &package_env.type_kinds {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_type_kind(
                &qualified_name,
                qualify_package_type_kind(package_name, kind, package_env),
            );
            if package_env.type_aliases.contains(name) {
                self.set_type_alias(
                    &qualified_name,
                    package_env.type_alias_targets.get(name).map(|target| {
                        qualify_package_interface_name(package_name, target, package_env)
                    }),
                    package_env.instantiated_type_aliases.contains(name),
                );
            }
        }
        for (name, count) in &package_env.type_param_counts {
            self.set_type_param_count(
                &qualify_package_member_name(package_name, name, package_env),
                *count,
            );
        }
        for (name, type_params) in &package_env.type_param_names {
            self.set_type_param_names(
                &qualify_package_member_name(package_name, name, package_env),
                type_params.clone(),
            );
        }
        for (name, methods) in &package_env.interface_methods {
            self.set_interface_methods(
                &qualify_package_member_name(package_name, name, package_env),
                methods.clone(),
            );
        }
        for (name, embedded) in &package_env.interface_embedded {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            let qualified = embedded
                .iter()
                .map(|embedded_name| {
                    qualify_package_interface_name(package_name, embedded_name, package_env)
                })
                .collect();
            self.set_interface_embedded(&qualified_name, qualified);
        }
        for (name, terms) in &package_env.interface_type_terms {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_interface_type_terms(
                &qualified_name,
                terms
                    .iter()
                    .map(|term| qualify_package_type(package_name, term, package_env))
                    .collect(),
            );
        }
        for (name, fields) in &package_env.struct_fields {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            let qualified_fields = fields
                .iter()
                .map(|(field_name, ty)| {
                    (
                        field_name.clone(),
                        qualify_package_type(package_name, ty, package_env),
                    )
                })
                .collect();
            self.set_struct_fields(&qualified_name, qualified_fields);
        }
        for (name, fields) in &package_env.struct_embedded_fields {
            self.set_struct_embedded_fields(
                &qualify_package_member_name(package_name, name, package_env),
                fields.clone(),
            );
        }
        for (name, fields) in &package_env.struct_field_array_lengths {
            let struct_name = qualify_package_member_name(package_name, name, package_env);
            for (field_name, len) in fields {
                self.set_struct_field_array_len(&struct_name, field_name, *len);
            }
        }
        for (name, ty) in &package_env.vars {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_var(
                &qualified_name,
                qualify_package_type(package_name, ty, package_env),
            );
        }
        for name in &package_env.top_level_vars {
            if let Some(ty) = package_env.top_level_var_types.get(name) {
                let qualified_name = qualify_package_member_name(package_name, name, package_env);
                self.set_top_level_var(
                    &qualified_name,
                    qualify_package_type(package_name, ty, package_env),
                );
            }
        }
        for name in &package_env.mutable_top_level_vars {
            self.mutable_top_level_vars
                .insert(qualify_package_member_name(package_name, name, package_env));
        }
        for name in &package_env.consts {
            self.set_const(&qualify_package_member_name(
                package_name,
                name,
                package_env,
            ));
        }
        for (name, ty) in &package_env.const_types {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            self.set_const_type(
                &qualified_name,
                qualify_package_type(package_name, ty, package_env),
            );
        }
        for (name, value) in &package_env.const_integer_values {
            self.set_const_integer_value(
                &qualify_package_member_name(package_name, name, package_env),
                *value,
            );
        }
        for (name, value) in &package_env.const_integer_exact_values {
            let Some(value) = ExactInt::parse_decimal(value) else {
                continue;
            };
            self.set_const_integer_exact_value(
                &qualify_package_member_name(package_name, name, package_env),
                &value,
            );
        }
        for name in &package_env.string_consts {
            self.set_string_const(&qualify_package_member_name(
                package_name,
                name,
                package_env,
            ));
        }
        self.merge_canonical_declared_type_identities(package_name, package_env);
    }

    pub fn merge_package_receiver_facts(
        &mut self,
        package_name: &str,
        package_env: &TypeEnv,
        receiver_names: &HashSet<std::string::String>,
    ) {
        if receiver_names.is_empty() {
            return;
        }
        for (name, returns) in &package_env.funcs {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                let qualified_name = qualify_package_member_name(package_name, name, package_env);
                self.set_func(
                    &qualified_name,
                    qualify_package_types(package_name, returns, package_env),
                );
            }
        }
        for (name, params) in &package_env.func_params {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                let qualified_name = qualify_package_member_name(package_name, name, package_env);
                self.set_func_params(
                    &qualified_name,
                    qualify_package_types(package_name, params, package_env),
                );
            }
        }
        for (name, indices) in &package_env.owned_interface_params {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_owned_interface_params(
                    &qualify_package_member_name(package_name, name, package_env),
                    indices.clone(),
                );
            }
        }
        for (name, indices) in &package_env.borrowed_slice_params {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_borrowed_slice_params(
                    &qualify_package_member_name(package_name, name, package_env),
                    indices.clone(),
                );
            }
        }
        for name in &package_env.pointer_receiver_methods {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_pointer_receiver_method(&qualify_package_member_name(
                    package_name,
                    name,
                    package_env,
                ));
            }
        }
        for (name, constraints) in &package_env.func_type_param_constraints {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                let qualified_name = qualify_package_member_name(package_name, name, package_env);
                self.set_func_type_param_constraints(
                    &qualified_name,
                    qualify_package_constraint_map(package_name, constraints, package_env),
                );
            }
        }
        for (name, start) in &package_env.func_variadic_start {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_func_variadic_start(
                    &qualify_package_member_name(package_name, name, package_env),
                    *start,
                );
            }
        }
        for (name, shapes) in &package_env.func_signature_shapes {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_func_signature_shapes(
                    &qualify_package_member_name(package_name, name, package_env),
                    shapes.clone(),
                );
            }
        }
        for (name, assertions) in &package_env.func_interface_assertions {
            if method_receiver_name(name).is_some_and(|receiver| receiver_names.contains(receiver))
            {
                self.set_func_interface_assertions(
                    &qualify_package_member_name(package_name, name, package_env),
                    qualify_package_interface_names(package_name, assertions, package_env),
                );
            }
        }
        for name in receiver_names {
            let qualified_name = qualify_package_member_name(package_name, name, package_env);
            if let Some(kind) = package_env.type_kinds.get(name) {
                self.set_type_kind(
                    &qualified_name,
                    qualify_package_type_kind(package_name, kind, package_env),
                );
            }
            if let Some(count) = package_env.type_param_counts.get(name) {
                self.set_type_param_count(&qualified_name, *count);
            }
            if let Some(type_params) = package_env.type_param_names.get(name) {
                self.set_type_param_names(&qualified_name, type_params.clone());
            }
            if package_env.type_aliases.contains(name) {
                self.set_type_alias(
                    &qualified_name,
                    package_env.type_alias_targets.get(name).map(|target| {
                        qualify_package_interface_name(package_name, target, package_env)
                    }),
                    package_env.instantiated_type_aliases.contains(name),
                );
            }
            if let Some(methods) = package_env.interface_methods.get(name) {
                self.set_interface_methods(&qualified_name, methods.clone());
            }
            if let Some(embedded) = package_env.interface_embedded.get(name) {
                self.set_interface_embedded(
                    &qualified_name,
                    embedded
                        .iter()
                        .map(|embedded_name| {
                            qualify_package_interface_name(package_name, embedded_name, package_env)
                        })
                        .collect(),
                );
            }
            if let Some(terms) = package_env.interface_type_terms.get(name) {
                self.set_interface_type_terms(
                    &qualified_name,
                    terms
                        .iter()
                        .map(|term| qualify_package_type(package_name, term, package_env))
                        .collect(),
                );
            }
            if let Some(fields) = package_env.struct_fields.get(name) {
                self.set_struct_fields(
                    &qualified_name,
                    fields
                        .iter()
                        .map(|(field_name, ty)| {
                            (
                                field_name.clone(),
                                qualify_package_type(package_name, ty, package_env),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(fields) = package_env.struct_embedded_fields.get(name) {
                self.set_struct_embedded_fields(&qualified_name, fields.clone());
            }
            if let Some(fields) = package_env.struct_field_array_lengths.get(name) {
                for (field_name, len) in fields {
                    self.set_struct_field_array_len(&qualified_name, field_name, *len);
                }
            }
        }
        self.merge_canonical_declared_type_identities(package_name, package_env);
    }

    fn merge_canonical_declared_type_identities(
        &mut self,
        package_name: &str,
        package_env: &TypeEnv,
    ) {
        for (name, canonical) in &package_env.canonical_declared_type_identities {
            let qualified_name =
                qualify_package_declared_type_name(package_name, name, package_env);
            let qualified_canonical =
                qualify_package_declared_type_name(package_name, canonical, package_env);
            if qualified_name != qualified_canonical {
                self.canonical_declared_type_identities
                    .insert(qualified_name, qualified_canonical);
            }
        }
    }

    /// Pre-scan a Go AST file to populate type declarations and function signatures.
    pub fn scan_file(&mut self, file: &ast::File) {
        for decl in &file.decls {
            match decl {
                ast::Decl::GenDecl(gd) => {
                    for spec in &gd.specs {
                        match spec {
                            ast::Spec::TypeSpec(ts) => self.scan_type_spec(ts),
                            ast::Spec::ValueSpec(value) => {
                                self.scan_top_level_value_interface_assertions(value);
                            }
                            ast::Spec::ImportSpec(_) => {}
                        }
                    }
                }
                ast::Decl::FuncDecl(fd) => {
                    self.scan_func_decl(fd);
                }
            }
        }
        self.refresh_borrowed_slice_params(&[file]);
        for decl in &file.decls {
            let ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            let mut inherited_const_type = None;
            let mut inherited_const_values: Option<&[ast::Expr<'_>]> = None;
            for (iota_value, spec) in gd.specs.iter().enumerate() {
                let ast::Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                self.scan_value_spec(
                    vs,
                    gd.tok,
                    inherited_const_type.as_ref(),
                    inherited_const_values,
                    iota_value as i64,
                );
                if gd.tok == token::Token::CONST {
                    if let Some(values) = vs.values.as_deref() {
                        inherited_const_values = Some(values);
                    }
                    if let Some(type_expr) = &vs.type_ {
                        inherited_const_type = Some(GoType::from_expr(type_expr));
                    } else if let Some(values) = &vs.values
                        && let Some(first) = values.first()
                    {
                        inherited_const_type = Some(GoType::infer_expr(first, self));
                    }
                }
                for name in &vs.names {
                    if let Some(ty) = self.get_var(name.name) {
                        self.set_top_level_var(name.name, ty);
                    }
                }
            }
        }
        for (key, shapes) in signature_shape_updates(&[file], self) {
            self.set_func_signature_shapes(&key, shapes);
        }
        let inference_env = self.clone();
        self.refresh_interface_assertions_from_env(file, &inference_env);
    }

    pub fn rescan_file_top_level_values(&mut self, file: &ast::File, inference_env: &TypeEnv) {
        let mut inference_env = inference_env.clone();
        for decl in &file.decls {
            let ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            let mut inherited_const_type = None;
            let mut inherited_const_values: Option<&[ast::Expr<'_>]> = None;
            for (iota_value, spec) in gd.specs.iter().enumerate() {
                let ast::Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                let explicit_type = value_spec_explicit_go_type(vs).or_else(|| {
                    (gd.tok == token::Token::CONST && vs.values.is_none())
                        .then(|| inherited_const_type.clone())
                        .flatten()
                });
                let values = if gd.tok == token::Token::CONST {
                    vs.values.as_deref().or(inherited_const_values)
                } else {
                    vs.values.as_deref()
                };
                for (i, name) in vs.names.iter().enumerate() {
                    let ty = if let Some(ref explicit_type) = explicit_type {
                        explicit_type.clone()
                    } else {
                        values
                            .and_then(|values| values.get(i))
                            .map(|expr| GoType::infer_expr(expr, &inference_env))
                            .unwrap_or(GoType::Unknown)
                    };
                    if !matches!(ty, GoType::Unknown) {
                        if gd.tok == token::Token::CONST {
                            self.set_const_type(name.name, ty.clone());
                            self.set_var(name.name, ty.clone());
                            inference_env.set_const_type(name.name, ty.clone());
                            inference_env.set_var(name.name, ty);
                            if let Some(value_expr) = values.and_then(|values| values.get(i))
                                && let Some(value) = const_integer_value_exact(
                                    value_expr,
                                    &inference_env,
                                    Some(iota_value as i64),
                                )
                            {
                                self.set_const_integer_exact_value(name.name, &value);
                                inference_env.set_const_integer_exact_value(name.name, &value);
                            }
                        } else {
                            self.set_var(name.name, ty.clone());
                            self.set_top_level_var(name.name, ty.clone());
                            inference_env.set_var(name.name, ty.clone());
                            inference_env.set_top_level_var(name.name, ty);
                        }
                    }
                }
                if gd.tok == token::Token::CONST {
                    if let Some(spec_values) = vs.values.as_deref() {
                        inherited_const_values = Some(spec_values);
                    }
                    if let Some(type_expr) = &vs.type_ {
                        inherited_const_type = Some(GoType::from_expr(type_expr));
                    } else if let Some(first) = values.and_then(|values| values.first()) {
                        inherited_const_type = Some(GoType::infer_expr(first, &inference_env));
                    }
                }
            }
        }
    }

    /// Pre-scan a package split across multiple files.
    ///
    /// Some generated stdlib files initialize constants with conversions to
    /// types declared in a different file, such as `const ENOENT = Errno(2)`.
    /// Scanning every file twice lets the first pass collect cross-file type
    /// and function declarations before the second pass refreshes value facts.
    pub fn scan_files(&mut self, files: &[&ast::File<'_>]) {
        for file in files {
            self.scan_file(file);
        }
        for file in files {
            self.scan_file(file);
        }
        self.refresh_borrowed_slice_params(files);
        for (key, shapes) in signature_shape_updates(files, self) {
            self.set_func_signature_shapes(&key, shapes);
        }
    }

    /// Replace file-local import qualifiers in package facts with stable
    /// package identities before those facts are shared with other files or
    /// downstream packages.
    ///
    /// Go import names are scoped to one file. The declarations recorded in a
    /// package `TypeEnv`, however, outlive that file, so retaining a source
    /// qualifier such as `x.Common` would let another file's unrelated `x`
    /// import change the declaration's meaning.
    pub(crate) fn canonicalize_file_import_qualifiers(
        &mut self,
        file: &ast::File<'_>,
        canonical_imports: &HashMap<std::string::String, std::string::String>,
    ) {
        if canonical_imports.is_empty() {
            return;
        }

        for decl in &file.decls {
            match decl {
                ast::Decl::FuncDecl(func) => {
                    let key = func
                        .recv
                        .as_ref()
                        .and_then(|receiver| receiver.list.first())
                        .and_then(|field| field.type_.as_ref())
                        .map(|receiver| {
                            format!("{}.{}", extract_type_name(receiver), func.name.name)
                        })
                        .unwrap_or_else(|| func.name.name.to_string());
                    self.canonicalize_func_import_qualifiers(&key, canonical_imports);
                }
                ast::Decl::GenDecl(general) => {
                    for spec in &general.specs {
                        match spec {
                            ast::Spec::TypeSpec(type_spec) => {
                                let Some(name) = type_spec.name.as_ref().map(|name| name.name)
                                else {
                                    continue;
                                };
                                if let Some(kind) = self.type_kinds.get_mut(name)
                                    && let TypeKind::Alias(ty) = kind
                                {
                                    *ty = canonical_import_qualified_type(ty, canonical_imports);
                                }
                                if let Some(target) = self.type_alias_targets.get_mut(name) {
                                    *target =
                                        canonical_import_qualified_name(target, canonical_imports);
                                }
                                if let Some(embedded) = self.interface_embedded.get_mut(name) {
                                    for embedded_name in embedded {
                                        *embedded_name = canonical_import_qualified_name(
                                            embedded_name,
                                            canonical_imports,
                                        );
                                    }
                                }
                                if let Some(terms) = self.interface_type_terms.get_mut(name) {
                                    for term in terms {
                                        *term = canonical_import_qualified_type(
                                            term,
                                            canonical_imports,
                                        );
                                    }
                                }
                                if let Some(fields) = self.struct_fields.get_mut(name) {
                                    for (_, field_type) in fields {
                                        *field_type = canonical_import_qualified_type(
                                            field_type,
                                            canonical_imports,
                                        );
                                    }
                                }
                                for method in self
                                    .interface_methods
                                    .get(name)
                                    .cloned()
                                    .unwrap_or_default()
                                {
                                    self.canonicalize_func_import_qualifiers(
                                        &format!("{name}.{method}"),
                                        canonical_imports,
                                    );
                                }
                            }
                            ast::Spec::ValueSpec(value_spec) => {
                                for name in &value_spec.names {
                                    self.canonicalize_value_import_qualifiers(
                                        name.name,
                                        canonical_imports,
                                    );
                                    if let Some(assertions) =
                                        self.top_level_interface_assertions.get_mut(name.name)
                                    {
                                        for interface_name in assertions {
                                            *interface_name = canonical_import_qualified_name(
                                                interface_name,
                                                canonical_imports,
                                            );
                                        }
                                    }
                                }
                                if let Some((type_name, _)) =
                                    anonymous_value_spec_struct_type(value_spec)
                                    && let Some(fields) = self.struct_fields.get_mut(&type_name)
                                {
                                    for (_, field_type) in fields {
                                        *field_type = canonical_import_qualified_type(
                                            field_type,
                                            canonical_imports,
                                        );
                                    }
                                }
                            }
                            ast::Spec::ImportSpec(_) => {}
                        }
                    }
                }
            }
        }
        for (interface_name, facts) in interface_assertion_facts_in_file(file, self).anonymous {
            if let Some(embedded) = self.interface_embedded.get_mut(&interface_name) {
                for embedded_name in embedded {
                    *embedded_name =
                        canonical_import_qualified_name(embedded_name, canonical_imports);
                }
            }
            for method in facts.methods {
                self.canonicalize_func_import_qualifiers(
                    &format!("{interface_name}.{}", method.name),
                    canonical_imports,
                );
            }
        }
    }

    fn canonicalize_func_import_qualifiers(
        &mut self,
        key: &str,
        canonical_imports: &HashMap<std::string::String, std::string::String>,
    ) {
        for types in [self.func_params.get_mut(key), self.funcs.get_mut(key)]
            .into_iter()
            .flatten()
        {
            for ty in types {
                *ty = canonical_import_qualified_type(ty, canonical_imports);
            }
        }
        if let Some(constraints) = self.func_type_param_constraints.get_mut(key) {
            for terms in constraints.values_mut() {
                for term in terms {
                    *term = canonical_import_qualified_type(term, canonical_imports);
                }
            }
        }
        if let Some(assertions) = self.func_interface_assertions.get_mut(key) {
            for interface_name in assertions {
                *interface_name =
                    canonical_import_qualified_name(interface_name, canonical_imports);
            }
        }
    }

    fn canonicalize_value_import_qualifiers(
        &mut self,
        name: &str,
        canonical_imports: &HashMap<std::string::String, std::string::String>,
    ) {
        for ty in [
            self.vars.get_mut(name),
            self.top_level_var_types.get_mut(name),
            self.const_types.get_mut(name),
        ]
        .into_iter()
        .flatten()
        {
            *ty = canonical_import_qualified_type(ty, canonical_imports);
        }
    }

    pub fn refresh_borrowed_slice_params(&mut self, files: &[&ast::File<'_>]) {
        loop {
            let inference_env = self.clone();
            let changed = self.refresh_borrowed_slice_params_from_env(files, &inference_env);
            if !changed {
                break;
            }
        }
    }

    pub fn refresh_borrowed_slice_params_from_env(
        &mut self,
        files: &[&ast::File<'_>],
        inference_env: &TypeEnv,
    ) -> bool {
        let mut changed = false;
        for file in files {
            for decl in &file.decls {
                let ast::Decl::FuncDecl(fd) = decl else {
                    continue;
                };
                changed |= self.refresh_func_borrowed_slice_params_from_env(fd, inference_env);
            }
        }
        changed
    }

    pub(crate) fn refresh_signature_shapes_from_env(
        &mut self,
        files: &[&ast::File<'_>],
        inference_env: &TypeEnv,
    ) {
        for (key, shapes) in signature_shape_updates(files, inference_env) {
            self.set_func_signature_shapes(&key, shapes);
        }
    }

    pub fn borrowed_slice_param_indices_for_func_decl(&self, fd: &ast::FuncDecl) -> HashSet<usize> {
        let local_env = self.scoped_for_func_decl(fd);
        borrowed_slice_param_indices_for_func(fd, &local_env)
    }

    fn refresh_func_borrowed_slice_params_from_env(
        &mut self,
        fd: &ast::FuncDecl,
        inference_env: &TypeEnv,
    ) -> bool {
        let local_env = inference_env.scoped_for_func_decl(fd);
        let params = borrowed_slice_param_indices_for_func(fd, &local_env);
        let mut changed = false;
        if let Some(ref recv) = fd.recv
            && let Some(recv_field) = recv.list.first()
            && let Some(ref recv_type) = recv_field.type_
        {
            let method_key = format!("{}.{}", extract_type_name(recv_type), fd.name.name);
            changed |= self.replace_borrowed_slice_params_if_changed(&method_key, params.clone());
        }
        if fd.recv.is_none() {
            changed |= self.replace_borrowed_slice_params_if_changed(fd.name.name, params);
        }
        changed
    }

    pub(crate) fn scoped_for_func_decl(&self, fd: &ast::FuncDecl<'_>) -> TypeEnv {
        let mut env = self.clone();
        env.retain_package_value_bindings();
        env.extend_scoped_type_param_constraints_from_fields(fd.type_.type_params.as_ref());
        if let Some(recv) = &fd.recv {
            for field in &recv.list {
                let Some(recv_type) = field.type_.as_ref() else {
                    continue;
                };
                let recv_go_type = GoType::from_expr(recv_type);
                if let Some(names) = &field.names {
                    for name in names {
                        env.set_var(name.name, recv_go_type.clone());
                    }
                }
            }
        }
        seed_field_list_bindings(&mut env, &fd.type_.params);
        if let Some(results) = &fd.type_.results {
            seed_field_list_bindings(&mut env, results);
        }
        env
    }

    fn replace_borrowed_slice_params_if_changed(
        &mut self,
        name: &str,
        params: HashSet<usize>,
    ) -> bool {
        let current = self
            .borrowed_slice_params
            .get(name)
            .cloned()
            .unwrap_or_default();
        if current == params {
            return false;
        }
        self.set_borrowed_slice_params(name, params);
        true
    }

    fn set_struct_type_facts(&mut self, name: &str, struct_type: &ast::StructType<'_>) {
        self.set_type_kind(name, TypeKind::Struct);
        let facts = collect_struct_field_facts(struct_type);
        self.set_struct_fields(name, facts.fields);
        self.set_struct_embedded_fields(name, facts.embedded_fields);
        for (field_name, len) in facts.array_lengths {
            self.set_struct_field_array_len(name, &field_name, len);
        }
    }

    fn scan_anonymous_interface_facts(&mut self, facts: AnonymousInterfaceFacts) {
        let AnonymousInterfaceFacts {
            name,
            methods,
            embedded,
        } = facts;
        self.set_type_kind(&name, TypeKind::Interface);
        self.set_interface_methods(
            &name,
            methods
                .iter()
                .map(|signature| signature.name.clone())
                .collect(),
        );
        self.set_interface_embedded(&name, embedded);
        for signature in methods {
            let method_key = format!("{name}.{}", signature.name);
            let mut borrowed_slice_params = borrowed_slice_indices_from_params(&signature.params);
            if let Some(start) = signature.variadic_start {
                borrowed_slice_params.remove(&start);
            }
            self.set_func_params(&method_key, signature.params);
            self.set_func(&method_key, signature.results);
            self.set_func_signature_shapes(&method_key, signature.shapes);
            self.set_borrowed_slice_params(&method_key, borrowed_slice_params);
            if let Some(start) = signature.variadic_start {
                self.set_func_variadic_start(&method_key, start);
            }
        }
    }

    fn scan_interface_assertions_in_block(
        &mut self,
        block: &ast::BlockStmt<'_>,
    ) -> Vec<std::string::String> {
        let inference_env = self.clone();
        self.scan_interface_assertions_in_block_from_env(block, &inference_env)
    }

    fn scan_interface_assertions_in_block_from_env(
        &mut self,
        block: &ast::BlockStmt<'_>,
        inference_env: &TypeEnv,
    ) -> Vec<std::string::String> {
        let facts = interface_assertion_facts_in_block(block, inference_env);
        let names = facts.sorted_names();
        for anonymous in facts.anonymous.into_values() {
            self.scan_anonymous_interface_facts(anonymous);
        }
        names
    }

    fn scan_top_level_value_interface_assertions(&mut self, value: &ast::ValueSpec<'_>) {
        let inference_env = self.clone();
        self.scan_top_level_value_interface_assertions_from_env(value, &inference_env);
    }

    fn scan_top_level_value_interface_assertions_from_env(
        &mut self,
        value: &ast::ValueSpec<'_>,
        inference_env: &TypeEnv,
    ) {
        let mut facts = InterfaceAssertionFacts::new(inference_env);
        collect_interface_assertion_facts_from_value_spec(value, &mut facts);
        let names = facts.sorted_names();
        for anonymous in facts.anonymous.into_values() {
            self.scan_anonymous_interface_facts(anonymous);
        }
        for name in &value.names {
            self.set_top_level_interface_assertions(name.name, names.clone());
        }
    }

    pub(crate) fn refresh_interface_assertions_from_env(
        &mut self,
        file: &ast::File<'_>,
        inference_env: &TypeEnv,
    ) {
        for decl in &file.decls {
            match decl {
                ast::Decl::FuncDecl(func) => {
                    let Some(body) = &func.body else {
                        continue;
                    };
                    let assertions =
                        self.scan_interface_assertions_in_block_from_env(body, inference_env);
                    let key = func
                        .recv
                        .as_ref()
                        .and_then(|receiver| receiver.list.first())
                        .and_then(|field| field.type_.as_ref())
                        .map(|receiver| {
                            format!("{}.{}", extract_type_name(receiver), func.name.name)
                        })
                        .unwrap_or_else(|| func.name.name.to_string());
                    self.set_func_interface_assertions(&key, assertions);
                }
                ast::Decl::GenDecl(general) => {
                    for spec in &general.specs {
                        if let ast::Spec::ValueSpec(value) = spec {
                            self.scan_top_level_value_interface_assertions_from_env(
                                value,
                                inference_env,
                            );
                        }
                    }
                }
            }
        }
    }

    fn scan_type_spec(&mut self, ts: &ast::TypeSpec) {
        let Some(ref name) = ts.name else { return };
        let type_param_names = type_parameter_names(ts.type_params.as_ref());
        self.set_type_param_count(name.name, type_param_names.len());
        self.set_type_param_names(name.name, type_param_names);
        if ts.assign.is_some() {
            self.set_type_alias(
                name.name,
                alias_target_name(&ts.type_),
                type_expr_is_instantiated(&ts.type_),
            );
        }
        match &ts.type_ {
            ast::Expr::StructType(st) => {
                self.set_struct_type_facts(name.name, st);
            }
            ast::Expr::InterfaceType(_) => {
                self.set_type_kind(name.name, TypeKind::Interface);
                self.set_interface_methods(name.name, interface_method_names(&ts.type_));
                self.set_interface_embedded(name.name, interface_embedded_names(&ts.type_));
                self.set_interface_type_terms(name.name, interface_constraint_terms(&ts.type_));
                for signature in interface_method_signatures(&ts.type_, self) {
                    let method_key = format!("{}.{}", name.name, signature.name);
                    self.set_func_params(&method_key, signature.params);
                    self.set_func(&method_key, signature.results);
                    self.set_func_signature_shapes(&method_key, signature.shapes);
                    self.set_borrowed_slice_params(
                        &method_key,
                        borrowed_slice_indices_from_params(&self.get_func_params(&method_key)),
                    );
                    if let Some(start) = signature.variadic_start {
                        self.set_func_variadic_start(&method_key, start);
                    }
                }
            }
            other => {
                let underlying = GoType::from_expr(other);
                self.set_type_kind(name.name, TypeKind::Alias(underlying));
            }
        }
    }

    fn scan_value_spec(
        &mut self,
        vs: &ast::ValueSpec,
        tok: token::Token,
        inherited_const_type: Option<&GoType>,
        inherited_const_values: Option<&[ast::Expr<'_>]>,
        iota_value: i64,
    ) {
        if tok != token::Token::CONST
            && let Some((type_name, struct_type)) = anonymous_value_spec_struct_type(vs)
        {
            self.set_struct_type_facts(&type_name, struct_type);
        }
        let explicit_type = value_spec_explicit_go_type(vs).or_else(|| {
            (tok == token::Token::CONST && vs.values.is_none())
                .then(|| inherited_const_type.cloned())
                .flatten()
        });
        let values = if tok == token::Token::CONST {
            vs.values.as_deref().or(inherited_const_values)
        } else {
            vs.values.as_deref()
        };

        for (i, name) in vs.names.iter().enumerate() {
            let ty = if let Some(ref et) = explicit_type {
                et.clone()
            } else {
                values
                    .and_then(|v| v.get(i))
                    .map(|e| GoType::infer_expr(e, self))
                    .unwrap_or_else(|| {
                        if tok == token::Token::CONST {
                            GoType::Int
                        } else {
                            GoType::Unknown
                        }
                    })
            };
            if tok == token::Token::CONST {
                self.set_const_type(name.name, ty.clone());
                if let Some(value_expr) = values.and_then(|values| values.get(i))
                    && let Some(value) =
                        const_integer_value_exact(value_expr, self, Some(iota_value))
                {
                    self.set_const_integer_exact_value(name.name, &value);
                }
            }
            if tok == token::Token::CONST && matches!(ty, GoType::String) {
                self.set_string_const(name.name);
            }
            self.set_var(name.name, ty);
        }
    }

    fn scan_func_decl(&mut self, fd: &ast::FuncDecl) {
        let name = fd.name.name;
        let signature_shapes = function_signature_shapes(&fd.type_, self);

        let mut variadic_start = None;
        let mut param_count = 0;
        let params: Vec<GoType> = fd
            .type_
            .params
            .list
            .iter()
            .flat_map(|f| {
                let ty = f
                    .type_
                    .as_ref()
                    .map(GoType::from_expr)
                    .unwrap_or(GoType::Unknown);
                let count = f.names.as_ref().map_or(1, |n| n.len());
                if matches!(f.type_, Some(ast::Expr::Ellipsis(_))) {
                    variadic_start = Some(param_count);
                }
                param_count += count;
                std::iter::repeat_n(ty, count)
            })
            .collect();

        // Collect return types
        let returns: Vec<GoType> = fd
            .type_
            .results
            .as_ref()
            .map(|r| {
                r.list
                    .iter()
                    .flat_map(|f| {
                        let ty = f
                            .type_
                            .as_ref()
                            .map(GoType::from_expr)
                            .unwrap_or(GoType::Unknown);
                        let count = f.names.as_ref().map_or(1, |n| n.len());
                        std::iter::repeat_n(ty, count)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let is_method = fd.recv.is_some();
        let type_param_constraints = type_param_constraints(fd.type_.type_params.as_ref());

        if let Some(ref recv) = fd.recv {
            if let Some(recv_field) = recv.list.first() {
                if let Some(ref recv_type) = recv_field.type_ {
                    let recv_name = extract_type_name(recv_type);
                    let method_key = format!("{}.{}", recv_name, name);
                    self.set_func_params(&method_key, params.clone());
                    self.set_func(&method_key, returns.clone());
                    self.set_func_signature_shapes(&method_key, signature_shapes.clone());
                    self.set_func_type_param_constraints(
                        &method_key,
                        type_param_constraints.clone(),
                    );
                    if receiver_type_has_pointer_indirection(recv_type) {
                        self.set_pointer_receiver_method(&method_key);
                    }
                    if let Some(start) = variadic_start {
                        self.set_func_variadic_start(&method_key, start);
                    }
                    if let Some(body) = &fd.body {
                        let assertions = self.scan_interface_assertions_in_block(body);
                        self.set_func_interface_assertions(&method_key, assertions);
                        let owned = owned_interface_param_indices(
                            &params,
                            &fd.type_.params,
                            body,
                            |name| self.is_interface(name),
                        );
                        self.set_owned_interface_params(&method_key, owned);
                    }
                }
            }
        }
        if !is_method {
            self.set_func_params(name, params.clone());
            self.set_func(name, returns);
            self.set_func_signature_shapes(name, signature_shapes);
            self.set_func_type_param_constraints(name, type_param_constraints);
            if let Some(start) = variadic_start {
                self.set_func_variadic_start(name, start);
            }
            if let Some(body) = &fd.body {
                let assertions = self.scan_interface_assertions_in_block(body);
                self.set_func_interface_assertions(name, assertions);
                let owned =
                    owned_interface_param_indices(&params, &fd.type_.params, body, |name| {
                        self.is_interface(name)
                    });
                self.set_owned_interface_params(name, owned);
            }
        }
    }
}

fn seed_field_list_bindings(env: &mut TypeEnv, fields: &ast::FieldList<'_>) {
    for field in &fields.list {
        let ty = field
            .type_
            .as_ref()
            .map(GoType::from_expr)
            .unwrap_or(GoType::Unknown);
        if let Some(names) = &field.names {
            for name in names {
                env.set_var(name.name, ty.clone());
            }
        }
    }
}

fn embedded_field_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::StarExpr(star) => embedded_field_name(&star.x),
        ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
        ast::Expr::IndexExpr(index) => embedded_field_name(&index.x),
        ast::Expr::IndexListExpr(index) => embedded_field_name(&index.x),
        _ => None,
    }
}

fn array_type_len_value(expr: &ast::Expr<'_>) -> Option<i128> {
    match expr {
        ast::Expr::ArrayType(array) => integer_array_len_value(array.len.as_deref()?),
        ast::Expr::ParenExpr(paren) => array_type_len_value(&paren.x),
        _ => None,
    }
}

fn integer_array_len_value(expr: &ast::Expr<'_>) -> Option<i128> {
    match expr {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => parse_int_literal(lit.value),
        ast::Expr::ParenExpr(paren) => integer_array_len_value(&paren.x),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            integer_array_len_value(&unary.x)
        }
        _ => None,
    }
}

fn parse_int_literal(value: &str) -> Option<i128> {
    let cleaned = value.replace('_', "");
    let (radix, digits) = if let Some(rest) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        (8, rest)
    } else if let Some(rest) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        (2, rest)
    } else if cleaned.len() > 1 && cleaned.starts_with('0') {
        (8, &cleaned[1..])
    } else {
        (10, cleaned.as_str())
    };
    i128::from_str_radix(digits, radix).ok()
}

fn extract_type_name<'a>(expr: &'a ast::Expr<'a>) -> &'a str {
    match expr {
        ast::Expr::Ident(id) => id.name,
        ast::Expr::StarExpr(star) => extract_type_name(&star.x),
        ast::Expr::IndexExpr(index) => extract_type_name(&index.x),
        ast::Expr::IndexListExpr(index) => extract_type_name(&index.x),
        _ => "",
    }
}

fn receiver_type_has_pointer_indirection(expr: &ast::Expr<'_>) -> bool {
    match expr {
        ast::Expr::StarExpr(_) => true,
        ast::Expr::ParenExpr(paren) => receiver_type_has_pointer_indirection(&paren.x),
        _ => false,
    }
}

fn go_name_is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn scan_file_records_structural_method_interfaces_used_by_assertions() {
        let file = parse_file(
            "test.go",
            r#"
package p

func inspect(err error, target error) bool {
	if x, ok := err.(interface{ Is(error) bool }); ok && x.Is(target) {
		return true
	}
	switch x := err.(type) {
	case interface{ Unwrap() error }:
		return x.Unwrap() != nil
	case interface{ Unwrap() []error }:
		return len(x.Unwrap()) != 0
	}
	return false
}
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        let names = env.interface_assertion_names();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().all(|name| env.is_interface(name)));
        assert!(names.iter().all(|name| {
            name.starts_with(ANONYMOUS_INTERFACE_PREFIX)
                && env
                    .get_interface_direct_methods(name)
                    .is_some_and(|methods| methods.len() == 1)
        }));
        assert!(names.iter().any(|name| {
            env.get_interface_direct_methods(name) == Some(vec!["Is".to_string()])
                && env.get_method_params(name, "Is") == vec![GoType::Error]
                && env.get_method_returns(name, "Is") == vec![GoType::Bool]
        }));
        assert!(names.iter().any(|name| {
            env.get_interface_direct_methods(name) == Some(vec!["Unwrap".to_string()])
                && env.get_method_returns(name, "Unwrap") == vec![GoType::Error]
        }));
        assert!(names.iter().any(|name| {
            env.get_interface_direct_methods(name) == Some(vec!["Unwrap".to_string()])
                && env.get_method_returns(name, "Unwrap")
                    == vec![GoType::Slice(Box::new(GoType::Error))]
        }));
    }

    #[test]
    fn anonymous_interface_hash_and_shape_resolve_imported_array_constants() {
        let sizes = parse_file(
            "sizes.go",
            r#"
package sizes

const Width = 3
"#,
        )
        .unwrap();
        let mut sizes_env = TypeEnv::new();
        sizes_env.scan_file(&sizes);

        let consumer = parse_file(
            "consumer.go",
            r#"
package consumer

import "example/sizes"

func inspect(value any) {
	_, _ = value.(interface{ Fixed([sizes.Width]byte) [sizes.Width]byte })
}
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.merge_package("sizes", &sizes_env);
        env.scan_file(&consumer);

        let assertions = env.get_func_interface_assertions("inspect");
        let [interface_name] = assertions.as_slice() else {
            panic!("expected one structural assertion")
        };
        let shapes = env
            .get_func_signature_shapes(&format!("{interface_name}.Fixed"))
            .unwrap();
        let exact = SignatureTypeShape::Array {
            length: Some("3".to_string()),
            elem: Box::new(SignatureTypeShape::Other),
        };
        assert_eq!(shapes.params, vec![exact.clone()]);
        assert_eq!(shapes.results, vec![exact]);

        let literal = parse_file(
            "literal.go",
            r#"
package consumer

func inspect(value any) {
	_, _ = value.(interface{ Fixed([3]byte) [3]byte })
}
"#,
        )
        .unwrap();
        let mut literal_env = TypeEnv::new();
        literal_env.scan_file(&literal);
        assert_eq!(
            env.get_func_interface_assertions("inspect"),
            literal_env.get_func_interface_assertions("inspect"),
            "equivalent constant and literal array signatures must share one structural identity"
        );
    }

    #[test]
    fn scan_file_separates_structural_interface_discovery_from_assertion_roots() {
        let file = parse_file(
            "test.go",
            r#"
package p

func allocate() {
	_ = make([]interface{ Ghost() }, 1)
}
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert!(env.interface_assertion_names().is_empty());
        assert!(env.interface_names().iter().any(|name| {
            name.starts_with(ANONYMOUS_INTERFACE_PREFIX)
                && env.get_interface_direct_methods(name) == Some(vec!["Ghost".to_string()])
        }));
    }

    #[test]
    fn scan_file_records_package_initializer_structural_assertions() {
        let file = parse_file(
            "test.go",
            r#"
package p

var source error
var unwrapped, ok = source.(interface{ Unwrap() error })
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        let names = env.interface_assertion_names();
        assert_eq!(names.len(), 1, "{names:?}");
        assert_eq!(
            env.get_interface_direct_methods(&names[0]),
            Some(vec!["Unwrap".to_string()])
        );
        assert_eq!(
            env.get_method_returns(&names[0], "Unwrap"),
            vec![GoType::Error]
        );
    }

    #[test]
    fn merge_package_qualifies_local_types_in_signatures() {
        let mut io_env = TypeEnv::new();
        io_env.set_type_kind("Reader", TypeKind::Interface);
        io_env.set_interface_methods("Reader", vec!["Read".to_string()]);

        let mut bytes_env = TypeEnv::new();
        bytes_env.set_type_kind("Reader", TypeKind::Struct);
        bytes_env.set_func(
            "NewReader",
            vec![GoType::Pointer(Box::new(GoType::Named(
                "Reader".to_string(),
            )))],
        );
        bytes_env.set_func("Reader.Read", vec![GoType::Int, GoType::Error]);
        bytes_env.set_pointer_receiver_method("Reader.Read");

        let mut env = TypeEnv::new();
        env.merge_package("io", &io_env);
        env.merge_package("bytes", &bytes_env);

        assert_eq!(
            env.get_func_return("bytes.NewReader"),
            GoType::Pointer(Box::new(GoType::Named("bytes.Reader".to_string())))
        );
        assert!(env.named_type_implements_interface("bytes.Reader", "io.Reader", true));
    }

    #[test]
    fn merge_package_qualifies_mutable_top_level_var_representation() {
        let mut dependency_env = TypeEnv::new();
        dependency_env.set_top_level_var("Shared", GoType::Int);
        dependency_env.set_package_mutable_top_level_vars(HashSet::from(["Shared".to_string()]));

        let mut package_env = TypeEnv::new();
        package_env.set_top_level_var(
            "Default",
            GoType::Pointer(Box::new(GoType::Named("Config".to_string()))),
        );
        package_env.merge_package("dependency", &dependency_env);
        package_env.set_package_mutable_top_level_vars(HashSet::from(["Default".to_string()]));

        let mut env = TypeEnv::new();
        env.merge_package("settings", &package_env);

        assert!(env.is_mutable_top_level_var("settings.Default"));
        assert!(env.is_mutable_top_level_var("dependency.Shared"));
        assert!(!env.is_mutable_top_level_var("Default"));
    }

    #[test]
    fn merge_package_preserves_canonical_declared_type_identity_in_method_signatures() {
        let mut fs_env = TypeEnv::new();
        fs_env.set_type_kind("File", TypeKind::Interface);
        fs_env.set_type_kind("FS", TypeKind::Interface);
        fs_env.set_interface_methods("FS", vec!["Open".to_string()]);
        fs_env.set_func_params("FS.Open", vec![GoType::String]);
        fs_env.set_func(
            "FS.Open",
            vec![GoType::Named("File".to_string()), GoType::Error],
        );
        fs_env.record_canonical_package_identity("io__fs");

        let mut fstest_env = TypeEnv::new();
        fstest_env.set_type_kind(
            "MapFS",
            TypeKind::Alias(GoType::Map(Box::new(GoType::String), Box::new(GoType::Int))),
        );
        fstest_env.set_func_params("MapFS.Open", vec![GoType::String]);
        fstest_env.set_func(
            "MapFS.Open",
            vec![GoType::Named("io__fs.File".to_string()), GoType::Error],
        );
        fstest_env.set_type_kind(
            "WrongMapFS",
            TypeKind::Alias(GoType::Map(Box::new(GoType::String), Box::new(GoType::Int))),
        );
        fstest_env.set_func_params("WrongMapFS.Open", vec![GoType::String]);
        fstest_env.set_func(
            "WrongMapFS.Open",
            vec![GoType::Named("other.File".to_string()), GoType::Error],
        );
        fstest_env.record_canonical_package_identity("testing__fstest");

        let mut env = TypeEnv::new();
        env.merge_package("fs", &fs_env);
        env.merge_package("fstest", &fstest_env);

        assert_eq!(env.canonical_declared_type_name("fs.File"), "io__fs.File");
        assert!(env.named_type_implements_interface("fstest.MapFS", "fs.FS", false));
        assert!(!env.named_type_implements_interface("fstest.WrongMapFS", "fs.FS", false));
    }

    #[test]
    fn interface_implementor_discovery_includes_defined_non_struct_types() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Reader", TypeKind::Interface);
        env.set_interface_methods("Reader", vec!["Read".to_string()]);
        env.set_func_params("Reader.Read", vec![GoType::Slice(Box::new(GoType::Uint8))]);
        env.set_func("Reader.Read", vec![GoType::Int]);

        let map_type = GoType::Map(Box::new(GoType::String), Box::new(GoType::Int));
        env.set_type_kind("NamedMap", TypeKind::Alias(map_type.clone()));
        env.set_func_params(
            "NamedMap.Read",
            vec![GoType::Slice(Box::new(GoType::Uint8))],
        );
        env.set_func("NamedMap.Read", vec![GoType::Int]);

        env.set_type_kind("MapAlias", TypeKind::Alias(map_type));
        env.set_type_alias("MapAlias", Some("NamedMap".to_string()), false);
        env.set_func_params(
            "MapAlias.Read",
            vec![GoType::Slice(Box::new(GoType::Uint8))],
        );
        env.set_func("MapAlias.Read", vec![GoType::Int]);

        env.set_type_kind(
            "NamedSlice",
            TypeKind::Alias(GoType::Slice(Box::new(GoType::Uint8))),
        );
        env.set_func_params(
            "NamedSlice.Read",
            vec![GoType::Slice(Box::new(GoType::Uint8))],
        );
        env.set_func("NamedSlice.Read", vec![GoType::Int]);
        env.set_pointer_receiver_method("NamedSlice.Read");

        assert_eq!(
            env.interface_implementors("Reader"),
            vec!["NamedMap".to_string()]
        );
        assert_eq!(
            env.interface_pointer_implementors("Reader"),
            vec!["NamedMap".to_string(), "NamedSlice".to_string()]
        );
        assert!(
            !env.concrete_named_type_names()
                .contains(&"MapAlias".to_string())
        );
    }

    #[test]
    fn rescan_file_top_level_vars_uses_imported_function_returns() {
        let file = parse_file(
            "test.go",
            r#"
                package tarpkg

                import "example/debugpkg"

                var Debug = debugpkg.NewSetting("on")
            "#,
        )
        .unwrap();
        let mut debug_env = TypeEnv::new();
        debug_env.set_type_kind("Setting", TypeKind::Struct);
        debug_env.set_func(
            "NewSetting",
            vec![GoType::Pointer(Box::new(GoType::Named(
                "Setting".to_string(),
            )))],
        );

        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let mut inference_env = env.clone();
        inference_env.merge_package("debugpkg", &debug_env);

        env.rescan_file_top_level_values(&file, &inference_env);

        assert_eq!(
            env.get_top_level_var("Debug"),
            Some(GoType::Pointer(Box::new(GoType::Named(
                "debugpkg.Setting".to_string()
            ))))
        );
    }

    #[test]
    fn scan_file_keeps_function_bindings_lexical() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                var packageValue string

                func first(route string) (result string) {
                    return route
                }

                func second(path int) {}
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(env.get_var("packageValue"), Some(GoType::String));
        for local in ["route", "result", "path"] {
            assert_eq!(
                env.get_var(local),
                None,
                "function-local binding {local} leaked into package facts",
            );
        }

        let first = file
            .decls
            .iter()
            .find_map(|decl| match decl {
                ast::Decl::FuncDecl(func) if func.name.name == "first" => Some(func),
                _ => None,
            })
            .expect("first function");
        let local_env = env.scoped_for_func_decl(first);
        assert_eq!(local_env.get_var("route"), Some(GoType::String));
        assert_eq!(local_env.get_var("result"), Some(GoType::String));
        assert_eq!(local_env.get_var("path"), None);
    }

    #[test]
    fn merge_package_receiver_facts_copies_selected_methods_only() {
        let mut package_env = TypeEnv::new();
        package_env.set_type_kind("Setting", TypeKind::Struct);
        package_env.set_func(
            "New",
            vec![GoType::Pointer(Box::new(GoType::Named(
                "Setting".to_string(),
            )))],
        );
        package_env.set_func("Setting.Value", vec![GoType::String]);
        package_env.set_func_params("Setting.Value", Vec::new());
        package_env.set_pointer_receiver_method("Setting.Value");

        let mut env = TypeEnv::new();
        env.merge_package_receiver_facts(
            "godebug",
            &package_env,
            &HashSet::from(["Setting".to_string()]),
        );

        assert!(!env.has_func("godebug.New"));
        assert!(env.has_func("godebug.Setting.Value"));
        assert!(env.method_has_pointer_receiver("godebug.Setting.Value"));
        assert_eq!(
            env.get_type_kind("godebug.Setting"),
            Some(&TypeKind::Struct)
        );
    }

    #[test]
    fn scan_file_expands_embedded_interface_methods() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Reader interface {
                    Read([]byte) (int, error)
                }

                type ReadWriter interface {
                    Reader
                    Write([]byte) (int, error)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(
            env.get_interface_methods("ReadWriter"),
            Some(vec!["Write".to_string(), "Read".to_string()])
        );
    }

    #[test]
    fn refresh_borrowed_slice_params_uses_embedded_interface_method_owner() {
        let file = parse_file(
            "test.go",
            r#"
package p

                type Reader interface {
                    Read([]byte) (int, error)
                }

                type fileReader interface {
                    Reader
                }

                type holder struct {
                    curr fileReader
                }

                func (h *holder) Read(b []byte) (int, error) {
                    n, err := h.curr.Read(b)
                    return n, err
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert!(env.func_param_needs_borrowed_slice("holder.Read", 0));
    }

    #[test]
    fn refresh_borrowed_slice_params_follows_forwarded_interface_field_calls() {
        let file = parse_file(
            "test.go",
            r#"
package p

type WriterAt interface {
	WriteAt([]byte, int64) (int, error)
}

type OffsetWriter struct {
	w WriterAt
}

func (o *OffsetWriter) WriteAt(p []byte, off int64) (int, error) {
	return o.w.WriteAt(p, off)
}
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert!(env.func_param_needs_borrowed_slice("OffsetWriter.WriteAt", 0));
    }

    #[test]
    fn refresh_borrowed_slice_params_preserves_self_reslice_forwarding() {
        let file = parse_file(
            "test.go",
            r#"
package p

type Reader interface {
	Read([]byte) (int, error)
}

type regFileReader struct {
	r  Reader
	nb int64
}

func (fr *regFileReader) Read(b []byte) (n int, err error) {
	if int64(len(b)) > fr.nb {
		b = b[:fr.nb]
	}
	if len(b) > 0 {
		n, err = fr.r.Read(b)
		fr.nb -= int64(n)
	}
	return n, err
}
"#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert!(env.func_param_needs_borrowed_slice("regFileReader.Read", 0));
    }

    #[test]
    fn refresh_borrowed_slice_params_follows_external_interface_field_calls_after_reslice() {
        let file = parse_file(
            "test.go",
            r#"
package p

import "io"

type regFileReader struct {
	r  io.Reader
	nb int64
}

func (fr *regFileReader) Read(b []byte) (n int, err error) {
	if int64(len(b)) > fr.nb {
		b = b[:fr.nb]
	}
	if len(b) > 0 {
		n, err = fr.r.Read(b)
		fr.nb -= int64(n)
	}
	return n, err
}
"#,
        )
        .unwrap();
        let io_file = parse_file(
            "io.go",
            r#"
package io

type Reader interface {
	Read([]byte) (int, error)
}
"#,
        )
        .unwrap();
        let mut io_env = TypeEnv::new();
        io_env.scan_file(&io_file);
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let mut inference_env = env.clone();
        inference_env.merge_package("io", &io_env);

        env.refresh_borrowed_slice_params_from_env(&[&file], &inference_env);

        assert!(env.func_param_needs_borrowed_slice("regFileReader.Read", 0));
    }

    #[test]
    fn scan_file_records_interface_method_signatures() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Heap interface {
                    Push(any)
                    Pop() any
                }

                func Use(h Heap) any {
                    return h.Pop()
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(env.get_func_params("Heap.Push"), vec![GoType::Any]);
        assert_eq!(env.get_func_return("Heap.Pop"), GoType::Any);

        let (func, ret) = file
            .decls
            .iter()
            .find_map(|decl| match decl {
                ast::Decl::FuncDecl(func) if func.name.name == "Use" => {
                    func.body.as_ref().map(|body| (func, body))
                }
                _ => None,
            })
            .and_then(|(func, body)| body.list.first().map(|stmt| (func, stmt)))
            .and_then(|stmt| match stmt {
                (func, ast::Stmt::ReturnStmt(ret)) => {
                    ret.results.first().map(|result| (func, result))
                }
                _ => None,
            })
            .expect("return expression");
        let local_env = env.scoped_for_func_decl(func);

        assert_eq!(GoType::infer_expr(ret, &local_env), GoType::Any);
    }

    #[test]
    fn scan_file_marks_stored_interface_params_owned() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Context interface {
                    Done() int
                }

                type wrapper struct {
                    Context
                }

                func Wrap(parent Context) Context {
                    return wrapper{parent}
                }

                func Return(parent Context) Context {
                    return parent
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert!(env.func_param_needs_owned_interface("Wrap", 0));
        assert!(env.func_param_needs_owned_interface("Return", 0));
    }

    #[test]
    fn merge_package_qualifies_embedded_interface_methods() {
        let mut io_env = TypeEnv::new();
        io_env.set_type_kind("Reader", TypeKind::Interface);
        io_env.set_interface_methods("Reader", vec!["Read".to_string()]);
        io_env.set_type_kind("Writer", TypeKind::Interface);
        io_env.set_interface_methods("Writer", vec!["Write".to_string()]);
        io_env.set_type_kind("ReadWriter", TypeKind::Interface);
        io_env.set_interface_methods("ReadWriter", vec![]);
        io_env.set_interface_embedded(
            "ReadWriter",
            vec!["Reader".to_string(), "Writer".to_string()],
        );

        let mut env = TypeEnv::new();
        env.merge_package("io", &io_env);

        assert_eq!(
            env.get_interface_methods("io.ReadWriter"),
            Some(vec!["Read".to_string(), "Write".to_string()])
        );
    }

    #[test]
    fn named_type_implements_interface_promotes_only_embedded_field_methods() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Closer", TypeKind::Interface);
        env.set_interface_methods("Closer", vec!["Close".to_string()]);
        env.set_type_kind("Inner", TypeKind::Struct);
        env.set_func("Inner.Close", vec![GoType::Error]);
        env.set_type_kind("NamedField", TypeKind::Struct);
        env.set_struct_fields(
            "NamedField",
            vec![("inner".to_string(), GoType::Named("Inner".to_string()))],
        );
        env.set_type_kind("EmbeddedField", TypeKind::Struct);
        env.set_struct_fields(
            "EmbeddedField",
            vec![("Inner".to_string(), GoType::Named("Inner".to_string()))],
        );
        env.set_struct_embedded_fields(
            "EmbeddedField",
            std::collections::HashSet::from(["Inner".to_string()]),
        );

        assert!(!env.named_type_implements_interface("NamedField", "Closer", false));
        assert!(env.named_type_implements_interface("EmbeddedField", "Closer", false));
    }

    #[test]
    fn named_type_implements_interface_requires_matching_method_signatures() {
        let mut env = TypeEnv::new();
        env.set_type_kind("FS", TypeKind::Interface);
        env.set_interface_methods("FS", vec!["Open".to_string()]);
        env.set_func_params("FS.Open", vec![GoType::String]);
        env.set_func("FS.Open", vec![GoType::Int]);

        env.set_type_kind("ArchiveFile", TypeKind::Struct);
        env.set_func_params("ArchiveFile.Open", vec![]);
        env.set_func("ArchiveFile.Open", vec![GoType::Int]);

        env.set_type_kind("Directory", TypeKind::Struct);
        env.set_func_params("Directory.Open", vec![GoType::String]);
        env.set_func("Directory.Open", vec![GoType::Int]);

        assert!(!env.named_type_implements_interface("ArchiveFile", "FS", false));
        assert!(env.named_type_implements_interface("Directory", "FS", false));
    }

    #[test]
    fn fixed_array_lengths_distinguish_interface_method_signatures() {
        let file = parse_file(
            "arrays.go",
            r#"
                package arrays

                type Two interface {
                    RoundTrip([2]byte) [2]byte
                }

                type Three interface {
                    RoundTrip([3]byte) [3]byte
                }

                type pair struct{}
                func (pair) RoundTrip(value [2]byte) [2]byte { return value }

                type triple struct{}
                func (triple) RoundTrip(value [3]byte) [3]byte { return value }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert!(env.named_type_implements_interface("pair", "Two", false));
        assert!(!env.named_type_implements_interface("pair", "Three", false));
        assert!(env.named_type_implements_interface("triple", "Three", false));
        assert!(!env.named_type_implements_interface("triple", "Two", false));

        let two = env.get_func_signature_shapes("Two.RoundTrip").unwrap();
        let three = env.get_func_signature_shapes("Three.RoundTrip").unwrap();
        assert_ne!(two, three);
    }

    #[test]
    fn interface_compatibility_requires_full_signature_and_abi_match() {
        let file = parse_file(
            "interfaces.go",
            r#"
                package interfaces

                type Reader interface { Read([]byte) int }

                type Target interface {
                    Transform(Reader, [2]byte, ...string) [2]byte
                }
                type Compatible interface {
                    Transform(Reader, [2]byte, ...string) [2]byte
                }
                type WrongParam interface {
                    Transform(Reader, [2]string, ...string) [2]byte
                }
                type WrongResult interface {
                    Transform(Reader, [2]byte, ...string) [2]string
                }
                type WrongVariadic interface {
                    Transform(Reader, [2]byte, []string) [2]byte
                }
                type WrongArrayLength interface {
                    Transform(Reader, [3]byte, ...string) [2]byte
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert!(env.interface_implements_interface("Compatible", "Target"));
        assert!(!env.interface_implements_interface("WrongParam", "Target"));
        assert!(!env.interface_implements_interface("WrongResult", "Target"));
        assert!(!env.interface_implements_interface("WrongVariadic", "Target"));
        assert!(!env.interface_implements_interface("WrongArrayLength", "Target"));

        env.set_owned_interface_params("Compatible.Transform", HashSet::from([0]));
        assert!(!env.interface_implements_interface("Compatible", "Target"));
        env.set_owned_interface_params("Compatible.Transform", HashSet::new());
        assert!(env.interface_implements_interface("Compatible", "Target"));

        env.set_borrowed_slice_params("Compatible.Transform", HashSet::new());
        assert!(!env.interface_implements_interface("Compatible", "Target"));
        env.set_borrowed_slice_params("Compatible.Transform", HashSet::from([2]));
        assert!(env.interface_implements_interface("Compatible", "Target"));

        env.func_signature_shapes.remove("Compatible.Transform");
        assert!(!env.interface_implements_interface("Compatible", "Target"));
    }

    #[test]
    fn named_type_implements_interface_checks_promoted_method_signatures() {
        let mut env = TypeEnv::new();
        env.set_type_kind("ReaderFrom", TypeKind::Interface);
        env.set_interface_methods("ReaderFrom", vec!["ReadFrom".to_string()]);
        env.set_func_params(
            "ReaderFrom.ReadFrom",
            vec![GoType::Named("Reader".to_string())],
        );
        env.set_func("ReaderFrom.ReadFrom", vec![GoType::Int64, GoType::Error]);

        env.set_type_kind("Socket", TypeKind::Struct);
        env.set_func_params(
            "Socket.ReadFrom",
            vec![GoType::Slice(Box::new(GoType::Uint8))],
        );
        env.set_func(
            "Socket.ReadFrom",
            vec![
                GoType::Int,
                GoType::Named("Sockaddr".to_string()),
                GoType::Error,
            ],
        );
        env.set_type_kind("FD", TypeKind::Struct);
        env.set_struct_fields(
            "FD",
            vec![("Socket".to_string(), GoType::Named("Socket".to_string()))],
        );
        env.set_struct_embedded_fields(
            "FD",
            std::collections::HashSet::from(["Socket".to_string()]),
        );

        env.set_type_kind("CopyReader", TypeKind::Struct);
        env.set_func_params(
            "CopyReader.ReadFrom",
            vec![GoType::Named("Reader".to_string())],
        );
        env.set_func("CopyReader.ReadFrom", vec![GoType::Int64, GoType::Error]);
        env.set_type_kind("BufferedCopyReader", TypeKind::Struct);
        env.set_struct_fields(
            "BufferedCopyReader",
            vec![(
                "CopyReader".to_string(),
                GoType::Named("CopyReader".to_string()),
            )],
        );
        env.set_struct_embedded_fields(
            "BufferedCopyReader",
            std::collections::HashSet::from(["CopyReader".to_string()]),
        );

        assert!(!env.named_type_implements_interface("FD", "ReaderFrom", false));
        assert!(env.named_type_implements_interface("BufferedCopyReader", "ReaderFrom", false,));
    }

    #[test]
    fn field_and_method_inference_follow_deep_instantiated_embeddings() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Leaf", TypeKind::Struct);
        env.set_type_param_names("Leaf", vec!["T".to_string()]);
        env.set_struct_fields(
            "Leaf",
            vec![("value".to_string(), GoType::Named("T".to_string()))],
        );
        env.set_func_params("Leaf.take", vec![GoType::Named("T".to_string())]);
        env.set_func("Leaf.take", vec![GoType::Named("T".to_string())]);
        env.set_pointer_receiver_method("Leaf.take");

        env.set_type_kind("Middle", TypeKind::Struct);
        env.set_type_param_names("Middle", vec!["U".to_string()]);
        env.set_struct_fields(
            "Middle",
            vec![(
                "Leaf".to_string(),
                GoType::Pointer(Box::new(GoType::Instantiated {
                    name: "Leaf".to_string(),
                    args: vec![GoType::Named("U".to_string())],
                })),
            )],
        );
        env.set_struct_embedded_fields("Middle", HashSet::from(["Leaf".to_string()]));

        env.set_type_kind("Outer", TypeKind::Struct);
        env.set_struct_fields(
            "Outer",
            vec![(
                "Middle".to_string(),
                GoType::Instantiated {
                    name: "Middle".to_string(),
                    args: vec![GoType::Int],
                },
            )],
        );
        env.set_struct_embedded_fields("Outer", HashSet::from(["Middle".to_string()]));

        let outer = GoType::Named("Outer".to_string());
        assert_eq!(
            field_type_from_receiver_type(outer.clone(), "value", &env),
            GoType::Int
        );
        assert_eq!(
            method_return_from_receiver_type(outer.clone(), "take", &env),
            GoType::Int
        );
        assert_eq!(
            method_func_from_receiver_type(outer, "take", &env),
            GoType::Func {
                params: vec![GoType::Int],
                results: vec![GoType::Int],
                variadic_start: None,
            }
        );
    }

    #[test]
    fn interface_satisfaction_rejects_ambiguous_promoted_methods() {
        let mut env = TypeEnv::new();
        env.set_type_kind("Reader", TypeKind::Interface);
        env.set_interface_methods("Reader", vec!["Read".to_string()]);
        env.set_func("Reader.Read", vec![GoType::Int]);

        for name in ["Left", "Right"] {
            env.set_type_kind(name, TypeKind::Struct);
            env.set_func(&format!("{name}.Read"), vec![GoType::Int]);
        }
        env.set_type_kind("Outer", TypeKind::Struct);
        env.set_struct_fields(
            "Outer",
            vec![
                ("Left".to_string(), GoType::Named("Left".to_string())),
                ("Right".to_string(), GoType::Named("Right".to_string())),
            ],
        );
        env.set_struct_embedded_fields(
            "Outer",
            HashSet::from(["Left".to_string(), "Right".to_string()]),
        );

        assert!(!env.named_type_implements_interface("Outer", "Reader", false));

        env.set_func("Outer.Read", vec![GoType::Int]);
        assert!(env.named_type_implements_interface("Outer", "Reader", false));
    }

    #[test]
    fn scan_file_carries_grouped_const_type_to_implicit_specs() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type ParameterSizes int

                const (
                    L1024N160 ParameterSizes = iota
                    L2048N224
                    L2048N256
                )
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(
            env.get_var("L2048N224"),
            Some(GoType::Named("ParameterSizes".to_string()))
        );
        assert_eq!(
            env.get_var("L2048N256"),
            Some(GoType::Named("ParameterSizes".to_string()))
        );
    }

    #[test]
    fn scan_file_does_not_inherit_const_type_when_value_is_present() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                const (
                    magic = "md5"
                    marshaledSize = len(magic) + 4
                )
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(env.get_var("magic"), Some(GoType::String));
        assert_eq!(env.get_var("marshaledSize"), Some(GoType::Int));
    }

    #[test]
    fn scan_file_preserves_named_const_type_through_constant_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Duration int64

                const (
                    Nanosecond Duration = 1
                    Microsecond          = 1000 * Nanosecond
                    Millisecond          = 1000 * Microsecond
                    Second               = 1000 * Millisecond
                )
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(
            env.get_var("Microsecond"),
            Some(GoType::Named("Duration".to_string()))
        );
        assert_eq!(
            env.get_var("Millisecond"),
            Some(GoType::Named("Duration".to_string()))
        );
        assert_eq!(
            env.get_var("Second"),
            Some(GoType::Named("Duration".to_string()))
        );
    }

    #[test]
    fn scan_file_records_package_anonymous_struct_var_fields_before_use() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type User struct{}
                type Once struct{}

                func Current() error {
                    return cache.err
                }

                var cache struct {
                    Once
                    u *User
                    err error
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let cache_type = anonymous_top_level_struct_type_name("cache");
        assert_eq!(
            env.get_var("cache"),
            Some(GoType::Named(cache_type.clone()))
        );
        assert_eq!(
            env.get_field_type(&cache_type, "u"),
            GoType::Pointer(Box::new(GoType::Named("User".to_string())))
        );
        assert_eq!(env.get_field_type(&cache_type, "err"), GoType::Error);
        assert_eq!(
            env.get_field_type(&cache_type, "Once"),
            GoType::Named("Once".to_string())
        );
        assert!(env.is_struct_embedded_field(&cache_type, "Once"));
    }

    #[test]
    fn scan_files_preserves_const_conversion_type_declared_in_later_file() {
        let const_file = parse_file(
            "zerrors.go",
            r#"
                package p

                const ENOENT = Errno(2)
            "#,
        )
        .unwrap();
        let type_file = parse_file(
            "syscall.go",
            r#"
                package p

                type Errno uintptr
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_files(&[&const_file, &type_file]);

        assert_eq!(
            env.get_var("ENOENT"),
            Some(GoType::Named("Errno".to_string()))
        );
    }

    #[test]
    fn infer_binary_expr_uses_typed_operand_for_untyped_constants() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                const absoluteYears = 292277022400

                func f(year int64) {
                    century := uint64(year) / 100
                    centurydays := 146097 * century / 4
                    _ = centurydays + absoluteYears
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let ast::Decl::FuncDecl(func) = file.decls.get(1).expect("expected declaration") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(century) = body.list.first().expect("expected statement") else {
            panic!("expected century assignment");
        };
        let ast::Stmt::AssignStmt(centurydays) = body.list.get(1).expect("expected statement")
        else {
            panic!("expected centurydays assignment");
        };
        env.set_var(
            "century",
            GoType::infer_expr(century.rhs.first().expect("expected rhs"), &env),
        );

        assert_eq!(env.get_var("century"), Some(GoType::Uint64));
        assert_eq!(
            GoType::infer_expr(centurydays.rhs.first().expect("expected rhs"), &env),
            GoType::Uint64
        );
    }

    #[test]
    fn infer_ordered_builtin_result_uses_the_first_typed_operand() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Count uint64

                func floor(value Count) Count {
                    return max(1, value)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        env.set_var("value", GoType::Named("Count".to_string()));

        let ast::Decl::FuncDecl(func) = file.decls.get(1).expect("expected function") else {
            panic!("expected function declaration");
        };
        let ast::Stmt::ReturnStmt(return_stmt) = func
            .body
            .as_ref()
            .and_then(|body| body.list.first())
            .expect("expected return")
        else {
            panic!("expected return statement");
        };

        assert_eq!(
            GoType::infer_expr(return_stmt.results.first().expect("expected result"), &env),
            GoType::Named("Count".to_string())
        );
    }

    #[test]
    fn infer_slice_expr_preserves_defined_slice_identity_but_resolves_true_aliases() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type Buffer []byte
                type Alias = Buffer

                func named(b Buffer) Buffer { return b[1:] }
                func aliased(b Alias) Alias { return b[1:] }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let slice_result = |decl_index: usize, binding: &str, binding_ty: GoType| {
            let ast::Decl::FuncDecl(func) = file.decls.get(decl_index).unwrap() else {
                panic!("expected function declaration");
            };
            let ast::Stmt::ReturnStmt(return_stmt) = func
                .body
                .as_ref()
                .and_then(|body| body.list.first())
                .expect("expected return statement")
            else {
                panic!("expected return statement");
            };
            let mut scoped = env.clone();
            scoped.set_var(binding, binding_ty);
            GoType::infer_expr(return_stmt.results.first().unwrap(), &scoped)
        };

        assert_eq!(
            slice_result(2, "b", GoType::Named("Buffer".to_string())),
            GoType::Named("Buffer".to_string())
        );
        assert_eq!(
            slice_result(3, "b", GoType::Named("Alias".to_string())),
            GoType::Named("Buffer".to_string())
        );
    }

    #[test]
    fn infer_function_value_field_call_uses_named_func_result() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type hashFunc func(uintptr) uintptr

                type table struct {
                    hash hashFunc
                }

                func f(t *table, key uintptr) uintptr {
                    hash := t.hash(key)
                    return hash & 15
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        env.set_var(
            "t",
            GoType::Pointer(Box::new(GoType::Named("table".to_string()))),
        );
        env.set_var("key", GoType::Uintptr);

        let ast::Decl::FuncDecl(func) = file.decls.get(2).expect("expected function") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(assign) = body.list.first().expect("expected assignment") else {
            panic!("expected assignment");
        };
        let hash_ty = GoType::infer_expr(assign.rhs.first().expect("expected rhs"), &env);

        assert_eq!(hash_ty, GoType::Uintptr);
        env.set_var("hash", hash_ty);

        let ast::Stmt::ReturnStmt(ret) = body.list.get(1).expect("expected return") else {
            panic!("expected return");
        };
        assert_eq!(
            GoType::infer_expr(ret.results.first().expect("expected result"), &env),
            GoType::Uintptr
        );
    }

    #[test]
    fn infer_method_expression_function_type_includes_receiver_argument() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type T struct{}
                func (t T) M(x int) string { return "" }

                func main() {
                    _ = T.M
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        let ast::Decl::FuncDecl(func) = file.decls.get(2).expect("expected function") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(assign) = body.list.first().expect("expected assignment") else {
            panic!("expected assignment");
        };

        assert_eq!(
            GoType::infer_expr(assign.rhs.first().expect("expected rhs"), &env),
            GoType::Func {
                params: vec![GoType::Named("T".to_string()), GoType::Int],
                results: vec![GoType::String],
                variadic_start: None,
            }
        );
    }

    #[test]
    fn infer_method_value_prefers_declared_value_over_type_name() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type T interface {
                    M(int) int
                }

                func use(v T) {
                    T := v
                    _ = T.M
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        env.set_var("T", GoType::Interface("T".to_string()));

        let ast::Decl::FuncDecl(func) = file.decls.get(1).expect("expected function") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(assign) = body.list.get(1).expect("expected assignment") else {
            panic!("expected assignment");
        };

        assert_eq!(
            GoType::infer_expr(assign.rhs.first().expect("expected rhs"), &env),
            GoType::Func {
                params: vec![GoType::Int],
                results: vec![GoType::Int],
                variadic_start: None,
            }
        );
    }

    #[test]
    fn infer_slice_of_pointer_to_array_as_slice() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                func f(buf *[32]byte) {
                    _ = buf[:]
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.set_var(
            "buf",
            GoType::Pointer(Box::new(GoType::Array(Box::new(GoType::Uint8)))),
        );

        let ast::Decl::FuncDecl(func) = file.decls.first().expect("expected declaration") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(assign) = body.list.first().expect("expected statement") else {
            panic!("expected assignment");
        };

        assert_eq!(
            GoType::infer_expr(assign.rhs.first().expect("expected rhs"), &env),
            GoType::Slice(Box::new(GoType::Uint8))
        );
    }

    #[test]
    fn infer_selector_type_conversion_as_named_type() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                func f(mode int64) {
                    _ = fs.FileMode(mode)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.set_type_kind("fs.FileMode", TypeKind::Alias(GoType::Uint32));
        env.set_var("mode", GoType::Int64);

        let ast::Decl::FuncDecl(func) = file.decls.first().expect("expected declaration") else {
            panic!("expected function declaration");
        };
        let body = func.body.as_ref().unwrap();
        let ast::Stmt::AssignStmt(assign) = body.list.first().expect("expected statement") else {
            panic!("expected assignment");
        };

        assert_eq!(
            GoType::infer_expr(assign.rhs.first().expect("expected rhs"), &env),
            GoType::Named("fs.FileMode".to_string())
        );
    }

    #[test]
    fn resolves_instantiated_interface_constraint_to_structural_shape() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                type SliceOf[E any] interface {
                    ~[]E
                }

                type OrderedNumber interface {
                    ~int | ~float64
                }

                func Index[S SliceOf[E], E comparable](values S) {}
                func Larger[T OrderedNumber](value T) {}
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);

        assert_eq!(
            env.resolve_alias_or_type_param_constraint(&GoType::Named("S".to_string())),
            GoType::Slice(Box::new(GoType::Unknown))
        );
        assert_eq!(
            env.resolve_alias_or_type_param_constraint(&GoType::Named("T".to_string())),
            GoType::Named("OrderedNumber".to_string())
        );
    }

    #[test]
    fn scan_file_infers_untyped_iota_consts_as_int() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                const (
                    UpperCase = iota
                    LowerCase
                    TitleCase
                    MaxCase
                )

                type d [MaxCase]rune
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert_eq!(env.get_var("UpperCase"), Some(GoType::Int));
        assert_eq!(env.get_var("MaxCase"), Some(GoType::Int));
    }

    #[test]
    fn scan_file_preserves_wide_integer_constants_across_declarations() {
        let file = parse_file(
            "test.go",
            r#"
                package p

                const high = 1 << 255
                const folded = ((high >> 200) | 0xffff) &^ (1 << 55)
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();

        env.scan_file(&file);

        assert!(
            env.get_const_integer_exact_value("high")
                .is_some_and(|value| value.to_i128().is_none())
        );
        assert_eq!(
            env.get_const_integer_exact_value("folded")
                .and_then(|value| value.to_i128()),
            Some(0xffff)
        );
    }
}
