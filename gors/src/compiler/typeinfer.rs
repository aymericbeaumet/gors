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
                name => env.get_var(name).unwrap_or_else(|| {
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
                        if let ast::Expr::Ident(pkg) = &*sel.x {
                            let key = format!("{}.{}", pkg.name, sel.sel.name);
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
                match env.resolve_alias_outer(&container) {
                    GoType::String => GoType::Uint8,
                    GoType::Slice(elem) | GoType::Array(elem) => *elem,
                    GoType::Map(_, value) => *value,
                    _ => GoType::Unknown,
                }
            }
            ast::Expr::SelectorExpr(sel) => {
                if let Some(func) = type_method_expression_func(sel, env) {
                    return func;
                }
                if let ast::Expr::Ident(id) = &*sel.x {
                    let package_key = format!("{}.{}", id.name, sel.sel.name);
                    if let Some(ty) = env.get_var(&package_key) {
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
                let base_type = GoType::infer_expr(&sel.x, env);
                let field_type =
                    field_type_from_receiver_type(base_type.clone(), sel.sel.name, env);
                if !matches!(field_type, GoType::Unknown) {
                    return field_type;
                }
                method_func_from_receiver_type(base_type, sel.sel.name, env)
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

fn new_arg_is_type(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) => {
            !matches!(ident.name, "true" | "false" | "nil")
                && env.get_var(ident.name).is_none()
                && !env.has_func(ident.name)
                && !env.is_const(ident.name)
                && (predeclared_type_name(ident.name) || env.get_type_kind(ident.name).is_some())
        }
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return false;
            };
            if pkg.name == "unsafe" && selector.sel.name == "Pointer" {
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

fn predeclared_type_name(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "complex64"
            | "complex128"
            | "error"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "rune"
            | "string"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
    )
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
        GoType::Named(name) => env.get_field_type(&name, field),
        GoType::Pointer(inner) => field_type_from_receiver_type(*inner, field, env),
        _ => GoType::Unknown,
    }
}

fn method_return_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    match env.resolve_alias(&receiver_type) {
        GoType::Named(name) => env.get_func_return(&format!("{name}.{method}")),
        GoType::Pointer(inner) => method_return_from_receiver_type(*inner, method, env),
        _ => GoType::Unknown,
    }
}

fn method_func_from_receiver_type(receiver_type: GoType, method: &str, env: &TypeEnv) -> GoType {
    match env.resolve_alias(&receiver_type) {
        GoType::Named(name) => {
            let key = format!("{name}.{method}");
            if env.has_func(&key) {
                GoType::Func {
                    params: env.get_func_params(&key),
                    results: env.get_func_returns(&key),
                    variadic_start: env.get_func_variadic_start(&key),
                }
            } else {
                GoType::Unknown
            }
        }
        GoType::Pointer(inner) => method_func_from_receiver_type(*inner, method, env),
        _ => GoType::Unknown,
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
    /// Method names declared with pointer receivers.
    pointer_receiver_methods: HashSet<std::string::String>,
    /// Function/method name → type parameter name → accepted constraint terms.
    func_type_param_constraints:
        HashMap<std::string::String, HashMap<std::string::String, Vec<GoType>>>,
    /// Function/method name → index where a variadic parameter starts
    func_variadic_start: HashMap<std::string::String, usize>,
    /// Type name → kind (struct, interface, alias)
    type_kinds: HashMap<std::string::String, TypeKind>,
    /// Type name → declared type parameter count
    type_param_counts: HashMap<std::string::String, usize>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Interface,
    Alias(GoType),
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

fn type_parameter_count(type_params: Option<&ast::FieldList<'_>>) -> usize {
    type_params
        .map(|fields| {
            fields
                .list
                .iter()
                .map(|field| field.names.as_ref().map_or(1, Vec::len))
                .sum()
        })
        .unwrap_or(0)
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

    pub fn set_func(&mut self, name: &str, returns: Vec<GoType>) {
        self.funcs.insert(name.to_string(), returns);
    }

    pub fn set_func_params(&mut self, name: &str, params: Vec<GoType>) {
        self.func_params.insert(name.to_string(), params);
    }

    pub fn set_func_variadic_start(&mut self, name: &str, start: usize) {
        self.func_variadic_start.insert(name.to_string(), start);
    }

    pub fn get_func_variadic_start(&self, name: &str) -> Option<usize> {
        self.func_variadic_start.get(name).copied()
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

    pub fn set_type_kind(&mut self, name: &str, kind: TypeKind) {
        self.type_kinds.insert(name.to_string(), kind);
    }

    pub fn get_type_kind(&self, name: &str) -> Option<&TypeKind> {
        self.type_kinds.get(name)
    }

    pub fn set_type_param_count(&mut self, name: &str, count: usize) {
        self.type_param_counts.insert(name.to_string(), count);
    }

    pub fn get_type_param_count(&self, name: &str) -> Option<usize> {
        self.type_param_counts.get(name).copied()
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
        self.func_type_param_constraints
            .values()
            .find_map(|constraints| {
                constraints
                    .get(name)
                    .and_then(|terms| terms.first().cloned())
            })
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
            self.set_func(
                &format!("{package_name}.{name}"),
                qualify_package_types(package_name, returns, package_env),
            );
        }
        for (name, params) in &package_env.func_params {
            self.set_func_params(
                &format!("{package_name}.{name}"),
                qualify_package_types(package_name, params, package_env),
            );
        }
        for name in &package_env.pointer_receiver_methods {
            self.set_pointer_receiver_method(&format!("{package_name}.{name}"));
        }
        for (name, constraints) in &package_env.func_type_param_constraints {
            self.set_func_type_param_constraints(
                &format!("{package_name}.{name}"),
                qualify_package_constraint_map(package_name, constraints, package_env),
            );
        }
        for (name, start) in &package_env.func_variadic_start {
            self.set_func_variadic_start(&format!("{package_name}.{name}"), *start);
        }
        for (name, kind) in &package_env.type_kinds {
            self.set_type_kind(
                &format!("{package_name}.{name}"),
                qualify_package_type_kind(package_name, kind, package_env),
            );
        }
        for (name, methods) in &package_env.interface_methods {
            self.set_interface_methods(&format!("{package_name}.{name}"), methods.clone());
        }
        for (name, embedded) in &package_env.interface_embedded {
            let qualified = embedded
                .iter()
                .map(|embedded_name| {
                    qualify_package_interface_name(package_name, embedded_name, package_env)
                })
                .collect();
            self.set_interface_embedded(&format!("{package_name}.{name}"), qualified);
        }
        for (name, terms) in &package_env.interface_type_terms {
            self.set_interface_type_terms(
                &format!("{package_name}.{name}"),
                terms
                    .iter()
                    .map(|term| qualify_package_type(package_name, term, package_env))
                    .collect(),
            );
        }
        for (name, fields) in &package_env.struct_fields {
            let qualified_fields = fields
                .iter()
                .map(|(field_name, ty)| {
                    (
                        field_name.clone(),
                        qualify_package_type(package_name, ty, package_env),
                    )
                })
                .collect();
            self.set_struct_fields(&format!("{package_name}.{name}"), qualified_fields);
        }
        for (name, fields) in &package_env.struct_embedded_fields {
            self.set_struct_embedded_fields(&format!("{package_name}.{name}"), fields.clone());
        }
        for (name, fields) in &package_env.struct_field_array_lengths {
            let struct_name = format!("{package_name}.{name}");
            for (field_name, len) in fields {
                self.set_struct_field_array_len(&struct_name, field_name, *len);
            }
        }
        for (name, ty) in &package_env.vars {
            self.set_var(
                &format!("{package_name}.{name}"),
                qualify_package_type(package_name, ty, package_env),
            );
        }
        for name in &package_env.top_level_vars {
            if let Some(ty) = package_env.top_level_var_types.get(name) {
                self.set_top_level_var(
                    &format!("{package_name}.{name}"),
                    qualify_package_type(package_name, ty, package_env),
                );
            }
        }
        for name in &package_env.consts {
            self.set_const(&format!("{package_name}.{name}"));
        }
        for (name, ty) in &package_env.const_types {
            self.set_const_type(
                &format!("{package_name}.{name}"),
                qualify_package_type(package_name, ty, package_env),
            );
        }
        for name in &package_env.string_consts {
            self.set_string_const(&format!("{package_name}.{name}"));
        }
    }

    /// Pre-scan a Go AST file to populate type declarations and function signatures.
    pub fn scan_file(&mut self, file: &ast::File) {
        for decl in &file.decls {
            match decl {
                ast::Decl::GenDecl(gd) => {
                    let mut inherited_const_type = None;
                    for spec in &gd.specs {
                        match spec {
                            ast::Spec::TypeSpec(ts) => {
                                self.scan_type_spec(ts);
                            }
                            ast::Spec::ValueSpec(vs) => {
                                self.scan_value_spec(vs, gd.tok, inherited_const_type.as_ref());
                                if gd.tok == token::Token::CONST {
                                    if let Some(type_expr) = &vs.type_ {
                                        inherited_const_type = Some(GoType::from_expr(type_expr));
                                    } else if let Some(values) = &vs.values
                                        && let Some(first) = values.first()
                                    {
                                        inherited_const_type =
                                            Some(GoType::infer_expr(first, self));
                                    }
                                }
                                for name in &vs.names {
                                    if let Some(ty) = self.get_var(name.name) {
                                        self.set_top_level_var(name.name, ty);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ast::Decl::FuncDecl(fd) => {
                    self.scan_func_decl(fd);
                }
            }
        }
    }

    fn scan_type_spec(&mut self, ts: &ast::TypeSpec) {
        let Some(ref name) = ts.name else { return };
        self.set_type_param_count(name.name, type_parameter_count(ts.type_params.as_ref()));
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
                }
            }
        }
        if !is_method {
            self.set_func_params(name, params);
            self.set_func(name, returns);
            self.set_func_type_param_constraints(name, type_param_constraints);
            if let Some(start) = variadic_start {
                self.set_func_variadic_start(name, start);
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
