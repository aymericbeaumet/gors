use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::token;

#[derive(Debug, Clone, PartialEq)]
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
    Interface(std::string::String),
    Any,
    Error,
    Unit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
                name => env
                    .get_var(name)
                    .or_else(|| env.get_top_level_var(name))
                    .unwrap_or_else(|| {
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
                    }),
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
                                .and_then(|a| a.first())
                                .map(|e| GoType::infer_expr(e, env))
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
                        if let ast::Expr::Ident(pkg) = &*sel.x {
                            let key = format!("{}.{}", pkg.name, sel.sel.name);
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
                let base_type = GoType::infer_expr(&sel.x, env);
                if !matches!(base_type, GoType::Unknown) {
                    let field_type =
                        field_type_from_receiver_type(base_type.clone(), sel.sel.name, env);
                    if !matches!(field_type, GoType::Unknown) {
                        return field_type;
                    }
                    let method_type =
                        method_func_from_receiver_type(base_type.clone(), sel.sel.name, env);
                    if !matches!(method_type, GoType::Unknown) {
                        return method_type;
                    }
                }
                let base_is_value = matches!(
                    sel.x.as_ref(),
                    ast::Expr::Ident(id)
                        if env.get_var(id.name).is_some()
                            || env.get_top_level_var(id.name).is_some()
                );
                if base_is_value {
                    return GoType::Unknown;
                }
                if let ast::Expr::Ident(id) = &*sel.x {
                    let package_key = format!("{}.{}", id.name, sel.sel.name);
                    if let Some(ty) = env
                        .get_var(&package_key)
                        .or_else(|| env.get_top_level_var(&package_key))
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
                if let Some(func) = type_method_expression_func(sel, env) {
                    return func;
                }
                GoType::Unknown
            }
            ast::Expr::TypeAssertExpr(ta) => {
                if let Some(type_expr) = &ta.type_ {
                    GoType::from_expr(type_expr)
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
                let base = GoType::infer_expr(&slice.x, env);
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

fn expr_is_untyped_constant_for_inference(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
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
        env.get_var(name).or_else(|| env.get_top_level_var(name)),
        Some(GoType::Named(_))
    )
}

fn const_integer_value_i128(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<i128> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            parse_integer_literal_i128(lit.value)
        }
        ast::Expr::BasicLit(lit)
            if lit.kind == token::Token::FLOAT && decimal_float_literal_is_integer(lit.value) =>
        {
            parse_decimal_float_integer_i128(lit.value)
        }
        ast::Expr::Ident(ident) => env.get_const_integer_value(ident.name),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            const_integer_value_i128(&unary.x, env)
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::SUB => {
            const_integer_value_i128(&unary.x, env).and_then(i128::checked_neg)
        }
        _ => None,
    }
}

fn parse_integer_literal_i128(value: &str) -> Option<i128> {
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
    i128::from_str_radix(if digits.is_empty() { "0" } else { digits }, radix).ok()
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
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return None;
            };
            Some(format!("{}.{}", pkg.name, selector.sel.name))
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

fn field_type_from_receiver_type(receiver_type: GoType, field: &str, env: &TypeEnv) -> GoType {
    match env.resolve_alias(&receiver_type) {
        GoType::Named(name) => {
            let direct = env.get_field_type(&name, field);
            if !matches!(direct, GoType::Unknown) {
                return direct;
            }
            promoted_field_type_from_struct(&name, field, env, &mut HashSet::new())
        }
        GoType::Pointer(inner) => field_type_from_receiver_type(*inner, field, env),
        _ => GoType::Unknown,
    }
}

fn promoted_field_type_from_struct(
    struct_name: &str,
    field: &str,
    env: &TypeEnv,
    visiting: &mut HashSet<std::string::String>,
) -> GoType {
    if !visiting.insert(struct_name.to_string()) {
        return GoType::Unknown;
    }
    for (embedded_field, embedded_ty) in env.get_struct_fields(struct_name) {
        if !env.is_struct_embedded_field(struct_name, &embedded_field) {
            continue;
        }
        let target_name = match env.resolve_alias(&embedded_ty) {
            GoType::Named(name) => Some(name),
            GoType::Pointer(inner) => match env.resolve_alias(&inner) {
                GoType::Named(name) => Some(name),
                _ => None,
            },
            _ => None,
        };
        let Some(target_name) = target_name else {
            continue;
        };
        let direct = env.get_field_type(&target_name, field);
        if !matches!(direct, GoType::Unknown) {
            return direct;
        }
        let promoted = promoted_field_type_from_struct(&target_name, field, env, visiting);
        if !matches!(promoted, GoType::Unknown) {
            return promoted;
        }
    }
    GoType::Unknown
}

fn method_return_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    match receiver_type {
        GoType::Named(name) | GoType::Interface(name) => {
            let direct = env.get_method_return(&name, method);
            if !matches!(direct, GoType::Unknown) {
                return direct;
            }
            match env.resolve_alias(&GoType::Named(name)) {
                GoType::Named(alias_name) | GoType::Interface(alias_name) => {
                    env.get_method_return(&alias_name, method)
                }
                _ => GoType::Unknown,
            }
        }
        GoType::Pointer(inner) => method_return_from_receiver_type(*inner, method, env),
        other => match env.resolve_alias(&other) {
            GoType::Named(name) | GoType::Interface(name) => env.get_method_return(&name, method),
            GoType::Pointer(inner) => method_return_from_receiver_type(*inner, method, env),
            _ => GoType::Unknown,
        },
    }
}

fn method_func_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    match receiver_type {
        GoType::Named(name) | GoType::Interface(name) => {
            if env.has_method_func(&name, method) {
                return GoType::Func {
                    params: env.get_method_params(&name, method),
                    results: env.get_method_returns(&name, method),
                    variadic_start: env.get_method_variadic_start(&name, method),
                };
            }
            match env.resolve_alias(&GoType::Named(name)) {
                GoType::Named(alias_name) | GoType::Interface(alias_name)
                    if env.has_method_func(&alias_name, method) =>
                {
                    GoType::Func {
                        params: env.get_method_params(&alias_name, method),
                        results: env.get_method_returns(&alias_name, method),
                        variadic_start: env.get_method_variadic_start(&alias_name, method),
                    }
                }
                _ => GoType::Unknown,
            }
        }
        GoType::Pointer(inner) => method_func_from_receiver_type(*inner, method, env),
        other => match env.resolve_alias(&other) {
            GoType::Named(name) | GoType::Interface(name) => {
                if env.has_method_func(&name, method) {
                    GoType::Func {
                        params: env.get_method_params(&name, method),
                        results: env.get_method_returns(&name, method),
                        variadic_start: env.get_method_variadic_start(&name, method),
                    }
                } else {
                    GoType::Unknown
                }
            }
            GoType::Pointer(inner) => method_func_from_receiver_type(*inner, method, env),
            _ => GoType::Unknown,
        },
    }
}

fn type_method_receiver_method_name(receiver_type: &GoType, env: &TypeEnv) -> Option<String> {
    match env.resolve_alias(receiver_type) {
        GoType::Named(name) => Some(name),
        GoType::Pointer(inner) => type_method_receiver_method_name(&inner, env),
        _ => None,
    }
}

fn type_method_expression_receiver_type(
    expr: &ast::Expr,
    method: &str,
    env: &TypeEnv,
) -> Option<GoType> {
    let receiver = match unparen_expr(expr) {
        ast::Expr::Ident(ident) => env
            .get_type_kind(ident.name)
            .is_some()
            .then_some(GoType::Named(ident.name.to_string()))?,
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return None;
            };
            let name = format!("{}.{}", pkg.name, selector.sel.name);
            env.get_type_kind(&name)
                .is_some()
                .then_some(GoType::Named(name))?
        }
        ast::Expr::StarExpr(star) => {
            let inner = type_method_expression_receiver_type(&star.x, method, env)?;
            GoType::Pointer(Box::new(inner))
        }
        ast::Expr::IndexExpr(index) => type_method_expression_receiver_type(&index.x, method, env)?,
        ast::Expr::IndexListExpr(index) => {
            type_method_expression_receiver_type(&index.x, method, env)?
        }
        _ => return None,
    };
    let receiver_name = type_method_receiver_method_name(&receiver, env)?;
    env.has_func(&format!("{receiver_name}.{method}"))
        .then_some(receiver)
}

fn type_method_expression_func(sel: &ast::SelectorExpr, env: &TypeEnv) -> Option<GoType> {
    let receiver = type_method_expression_receiver_type(&sel.x, sel.sel.name, env)?;
    let receiver_name = type_method_receiver_method_name(&receiver, env)?;
    let method_key = format!("{}.{}", receiver_name, sel.sel.name);
    let mut params = vec![receiver];
    params.extend(env.get_func_params(&method_key));
    Some(GoType::Func {
        params,
        results: env.get_func_returns(&method_key),
        variadic_start: env
            .get_func_variadic_start(&method_key)
            .map(|start| start + 1),
    })
}

/// Type environment for tracking Go types during compilation.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Variable name → Go type (current scope)
    vars: HashMap<std::string::String, GoType>,
    /// Function/method name → return types
    funcs: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → parameter types
    func_params: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → interface parameter indices that need owned values.
    owned_interface_params: HashMap<std::string::String, HashSet<usize>>,
    /// Method names declared with pointer receivers.
    pointer_receiver_methods: HashSet<std::string::String>,
    /// Function/method name → type parameter name → accepted constraint terms.
    func_type_param_constraints:
        HashMap<std::string::String, HashMap<std::string::String, Vec<GoType>>>,
    /// Type parameter constraints currently in lexical scope while validating/lowering a body.
    scoped_type_param_constraints: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → index where a variadic parameter starts
    func_variadic_start: HashMap<std::string::String, usize>,
    /// Function/method name → named interfaces asserted in the function body.
    func_interface_assertions: HashMap<std::string::String, Vec<std::string::String>>,
    /// Type name → kind (struct, interface, alias)
    type_kinds: HashMap<std::string::String, TypeKind>,
    /// Type name → declared type parameter count
    type_param_counts: HashMap<std::string::String, usize>,
    /// Type name → declared type parameter names in source order.
    type_param_names: HashMap<std::string::String, Vec<std::string::String>>,
    /// Type names declared with alias syntax.
    type_aliases: HashSet<std::string::String>,
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
    consts: HashSet<std::string::String>,
    const_types: HashMap<std::string::String, GoType>,
    const_integer_values: HashMap<std::string::String, i128>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Interface,
    TypeParam,
    Alias(GoType),
}

type InterfaceMethodSignature = (std::string::String, Vec<GoType>, Vec<GoType>, Option<usize>);

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

fn interface_method_signatures(expr: &ast::Expr) -> Vec<InterfaceMethodSignature> {
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
                    Some(names.iter().map(move |name| {
                        (
                            name.name.to_string(),
                            params.clone(),
                            results.clone(),
                            variadic_start,
                        )
                    }))
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default()
}

fn interface_assertion_names_in_block(block: &ast::BlockStmt<'_>) -> Vec<std::string::String> {
    let mut names = Vec::new();
    collect_interface_assertion_names_from_block(block, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_interface_assertion_names_from_block(
    block: &ast::BlockStmt<'_>,
    out: &mut Vec<std::string::String>,
) {
    for stmt in &block.list {
        collect_interface_assertion_names_from_stmt(stmt, out);
    }
}

fn collect_interface_assertion_names_from_stmt(
    stmt: &ast::Stmt<'_>,
    out: &mut Vec<std::string::String>,
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
                collect_interface_assertion_names_from_stmt(stmt, out);
            }
        }
    }
}

fn collect_interface_assertion_names_from_expr(
    expr: &ast::Expr<'_>,
    out: &mut Vec<std::string::String>,
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
                if let Some(name) = named_assertion_type(type_) {
                    out.push(name);
                }
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
    let assigned = assigned_ident_names_in_block(body);
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

fn qualify_package_type(package_name: &str, ty: &GoType, package_env: &TypeEnv) -> GoType {
    match ty {
        GoType::Named(name) if !name.contains('.') && package_env.get_type_kind(name).is_some() => {
            GoType::Named(format!("{package_name}.{name}"))
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
            .type_kinds
            .iter()
            .filter_map(|(type_name, kind)| {
                matches!(kind, TypeKind::Struct)
                    .then_some(type_name)
                    .filter(|type_name| {
                        self.named_type_implements_interface(type_name, name, false)
                    })
                    .cloned()
            })
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
            .type_kinds
            .iter()
            .filter_map(|(type_name, kind)| {
                matches!(kind, TypeKind::Struct)
                    .then_some(type_name)
                    .filter(|type_name| self.named_type_implements_interface(type_name, name, true))
                    .cloned()
            })
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
            .is_none_or(|methods| {
                methods.iter().all(|method| {
                    self.named_type_has_method(
                        type_name,
                        method,
                        include_pointer_receiver_methods,
                        &mut HashSet::new(),
                    )
                })
            })
    }

    fn named_type_has_method(
        &self,
        type_name: &str,
        method: &str,
        include_pointer_receiver_methods: bool,
        visiting: &mut HashSet<std::string::String>,
    ) -> bool {
        let method_key = format!("{type_name}.{method}");
        if if include_pointer_receiver_methods {
            self.has_func(&method_key)
        } else {
            self.has_value_method(&method_key)
        } {
            return true;
        }
        if !visiting.insert(type_name.to_string()) {
            return false;
        }
        let promoted = self
            .get_struct_fields(type_name)
            .iter()
            .any(|(_, field_ty)| {
                self.embedded_type_has_method(
                    field_ty,
                    method,
                    include_pointer_receiver_methods,
                    visiting,
                )
            });
        visiting.remove(type_name);
        promoted
    }

    fn embedded_type_has_method(
        &self,
        field_ty: &GoType,
        method: &str,
        include_pointer_receiver_methods: bool,
        visiting: &mut HashSet<std::string::String>,
    ) -> bool {
        match self.resolve_alias(field_ty) {
            GoType::Named(name) if self.is_interface(&name) => self
                .get_interface_methods(&name)
                .is_some_and(|methods| methods.iter().any(|candidate| candidate == method)),
            GoType::Named(name) => self.named_type_has_method(
                &name,
                method,
                include_pointer_receiver_methods,
                visiting,
            ),
            GoType::Pointer(inner) => match *inner {
                GoType::Named(name) => self.named_type_has_method(&name, method, true, visiting),
                _ => false,
            },
            _ => false,
        }
    }

    pub fn resolve_alias(&self, ty: &GoType) -> GoType {
        match ty {
            GoType::Named(name) => match self.type_kinds.get(name) {
                Some(TypeKind::Alias(inner)) => self.resolve_alias(inner),
                _ => ty.clone(),
            },
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
            _ => ty.clone(),
        }
    }

    pub fn resolve_type_param_constraint(&self, ty: &GoType) -> Option<GoType> {
        let GoType::Named(name) = ty else {
            return None;
        };
        self.get_type_param_constraints(name)
            .and_then(|terms| terms.first().cloned())
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
            GoType::Named(name) => self.get_field_array_len(&name, field_name),
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
    }

    pub fn get_const_integer_value(&self, name: &str) -> Option<i128> {
        self.const_integer_values.get(name).copied()
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
        for (name, assertions) in &package_env.func_interface_assertions {
            self.set_func_interface_assertions(
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
        for name in &package_env.string_consts {
            self.set_string_const(&qualify_package_member_name(
                package_name,
                name,
                package_env,
            ));
        }
    }

    /// Pre-scan a Go AST file to populate type declarations and function signatures.
    pub fn scan_file(&mut self, file: &ast::File) {
        for decl in &file.decls {
            match decl {
                ast::Decl::GenDecl(gd) => {
                    for spec in &gd.specs {
                        if let ast::Spec::TypeSpec(ts) = spec {
                            self.scan_type_spec(ts);
                        }
                    }
                }
                ast::Decl::FuncDecl(fd) => {
                    self.scan_func_decl(fd);
                }
            }
        }
        for decl in &file.decls {
            let ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            let mut inherited_const_type = None;
            for spec in &gd.specs {
                let ast::Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                self.scan_value_spec(vs, gd.tok, inherited_const_type.as_ref());
                if gd.tok == token::Token::CONST {
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
    }

    pub fn rescan_file_top_level_vars(&mut self, file: &ast::File, inference_env: &TypeEnv) {
        for decl in &file.decls {
            let ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            if gd.tok == token::Token::CONST {
                continue;
            }
            for spec in &gd.specs {
                let ast::Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                let explicit_type = vs.type_.as_ref().map(GoType::from_expr);
                let values = vs.values.as_ref();
                for (i, name) in vs.names.iter().enumerate() {
                    let ty = if let Some(ref explicit_type) = explicit_type {
                        explicit_type.clone()
                    } else {
                        values
                            .and_then(|values| values.get(i))
                            .map(|expr| GoType::infer_expr(expr, inference_env))
                            .unwrap_or(GoType::Unknown)
                    };
                    if !matches!(ty, GoType::Unknown) {
                        self.set_var(name.name, ty.clone());
                        self.set_top_level_var(name.name, ty);
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
                self.set_type_kind(name.name, TypeKind::Struct);
                if let Some(ref field_list) = st.fields {
                    let mut fields = vec![];
                    let mut embedded_fields = HashSet::new();
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
                                    self.set_struct_field_array_len(
                                        name.name,
                                        field_name.name,
                                        len,
                                    );
                                }
                            }
                        } else if let Some(type_expr) = &field.type_
                            && let Some(field_name) = embedded_field_name(type_expr)
                        {
                            embedded_fields.insert(field_name.clone());
                            fields.push((field_name, ty.clone()));
                        }
                    }
                    self.set_struct_fields(name.name, fields);
                    self.set_struct_embedded_fields(name.name, embedded_fields);
                }
            }
            ast::Expr::InterfaceType(_) => {
                self.set_type_kind(name.name, TypeKind::Interface);
                self.set_interface_methods(name.name, interface_method_names(&ts.type_));
                self.set_interface_embedded(name.name, interface_embedded_names(&ts.type_));
                self.set_interface_type_terms(name.name, interface_constraint_terms(&ts.type_));
                for (method_name, params, returns, variadic_start) in
                    interface_method_signatures(&ts.type_)
                {
                    let method_key = format!("{}.{}", name.name, method_name);
                    self.set_func_params(&method_key, params);
                    self.set_func(&method_key, returns);
                    if let Some(start) = variadic_start {
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
    ) {
        let explicit_type = vs.type_.as_ref().map(GoType::from_expr).or_else(|| {
            (tok == token::Token::CONST && vs.values.is_none())
                .then(|| inherited_const_type.cloned())
                .flatten()
        });
        let values = vs.values.as_ref();

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
                    && let Some(value) = const_integer_value_i128(value_expr, self)
                {
                    self.set_const_integer_value(name.name, value);
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
                        self.set_func_interface_assertions(
                            &method_key,
                            interface_assertion_names_in_block(body),
                        );
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
            self.set_func_type_param_constraints(name, type_param_constraints);
            if let Some(start) = variadic_start {
                self.set_func_variadic_start(name, start);
            }
            if let Some(body) = &fd.body {
                self.set_func_interface_assertions(name, interface_assertion_names_in_block(body));
                let owned =
                    owned_interface_param_indices(&params, &fd.type_.params, body, |name| {
                        self.is_interface(name)
                    });
                self.set_owned_interface_params(name, owned);
            }
        }

        // Register parameter types
        for param in &fd.type_.params.list {
            let ty = param
                .type_
                .as_ref()
                .map(GoType::from_expr)
                .unwrap_or(GoType::Unknown);
            if let Some(ref names) = param.names {
                for n in names {
                    self.set_var(n.name, ty.clone());
                }
            }
        }

        // Register named return value types
        if let Some(ref results) = fd.type_.results {
            for field in &results.list {
                let ty = field
                    .type_
                    .as_ref()
                    .map(GoType::from_expr)
                    .unwrap_or(GoType::Unknown);
                if let Some(ref names) = field.names {
                    for n in names {
                        self.set_var(n.name, ty.clone());
                    }
                }
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

        let ret = file
            .decls
            .iter()
            .find_map(|decl| match decl {
                ast::Decl::FuncDecl(func) if func.name.name == "Use" => func.body.as_ref(),
                _ => None,
            })
            .and_then(|body| body.list.first())
            .and_then(|stmt| match stmt {
                ast::Stmt::ReturnStmt(ret) => ret.results.first(),
                _ => None,
            })
            .expect("return expression");

        assert_eq!(GoType::infer_expr(ret, &env), GoType::Any);
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
}
