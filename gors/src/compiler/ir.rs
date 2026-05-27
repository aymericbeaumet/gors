//! Typed Go IR used as the semantic layer between the parser AST and Rust codegen.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ast, token};

use super::typeinfer::{GoChannelDirection, GoType, TypeEnv, TypeKind};

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub package: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Func(Func),
    GenDecl(GenDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Func {
    pub name: Option<String>,
    pub receiver: Vec<Binding>,
    pub signature: Signature,
    pub body: Option<Block>,
    pub captures: Vec<Capture>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signature {
    pub params: Vec<Binding>,
    pub results: Vec<Binding>,
    pub variadic_start: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: Option<String>,
    pub ty: GoType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub name: String,
    pub ty: GoType,
    pub mode: CaptureMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Borrow,
    BorrowMut,
    Move,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenDecl {
    pub kind: DeclKind,
    pub specs: Vec<Spec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    Const,
    Import,
    Type,
    Var,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Spec {
    Import {
        name: Option<String>,
        path: String,
    },
    Type {
        name: String,
        ty: Expr,
        alias: bool,
    },
    Value {
        names: Vec<String>,
        ty: Option<Expr>,
        values: Vec<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Assign(Assign),
    Block(Block),
    Branch {
        kind: BranchKind,
        label: Option<String>,
    },
    Case(Case),
    Comm(CommCase),
    Decl(GenDecl),
    Defer(Call),
    Empty,
    Expr(Expr),
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Block,
    },
    Go(Call),
    If {
        init: Option<Box<Stmt>>,
        cond: Expr,
        body: Block,
        else_branch: Option<Box<Stmt>>,
    },
    IncDec {
        expr: Expr,
        op: IncDecOp,
    },
    Label {
        name: String,
        stmt: Box<Stmt>,
    },
    Range {
        key: Option<Expr>,
        value: Option<Expr>,
        define: bool,
        expr: Expr,
        body: Block,
    },
    Return(Vec<Expr>),
    Send {
        chan: Expr,
        value: Expr,
    },
    Select {
        cases: Vec<CommCase>,
    },
    Switch {
        init: Option<Box<Stmt>>,
        tag: Option<Expr>,
        cases: Vec<Case>,
    },
    TypeSwitch {
        init: Option<Box<Stmt>>,
        assign: Box<Stmt>,
        cases: Vec<Case>,
    },
    Opaque(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assign {
    pub lhs: Vec<Expr>,
    pub op: AssignOp,
    pub rhs: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Define,
    Assign,
    Add,
    Sub,
    Mul,
    Quo,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    AndNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    Break,
    Continue,
    Fallthrough,
    Goto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncDecOp {
    Inc,
    Dec,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub exprs: Vec<Expr>,
    pub body: Vec<Stmt>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommCase {
    pub comm: Option<Box<Stmt>>,
    pub body: Vec<Stmt>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: GoType,
    pub addressability: Addressability,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    ArrayType {
        len: Option<Box<Expr>>,
        elem: Box<Expr>,
    },
    BasicLit(String),
    Binary {
        op: String,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call(Call),
    ChannelType {
        elem: Box<Expr>,
        direction: ChannelDirection,
    },
    CompositeLit {
        ty: Option<Box<Expr>>,
        elems: Vec<Expr>,
    },
    Ellipsis(Option<Box<Expr>>),
    FuncLit(Box<Func>),
    FuncType(Signature),
    Ident(String),
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    IndexList {
        target: Box<Expr>,
        indices: Vec<Expr>,
    },
    InterfaceType,
    KeyValue {
        key: Box<Expr>,
        value: Box<Expr>,
    },
    MapType {
        key: Box<Expr>,
        value: Box<Expr>,
    },
    Paren(Box<Expr>),
    Selector {
        target: Box<Expr>,
        field: String,
    },
    Slice {
        target: Box<Expr>,
        low: Option<Box<Expr>>,
        high: Option<Box<Expr>>,
        max: Option<Box<Expr>>,
    },
    Star(Box<Expr>),
    StructType,
    TypeAssert {
        target: Box<Expr>,
        ty: Option<Box<Expr>>,
    },
    Unary {
        op: String,
        expr: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressability {
    Addressable,
    NotAddressable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelDirection {
    Bidirectional,
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKind {
    String,
    Integer,
    Indexed,
    Map,
    Channel,
    Function,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    MayComplete,
    Terminates,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub fun: Box<Expr>,
    pub args: Vec<Expr>,
    pub spread: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinCallKind {
    Append,
    Cap,
    Clear,
    Close,
    Complex,
    Copy,
    Delete,
    Imag,
    Len,
    Make,
    Max,
    Min,
    New,
    Panic,
    Print,
    Println,
    Real,
    Recover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialTypeConversionKind {
    Any,
    ByteSlice,
    Complex64,
    Complex128,
    RuneSlice,
    String,
}

impl SpecialTypeConversionKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::ByteSlice => "[]byte",
            Self::Complex64 => "complex64",
            Self::Complex128 => "complex128",
            Self::RuneSlice => "[]rune",
            Self::String => "string",
        }
    }
}

impl BuiltinCallKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Cap => "cap",
            Self::Clear => "clear",
            Self::Close => "close",
            Self::Complex => "complex",
            Self::Copy => "copy",
            Self::Delete => "delete",
            Self::Imag => "imag",
            Self::Len => "len",
            Self::Make => "make",
            Self::Max => "max",
            Self::Min => "min",
            Self::New => "new",
            Self::Panic => "panic",
            Self::Print => "print",
            Self::Println => "println",
            Self::Real => "real",
            Self::Recover => "recover",
        }
    }
}

pub fn lower_file(file: &ast::File<'_>, env: &TypeEnv) -> File {
    File {
        package: file.name.name.to_string(),
        items: file
            .decls
            .iter()
            .filter_map(|decl| lower_decl(decl, env))
            .collect(),
    }
}

pub fn builtin_call_kind(call_expr: &ast::CallExpr<'_>) -> Option<BuiltinCallKind> {
    let ast::Expr::Ident(ident) = call_expr.fun.as_ref() else {
        return None;
    };
    match ident.name {
        "append" => Some(BuiltinCallKind::Append),
        "cap" => Some(BuiltinCallKind::Cap),
        "clear" => Some(BuiltinCallKind::Clear),
        "close" => Some(BuiltinCallKind::Close),
        "complex" => Some(BuiltinCallKind::Complex),
        "copy" => Some(BuiltinCallKind::Copy),
        "delete" => Some(BuiltinCallKind::Delete),
        "imag" => Some(BuiltinCallKind::Imag),
        "len" => Some(BuiltinCallKind::Len),
        "make" => Some(BuiltinCallKind::Make),
        "max" => Some(BuiltinCallKind::Max),
        "min" => Some(BuiltinCallKind::Min),
        "new" => Some(BuiltinCallKind::New),
        "panic" => Some(BuiltinCallKind::Panic),
        "print" => Some(BuiltinCallKind::Print),
        "println" => Some(BuiltinCallKind::Println),
        "real" => Some(BuiltinCallKind::Real),
        "recover" => Some(BuiltinCallKind::Recover),
        _ => None,
    }
}

pub fn call_func_key(fun: &ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    match fun {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::SelectorExpr(sel) => {
            if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                if !env.get_func_params(&package_key).is_empty()
                    || env.get_func_variadic_start(&package_key).is_some()
                {
                    return Some(package_key);
                }

                if let Some(GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                    return Some(format!("{}.{}", name, sel.sel.name));
                }
            }
            None
        }
        _ => None,
    }
}

pub fn variadic_call_start(call_expr: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<usize> {
    let key = call_func_key(&call_expr.fun, env)?;
    env.get_func_variadic_start(&key)
}

pub fn range_kind(expr: &ast::Expr<'_>, env: &TypeEnv) -> RangeKind {
    match env.resolve_alias(&GoType::infer_expr(expr, env)) {
        GoType::String => RangeKind::String,
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
        | GoType::Uintptr => RangeKind::Integer,
        GoType::Slice(_) | GoType::Array(_) => RangeKind::Indexed,
        GoType::Map(_, _) => RangeKind::Map,
        GoType::Chan { direction, .. } if direction.can_receive() => RangeKind::Channel,
        GoType::Func { .. } => RangeKind::Function,
        _ => RangeKind::Other,
    }
}

pub fn special_type_conversion(call_expr: &ast::CallExpr<'_>) -> Option<SpecialTypeConversionKind> {
    let args = call_expr.args.as_ref()?;
    if args.len() != 1 {
        return None;
    }
    match &*call_expr.fun {
        ast::Expr::Ident(id) if id.name == "string" => Some(SpecialTypeConversionKind::String),
        ast::Expr::Ident(id) if id.name == "any" => Some(SpecialTypeConversionKind::Any),
        ast::Expr::Ident(id) if id.name == "complex64" => {
            Some(SpecialTypeConversionKind::Complex64)
        }
        ast::Expr::Ident(id) if id.name == "complex128" => {
            Some(SpecialTypeConversionKind::Complex128)
        }
        ast::Expr::ArrayType(arr) if arr.len.is_none() => match &*arr.elt {
            ast::Expr::Ident(elt_id) if matches!(elt_id.name, "byte" | "uint8") => {
                Some(SpecialTypeConversionKind::ByteSlice)
            }
            ast::Expr::Ident(elt_id) if matches!(elt_id.name, "rune" | "int32") => {
                Some(SpecialTypeConversionKind::RuneSlice)
            }
            _ => None,
        },
        _ => None,
    }
}

pub fn is_general_type_conversion_fun(fun: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match fun {
        ast::Expr::ParenExpr(paren) => is_general_type_conversion_fun(&paren.x, env),
        ast::Expr::StarExpr(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::ArrayType(arr) => arr.len.is_some(),
        ast::Expr::IndexExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name).cloned())
            .is_some(),
        ast::Expr::IndexListExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name).cloned())
            .is_some(),
        ast::Expr::SelectorExpr(sel) => {
            if let ast::Expr::Ident(pkg) = &*sel.x {
                if pkg.name == "unsafe" && sel.sel.name == "Pointer" {
                    return true;
                }
                let key = format!("{}.{}", pkg.name, sel.sel.name);
                return env.get_type_kind(&key).is_some();
            }
            false
        }
        ast::Expr::Ident(id) => {
            is_predeclared_type_name(id.name) || env.get_type_kind(id.name).is_some()
        }
        _ => false,
    }
}

fn type_name(expr: &ast::Expr<'_>) -> Option<String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::StarExpr(star) => type_name(&star.x),
        ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
        ast::Expr::IndexExpr(index) => type_name(&index.x),
        ast::Expr::IndexListExpr(index) => type_name(&index.x),
        _ => None,
    }
}

fn is_predeclared_type_name(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "bool"
            | "byte"
            | "rune"
            | "string"
            | "float32"
            | "float64"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "error"
    )
}

fn lower_decl(decl: &ast::Decl<'_>, env: &TypeEnv) -> Option<Item> {
    match decl {
        ast::Decl::FuncDecl(func) => Some(Item::Func(lower_func_decl(func, env))),
        ast::Decl::GenDecl(gen_decl) => Some(Item::GenDecl(lower_gen_decl(gen_decl, env))),
    }
}

fn lower_func_decl(func: &ast::FuncDecl<'_>, env: &TypeEnv) -> Func {
    let mut body_env = env.clone();
    seed_func_bindings(func.recv.as_ref(), &func.type_, &mut body_env);
    Func {
        name: Some(func.name.name.to_string()),
        receiver: func
            .recv
            .as_ref()
            .map_or_else(Vec::new, |receiver| lower_fields(receiver)),
        signature: lower_signature(&func.type_),
        body: func
            .body
            .as_ref()
            .map(|body| lower_block_with_env(body, &mut body_env)),
        captures: Vec::new(),
    }
}

fn lower_func_lit(func_lit: &ast::FuncLit<'_>, env: &TypeEnv) -> Func {
    let mut body_env = env.clone();
    seed_func_bindings(None, &func_lit.type_, &mut body_env);
    Func {
        name: None,
        receiver: Vec::new(),
        signature: lower_signature(&func_lit.type_),
        body: Some(lower_block_with_env(&func_lit.body, &mut body_env)),
        captures: func_lit_captures(func_lit, env),
    }
}

fn seed_func_bindings(
    recv: Option<&ast::FieldList<'_>>,
    sig: &ast::FuncType<'_>,
    env: &mut TypeEnv,
) {
    if let Some(recv) = recv {
        for field in &recv.list {
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
    seed_field_bindings(&sig.params, env);
    if let Some(results) = &sig.results {
        seed_field_bindings(results, env);
    }
}

fn seed_field_bindings(fields: &ast::FieldList<'_>, env: &mut TypeEnv) {
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

fn lower_signature(func_type: &ast::FuncType<'_>) -> Signature {
    let mut variadic_start = None;
    let mut param_count = 0usize;
    let params = func_type
        .params
        .list
        .iter()
        .flat_map(|field| {
            let bindings = lower_field(field);
            if matches!(field.type_, Some(ast::Expr::Ellipsis(_))) {
                variadic_start = Some(param_count);
            }
            param_count += bindings.len();
            bindings
        })
        .collect();
    let results = func_type
        .results
        .as_ref()
        .map_or_else(Vec::new, lower_fields);
    Signature {
        params,
        results,
        variadic_start,
    }
}

fn lower_fields(fields: &ast::FieldList<'_>) -> Vec<Binding> {
    fields.list.iter().flat_map(lower_field).collect()
}

fn lower_field(field: &ast::Field<'_>) -> Vec<Binding> {
    let ty = field
        .type_
        .as_ref()
        .map(GoType::from_expr)
        .unwrap_or(GoType::Unknown);
    field.names.as_ref().map_or_else(
        || {
            vec![Binding {
                name: None,
                ty: ty.clone(),
            }]
        },
        |names| {
            names
                .iter()
                .map(|name| Binding {
                    name: Some(name.name.to_string()),
                    ty: ty.clone(),
                })
                .collect()
        },
    )
}

fn lower_gen_decl(gen_decl: &ast::GenDecl<'_>, env: &TypeEnv) -> GenDecl {
    GenDecl {
        kind: lower_decl_kind(gen_decl.tok),
        specs: gen_decl
            .specs
            .iter()
            .filter_map(|spec| lower_spec(spec, env))
            .collect(),
    }
}

fn lower_decl_kind(tok: token::Token) -> DeclKind {
    match tok {
        token::Token::CONST => DeclKind::Const,
        token::Token::IMPORT => DeclKind::Import,
        token::Token::TYPE => DeclKind::Type,
        token::Token::VAR => DeclKind::Var,
        _ => DeclKind::Var,
    }
}

fn lower_spec(spec: &ast::Spec<'_>, env: &TypeEnv) -> Option<Spec> {
    match spec {
        ast::Spec::ImportSpec(import) => Some(Spec::Import {
            name: import.name.as_ref().map(|name| name.name.to_string()),
            path: import.path.value.trim_matches('"').to_string(),
        }),
        ast::Spec::TypeSpec(type_spec) => type_spec.name.as_ref().map(|name| Spec::Type {
            name: name.name.to_string(),
            ty: lower_expr(&type_spec.type_, env),
            alias: type_spec.assign.is_some(),
        }),
        ast::Spec::ValueSpec(value_spec) => Some(Spec::Value {
            names: value_spec
                .names
                .iter()
                .map(|name| name.name.to_string())
                .collect(),
            ty: value_spec.type_.as_ref().map(|ty| lower_expr(ty, env)),
            values: value_spec.values.as_ref().map_or_else(Vec::new, |values| {
                values.iter().map(|value| lower_expr(value, env)).collect()
            }),
        }),
    }
}

fn lower_block(block: &ast::BlockStmt<'_>, env: &TypeEnv) -> Block {
    let mut env = env.clone();
    lower_block_with_env(block, &mut env)
}

fn lower_block_with_env(block: &ast::BlockStmt<'_>, env: &mut TypeEnv) -> Block {
    Block {
        stmts: block
            .list
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, env))
            .collect(),
    }
}

fn record_decl_bindings(gen_decl: &ast::GenDecl<'_>, env: &mut TypeEnv) {
    for spec in &gen_decl.specs {
        let ast::Spec::ValueSpec(value_spec) = spec else {
            continue;
        };
        let explicit_type = value_spec.type_.as_ref().map(GoType::from_expr);
        for (idx, name) in value_spec.names.iter().enumerate() {
            if name.name == "_" {
                continue;
            }
            let ty = explicit_type.clone().unwrap_or_else(|| {
                value_spec
                    .values
                    .as_ref()
                    .and_then(|values| values.get(idx))
                    .map(|expr| GoType::infer_expr(expr, env))
                    .unwrap_or(GoType::Unknown)
            });
            if gen_decl.tok == token::Token::CONST {
                env.set_const_type(name.name, ty.clone());
            }
            env.set_var(name.name, ty);
        }
    }
}

fn record_define_bindings(assign: &ast::AssignStmt<'_>, env: &mut TypeEnv) {
    if assign.tok != token::Token::DEFINE {
        return;
    }
    let inferred = if assign.lhs.len() == assign.rhs.len() {
        assign
            .rhs
            .iter()
            .map(|rhs| GoType::infer_expr(rhs, env))
            .collect::<Vec<_>>()
    } else {
        vec![GoType::Unknown; assign.lhs.len()]
    };
    for (lhs, ty) in assign.lhs.iter().zip(inferred) {
        if let ast::Expr::Ident(ident) = lhs
            && ident.name != "_"
        {
            env.set_var(ident.name, ty);
        }
    }
}

fn declare_define_names(assign: &ast::AssignStmt<'_>, scopes: &mut ShortVarScopes) {
    if assign.tok != token::Token::DEFINE {
        return;
    }
    for lhs in &assign.lhs {
        if let Some(name) = short_var_decl_ident_name(lhs) {
            scopes.declare(name);
        }
    }
}

fn record_range_bindings(range: &ast::RangeStmt<'_>, env: &mut TypeEnv) {
    if range.tok != Some(token::Token::DEFINE) {
        return;
    }
    let range_type = env.resolve_alias(&GoType::infer_expr(&range.x, env));
    let (key_type, value_type) = match range_type {
        GoType::String => (GoType::Int, Some(GoType::Int32)),
        GoType::Slice(elem) | GoType::Array(elem) => (GoType::Int, Some(*elem)),
        GoType::Map(key, value) => (*key, Some(*value)),
        GoType::Chan { elem, .. } => (*elem, None),
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
        | GoType::Uintptr => (GoType::Int, None),
        _ => (GoType::Unknown, Some(GoType::Unknown)),
    };
    record_range_binding(range.key.as_ref(), key_type, env);
    if let Some(value_type) = value_type {
        record_range_binding(range.value.as_ref(), value_type, env);
    }
}

fn declare_range_names(range: &ast::RangeStmt<'_>, scopes: &mut ShortVarScopes) {
    if range.tok != Some(token::Token::DEFINE) {
        return;
    }
    if let Some(key) = &range.key
        && let Some(name) = short_var_decl_ident_name(key)
    {
        scopes.declare(name);
    }
    if let Some(value) = &range.value
        && let Some(name) = short_var_decl_ident_name(value)
    {
        scopes.declare(name);
    }
}

fn record_range_binding(target: Option<&ast::Expr<'_>>, ty: GoType, env: &mut TypeEnv) {
    if let Some(ast::Expr::Ident(ident)) = target
        && ident.name != "_"
    {
        env.set_var(ident.name, ty);
    }
}

fn declare_gen_decl_names(gen_decl: &ast::GenDecl<'_>, scopes: &mut ShortVarScopes) {
    for spec in &gen_decl.specs {
        let ast::Spec::ValueSpec(value_spec) = spec else {
            continue;
        };
        for name in &value_spec.names {
            scopes.declare(name.name);
        }
    }
}

pub fn ast_block_completion(block: &ast::BlockStmt<'_>, env: &TypeEnv) -> Completion {
    block_completion(&lower_block(block, env))
}

pub fn block_completion(block: &Block) -> Completion {
    stmts_completion(&block.stmts)
}

fn stmts_completion(stmts: &[Stmt]) -> Completion {
    stmts
        .iter()
        .rev()
        .find(|stmt| !matches!(stmt, Stmt::Empty))
        .map_or(Completion::MayComplete, stmt_completion)
}

pub fn stmt_completion(stmt: &Stmt) -> Completion {
    stmt_completion_with_label(stmt, None)
}

pub fn ast_stmt_has_goto_to_label(stmt: &ast::Stmt<'_>, label: &str) -> bool {
    match stmt {
        ast::Stmt::BranchStmt(branch) => {
            branch.tok == token::Token::GOTO
                && branch
                    .label
                    .as_ref()
                    .is_some_and(|target| target.name == label)
        }
        ast::Stmt::BlockStmt(block) => ast_block_has_goto_to_label(block, label),
        ast::Stmt::CaseClause(case) => case
            .body
            .iter()
            .any(|stmt| ast_stmt_has_goto_to_label(stmt, label)),
        ast::Stmt::CommClause(comm) => {
            comm.comm
                .as_ref()
                .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || comm
                    .body
                    .iter()
                    .any(|stmt| ast_stmt_has_goto_to_label(stmt, label))
        }
        ast::Stmt::ForStmt(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || for_stmt
                    .post
                    .as_ref()
                    .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || ast_block_has_goto_to_label(&for_stmt.body, label)
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt
                .init
                .as_ref()
                .as_ref()
                .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || ast_block_has_goto_to_label(&if_stmt.body, label)
                || if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
        }
        ast::Stmt::LabeledStmt(labeled) => ast_stmt_has_goto_to_label(&labeled.stmt, label),
        ast::Stmt::RangeStmt(range) => ast_block_has_goto_to_label(&range.body, label),
        ast::Stmt::SelectStmt(select) => ast_block_has_goto_to_label(&select.body, label),
        ast::Stmt::SwitchStmt(switch) => {
            switch
                .init
                .as_ref()
                .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || ast_block_has_goto_to_label(&switch.body, label)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            type_switch
                .init
                .as_ref()
                .is_some_and(|stmt| ast_stmt_has_goto_to_label(stmt, label))
                || ast_stmt_has_goto_to_label(&type_switch.assign, label)
                || ast_block_has_goto_to_label(&type_switch.body, label)
        }
        ast::Stmt::AssignStmt(_)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GotoStatePlan {
    pub labels: Vec<String>,
    pub hoisted_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidGoto {
    SkipsDeclarations {
        label: String,
        skipped_names: Vec<String>,
    },
    EntersBlock {
        label: String,
    },
    UndefinedLabel {
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidBranch {
    BreakLabel { label: String },
    BreakOutside,
    ContinueLabel { label: String },
    ContinueOutside,
    FallthroughInFinalCase,
    FallthroughInTypeSwitch,
    FallthroughNotFinal,
    FallthroughOutsideSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidStatement {
    Assignment {
        reason: InvalidAssignmentReason,
    },
    Condition {
        reason: InvalidConditionReason,
    },
    Defer {
        reason: InvalidStatementReason,
    },
    Declaration {
        reason: InvalidDeclaration,
    },
    DuplicateDefault {
        kind: DefaultClauseKind,
    },
    Expr {
        reason: InvalidStatementReason,
    },
    Expression {
        reason: InvalidStatementReason,
    },
    ForPostShortVarDecl,
    Go {
        reason: InvalidStatementReason,
    },
    IncDec {
        reason: InvalidIncDecReason,
    },
    MissingReturn,
    Range {
        reason: InvalidRangeReason,
    },
    Return {
        reason: InvalidReturnReason,
    },
    Receive {
        reason: InvalidReceiveReason,
    },
    Send {
        reason: InvalidSendReason,
    },
    SelectComm {
        reason: InvalidSelectCommReason,
    },
    ShortVarDecl {
        reason: InvalidShortVarDeclReason,
    },
    Switch {
        reason: InvalidSwitchReason,
    },
    TypeSwitchGuard {
        reason: InvalidTypeSwitchGuardReason,
    },
    TypeSwitch {
        reason: InvalidTypeSwitchReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidAssignmentReason {
    CompoundBlankIdentifier,
    CompoundInvalidOperand {
        op: String,
        side: String,
        type_name: String,
    },
    CompoundOperandCount {
        lhs: usize,
        rhs: usize,
    },
    CountMismatch {
        lhs: usize,
        values: usize,
    },
    InvalidLeftOperand,
    MultiValueInSingleValueContext,
    TypeMismatch {
        expected: String,
        actual: String,
    },
    UntypedNil,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidConditionReason {
    pub kind: ConditionKind,
    pub type_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    For,
    If,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidIncDecReason {
    InvalidOperand,
    NonNumericOperand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReceiveReason {
    NonChannel { type_name: String },
    SendOnlyChannel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReturnReason {
    CountMismatch { expected: usize, values: usize },
    MultiValueInSingleValueContext,
    TypeMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidSendReason {
    NonChannel { type_name: String },
    ReceiveOnlyChannel,
    ValueTypeMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidSwitchReason {
    CaseMultiValue { values: usize },
    CaseTypeMismatch { expected: String, actual: String },
    DuplicateConstantCase { value: String },
    NilTag,
    NonComparableCase { type_name: String },
    NonComparableTag { type_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidSelectCommReason {
    InvalidAssignmentToken,
    MissingReceiveExpression,
    NonCommunication,
    ShortReceiveDeclarationLhs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidTypeSwitchGuardReason {
    BlankIdentifier,
    InvalidAssignmentToken,
    InvalidExpression,
    InvalidIdentifierCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidTypeSwitchReason {
    CaseDoesNotImplement {
        case_type: String,
        interface_type: String,
    },
    DuplicateCase {
        type_name: String,
    },
    DuplicateNil,
    NonInterfaceGuard {
        type_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultClauseKind {
    Select,
    Switch,
    TypeSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidStatementReason {
    BlankIdentifier,
    InvalidArrayType { reason: String },
    InvalidBinary { op: String, reason: String },
    DisallowedBuiltin(String),
    InvalidBuiltinCall { name: String, reason: String },
    InvalidCall { target: String, reason: String },
    InvalidCompositeLiteral { reason: String },
    InvalidIndex { reason: String },
    InvalidMapType { reason: String },
    InvalidSlice { reason: String },
    InvalidTypeAssert { reason: String },
    InvalidTypeConversion { target: String, reason: String },
    InvalidUnary { op: String, reason: String },
    NonCallOrReceive,
    TypeConversion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidRangeReason {
    BindingCount {
        kind: RangeKind,
        max: usize,
        got: usize,
    },
    NonRangeable {
        type_name: String,
    },
    TypeMismatch {
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidShortVarDeclReason {
    DuplicateName(String),
    NonIdentifier,
    NoNewVariables,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidLabel {
    Duplicate { label: String },
    Unused { label: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidSignature {
    DuplicateInterfaceMethod {
        name: String,
    },
    DuplicateName {
        name: String,
    },
    DuplicateTypeParameterName {
        name: String,
    },
    InitFunction {
        type_params: usize,
        params: usize,
        results: usize,
    },
    InvalidTypeParameterDecl,
    MainFunction {
        type_params: usize,
        params: usize,
        results: usize,
    },
    MissingMainFunction,
    MethodTypeParams {
        count: usize,
    },
    MixedNamedUnnamed {
        list: SignatureList,
    },
    ReceiverCount {
        count: usize,
    },
    ReceiverType {
        base: Option<String>,
        reason: InvalidReceiverTypeReason,
    },
    ReceiverTypeParameterCount {
        base: String,
        expected: usize,
        got: usize,
    },
    ReceiverTypeParameterNotIdentifier,
    ReceiverVariadic,
    VariadicNotFinal,
    VariadicResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidReceiverTypeReason {
    GenericAlias,
    InstantiatedAlias,
    Interface,
    Pointer,
    Undefined,
    Unnamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureList {
    Parameter,
    Result,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidDeclaration {
    ConstInvalidInitializer {
        reason: String,
    },
    ConstNonConstantInitializer,
    ConstTypeMismatch {
        expected: String,
        actual: String,
    },
    ConstValueCount {
        names: usize,
        values: usize,
    },
    AliasToOwnTypeParameter {
        name: String,
    },
    DuplicateMethod {
        base: String,
        method: String,
    },
    DuplicateStructField {
        type_name: Option<String>,
        field: String,
    },
    DuplicateTopLevelName {
        name: String,
    },
    DuplicateDeclarationName {
        name: String,
    },
    DuplicateImportName {
        name: String,
    },
    DuplicateLexicalName {
        name: String,
    },
    ImportPackageBlockConflict {
        name: String,
    },
    InvalidInitIdentifier,
    InvalidPackageName {
        name: String,
    },
    MethodFieldConflict {
        base: String,
        name: String,
    },
    MissingConstInitializer,
    TypeDefinitionFromTypeParameter {
        name: String,
    },
    UnusedImport {
        path: String,
        alias: Option<String>,
    },
    UnusedVariable {
        name: String,
    },
    VarMissingTypeOrInitializer,
    VarMultiValueInSingleValueContext,
    VarUntypedNil,
    VarTypeMismatch {
        expected: String,
        actual: String,
    },
    VarValueCount {
        names: usize,
        values: usize,
    },
}

pub fn invalid_forward_goto_in_block(block: &ast::BlockStmt<'_>) -> Option<InvalidGoto> {
    invalid_forward_goto_in_stmt_list(&block.list)
}

pub fn invalid_forward_goto_in_func(block: &ast::BlockStmt<'_>) -> Option<InvalidGoto> {
    invalid_forward_goto_in_func_scope(block).or_else(|| {
        invalid_in_func_lits_in_block(block, &mut |body| invalid_forward_goto_in_func(body))
    })
}

fn invalid_forward_goto_in_func_scope(block: &ast::BlockStmt<'_>) -> Option<InvalidGoto> {
    invalid_forward_goto_in_stmt_list(&block.list)
        .or_else(|| invalid_forward_goto_in_nested_stmt_list(&block.list))
}

fn invalid_forward_goto_in_stmt_list(stmts: &[ast::Stmt<'_>]) -> Option<InvalidGoto> {
    let mut label_positions = BTreeMap::new();
    for (idx, stmt) in stmts.iter().enumerate() {
        for label in direct_label_names_in_stmt(stmt) {
            label_positions.entry(label).or_insert(idx);
        }
    }
    if label_positions.is_empty() {
        return None;
    }

    for (idx, stmt) in stmts.iter().enumerate() {
        let mut targets = BTreeSet::new();
        collect_goto_targets_in_stmt(stmt, &mut targets);
        for target in targets {
            let Some(target_idx) = label_positions.get(&target).copied() else {
                continue;
            };
            if target_idx <= idx {
                continue;
            }
            let mut skipped_names = BTreeSet::new();
            for skipped in stmts.iter().take(target_idx).skip(idx + 1) {
                collect_direct_declared_names_in_stmt(skipped, &mut skipped_names);
            }
            if !skipped_names.is_empty() {
                return Some(InvalidGoto::SkipsDeclarations {
                    label: target,
                    skipped_names: skipped_names.into_iter().collect(),
                });
            }
        }
    }
    None
}

fn invalid_forward_goto_in_nested_stmt_list(stmts: &[ast::Stmt<'_>]) -> Option<InvalidGoto> {
    stmts.iter().find_map(invalid_forward_goto_in_nested_stmt)
}

fn invalid_forward_goto_in_nested_stmt(stmt: &ast::Stmt<'_>) -> Option<InvalidGoto> {
    match stmt {
        ast::Stmt::BlockStmt(block) => invalid_forward_goto_in_func_scope(block),
        ast::Stmt::CaseClause(case) => invalid_forward_goto_in_stmt_list(&case.body)
            .or_else(|| invalid_forward_goto_in_nested_stmt_list(&case.body)),
        ast::Stmt::CommClause(comm) => comm
            .comm
            .as_deref()
            .and_then(invalid_forward_goto_in_nested_stmt)
            .or_else(|| invalid_forward_goto_in_stmt_list(&comm.body))
            .or_else(|| invalid_forward_goto_in_nested_stmt_list(&comm.body)),
        ast::Stmt::ForStmt(for_stmt) => for_stmt
            .init
            .as_deref()
            .and_then(invalid_forward_goto_in_nested_stmt)
            .or_else(|| {
                for_stmt
                    .post
                    .as_deref()
                    .and_then(invalid_forward_goto_in_nested_stmt)
            })
            .or_else(|| invalid_forward_goto_in_func_scope(&for_stmt.body)),
        ast::Stmt::IfStmt(if_stmt) => if_stmt
            .init
            .as_ref()
            .as_ref()
            .and_then(|init| invalid_forward_goto_in_nested_stmt(init))
            .or_else(|| invalid_forward_goto_in_func_scope(&if_stmt.body))
            .or_else(|| {
                if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .and_then(|else_branch| invalid_forward_goto_in_nested_stmt(else_branch))
            }),
        ast::Stmt::LabeledStmt(labeled) => invalid_forward_goto_in_nested_stmt(&labeled.stmt),
        ast::Stmt::RangeStmt(range) => invalid_forward_goto_in_func_scope(&range.body),
        ast::Stmt::SelectStmt(select) => invalid_forward_goto_in_func_scope(&select.body),
        ast::Stmt::SwitchStmt(switch) => switch
            .init
            .as_deref()
            .and_then(invalid_forward_goto_in_nested_stmt)
            .or_else(|| invalid_forward_goto_in_func_scope(&switch.body)),
        ast::Stmt::TypeSwitchStmt(type_switch) => type_switch
            .init
            .as_deref()
            .and_then(invalid_forward_goto_in_nested_stmt)
            .or_else(|| invalid_forward_goto_in_nested_stmt(&type_switch.assign))
            .or_else(|| invalid_forward_goto_in_func_scope(&type_switch.body)),
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => None,
    }
}

pub fn invalid_branch_in_func(block: &ast::BlockStmt<'_>) -> Option<InvalidBranch> {
    let mut context = BranchContext::default();
    invalid_branch_in_block(block, &mut context)
}

pub fn invalid_statement_in_func(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    let mut env = env.clone();
    let mut scopes = ShortVarScopes::new();
    invalid_statement_in_block(block, &mut env, &mut scopes)
}

pub fn invalid_statement_in_func_with_type(
    func_type: &ast::FuncType<'_>,
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    let mut env = env.clone();
    let mut scopes = ShortVarScopes::new();
    record_func_type_bindings(func_type, &mut env);
    seed_field_names_in_short_var_scope(&func_type.params, &mut scopes);
    if let Some(results) = &func_type.results {
        seed_field_names_in_short_var_scope(results, &mut scopes);
    }
    invalid_statement_in_block(block, &mut env, &mut scopes)
}

pub fn invalid_return_in_func(
    func_type: &ast::FuncType<'_>,
    body: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    let mut env = env.clone();
    record_func_type_bindings(func_type, &mut env);
    invalid_return_in_block(body, &return_signature(func_type), &mut env)
}

pub fn invalid_body_completion_in_func(
    func_type: &ast::FuncType<'_>,
    body: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    (field_list_binding_count(func_type.results.as_ref()) > 0
        && ast_block_completion(body, env) != Completion::Terminates)
        .then_some(InvalidStatement::MissingReturn)
}

pub fn invalid_label_in_func(block: &ast::BlockStmt<'_>) -> Option<InvalidLabel> {
    let mut labels = Vec::new();
    collect_labels_in_block(block, &mut labels);
    let mut seen = BTreeSet::new();
    for label in &labels {
        if !seen.insert(label.clone()) {
            return Some(InvalidLabel::Duplicate {
                label: label.clone(),
            });
        }
    }

    let mut uses = BTreeSet::new();
    collect_label_uses_in_block(block, &mut uses);
    labels
        .into_iter()
        .find(|label| !uses.contains(label))
        .map(|label| InvalidLabel::Unused { label })
        .or_else(|| invalid_in_func_lits_in_block(block, &mut invalid_label_in_func))
}

pub fn invalid_signature_in_file(file: &ast::File<'_>) -> Option<InvalidSignature> {
    for decl in &file.decls {
        if let Some(invalid) = invalid_signature_in_decl(decl) {
            return Some(invalid);
        }
    }
    invalid_main_signature_in_file(file)
}

pub fn invalid_main_package_in_file(file: &ast::File<'_>) -> Option<InvalidSignature> {
    if file.name.name != "main" {
        return None;
    }
    let has_main = file.decls.iter().any(|decl| {
        matches!(
            decl,
            ast::Decl::FuncDecl(func) if func.recv.is_none() && func.name.name == "main"
        )
    });
    (!has_main).then_some(InvalidSignature::MissingMainFunction)
}

pub fn invalid_receiver_type_in_file(
    file: &ast::File<'_>,
    env: &TypeEnv,
) -> Option<InvalidSignature> {
    for decl in &file.decls {
        if let Some(invalid) = invalid_receiver_type_in_decl(decl, env) {
            return Some(invalid);
        }
    }
    None
}

pub fn invalid_declaration_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    invalid_declaration_in_file_with_import_package_names(file, &BTreeMap::new())
}

pub fn invalid_declaration_in_file_with_import_package_names(
    file: &ast::File<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Option<InvalidDeclaration> {
    invalid_package_name_in_file(file).or_else(|| {
        invalid_import_names_in_file(file, import_package_names).or_else(|| {
            invalid_top_level_names_in_file(file).or_else(|| {
                invalid_method_names_in_file(file).or_else(|| {
                    for decl in &file.decls {
                        if let Some(invalid) = invalid_declaration_in_decl(decl) {
                            return Some(invalid);
                        }
                    }
                    invalid_local_declaration_names_in_file(file)
                        .or_else(|| invalid_type_parameter_type_declarations_in_file(file))
                })
            })
        })
    })
}

fn invalid_package_name_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    (file.name.name == "_").then(|| InvalidDeclaration::InvalidPackageName {
        name: file.name.name.to_string(),
    })
}

fn invalid_import_names_in_file(
    file: &ast::File<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Option<InvalidDeclaration> {
    let package_names = package_block_declared_names(file);
    let mut names_by_file: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for decl in &file.decls {
        let ast::Decl::GenDecl(gen_decl) = decl else {
            continue;
        };
        if gen_decl.tok != token::Token::IMPORT {
            continue;
        }
        for spec in &gen_decl.specs {
            let ast::Spec::ImportSpec(import) = spec else {
                continue;
            };
            let Some(binding) = import_binding_name(import, import_package_names) else {
                continue;
            };
            let file_key = import_file_key(import);
            if package_names.contains(&binding.name) {
                return Some(InvalidDeclaration::ImportPackageBlockConflict { name: binding.name });
            }
            let names = names_by_file.entry(file_key.clone()).or_default();
            if !names.insert(binding.name.clone()) {
                return Some(InvalidDeclaration::DuplicateImportName { name: binding.name });
            }
        }
    }
    None
}

pub fn invalid_unused_import_in_file_with_import_package_names(
    file: &ast::File<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Option<InvalidDeclaration> {
    let used_import_names_by_file = used_import_names_by_file(file);
    for import in file.imports() {
        let Some(binding) = import_binding_name(import, import_package_names) else {
            continue;
        };
        let file_key = import_file_key(import);
        if !used_import_names_by_file
            .get(&file_key)
            .is_some_and(|used| used.contains(&binding.name))
        {
            return Some(InvalidDeclaration::UnusedImport {
                path: binding.path,
                alias: binding.alias,
            });
        }
    }
    None
}

pub fn invalid_unused_local_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    for decl in &file.decls {
        let ast::Decl::FuncDecl(func) = decl else {
            continue;
        };
        if let Some(body) = &func.body
            && let Some(invalid) = invalid_unused_local_in_func(func, body)
        {
            return Some(invalid);
        }
    }
    None
}

#[derive(Clone)]
struct LocalBinding {
    used: bool,
    check_unused: bool,
}

#[derive(Default)]
struct LocalUseScopes {
    scopes: Vec<BTreeMap<String, LocalBinding>>,
}

type LocalUseResult = Result<(), InvalidDeclaration>;

impl LocalUseScopes {
    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) -> Option<InvalidDeclaration> {
        let scope = self.scopes.pop()?;
        scope.into_iter().find_map(|(name, binding)| {
            (binding.check_unused && !binding.used)
                .then_some(InvalidDeclaration::UnusedVariable { name })
        })
    }

    fn declare_checked(&mut self, name: &str) {
        self.declare(name, true);
    }

    fn declare_ignored(&mut self, name: &str) {
        self.declare(name, false);
    }

    fn declare(&mut self, name: &str, check_unused: bool) {
        if name == "_" {
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.entry(name.to_string()).or_insert(LocalBinding {
                used: false,
                check_unused,
            });
        }
    }

    fn mark_used(&mut self, name: &str) {
        if name == "_" {
            return;
        }
        if let Some(binding) = self
            .scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.get_mut(name))
        {
            binding.used = true;
        }
    }
}

fn finish_local_scope(scopes: &mut LocalUseScopes) -> LocalUseResult {
    scopes.pop_scope().map_or(Ok(()), Err)
}

fn invalid_unused_local_in_func(
    func: &ast::FuncDecl<'_>,
    body: &ast::BlockStmt<'_>,
) -> Option<InvalidDeclaration> {
    let mut scopes = LocalUseScopes::default();
    scopes.push_scope();
    if let Some(recv) = &func.recv {
        declare_field_names_ignored(recv, &mut scopes);
    }
    declare_func_type_names_ignored(&func.type_, &mut scopes);
    collect_unused_local_in_stmt_list(&body.list, &mut scopes)
        .err()
        .or_else(|| scopes.pop_scope())
}

fn declare_func_type_names_ignored(func_type: &ast::FuncType<'_>, scopes: &mut LocalUseScopes) {
    declare_field_names_ignored(&func_type.params, scopes);
    if let Some(results) = &func_type.results {
        declare_field_names_ignored(results, scopes);
    }
}

fn declare_field_names_ignored(fields: &ast::FieldList<'_>, scopes: &mut LocalUseScopes) {
    for field in &fields.list {
        if let Some(names) = &field.names {
            for name in names {
                scopes.declare_ignored(name.name);
            }
        }
    }
}

fn collect_unused_local_in_nested_block(
    block: &ast::BlockStmt<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    scopes.push_scope();
    collect_unused_local_in_stmt_list(&block.list, scopes)?;
    finish_local_scope(scopes)
}

fn collect_unused_local_in_nested_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    scopes.push_scope();
    collect_unused_local_in_stmt_list(stmts, scopes)?;
    finish_local_scope(scopes)
}

fn collect_unused_local_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    for stmt in stmts {
        collect_unused_local_in_stmt(stmt, scopes)?;
    }
    Ok(())
}

fn collect_unused_local_in_stmt(
    stmt: &ast::Stmt<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    match stmt {
        ast::Stmt::AssignStmt(assign) => collect_unused_local_in_assign(assign, scopes),
        ast::Stmt::BlockStmt(block) => collect_unused_local_in_nested_block(block, scopes),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => Ok(()),
        ast::Stmt::CaseClause(case) => {
            if let Some(list) = &case.list {
                for expr in list {
                    collect_unused_local_in_expr(expr, scopes)?;
                }
            }
            collect_unused_local_in_nested_stmt_list(&case.body, scopes)
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm {
                collect_unused_local_in_stmt(comm, scopes)?;
            }
            collect_unused_local_in_nested_stmt_list(&comm.body, scopes)
        }
        ast::Stmt::DeclStmt(decl) => collect_unused_local_in_gen_decl(&decl.decl, scopes),
        ast::Stmt::DeferStmt(defer) => collect_unused_local_in_call(&defer.call, scopes),
        ast::Stmt::ExprStmt(expr) => collect_unused_local_in_expr(&expr.x, scopes),
        ast::Stmt::ForStmt(for_stmt) => {
            let has_clause_scope = for_stmt.init.is_some();
            if has_clause_scope {
                scopes.push_scope();
            }
            if let Some(init) = &for_stmt.init {
                collect_unused_local_in_stmt(init, scopes)?;
            }
            if let Some(cond) = &for_stmt.cond {
                collect_unused_local_in_expr(cond, scopes)?;
            }
            if let Some(post) = &for_stmt.post {
                collect_unused_local_in_stmt(post, scopes)?;
            }
            collect_unused_local_in_nested_block(&for_stmt.body, scopes)?;
            if has_clause_scope {
                finish_local_scope(scopes)?;
            }
            Ok(())
        }
        ast::Stmt::GoStmt(go) => collect_unused_local_in_call(&go.call, scopes),
        ast::Stmt::IfStmt(if_stmt) => {
            let has_clause_scope = if_stmt.init.as_ref().as_ref().is_some();
            if has_clause_scope {
                scopes.push_scope();
            }
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_unused_local_in_stmt(init, scopes)?;
            }
            collect_unused_local_in_expr(&if_stmt.cond, scopes)?;
            collect_unused_local_in_nested_block(&if_stmt.body, scopes)?;
            if let Some(else_) = if_stmt.else_.as_ref().as_ref() {
                collect_unused_local_in_stmt(else_, scopes)?;
            }
            if has_clause_scope {
                finish_local_scope(scopes)?;
            }
            Ok(())
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            collect_unused_local_in_assignment_lhs(&inc_dec.x, scopes, true)
        }
        ast::Stmt::LabeledStmt(label) => collect_unused_local_in_stmt(&label.stmt, scopes),
        ast::Stmt::RangeStmt(range) => {
            collect_unused_local_in_expr(&range.x, scopes)?;
            let has_range_scope = matches!(range.tok, Some(token::Token::DEFINE));
            if has_range_scope {
                scopes.push_scope();
                if let Some(key) = &range.key
                    && let Some(name) = ident_name(key)
                {
                    scopes.declare_checked(&name);
                }
                if let Some(value) = &range.value
                    && let Some(name) = ident_name(value)
                {
                    scopes.declare_checked(&name);
                }
            } else {
                if let Some(key) = &range.key {
                    collect_unused_local_in_assignment_lhs(key, scopes, false)?;
                }
                if let Some(value) = &range.value {
                    collect_unused_local_in_assignment_lhs(value, scopes, false)?;
                }
            }
            collect_unused_local_in_nested_block(&range.body, scopes)?;
            if has_range_scope {
                finish_local_scope(scopes)?;
            }
            Ok(())
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_unused_local_in_expr(expr, scopes)?;
            }
            Ok(())
        }
        ast::Stmt::SelectStmt(select) => collect_unused_local_in_nested_block(&select.body, scopes),
        ast::Stmt::SendStmt(send) => {
            collect_unused_local_in_expr(&send.chan, scopes)?;
            collect_unused_local_in_expr(&send.value, scopes)
        }
        ast::Stmt::SwitchStmt(switch) => {
            let has_clause_scope = switch.init.is_some();
            if has_clause_scope {
                scopes.push_scope();
            }
            if let Some(init) = &switch.init {
                collect_unused_local_in_stmt(init, scopes)?;
            }
            if let Some(tag) = &switch.tag {
                collect_unused_local_in_expr(tag, scopes)?;
            }
            collect_unused_local_in_nested_block(&switch.body, scopes)?;
            if has_clause_scope {
                finish_local_scope(scopes)?;
            }
            Ok(())
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            let has_clause_scope = type_switch.init.is_some();
            if has_clause_scope {
                scopes.push_scope();
            }
            if let Some(init) = &type_switch.init {
                collect_unused_local_in_stmt(init, scopes)?;
            }
            collect_unused_local_in_stmt(&type_switch.assign, scopes)?;
            collect_unused_local_in_nested_block(&type_switch.body, scopes)?;
            if has_clause_scope {
                finish_local_scope(scopes)?;
            }
            Ok(())
        }
    }
}

fn collect_unused_local_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    for spec in &gen_decl.specs {
        match spec {
            ast::Spec::ImportSpec(_) | ast::Spec::TypeSpec(_) => {}
            ast::Spec::ValueSpec(value) if gen_decl.tok == token::Token::VAR => {
                if let Some(type_) = &value.type_ {
                    collect_unused_local_in_expr(type_, scopes)?;
                }
                if let Some(values) = &value.values {
                    for value in values {
                        collect_unused_local_in_expr(value, scopes)?;
                    }
                }
                for name in &value.names {
                    scopes.declare_checked(name.name);
                }
            }
            ast::Spec::ValueSpec(value) => {
                if let Some(values) = &value.values {
                    for value in values {
                        collect_unused_local_in_expr(value, scopes)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_unused_local_in_assign(
    assign: &ast::AssignStmt<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    for expr in &assign.rhs {
        collect_unused_local_in_expr(expr, scopes)?;
    }
    if assign.tok == token::Token::DEFINE {
        for expr in &assign.lhs {
            if let Some(name) = ident_name(expr) {
                if !scopes
                    .scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(&name))
                {
                    scopes.declare_checked(&name);
                }
            } else {
                collect_unused_local_in_assignment_lhs(expr, scopes, false)?;
            }
        }
    } else {
        let mutation_counts_as_use = assign.tok.is_assign_op();
        for expr in &assign.lhs {
            collect_unused_local_in_assignment_lhs(expr, scopes, mutation_counts_as_use)?;
        }
    }
    Ok(())
}

fn collect_unused_local_in_assignment_lhs(
    expr: &ast::Expr<'_>,
    scopes: &mut LocalUseScopes,
    bare_ident_counts_as_use: bool,
) -> LocalUseResult {
    match expr {
        ast::Expr::Ident(ident) => {
            if bare_ident_counts_as_use {
                scopes.mark_used(ident.name);
            }
            Ok(())
        }
        ast::Expr::ParenExpr(paren) => {
            collect_unused_local_in_assignment_lhs(&paren.x, scopes, bare_ident_counts_as_use)
        }
        ast::Expr::SelectorExpr(selector) => collect_unused_local_in_expr(&selector.x, scopes),
        ast::Expr::IndexExpr(index) => {
            collect_unused_local_in_expr(&index.x, scopes)?;
            collect_unused_local_in_expr(&index.index, scopes)
        }
        ast::Expr::IndexListExpr(index) => {
            collect_unused_local_in_expr(&index.x, scopes)?;
            for index in &index.indices {
                collect_unused_local_in_expr(index, scopes)?;
            }
            Ok(())
        }
        ast::Expr::StarExpr(star) => collect_unused_local_in_expr(&star.x, scopes),
        ast::Expr::TypeAssertExpr(assert) => {
            collect_unused_local_in_expr(&assert.x, scopes)?;
            if let Some(type_) = &assert.type_ {
                collect_unused_local_in_expr(type_, scopes)?;
            }
            Ok(())
        }
        _ => collect_unused_local_in_expr(expr, scopes),
    }
}

fn collect_unused_local_in_call(
    call: &ast::CallExpr<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    collect_unused_local_in_expr(&call.fun, scopes)?;
    if let Some(args) = &call.args {
        for arg in args {
            collect_unused_local_in_expr(arg, scopes)?;
        }
    }
    Ok(())
}

fn collect_unused_local_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    match expr {
        ast::Expr::Ident(ident) => {
            scopes.mark_used(ident.name);
            Ok(())
        }
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_unused_local_in_expr(len, scopes)?;
            }
            collect_unused_local_in_expr(&array.elt, scopes)
        }
        ast::Expr::BasicLit(_) => Ok(()),
        ast::Expr::BinaryExpr(binary) => {
            collect_unused_local_in_expr(&binary.x, scopes)?;
            collect_unused_local_in_expr(&binary.y, scopes)
        }
        ast::Expr::CallExpr(call) => collect_unused_local_in_call(call, scopes),
        ast::Expr::ChanType(chan) => collect_unused_local_in_expr(&chan.value, scopes),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_ {
                collect_unused_local_in_expr(type_, scopes)?;
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_unused_local_in_expr(elt, scopes)?;
                }
            }
            Ok(())
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_unused_local_in_expr(elt, scopes)?;
            }
            Ok(())
        }
        ast::Expr::FuncLit(func_lit) => {
            scopes.push_scope();
            declare_func_type_names_ignored(&func_lit.type_, scopes);
            collect_unused_local_in_stmt_list(&func_lit.body.list, scopes)?;
            finish_local_scope(scopes)
        }
        ast::Expr::FuncType(func_type) => {
            collect_unused_local_in_field_list(&func_type.params, scopes)?;
            if let Some(results) = &func_type.results {
                collect_unused_local_in_field_list(results, scopes)?;
            }
            Ok(())
        }
        ast::Expr::IndexExpr(index) => {
            collect_unused_local_in_expr(&index.x, scopes)?;
            collect_unused_local_in_expr(&index.index, scopes)
        }
        ast::Expr::IndexListExpr(index) => {
            collect_unused_local_in_expr(&index.x, scopes)?;
            for index in &index.indices {
                collect_unused_local_in_expr(index, scopes)?;
            }
            Ok(())
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                collect_unused_local_in_field_list(methods, scopes)?;
            }
            Ok(())
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_unused_local_in_expr(&kv.key, scopes)?;
            collect_unused_local_in_expr(&kv.value, scopes)
        }
        ast::Expr::MapType(map) => {
            collect_unused_local_in_expr(&map.key, scopes)?;
            collect_unused_local_in_expr(&map.value, scopes)
        }
        ast::Expr::ParenExpr(paren) => collect_unused_local_in_expr(&paren.x, scopes),
        ast::Expr::SelectorExpr(selector) => collect_unused_local_in_expr(&selector.x, scopes),
        ast::Expr::SliceExpr(slice) => {
            collect_unused_local_in_expr(&slice.x, scopes)?;
            if let Some(low) = &slice.low {
                collect_unused_local_in_expr(low, scopes)?;
            }
            if let Some(high) = &slice.high {
                collect_unused_local_in_expr(high, scopes)?;
            }
            if let Some(max) = &slice.max {
                collect_unused_local_in_expr(max, scopes)?;
            }
            Ok(())
        }
        ast::Expr::StarExpr(star) => collect_unused_local_in_expr(&star.x, scopes),
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                collect_unused_local_in_field_list(fields, scopes)?;
            }
            Ok(())
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_unused_local_in_expr(&assert.x, scopes)?;
            if let Some(type_) = &assert.type_ {
                collect_unused_local_in_expr(type_, scopes)?;
            }
            Ok(())
        }
        ast::Expr::UnaryExpr(unary) => collect_unused_local_in_expr(&unary.x, scopes),
    }
}

fn collect_unused_local_in_field_list(
    fields: &ast::FieldList<'_>,
    scopes: &mut LocalUseScopes,
) -> LocalUseResult {
    for field in &fields.list {
        if let Some(type_) = &field.type_ {
            collect_unused_local_in_expr(type_, scopes)?;
        }
    }
    Ok(())
}

fn import_file_key(import: &ast::ImportSpec<'_>) -> (String, String) {
    let pos = import.path.value_pos;
    (pos.directory.to_string(), pos.file.to_string())
}

fn package_block_declared_names(file: &ast::File<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for decl in &file.decls {
        for name in top_level_declared_names(decl) {
            if name != "_" && name != "init" {
                names.insert(name);
            }
        }
    }
    names
}

struct ImportBinding {
    name: String,
    path: String,
    alias: Option<String>,
}

fn import_binding_name(
    import: &ast::ImportSpec<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Option<ImportBinding> {
    let path = import.path.value.trim_matches('"').to_string();
    if let Some(name) = &import.name {
        return (!matches!(name.name, "_" | ".")).then(|| ImportBinding {
            name: name.name.to_string(),
            path,
            alias: Some(name.name.to_string()),
        });
    }

    let path_base = path.rsplit('/').next().unwrap_or(&path);
    let name = import_package_names
        .get(&path)
        .cloned()
        .unwrap_or_else(|| path_base.to_string());
    let alias = (name != path_base).then(|| name.clone());
    Some(ImportBinding { name, path, alias })
}

fn used_import_names_by_file(file: &ast::File<'_>) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut used = BTreeMap::new();
    for decl in &file.decls {
        collect_used_import_names_in_decl(decl, &mut used);
    }
    used
}

fn collect_used_import_names_in_decl(
    decl: &ast::Decl<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    match decl {
        ast::Decl::FuncDecl(func) => {
            collect_used_import_names_in_func_type(&func.type_, used);
            if let Some(recv) = &func.recv {
                collect_used_import_names_in_field_list(recv, used);
            }
            if let Some(body) = &func.body {
                collect_used_import_names_in_block(body, used);
            }
        }
        ast::Decl::GenDecl(gen_decl) if gen_decl.tok != token::Token::IMPORT => {
            for spec in &gen_decl.specs {
                collect_used_import_names_in_spec(spec, used);
            }
        }
        ast::Decl::GenDecl(_) => {}
    }
}

fn collect_used_import_names_in_spec(
    spec: &ast::Spec<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    match spec {
        ast::Spec::ImportSpec(_) => {}
        ast::Spec::TypeSpec(type_spec) => {
            if let Some(type_params) = &type_spec.type_params {
                collect_used_import_names_in_field_list(type_params, used);
            }
            collect_used_import_names_in_expr(&type_spec.type_, used);
        }
        ast::Spec::ValueSpec(value) => {
            if let Some(type_) = &value.type_ {
                collect_used_import_names_in_expr(type_, used);
            }
            if let Some(values) = &value.values {
                for value in values {
                    collect_used_import_names_in_expr(value, used);
                }
            }
        }
    }
}

fn collect_used_import_names_in_field_list(
    fields: &ast::FieldList<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    for field in &fields.list {
        if let Some(type_) = &field.type_ {
            collect_used_import_names_in_expr(type_, used);
        }
    }
}

fn collect_used_import_names_in_func_type(
    func_type: &ast::FuncType<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    if let Some(type_params) = &func_type.type_params {
        collect_used_import_names_in_field_list(type_params, used);
    }
    collect_used_import_names_in_field_list(&func_type.params, used);
    if let Some(results) = &func_type.results {
        collect_used_import_names_in_field_list(results, used);
    }
}

fn collect_used_import_names_in_block(
    block: &ast::BlockStmt<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    for stmt in &block.list {
        collect_used_import_names_in_stmt(stmt, used);
    }
}

fn collect_used_import_names_in_stmt(
    stmt: &ast::Stmt<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_used_import_names_in_expr(expr, used);
            }
        }
        ast::Stmt::BlockStmt(block) => collect_used_import_names_in_block(block, used),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
        ast::Stmt::CaseClause(case) => {
            if let Some(list) = &case.list {
                for expr in list {
                    collect_used_import_names_in_expr(expr, used);
                }
            }
            for stmt in &case.body {
                collect_used_import_names_in_stmt(stmt, used);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = comm.comm.as_deref() {
                collect_used_import_names_in_stmt(stmt, used);
            }
            for stmt in &comm.body {
                collect_used_import_names_in_stmt(stmt, used);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                collect_used_import_names_in_spec(spec, used);
            }
        }
        ast::Stmt::DeferStmt(defer) => collect_used_import_names_in_call(&defer.call, used),
        ast::Stmt::ExprStmt(expr) => collect_used_import_names_in_expr(&expr.x, used),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_used_import_names_in_stmt(init, used);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_used_import_names_in_expr(cond, used);
            }
            if let Some(post) = &for_stmt.post {
                collect_used_import_names_in_stmt(post, used);
            }
            collect_used_import_names_in_block(&for_stmt.body, used);
        }
        ast::Stmt::GoStmt(go) => collect_used_import_names_in_call(&go.call, used),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_used_import_names_in_stmt(init, used);
            }
            collect_used_import_names_in_expr(&if_stmt.cond, used);
            collect_used_import_names_in_block(&if_stmt.body, used);
            if let Some(else_) = if_stmt.else_.as_ref().as_ref() {
                collect_used_import_names_in_stmt(else_, used);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => collect_used_import_names_in_expr(&inc_dec.x, used),
        ast::Stmt::LabeledStmt(label) => collect_used_import_names_in_stmt(&label.stmt, used),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_used_import_names_in_expr(key, used);
            }
            if let Some(value) = &range.value {
                collect_used_import_names_in_expr(value, used);
            }
            collect_used_import_names_in_expr(&range.x, used);
            collect_used_import_names_in_block(&range.body, used);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_used_import_names_in_expr(expr, used);
            }
        }
        ast::Stmt::SelectStmt(select) => collect_used_import_names_in_block(&select.body, used),
        ast::Stmt::SendStmt(send) => {
            collect_used_import_names_in_expr(&send.chan, used);
            collect_used_import_names_in_expr(&send.value, used);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_used_import_names_in_stmt(init, used);
            }
            if let Some(tag) = &switch.tag {
                collect_used_import_names_in_expr(tag, used);
            }
            collect_used_import_names_in_block(&switch.body, used);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_used_import_names_in_stmt(init, used);
            }
            collect_used_import_names_in_stmt(&type_switch.assign, used);
            collect_used_import_names_in_block(&type_switch.body, used);
        }
    }
}

fn collect_used_import_names_in_call(
    call: &ast::CallExpr<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    collect_used_import_names_in_expr(&call.fun, used);
    if let Some(args) = &call.args {
        for arg in args {
            collect_used_import_names_in_expr(arg, used);
        }
    }
}

fn collect_used_import_names_in_expr(
    expr: &ast::Expr<'_>,
    used: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_used_import_names_in_expr(len, used);
            }
            collect_used_import_names_in_expr(&array.elt, used);
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => {}
        ast::Expr::BinaryExpr(binary) => {
            collect_used_import_names_in_expr(&binary.x, used);
            collect_used_import_names_in_expr(&binary.y, used);
        }
        ast::Expr::CallExpr(call) => collect_used_import_names_in_call(call, used),
        ast::Expr::ChanType(chan) => collect_used_import_names_in_expr(&chan.value, used),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_ {
                collect_used_import_names_in_expr(type_, used);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_used_import_names_in_expr(elt, used);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_used_import_names_in_expr(elt, used);
            }
        }
        ast::Expr::FuncLit(func) => {
            collect_used_import_names_in_func_type(&func.type_, used);
            collect_used_import_names_in_block(&func.body, used);
        }
        ast::Expr::FuncType(func_type) => collect_used_import_names_in_func_type(func_type, used),
        ast::Expr::IndexExpr(index) => {
            collect_used_import_names_in_expr(&index.x, used);
            collect_used_import_names_in_expr(&index.index, used);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_used_import_names_in_expr(&index.x, used);
            for index in &index.indices {
                collect_used_import_names_in_expr(index, used);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                collect_used_import_names_in_field_list(methods, used);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_used_import_names_in_expr(&kv.key, used);
            collect_used_import_names_in_expr(&kv.value, used);
        }
        ast::Expr::MapType(map) => {
            collect_used_import_names_in_expr(&map.key, used);
            collect_used_import_names_in_expr(&map.value, used);
        }
        ast::Expr::ParenExpr(paren) => collect_used_import_names_in_expr(&paren.x, used),
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(base) = &*selector.x {
                used.entry(import_file_key_from_position(base.name_pos))
                    .or_default()
                    .insert(base.name.to_string());
            }
            collect_used_import_names_in_expr(&selector.x, used);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_used_import_names_in_expr(&slice.x, used);
            if let Some(low) = &slice.low {
                collect_used_import_names_in_expr(low, used);
            }
            if let Some(high) = &slice.high {
                collect_used_import_names_in_expr(high, used);
            }
            if let Some(max) = &slice.max {
                collect_used_import_names_in_expr(max, used);
            }
        }
        ast::Expr::StarExpr(star) => collect_used_import_names_in_expr(&star.x, used),
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                collect_used_import_names_in_field_list(fields, used);
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_used_import_names_in_expr(&assert.x, used);
            if let Some(type_) = &assert.type_ {
                collect_used_import_names_in_expr(type_, used);
            }
        }
        ast::Expr::UnaryExpr(unary) => collect_used_import_names_in_expr(&unary.x, used),
    }
}

fn import_file_key_from_position(pos: crate::token::Position<'_>) -> (String, String) {
    (pos.directory.to_string(), pos.file.to_string())
}

fn invalid_top_level_names_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    let mut names = BTreeSet::new();
    for decl in &file.decls {
        for name in top_level_declared_names(decl) {
            if name == "_" {
                continue;
            }
            if name == "init" {
                return Some(InvalidDeclaration::InvalidInitIdentifier);
            }
            if !names.insert(name.clone()) {
                return Some(InvalidDeclaration::DuplicateTopLevelName { name });
            }
        }
    }
    None
}

fn top_level_declared_names(decl: &ast::Decl<'_>) -> Vec<String> {
    match decl {
        ast::Decl::FuncDecl(func) if func.recv.is_none() && func.name.name != "init" => {
            vec![func.name.name.to_string()]
        }
        ast::Decl::FuncDecl(_) => Vec::new(),
        ast::Decl::GenDecl(gen_decl) => {
            let mut names = Vec::new();
            for spec in &gen_decl.specs {
                match spec {
                    ast::Spec::ImportSpec(_) => {}
                    ast::Spec::TypeSpec(type_spec) => {
                        if let Some(name) = &type_spec.name {
                            names.push(name.name.to_string());
                        }
                    }
                    ast::Spec::ValueSpec(value_spec) => {
                        names.extend(value_spec.names.iter().map(|name| name.name.to_string()));
                    }
                }
            }
            names
        }
    }
}

pub fn invalid_value_declaration_in_file(
    file: &ast::File<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    for decl in &file.decls {
        let ast::Decl::GenDecl(gen_decl) = decl else {
            continue;
        };
        if let Some(invalid) = invalid_value_declaration_in_gen_decl(gen_decl, env) {
            return Some(invalid);
        }
    }
    None
}

pub fn invalid_expression_in_file(file: &ast::File<'_>, env: &TypeEnv) -> Option<InvalidStatement> {
    for decl in &file.decls {
        let ast::Decl::GenDecl(gen_decl) = decl else {
            continue;
        };
        if let Some(reason) = invalid_expression_in_gen_decl(gen_decl, env) {
            return Some(InvalidStatement::Expression { reason });
        }
    }
    None
}

pub fn invalid_short_var_redeclaration_in_file(file: &ast::File<'_>) -> Option<InvalidStatement> {
    for decl in &file.decls {
        if let Some(invalid) = invalid_short_var_redeclaration_in_decl(decl) {
            return Some(invalid);
        }
    }
    None
}

pub fn invalid_goto_target_in_func(block: &ast::BlockStmt<'_>) -> Option<InvalidGoto> {
    let mut labels = BTreeMap::new();
    collect_label_paths_in_block(block, &[], &mut labels);
    let mut gotos = Vec::new();
    collect_goto_paths_in_block(block, &[], &mut gotos);

    for (label, goto_path) in gotos {
        let Some(label_path) = labels.get(&label) else {
            return Some(InvalidGoto::UndefinedLabel { label });
        };
        if !goto_path.starts_with(label_path) {
            return Some(InvalidGoto::EntersBlock { label });
        }
    }
    invalid_in_func_lits_in_block(block, &mut invalid_goto_target_in_func)
}

fn invalid_signature_in_decl(decl: &ast::Decl<'_>) -> Option<InvalidSignature> {
    match decl {
        ast::Decl::FuncDecl(func) => invalid_signature_in_func_decl(func),
        ast::Decl::GenDecl(gen_decl) => invalid_signature_in_gen_decl(gen_decl),
    }
}

fn invalid_receiver_type_in_decl(decl: &ast::Decl<'_>, env: &TypeEnv) -> Option<InvalidSignature> {
    match decl {
        ast::Decl::FuncDecl(func) => invalid_receiver_type_in_func_decl(func, env),
        ast::Decl::GenDecl(_) => None,
    }
}

fn invalid_receiver_type_in_func_decl(
    func: &ast::FuncDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidSignature> {
    let Some(recv) = &func.recv else {
        return None;
    };
    if recv.list.len() != 1 {
        return None;
    }
    let field = recv.list.first()?;
    if field_binding_count(field) != 1 || field_type_is_variadic(field) {
        return None;
    }
    if field.type_.is_none() {
        return Some(InvalidSignature::ReceiverType {
            base: None,
            reason: InvalidReceiverTypeReason::Unnamed,
        });
    }
    let Some(base) = receiver_base_type_name(recv) else {
        return Some(InvalidSignature::ReceiverType {
            base: None,
            reason: InvalidReceiverTypeReason::Unnamed,
        });
    };
    if let Some(expected) = env.get_type_param_count(&base) {
        let got = receiver_type_parameter_count(recv);
        if expected != got {
            return Some(InvalidSignature::ReceiverTypeParameterCount {
                base,
                expected,
                got,
            });
        }
    }
    if env.is_type_alias(&base) {
        if env
            .get_type_param_count(&base)
            .is_some_and(|count| count > 0)
        {
            return Some(InvalidSignature::ReceiverType {
                base: Some(base),
                reason: InvalidReceiverTypeReason::GenericAlias,
            });
        }
        if env.alias_denotes_instantiated_generic(&base) {
            return Some(InvalidSignature::ReceiverType {
                base: Some(base),
                reason: InvalidReceiverTypeReason::InstantiatedAlias,
            });
        }
    }
    invalid_receiver_base_type(&base, env).map(|reason| InvalidSignature::ReceiverType {
        base: Some(base),
        reason,
    })
}

fn invalid_receiver_base_type(base: &str, env: &TypeEnv) -> Option<InvalidReceiverTypeReason> {
    let Some(kind) = env.get_type_kind(base) else {
        return Some(InvalidReceiverTypeReason::Undefined);
    };
    match kind {
        TypeKind::Struct => None,
        TypeKind::Interface => Some(InvalidReceiverTypeReason::Interface),
        TypeKind::Alias(ty) => {
            let resolved = env.resolve_alias(ty);
            if matches!(resolved, GoType::Pointer(_)) {
                Some(InvalidReceiverTypeReason::Pointer)
            } else if go_type_is_interface(&resolved, env) {
                Some(InvalidReceiverTypeReason::Interface)
            } else {
                None
            }
        }
    }
}

fn go_type_is_interface(ty: &GoType, env: &TypeEnv) -> bool {
    match ty {
        GoType::Any | GoType::Error | GoType::Interface(_) => true,
        GoType::Named(name) => env.is_interface(name),
        _ => false,
    }
}

fn invalid_signature_in_func_decl(func: &ast::FuncDecl<'_>) -> Option<InvalidSignature> {
    if func.recv.is_none()
        && func.name.name == "init"
        && let Some(invalid) = invalid_init_signature(func)
    {
        return Some(invalid);
    }
    if func.recv.is_some() {
        let count = field_list_binding_count(func.type_.type_params.as_ref());
        if count != 0 {
            return Some(InvalidSignature::MethodTypeParams { count });
        }
    }
    let mut names = BTreeSet::new();
    if let Some(invalid) = invalid_type_parameter_list(func.type_.type_params.as_ref(), &mut names)
    {
        return Some(invalid);
    }
    if let Some(recv) = &func.recv
        && let Some(invalid) = invalid_receiver_signature(recv, &mut names)
    {
        return Some(invalid);
    }
    invalid_signature_in_func_type_with_names(&func.type_, &mut names)
        .or_else(|| func.body.as_ref().and_then(invalid_signature_in_block))
}

fn invalid_init_signature(func: &ast::FuncDecl<'_>) -> Option<InvalidSignature> {
    let type_params = field_list_binding_count(func.type_.type_params.as_ref());
    let params = field_list_binding_count(Some(&func.type_.params));
    let results = field_list_binding_count(func.type_.results.as_ref());
    (type_params != 0 || params != 0 || results != 0).then_some(InvalidSignature::InitFunction {
        type_params,
        params,
        results,
    })
}

fn invalid_main_signature_in_file(file: &ast::File<'_>) -> Option<InvalidSignature> {
    if file.name.name != "main" {
        return None;
    }
    for decl in &file.decls {
        let ast::Decl::FuncDecl(func) = decl else {
            continue;
        };
        if func.recv.is_none() && func.name.name == "main" {
            let type_params = field_list_binding_count(func.type_.type_params.as_ref());
            let params = field_list_binding_count(Some(&func.type_.params));
            let results = field_list_binding_count(func.type_.results.as_ref());
            if type_params != 0 || params != 0 || results != 0 {
                return Some(InvalidSignature::MainFunction {
                    type_params,
                    params,
                    results,
                });
            }
        }
    }
    None
}

fn invalid_receiver_signature(
    recv: &ast::FieldList<'_>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    let count: usize = recv.list.iter().map(field_binding_count).sum();
    if count != 1 {
        return Some(InvalidSignature::ReceiverCount { count });
    }
    for field in &recv.list {
        if field_type_is_variadic(field) {
            return Some(InvalidSignature::ReceiverVariadic);
        }
        if let Some(invalid) = record_signature_field_names(field, names) {
            return Some(invalid);
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_receiver_type_parameter_names(type_, names)
        {
            return Some(invalid);
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_signature_in_expr(type_)
        {
            return Some(invalid);
        }
    }
    None
}

fn invalid_signature_in_func_type(func_type: &ast::FuncType<'_>) -> Option<InvalidSignature> {
    let mut names = BTreeSet::new();
    invalid_signature_in_func_type_with_names(func_type, &mut names)
}

fn invalid_signature_in_func_type_with_names(
    func_type: &ast::FuncType<'_>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    invalid_signature_in_field_list(&func_type.params, SignatureList::Parameter, true, names)
        .or_else(|| {
            func_type.results.as_ref().and_then(|results| {
                invalid_signature_in_field_list(results, SignatureList::Result, false, names)
            })
        })
}

fn invalid_signature_in_field_list(
    fields: &ast::FieldList<'_>,
    list: SignatureList,
    allow_variadic: bool,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    let mut has_named = false;
    let mut has_unnamed = false;
    for (idx, field) in fields.list.iter().enumerate() {
        if field_has_names(field) {
            has_named = true;
        } else {
            has_unnamed = true;
        }
        if has_named && has_unnamed {
            return Some(InvalidSignature::MixedNamedUnnamed { list });
        }

        if field_type_is_variadic(field) {
            if !allow_variadic {
                return Some(InvalidSignature::VariadicResult);
            }
            if idx + 1 != fields.list.len() || field_binding_count(field) != 1 {
                return Some(InvalidSignature::VariadicNotFinal);
            }
        }

        if let Some(invalid) = record_signature_field_names(field, names) {
            return Some(invalid);
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_signature_in_expr(type_)
        {
            return Some(invalid);
        }
    }
    None
}

fn field_has_names(field: &ast::Field<'_>) -> bool {
    field.names.as_ref().is_some_and(|names| !names.is_empty())
}

fn field_binding_count(field: &ast::Field<'_>) -> usize {
    field.names.as_ref().map_or(1, Vec::len)
}

fn field_type_is_variadic(field: &ast::Field<'_>) -> bool {
    matches!(field.type_, Some(ast::Expr::Ellipsis(_)))
}

fn record_signature_field_names(
    field: &ast::Field<'_>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    let Some(field_names) = &field.names else {
        return None;
    };
    for name in field_names {
        if name.name == "_" {
            continue;
        }
        if !names.insert(name.name.to_string()) {
            return Some(InvalidSignature::DuplicateName {
                name: name.name.to_string(),
            });
        }
    }
    None
}

fn invalid_signature_in_gen_decl(gen_decl: &ast::GenDecl<'_>) -> Option<InvalidSignature> {
    for spec in &gen_decl.specs {
        if let Some(invalid) = invalid_signature_in_spec(spec) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_signature_in_spec(spec: &ast::Spec<'_>) -> Option<InvalidSignature> {
    match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => {
            let mut names = BTreeSet::new();
            invalid_type_parameter_list(type_spec.type_params.as_ref(), &mut names)
                .or_else(|| invalid_signature_in_expr(&type_spec.type_))
        }
        ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_) = &value_spec.type_
                && let Some(invalid) = invalid_signature_in_expr(type_)
            {
                return Some(invalid);
            }
            if let Some(values) = &value_spec.values {
                for value in values {
                    if let Some(invalid) = invalid_signature_in_expr(value) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
    }
}

fn invalid_type_parameter_list(
    type_params: Option<&ast::FieldList<'_>>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    let type_params = type_params?;
    for field in &type_params.list {
        let Some(field_names) = &field.names else {
            return Some(InvalidSignature::InvalidTypeParameterDecl);
        };
        if field_names.is_empty() || field.type_.is_none() {
            return Some(InvalidSignature::InvalidTypeParameterDecl);
        }
        for name in field_names {
            if name.name == "_" {
                continue;
            }
            if !names.insert(name.name.to_string()) {
                return Some(InvalidSignature::DuplicateTypeParameterName {
                    name: name.name.to_string(),
                });
            }
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_signature_in_expr(type_)
        {
            return Some(invalid);
        }
    }
    None
}

fn invalid_receiver_type_parameter_names(
    expr: &ast::Expr<'_>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    match expr {
        ast::Expr::IndexExpr(index) => invalid_receiver_type_parameter_name(&index.index, names),
        ast::Expr::IndexListExpr(index) => index
            .indices
            .iter()
            .find_map(|expr| invalid_receiver_type_parameter_name(expr, names)),
        ast::Expr::ParenExpr(paren) => invalid_receiver_type_parameter_names(&paren.x, names),
        ast::Expr::StarExpr(star) => invalid_receiver_type_parameter_names(&star.x, names),
        _ => None,
    }
}

fn invalid_receiver_type_parameter_name(
    expr: &ast::Expr<'_>,
    names: &mut BTreeSet<String>,
) -> Option<InvalidSignature> {
    let ast::Expr::Ident(ident) = expr else {
        return Some(InvalidSignature::ReceiverTypeParameterNotIdentifier);
    };
    if ident.name == "_" {
        return None;
    }
    if !names.insert(ident.name.to_string()) {
        return Some(InvalidSignature::DuplicateTypeParameterName {
            name: ident.name.to_string(),
        });
    }
    None
}

fn invalid_signature_in_block(block: &ast::BlockStmt<'_>) -> Option<InvalidSignature> {
    invalid_signature_in_stmt_list(&block.list)
}

fn invalid_signature_in_stmt_list(stmts: &[ast::Stmt<'_>]) -> Option<InvalidSignature> {
    for stmt in stmts {
        if let Some(invalid) = invalid_signature_in_stmt(stmt) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_signature_in_stmt(stmt: &ast::Stmt<'_>) -> Option<InvalidSignature> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                if let Some(invalid) = invalid_signature_in_expr(expr) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::BlockStmt(block) => invalid_signature_in_block(block),
        ast::Stmt::BranchStmt(_) => None,
        ast::Stmt::CaseClause(case) => invalid_signature_in_case_clause(case),
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_signature_in_stmt(comm)
            {
                return Some(invalid);
            }
            invalid_signature_in_stmt_list(&comm.body)
        }
        ast::Stmt::DeclStmt(decl) => invalid_signature_in_gen_decl(&decl.decl),
        ast::Stmt::DeferStmt(defer) => invalid_signature_in_call(&defer.call),
        ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::ExprStmt(expr) => invalid_signature_in_expr(&expr.x),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_signature_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_signature_in_expr(cond)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_signature_in_stmt(post)
            {
                return Some(invalid);
            }
            invalid_signature_in_block(&for_stmt.body)
        }
        ast::Stmt::GoStmt(go) => invalid_signature_in_call(&go.call),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_signature_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_signature_in_expr(&if_stmt.cond) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_signature_in_block(&if_stmt.body) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                return invalid_signature_in_stmt(else_branch);
            }
            None
        }
        ast::Stmt::IncDecStmt(inc_dec) => invalid_signature_in_expr(&inc_dec.x),
        ast::Stmt::LabeledStmt(labeled) => invalid_signature_in_stmt(&labeled.stmt),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key
                && let Some(invalid) = invalid_signature_in_expr(key)
            {
                return Some(invalid);
            }
            if let Some(value) = &range.value
                && let Some(invalid) = invalid_signature_in_expr(value)
            {
                return Some(invalid);
            }
            invalid_signature_in_expr(&range.x).or_else(|| invalid_signature_in_block(&range.body))
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_signature_in_expr(expr) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SelectStmt(select) => invalid_signature_in_block(&select.body),
        ast::Stmt::SendStmt(send) => {
            invalid_signature_in_expr(&send.chan).or_else(|| invalid_signature_in_expr(&send.value))
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_signature_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_signature_in_expr(tag)
            {
                return Some(invalid);
            }
            invalid_signature_in_block(&switch.body)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_signature_in_stmt(init)
            {
                return Some(invalid);
            }
            invalid_signature_in_stmt(&type_switch.assign)
                .or_else(|| invalid_signature_in_block(&type_switch.body))
        }
    }
}

fn invalid_signature_in_case_clause(case: &ast::CaseClause<'_>) -> Option<InvalidSignature> {
    if let Some(list) = &case.list {
        for expr in list {
            if let Some(invalid) = invalid_signature_in_expr(expr) {
                return Some(invalid);
            }
        }
    }
    invalid_signature_in_stmt_list(&case.body)
}

fn invalid_signature_in_call(call: &ast::CallExpr<'_>) -> Option<InvalidSignature> {
    if let Some(invalid) = invalid_signature_in_expr(&call.fun) {
        return Some(invalid);
    }
    if let Some(args) = &call.args {
        for arg in args {
            if let Some(invalid) = invalid_signature_in_expr(arg) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_signature_in_expr(expr: &ast::Expr<'_>) -> Option<InvalidSignature> {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len
                && let Some(invalid) = invalid_signature_in_expr(len)
            {
                return Some(invalid);
            }
            invalid_signature_in_expr(&array.elt)
        }
        ast::Expr::BinaryExpr(binary) => {
            invalid_signature_in_expr(&binary.x).or_else(|| invalid_signature_in_expr(&binary.y))
        }
        ast::Expr::CallExpr(call) => invalid_signature_in_call(call),
        ast::Expr::ChanType(chan) => invalid_signature_in_expr(&chan.value),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_
                && let Some(invalid) = invalid_signature_in_expr(type_)
            {
                return Some(invalid);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    if let Some(invalid) = invalid_signature_in_expr(elt) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|expr| invalid_signature_in_expr(expr)),
        ast::Expr::FuncLit(func_lit) => invalid_signature_in_func_type(&func_lit.type_)
            .or_else(|| invalid_signature_in_block(&func_lit.body)),
        ast::Expr::FuncType(func_type) => invalid_signature_in_func_type(func_type),
        ast::Expr::IndexExpr(index) => {
            invalid_signature_in_expr(&index.x).or_else(|| invalid_signature_in_expr(&index.index))
        }
        ast::Expr::IndexListExpr(index) => {
            if let Some(invalid) = invalid_signature_in_expr(&index.x) {
                return Some(invalid);
            }
            for index in &index.indices {
                if let Some(invalid) = invalid_signature_in_expr(index) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Expr::InterfaceType(interface) => invalid_signature_in_interface_type(interface),
        ast::Expr::KeyValueExpr(kv) => {
            invalid_signature_in_expr(&kv.key).or_else(|| invalid_signature_in_expr(&kv.value))
        }
        ast::Expr::MapType(map) => {
            invalid_signature_in_expr(&map.key).or_else(|| invalid_signature_in_expr(&map.value))
        }
        ast::Expr::ParenExpr(paren) => invalid_signature_in_expr(&paren.x),
        ast::Expr::SelectorExpr(selector) => invalid_signature_in_expr(&selector.x),
        ast::Expr::SliceExpr(slice) => {
            if let Some(invalid) = invalid_signature_in_expr(&slice.x) {
                return Some(invalid);
            }
            if let Some(low) = &slice.low
                && let Some(invalid) = invalid_signature_in_expr(low)
            {
                return Some(invalid);
            }
            if let Some(high) = &slice.high
                && let Some(invalid) = invalid_signature_in_expr(high)
            {
                return Some(invalid);
            }
            if let Some(max) = &slice.max
                && let Some(invalid) = invalid_signature_in_expr(max)
            {
                return Some(invalid);
            }
            None
        }
        ast::Expr::StarExpr(star) => invalid_signature_in_expr(&star.x),
        ast::Expr::StructType(struct_type) => struct_type.fields.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_signature_in_expr(type_)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::TypeAssertExpr(assert) => {
            if let Some(invalid) = invalid_signature_in_expr(&assert.x) {
                return Some(invalid);
            }
            assert
                .type_
                .as_ref()
                .and_then(|ty| invalid_signature_in_expr(ty))
        }
        ast::Expr::UnaryExpr(unary) => invalid_signature_in_expr(&unary.x),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_signature_in_interface_type(
    interface: &ast::InterfaceType<'_>,
) -> Option<InvalidSignature> {
    let fields = interface.methods.as_ref()?;
    let mut names = BTreeSet::new();
    for field in &fields.list {
        if let Some(field_names) = &field.names {
            for name in field_names {
                if !names.insert(name.name.to_string()) {
                    return Some(InvalidSignature::DuplicateInterfaceMethod {
                        name: name.name.to_string(),
                    });
                }
            }
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_signature_in_expr(type_)
        {
            return Some(invalid);
        }
    }
    None
}

fn invalid_method_names_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    let struct_fields = top_level_struct_fields(file);
    let mut methods_by_base = BTreeMap::<String, BTreeSet<String>>::new();

    for decl in &file.decls {
        let ast::Decl::FuncDecl(func) = decl else {
            continue;
        };
        if func.name.name == "_" {
            continue;
        }
        let Some(recv) = &func.recv else {
            continue;
        };
        let Some(base) = receiver_base_type_name(recv) else {
            continue;
        };
        let method = func.name.name.to_string();
        let methods = methods_by_base.entry(base.clone()).or_default();
        if !methods.insert(method.clone()) {
            return Some(InvalidDeclaration::DuplicateMethod { base, method });
        }
        if struct_fields
            .get(&base)
            .is_some_and(|fields| fields.contains(&method))
        {
            return Some(InvalidDeclaration::MethodFieldConflict { base, name: method });
        }
    }

    None
}

fn top_level_struct_fields(file: &ast::File<'_>) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for decl in &file.decls {
        let ast::Decl::GenDecl(gen_decl) = decl else {
            continue;
        };
        for spec in &gen_decl.specs {
            let ast::Spec::TypeSpec(type_spec) = spec else {
                continue;
            };
            let Some(name) = &type_spec.name else {
                continue;
            };
            let Some(struct_type) = struct_type_from_expr(&type_spec.type_) else {
                continue;
            };
            out.insert(name.name.to_string(), struct_field_name_set(struct_type));
        }
    }
    out
}

fn struct_type_from_expr<'src>(expr: &'src ast::Expr<'src>) -> Option<&'src ast::StructType<'src>> {
    match expr {
        ast::Expr::ParenExpr(paren) => struct_type_from_expr(&paren.x),
        ast::Expr::StructType(struct_type) => Some(struct_type),
        _ => None,
    }
}

fn struct_field_name_set(struct_type: &ast::StructType<'_>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(fields) = &struct_type.fields {
        for field in &fields.list {
            for name in struct_field_names(field) {
                if name != "_" {
                    out.insert(name);
                }
            }
        }
    }
    out
}

fn receiver_base_type_name(recv: &ast::FieldList<'_>) -> Option<String> {
    let field = recv.list.first()?;
    receiver_type_base_name(field.type_.as_ref()?)
}

fn receiver_type_parameter_count(recv: &ast::FieldList<'_>) -> usize {
    recv.list
        .first()
        .and_then(|field| field.type_.as_ref())
        .map(receiver_type_expr_parameter_count)
        .unwrap_or(0)
}

fn receiver_type_expr_parameter_count(expr: &ast::Expr<'_>) -> usize {
    match expr {
        ast::Expr::IndexExpr(_) => 1,
        ast::Expr::IndexListExpr(index) => index.indices.len(),
        ast::Expr::ParenExpr(paren) => receiver_type_expr_parameter_count(&paren.x),
        ast::Expr::StarExpr(star) => receiver_type_expr_parameter_count(&star.x),
        _ => 0,
    }
}

fn receiver_type_base_name(expr: &ast::Expr<'_>) -> Option<String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::IndexExpr(index) => receiver_type_base_name(&index.x),
        ast::Expr::IndexListExpr(index) => receiver_type_base_name(&index.x),
        ast::Expr::ParenExpr(paren) => receiver_type_base_name(&paren.x),
        ast::Expr::StarExpr(star) => receiver_type_base_name(&star.x),
        _ => None,
    }
}

fn invalid_declaration_in_decl(decl: &ast::Decl<'_>) -> Option<InvalidDeclaration> {
    match decl {
        ast::Decl::FuncDecl(func) => func.body.as_ref().and_then(invalid_declaration_in_block),
        ast::Decl::GenDecl(gen_decl) => invalid_declaration_in_gen_decl(gen_decl),
    }
}

fn invalid_declaration_in_gen_decl(gen_decl: &ast::GenDecl<'_>) -> Option<InvalidDeclaration> {
    if let Some(invalid) = invalid_gen_decl_names(gen_decl) {
        return Some(invalid);
    }
    for spec in &gen_decl.specs {
        if let Some(invalid) = invalid_declaration_in_spec(spec) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_gen_decl_names(gen_decl: &ast::GenDecl<'_>) -> Option<InvalidDeclaration> {
    if gen_decl.tok == token::Token::IMPORT {
        return None;
    }
    let mut names = BTreeSet::new();
    for spec in &gen_decl.specs {
        for name in spec_declared_names(spec) {
            if name == "_" {
                continue;
            }
            if !names.insert(name.clone()) {
                return Some(InvalidDeclaration::DuplicateDeclarationName { name });
            }
        }
    }
    None
}

fn spec_declared_names(spec: &ast::Spec<'_>) -> Vec<String> {
    match spec {
        ast::Spec::ImportSpec(_) => Vec::new(),
        ast::Spec::TypeSpec(type_spec) => type_spec
            .name
            .as_ref()
            .map(|name| vec![name.name.to_string()])
            .unwrap_or_default(),
        ast::Spec::ValueSpec(value_spec) => value_spec
            .names
            .iter()
            .map(|name| name.name.to_string())
            .collect(),
    }
}

fn invalid_declaration_in_spec(spec: &ast::Spec<'_>) -> Option<InvalidDeclaration> {
    match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => invalid_declaration_in_expr_with_struct_name(
            &type_spec.type_,
            type_spec.name.as_ref().map(|name| name.name),
        ),
        ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_) = &value_spec.type_
                && let Some(invalid) = invalid_declaration_in_expr(type_)
            {
                return Some(invalid);
            }
            if let Some(values) = &value_spec.values {
                for value in values {
                    if let Some(invalid) = invalid_declaration_in_expr(value) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
    }
}

fn invalid_declaration_in_block(block: &ast::BlockStmt<'_>) -> Option<InvalidDeclaration> {
    for stmt in &block.list {
        if let Some(invalid) = invalid_declaration_in_stmt(stmt) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_declaration_in_stmt(stmt: &ast::Stmt<'_>) -> Option<InvalidDeclaration> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                if let Some(invalid) = invalid_declaration_in_expr(expr) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::BlockStmt(block) => invalid_declaration_in_block(block),
        ast::Stmt::BranchStmt(_) => None,
        ast::Stmt::CaseClause(case) => invalid_declaration_in_case_clause(case),
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_declaration_in_stmt(comm)
            {
                return Some(invalid);
            }
            invalid_declaration_in_stmt_list(&comm.body)
        }
        ast::Stmt::DeclStmt(decl) => invalid_declaration_in_gen_decl(&decl.decl),
        ast::Stmt::DeferStmt(defer) => invalid_declaration_in_call(&defer.call),
        ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::ExprStmt(expr) => invalid_declaration_in_expr(&expr.x),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_declaration_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_declaration_in_expr(cond)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_declaration_in_stmt(post)
            {
                return Some(invalid);
            }
            invalid_declaration_in_block(&for_stmt.body)
        }
        ast::Stmt::GoStmt(go) => invalid_declaration_in_call(&go.call),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_declaration_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_declaration_in_expr(&if_stmt.cond) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_declaration_in_block(&if_stmt.body) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                return invalid_declaration_in_stmt(else_branch);
            }
            None
        }
        ast::Stmt::IncDecStmt(inc_dec) => invalid_declaration_in_expr(&inc_dec.x),
        ast::Stmt::LabeledStmt(labeled) => invalid_declaration_in_stmt(&labeled.stmt),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key
                && let Some(invalid) = invalid_declaration_in_expr(key)
            {
                return Some(invalid);
            }
            if let Some(value) = &range.value
                && let Some(invalid) = invalid_declaration_in_expr(value)
            {
                return Some(invalid);
            }
            invalid_declaration_in_expr(&range.x)
                .or_else(|| invalid_declaration_in_block(&range.body))
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_declaration_in_expr(expr) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SelectStmt(select) => invalid_declaration_in_block(&select.body),
        ast::Stmt::SendStmt(send) => invalid_declaration_in_expr(&send.chan)
            .or_else(|| invalid_declaration_in_expr(&send.value)),
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_declaration_in_stmt(init)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_declaration_in_expr(tag)
            {
                return Some(invalid);
            }
            invalid_declaration_in_block(&switch.body)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_declaration_in_stmt(init)
            {
                return Some(invalid);
            }
            invalid_declaration_in_stmt(&type_switch.assign)
                .or_else(|| invalid_declaration_in_block(&type_switch.body))
        }
    }
}

fn invalid_declaration_in_stmt_list(stmts: &[ast::Stmt<'_>]) -> Option<InvalidDeclaration> {
    for stmt in stmts {
        if let Some(invalid) = invalid_declaration_in_stmt(stmt) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_declaration_in_case_clause(case: &ast::CaseClause<'_>) -> Option<InvalidDeclaration> {
    if let Some(list) = &case.list {
        for expr in list {
            if let Some(invalid) = invalid_declaration_in_expr(expr) {
                return Some(invalid);
            }
        }
    }
    invalid_declaration_in_stmt_list(&case.body)
}

fn invalid_declaration_in_call(call: &ast::CallExpr<'_>) -> Option<InvalidDeclaration> {
    if let Some(invalid) = invalid_declaration_in_expr(&call.fun) {
        return Some(invalid);
    }
    if let Some(args) = &call.args {
        for arg in args {
            if let Some(invalid) = invalid_declaration_in_expr(arg) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_declaration_in_expr(expr: &ast::Expr<'_>) -> Option<InvalidDeclaration> {
    invalid_declaration_in_expr_with_struct_name(expr, None)
}

fn invalid_declaration_in_expr_with_struct_name(
    expr: &ast::Expr<'_>,
    struct_name: Option<&str>,
) -> Option<InvalidDeclaration> {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len
                && let Some(invalid) = invalid_declaration_in_expr(len)
            {
                return Some(invalid);
            }
            invalid_declaration_in_expr(&array.elt)
        }
        ast::Expr::BinaryExpr(binary) => invalid_declaration_in_expr(&binary.x)
            .or_else(|| invalid_declaration_in_expr(&binary.y)),
        ast::Expr::CallExpr(call) => invalid_declaration_in_call(call),
        ast::Expr::ChanType(chan) => invalid_declaration_in_expr(&chan.value),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_
                && let Some(invalid) = invalid_declaration_in_expr(type_)
            {
                return Some(invalid);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    if let Some(invalid) = invalid_declaration_in_expr(elt) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|expr| invalid_declaration_in_expr(expr)),
        ast::Expr::FuncLit(func_lit) => invalid_declaration_in_block(&func_lit.body),
        ast::Expr::FuncType(func_type) => {
            for field in &func_type.params.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_declaration_in_expr(type_)
                {
                    return Some(invalid);
                }
            }
            if let Some(results) = &func_type.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_
                        && let Some(invalid) = invalid_declaration_in_expr(type_)
                    {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::IndexExpr(index) => invalid_declaration_in_expr(&index.x)
            .or_else(|| invalid_declaration_in_expr(&index.index)),
        ast::Expr::IndexListExpr(index) => {
            if let Some(invalid) = invalid_declaration_in_expr(&index.x) {
                return Some(invalid);
            }
            for index in &index.indices {
                if let Some(invalid) = invalid_declaration_in_expr(index) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Expr::InterfaceType(interface) => interface.methods.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_declaration_in_expr(type_)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::KeyValueExpr(kv) => {
            invalid_declaration_in_expr(&kv.key).or_else(|| invalid_declaration_in_expr(&kv.value))
        }
        ast::Expr::MapType(map) => invalid_declaration_in_expr(&map.key)
            .or_else(|| invalid_declaration_in_expr(&map.value)),
        ast::Expr::ParenExpr(paren) => {
            invalid_declaration_in_expr_with_struct_name(&paren.x, struct_name)
        }
        ast::Expr::SelectorExpr(selector) => invalid_declaration_in_expr(&selector.x),
        ast::Expr::SliceExpr(slice) => {
            if let Some(invalid) = invalid_declaration_in_expr(&slice.x) {
                return Some(invalid);
            }
            if let Some(low) = &slice.low
                && let Some(invalid) = invalid_declaration_in_expr(low)
            {
                return Some(invalid);
            }
            if let Some(high) = &slice.high
                && let Some(invalid) = invalid_declaration_in_expr(high)
            {
                return Some(invalid);
            }
            if let Some(max) = &slice.max
                && let Some(invalid) = invalid_declaration_in_expr(max)
            {
                return Some(invalid);
            }
            None
        }
        ast::Expr::StarExpr(star) => invalid_declaration_in_expr(&star.x),
        ast::Expr::StructType(struct_type) => invalid_struct_declaration(struct_type, struct_name),
        ast::Expr::TypeAssertExpr(assert) => {
            if let Some(invalid) = invalid_declaration_in_expr(&assert.x) {
                return Some(invalid);
            }
            assert
                .type_
                .as_ref()
                .and_then(|ty| invalid_declaration_in_expr(ty))
        }
        ast::Expr::UnaryExpr(unary) => invalid_declaration_in_expr(&unary.x),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_struct_declaration(
    struct_type: &ast::StructType<'_>,
    type_name: Option<&str>,
) -> Option<InvalidDeclaration> {
    let Some(fields) = &struct_type.fields else {
        return None;
    };
    let mut seen = BTreeSet::new();
    for field in &fields.list {
        for name in struct_field_names(field) {
            if name == "_" {
                continue;
            }
            if !seen.insert(name.clone()) {
                return Some(InvalidDeclaration::DuplicateStructField {
                    type_name: type_name.map(str::to_string),
                    field: name,
                });
            }
        }
        if let Some(type_) = &field.type_
            && let Some(invalid) = invalid_declaration_in_expr(type_)
        {
            return Some(invalid);
        }
    }
    None
}

fn struct_field_names(field: &ast::Field<'_>) -> Vec<String> {
    if let Some(names) = &field.names {
        return names.iter().map(|name| name.name.to_string()).collect();
    }
    field
        .type_
        .as_ref()
        .and_then(embedded_field_name)
        .into_iter()
        .collect()
}

fn embedded_field_name(expr: &ast::Expr<'_>) -> Option<String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::IndexExpr(index) => embedded_field_name(&index.x),
        ast::Expr::IndexListExpr(index) => embedded_field_name(&index.x),
        ast::Expr::ParenExpr(paren) => embedded_field_name(&paren.x),
        ast::Expr::SelectorExpr(selector) => Some(selector.sel.name.to_string()),
        ast::Expr::StarExpr(star) => embedded_field_name(&star.x),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct DeclarationScopes {
    scopes: Vec<BTreeSet<String>>,
}

impl DeclarationScopes {
    fn new() -> Self {
        Self {
            scopes: vec![BTreeSet::new()],
        }
    }

    fn contains_current(&self, name: &str) -> bool {
        self.scopes.last().is_some_and(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) -> Option<InvalidDeclaration> {
        if name == "_" {
            return None;
        }
        let scope = self.scopes.last_mut()?;
        if !scope.insert(name.to_string()) {
            return Some(InvalidDeclaration::DuplicateLexicalName {
                name: name.to_string(),
            });
        }
        None
    }

    fn declare_if_new(&mut self, name: &str) {
        if name != "_"
            && !self.contains_current(name)
            && let Some(scope) = self.scopes.last_mut()
        {
            scope.insert(name.to_string());
        }
    }

    fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(BTreeSet::new());
        let out = f(self);
        self.scopes.pop();
        out
    }
}

fn invalid_local_declaration_names_in_file(file: &ast::File<'_>) -> Option<InvalidDeclaration> {
    for decl in &file.decls {
        let ast::Decl::FuncDecl(func) = decl else {
            continue;
        };
        if let Some(invalid) = invalid_local_declaration_names_in_func(func) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_local_declaration_names_in_func(func: &ast::FuncDecl<'_>) -> Option<InvalidDeclaration> {
    let body = func.body.as_ref()?;
    let mut scopes = DeclarationScopes::new();
    if let Some(recv) = &func.recv {
        seed_decl_scope_field_names(recv, &mut scopes);
        seed_decl_scope_receiver_type_parameter_names(recv, &mut scopes);
    }
    seed_decl_scope_type_parameter_names(func.type_.type_params.as_ref(), &mut scopes);
    seed_decl_scope_field_names(&func.type_.params, &mut scopes);
    if let Some(results) = &func.type_.results {
        seed_decl_scope_field_names(results, &mut scopes);
    }
    invalid_local_declaration_names_in_stmt_list(&body.list, &mut scopes)
}

fn seed_decl_scope_field_names(fields: &ast::FieldList<'_>, scopes: &mut DeclarationScopes) {
    for field in &fields.list {
        if let Some(names) = &field.names {
            for name in names {
                scopes.declare_if_new(name.name);
            }
        }
    }
}

fn seed_decl_scope_type_parameter_names(
    type_params: Option<&ast::FieldList<'_>>,
    scopes: &mut DeclarationScopes,
) {
    let Some(type_params) = type_params else {
        return;
    };
    seed_decl_scope_field_names(type_params, scopes);
}

fn seed_decl_scope_receiver_type_parameter_names(
    recv: &ast::FieldList<'_>,
    scopes: &mut DeclarationScopes,
) {
    for field in &recv.list {
        if let Some(type_) = &field.type_ {
            seed_decl_scope_receiver_type_parameter_names_in_expr(type_, scopes);
        }
    }
}

fn seed_decl_scope_receiver_type_parameter_names_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut DeclarationScopes,
) {
    match expr {
        ast::Expr::IndexExpr(index) => {
            if let ast::Expr::Ident(ident) = index.index.as_ref() {
                scopes.declare_if_new(ident.name);
            }
        }
        ast::Expr::IndexListExpr(index) => {
            for expr in &index.indices {
                if let ast::Expr::Ident(ident) = expr {
                    scopes.declare_if_new(ident.name);
                }
            }
        }
        ast::Expr::ParenExpr(paren) => {
            seed_decl_scope_receiver_type_parameter_names_in_expr(&paren.x, scopes);
        }
        ast::Expr::StarExpr(star) => {
            seed_decl_scope_receiver_type_parameter_names_in_expr(&star.x, scopes);
        }
        _ => {}
    }
}

fn invalid_local_declaration_names_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    for stmt in stmts {
        if let Some(invalid) = invalid_local_declaration_names_in_stmt(stmt, scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_local_declaration_names_in_nested_block(
    block: &ast::BlockStmt<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    scopes.with_scope(|scopes| invalid_local_declaration_names_in_stmt_list(&block.list, scopes))
}

fn invalid_local_declaration_names_in_stmt(
    stmt: &ast::Stmt<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => invalid_local_declaration_names_in_assign(assign, scopes),
        ast::Stmt::BlockStmt(block) => {
            invalid_local_declaration_names_in_nested_block(block, scopes)
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::CaseClause(case) => scopes.with_scope(|scopes| {
            if let Some(list) = &case.list {
                for expr in list {
                    if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes) {
                        return Some(invalid);
                    }
                }
            }
            invalid_local_declaration_names_in_stmt_list(&case.body, scopes)
        }),
        ast::Stmt::CommClause(comm) => scopes.with_scope(|scopes| {
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(comm, scopes)
            {
                return Some(invalid);
            }
            invalid_local_declaration_names_in_stmt_list(&comm.body, scopes)
        }),
        ast::Stmt::DeclStmt(decl) => {
            invalid_local_declaration_names_in_gen_decl(&decl.decl, scopes)
        }
        ast::Stmt::DeferStmt(defer) => invalid_local_declaration_names_in_call(&defer.call, scopes),
        ast::Stmt::ExprStmt(expr) => invalid_local_declaration_names_in_expr(&expr.x, scopes),
        ast::Stmt::ForStmt(for_stmt) => scopes.with_scope(|scopes| {
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_local_declaration_names_in_expr(cond, scopes)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(post, scopes)
            {
                return Some(invalid);
            }
            invalid_local_declaration_names_in_nested_block(&for_stmt.body, scopes)
        }),
        ast::Stmt::GoStmt(go) => invalid_local_declaration_names_in_call(&go.call, scopes),
        ast::Stmt::IfStmt(if_stmt) => scopes.with_scope(|scopes| {
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_local_declaration_names_in_expr(&if_stmt.cond, scopes) {
                return Some(invalid);
            }
            if let Some(invalid) =
                invalid_local_declaration_names_in_nested_block(&if_stmt.body, scopes)
            {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                return invalid_local_declaration_names_in_stmt(else_branch, scopes);
            }
            None
        }),
        ast::Stmt::IncDecStmt(inc_dec) => {
            invalid_local_declaration_names_in_expr(&inc_dec.x, scopes)
        }
        ast::Stmt::LabeledStmt(labeled) => {
            invalid_local_declaration_names_in_stmt(&labeled.stmt, scopes)
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(invalid) = invalid_local_declaration_names_in_expr(&range.x, scopes) {
                return Some(invalid);
            }
            scopes.with_scope(|scopes| {
                if range.tok == Some(token::Token::DEFINE) {
                    for expr in [&range.key, &range.value].into_iter().flatten() {
                        if let Some(name) = ident_name(expr) {
                            scopes.declare_if_new(&name);
                        } else if let Some(invalid) =
                            invalid_local_declaration_names_in_expr(expr, scopes)
                        {
                            return Some(invalid);
                        }
                    }
                } else {
                    for expr in [&range.key, &range.value].into_iter().flatten() {
                        if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes)
                        {
                            return Some(invalid);
                        }
                    }
                }
                invalid_local_declaration_names_in_nested_block(&range.body, scopes)
            })
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SelectStmt(select) => {
            invalid_local_declaration_names_in_nested_block(&select.body, scopes)
        }
        ast::Stmt::SendStmt(send) => invalid_local_declaration_names_in_expr(&send.chan, scopes)
            .or_else(|| invalid_local_declaration_names_in_expr(&send.value, scopes)),
        ast::Stmt::SwitchStmt(switch) => scopes.with_scope(|scopes| {
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_local_declaration_names_in_expr(tag, scopes)
            {
                return Some(invalid);
            }
            invalid_local_declaration_names_in_nested_block(&switch.body, scopes)
        }),
        ast::Stmt::TypeSwitchStmt(type_switch) => scopes.with_scope(|scopes| {
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_local_declaration_names_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(invalid) =
                invalid_local_declaration_names_in_stmt(&type_switch.assign, scopes)
            {
                return Some(invalid);
            }
            invalid_local_declaration_names_in_nested_block(&type_switch.body, scopes)
        }),
    }
}

fn invalid_local_declaration_names_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    for spec in &gen_decl.specs {
        match spec {
            ast::Spec::ImportSpec(_) => {}
            ast::Spec::TypeSpec(type_spec) => {
                if let Some(invalid) =
                    invalid_local_declaration_names_in_expr(&type_spec.type_, scopes)
                {
                    return Some(invalid);
                }
                if let Some(name) = &type_spec.name
                    && let Some(invalid) = scopes.declare(name.name)
                {
                    return Some(invalid);
                }
            }
            ast::Spec::ValueSpec(value_spec) => {
                if let Some(type_) = &value_spec.type_
                    && let Some(invalid) = invalid_local_declaration_names_in_expr(type_, scopes)
                {
                    return Some(invalid);
                }
                if let Some(values) = &value_spec.values {
                    for value in values {
                        if let Some(invalid) =
                            invalid_local_declaration_names_in_expr(value, scopes)
                        {
                            return Some(invalid);
                        }
                    }
                }
                for name in &value_spec.names {
                    if let Some(invalid) = scopes.declare(name.name) {
                        return Some(invalid);
                    }
                }
            }
        }
    }
    None
}

fn invalid_local_declaration_names_in_assign(
    assign: &ast::AssignStmt<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    for expr in &assign.rhs {
        if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes) {
            return Some(invalid);
        }
    }
    if assign.tok == token::Token::DEFINE {
        for expr in &assign.lhs {
            if let Some(name) = ident_name(expr) {
                scopes.declare_if_new(&name);
            } else if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes) {
                return Some(invalid);
            }
        }
    } else {
        for expr in &assign.lhs {
            if let Some(invalid) = invalid_local_declaration_names_in_expr(expr, scopes) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_local_declaration_names_in_call(
    call: &ast::CallExpr<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    invalid_local_declaration_names_in_expr(&call.fun, scopes).or_else(|| {
        call.args.as_ref().and_then(|args| {
            args.iter()
                .find_map(|arg| invalid_local_declaration_names_in_expr(arg, scopes))
        })
    })
}

fn invalid_local_declaration_names_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    match expr {
        ast::Expr::ArrayType(array) => array
            .len
            .as_ref()
            .and_then(|len| invalid_local_declaration_names_in_expr(len, scopes))
            .or_else(|| invalid_local_declaration_names_in_expr(&array.elt, scopes)),
        ast::Expr::BinaryExpr(binary) => invalid_local_declaration_names_in_expr(&binary.x, scopes)
            .or_else(|| invalid_local_declaration_names_in_expr(&binary.y, scopes)),
        ast::Expr::CallExpr(call) => invalid_local_declaration_names_in_call(call, scopes),
        ast::Expr::ChanType(chan) => invalid_local_declaration_names_in_expr(&chan.value, scopes),
        ast::Expr::CompositeLit(comp) => comp
            .type_
            .as_ref()
            .and_then(|type_| invalid_local_declaration_names_in_expr(type_, scopes))
            .or_else(|| {
                comp.elts.as_ref().and_then(|elts| {
                    elts.iter()
                        .find_map(|elt| invalid_local_declaration_names_in_expr(elt, scopes))
                })
            }),
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|elt| invalid_local_declaration_names_in_expr(elt, scopes)),
        ast::Expr::FuncLit(func_lit) => {
            invalid_local_declaration_names_in_func_lit(func_lit, scopes)
        }
        ast::Expr::FuncType(func_type) => {
            invalid_local_declaration_names_in_field_list(&func_type.params, scopes).or_else(|| {
                func_type.results.as_ref().and_then(|results| {
                    invalid_local_declaration_names_in_field_list(results, scopes)
                })
            })
        }
        ast::Expr::IndexExpr(index) => invalid_local_declaration_names_in_expr(&index.x, scopes)
            .or_else(|| invalid_local_declaration_names_in_expr(&index.index, scopes)),
        ast::Expr::IndexListExpr(index) => {
            invalid_local_declaration_names_in_expr(&index.x, scopes).or_else(|| {
                index
                    .indices
                    .iter()
                    .find_map(|index| invalid_local_declaration_names_in_expr(index, scopes))
            })
        }
        ast::Expr::InterfaceType(interface) => interface
            .methods
            .as_ref()
            .and_then(|fields| invalid_local_declaration_names_in_field_list(fields, scopes)),
        ast::Expr::KeyValueExpr(kv) => invalid_local_declaration_names_in_expr(&kv.key, scopes)
            .or_else(|| invalid_local_declaration_names_in_expr(&kv.value, scopes)),
        ast::Expr::MapType(map) => invalid_local_declaration_names_in_expr(&map.key, scopes)
            .or_else(|| invalid_local_declaration_names_in_expr(&map.value, scopes)),
        ast::Expr::ParenExpr(paren) => invalid_local_declaration_names_in_expr(&paren.x, scopes),
        ast::Expr::SelectorExpr(selector) => {
            invalid_local_declaration_names_in_expr(&selector.x, scopes)
        }
        ast::Expr::SliceExpr(slice) => invalid_local_declaration_names_in_expr(&slice.x, scopes)
            .or_else(|| {
                slice
                    .low
                    .as_ref()
                    .and_then(|low| invalid_local_declaration_names_in_expr(low, scopes))
            })
            .or_else(|| {
                slice
                    .high
                    .as_ref()
                    .and_then(|high| invalid_local_declaration_names_in_expr(high, scopes))
            })
            .or_else(|| {
                slice
                    .max
                    .as_ref()
                    .and_then(|max| invalid_local_declaration_names_in_expr(max, scopes))
            }),
        ast::Expr::StarExpr(star) => invalid_local_declaration_names_in_expr(&star.x, scopes),
        ast::Expr::StructType(struct_type) => struct_type
            .fields
            .as_ref()
            .and_then(|fields| invalid_local_declaration_names_in_field_list(fields, scopes)),
        ast::Expr::TypeAssertExpr(assert) => {
            invalid_local_declaration_names_in_expr(&assert.x, scopes).or_else(|| {
                assert
                    .type_
                    .as_ref()
                    .and_then(|type_| invalid_local_declaration_names_in_expr(type_, scopes))
            })
        }
        ast::Expr::UnaryExpr(unary) => invalid_local_declaration_names_in_expr(&unary.x, scopes),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_local_declaration_names_in_field_list(
    fields: &ast::FieldList<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    fields.list.iter().find_map(|field| {
        field
            .type_
            .as_ref()
            .and_then(|type_| invalid_local_declaration_names_in_expr(type_, scopes))
    })
}

fn invalid_local_declaration_names_in_func_lit(
    func_lit: &ast::FuncLit<'_>,
    scopes: &mut DeclarationScopes,
) -> Option<InvalidDeclaration> {
    scopes.with_scope(|scopes| {
        seed_decl_scope_field_names(&func_lit.type_.params, scopes);
        if let Some(results) = &func_lit.type_.results {
            seed_decl_scope_field_names(results, scopes);
        }
        invalid_local_declaration_names_in_stmt_list(&func_lit.body.list, scopes)
    })
}

#[derive(Debug, Clone, Default)]
struct TypeParameterScopes {
    scopes: Vec<BTreeSet<String>>,
}

impl TypeParameterScopes {
    fn new() -> Self {
        Self {
            scopes: vec![BTreeSet::new()],
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if name == "_" {
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn declare_all(&mut self, names: &BTreeSet<String>) {
        for name in names {
            self.declare(name);
        }
    }

    fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(BTreeSet::new());
        let out = f(self);
        self.scopes.pop();
        out
    }
}

fn invalid_type_parameter_type_declarations_in_file(
    file: &ast::File<'_>,
) -> Option<InvalidDeclaration> {
    let mut scopes = TypeParameterScopes::new();
    for decl in &file.decls {
        if let Some(invalid) = invalid_type_parameter_type_declaration_in_decl(decl, &mut scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_type_parameter_type_declaration_in_decl(
    decl: &ast::Decl<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    match decl {
        ast::Decl::FuncDecl(func) => {
            let body = func.body.as_ref()?;
            scopes.with_scope(|scopes| {
                let names = type_parameter_names(func.type_.type_params.as_ref());
                scopes.declare_all(&names);
                if let Some(recv) = &func.recv {
                    let names = receiver_type_parameter_names(recv);
                    scopes.declare_all(&names);
                }
                invalid_type_parameter_type_declaration_in_stmt_list(&body.list, scopes)
            })
        }
        ast::Decl::GenDecl(gen_decl) => {
            invalid_type_parameter_type_declaration_in_gen_decl(gen_decl, scopes)
        }
    }
}

fn invalid_type_parameter_type_declaration_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    for spec in &gen_decl.specs {
        let ast::Spec::TypeSpec(type_spec) = spec else {
            continue;
        };
        if let Some(invalid) =
            invalid_type_parameter_type_declaration_in_type_spec(type_spec, scopes)
        {
            return Some(invalid);
        }
    }
    None
}

fn invalid_type_parameter_type_declaration_in_type_spec(
    type_spec: &ast::TypeSpec<'_>,
    scopes: &TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    let rhs = single_ident_type_name(&type_spec.type_)?;
    let own = type_parameter_names(type_spec.type_params.as_ref());
    if type_spec.assign.is_some() {
        own.contains(&rhs)
            .then_some(InvalidDeclaration::AliasToOwnTypeParameter { name: rhs })
    } else if own.contains(&rhs) || scopes.contains(&rhs) {
        Some(InvalidDeclaration::TypeDefinitionFromTypeParameter { name: rhs })
    } else {
        None
    }
}

fn invalid_type_parameter_type_declaration_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    for stmt in stmts {
        if let Some(invalid) = invalid_type_parameter_type_declaration_in_stmt(stmt, scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_type_parameter_type_declaration_in_block(
    block: &ast::BlockStmt<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    invalid_type_parameter_type_declaration_in_stmt_list(&block.list, scopes)
}

fn invalid_type_parameter_type_declaration_in_stmt(
    stmt: &ast::Stmt<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => assign
            .rhs
            .iter()
            .find_map(|expr| invalid_type_parameter_type_declaration_in_expr(expr, scopes)),
        ast::Stmt::BlockStmt(block) => {
            invalid_type_parameter_type_declaration_in_block(block, scopes)
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::CaseClause(case) => {
            invalid_type_parameter_type_declaration_in_stmt_list(&case.body, scopes)
        }
        ast::Stmt::CommClause(comm) => comm
            .comm
            .as_ref()
            .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
            .or_else(|| invalid_type_parameter_type_declaration_in_stmt_list(&comm.body, scopes)),
        ast::Stmt::DeclStmt(decl) => {
            invalid_type_parameter_type_declaration_in_gen_decl(&decl.decl, scopes)
        }
        ast::Stmt::DeferStmt(defer) => {
            invalid_type_parameter_type_declaration_in_call(&defer.call, scopes)
        }
        ast::Stmt::ExprStmt(expr) => {
            invalid_type_parameter_type_declaration_in_expr(&expr.x, scopes)
        }
        ast::Stmt::ForStmt(for_stmt) => for_stmt
            .init
            .as_ref()
            .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
            .or_else(|| {
                for_stmt
                    .cond
                    .as_ref()
                    .and_then(|expr| invalid_type_parameter_type_declaration_in_expr(expr, scopes))
            })
            .or_else(|| {
                for_stmt
                    .post
                    .as_ref()
                    .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
            })
            .or_else(|| invalid_type_parameter_type_declaration_in_block(&for_stmt.body, scopes)),
        ast::Stmt::GoStmt(go) => invalid_type_parameter_type_declaration_in_call(&go.call, scopes),
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt
                .init
                .as_ref()
                .as_ref()
                .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&if_stmt.cond, scopes))
                .or_else(|| invalid_type_parameter_type_declaration_in_block(&if_stmt.body, scopes))
                .or_else(|| {
                    if_stmt.else_.as_ref().as_ref().and_then(|stmt| {
                        invalid_type_parameter_type_declaration_in_stmt(stmt, scopes)
                    })
                })
        }
        ast::Stmt::IncDecStmt(_) => None,
        ast::Stmt::LabeledStmt(labeled) => {
            invalid_type_parameter_type_declaration_in_stmt(&labeled.stmt, scopes)
        }
        ast::Stmt::RangeStmt(range) => {
            invalid_type_parameter_type_declaration_in_expr(&range.x, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_block(&range.body, scopes))
        }
        ast::Stmt::ReturnStmt(ret) => ret
            .results
            .iter()
            .find_map(|expr| invalid_type_parameter_type_declaration_in_expr(expr, scopes)),
        ast::Stmt::SelectStmt(select) => {
            invalid_type_parameter_type_declaration_in_block(&select.body, scopes)
        }
        ast::Stmt::SendStmt(send) => {
            invalid_type_parameter_type_declaration_in_expr(&send.chan, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&send.value, scopes))
        }
        ast::Stmt::SwitchStmt(switch) => switch
            .init
            .as_ref()
            .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
            .or_else(|| {
                switch
                    .tag
                    .as_ref()
                    .and_then(|expr| invalid_type_parameter_type_declaration_in_expr(expr, scopes))
            })
            .or_else(|| invalid_type_parameter_type_declaration_in_block(&switch.body, scopes)),
        ast::Stmt::TypeSwitchStmt(type_switch) => type_switch
            .init
            .as_ref()
            .and_then(|stmt| invalid_type_parameter_type_declaration_in_stmt(stmt, scopes))
            .or_else(|| {
                invalid_type_parameter_type_declaration_in_stmt(&type_switch.assign, scopes)
            })
            .or_else(|| {
                invalid_type_parameter_type_declaration_in_block(&type_switch.body, scopes)
            }),
    }
}

fn invalid_type_parameter_type_declaration_in_call(
    call: &ast::CallExpr<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    invalid_type_parameter_type_declaration_in_expr(&call.fun, scopes).or_else(|| {
        call.args.as_ref().and_then(|args| {
            args.iter()
                .find_map(|arg| invalid_type_parameter_type_declaration_in_expr(arg, scopes))
        })
    })
}

fn invalid_type_parameter_type_declaration_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut TypeParameterScopes,
) -> Option<InvalidDeclaration> {
    match expr {
        ast::Expr::ArrayType(array) => array
            .len
            .as_ref()
            .and_then(|len| invalid_type_parameter_type_declaration_in_expr(len, scopes))
            .or_else(|| invalid_type_parameter_type_declaration_in_expr(&array.elt, scopes)),
        ast::Expr::BinaryExpr(binary) => {
            invalid_type_parameter_type_declaration_in_expr(&binary.x, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&binary.y, scopes))
        }
        ast::Expr::CallExpr(call) => invalid_type_parameter_type_declaration_in_call(call, scopes),
        ast::Expr::ChanType(chan) => {
            invalid_type_parameter_type_declaration_in_expr(&chan.value, scopes)
        }
        ast::Expr::CompositeLit(comp) => comp
            .type_
            .as_ref()
            .and_then(|type_| invalid_type_parameter_type_declaration_in_expr(type_, scopes))
            .or_else(|| {
                comp.elts.as_ref().and_then(|elts| {
                    elts.iter().find_map(|elt| {
                        invalid_type_parameter_type_declaration_in_expr(elt, scopes)
                    })
                })
            }),
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|elt| invalid_type_parameter_type_declaration_in_expr(elt, scopes)),
        ast::Expr::FuncLit(func_lit) => {
            invalid_type_parameter_type_declaration_in_block(&func_lit.body, scopes)
        }
        ast::Expr::FuncType(_) | ast::Expr::Ident(_) | ast::Expr::BasicLit(_) => None,
        ast::Expr::IndexExpr(index) => {
            invalid_type_parameter_type_declaration_in_expr(&index.x, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&index.index, scopes))
        }
        ast::Expr::IndexListExpr(index) => {
            invalid_type_parameter_type_declaration_in_expr(&index.x, scopes).or_else(|| {
                index.indices.iter().find_map(|index| {
                    invalid_type_parameter_type_declaration_in_expr(index, scopes)
                })
            })
        }
        ast::Expr::InterfaceType(_) => None,
        ast::Expr::KeyValueExpr(kv) => {
            invalid_type_parameter_type_declaration_in_expr(&kv.key, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&kv.value, scopes))
        }
        ast::Expr::MapType(map) => {
            invalid_type_parameter_type_declaration_in_expr(&map.key, scopes)
                .or_else(|| invalid_type_parameter_type_declaration_in_expr(&map.value, scopes))
        }
        ast::Expr::ParenExpr(paren) => {
            invalid_type_parameter_type_declaration_in_expr(&paren.x, scopes)
        }
        ast::Expr::SelectorExpr(selector) => {
            invalid_type_parameter_type_declaration_in_expr(&selector.x, scopes)
        }
        ast::Expr::SliceExpr(slice) => {
            invalid_type_parameter_type_declaration_in_expr(&slice.x, scopes)
                .or_else(|| {
                    slice.low.as_ref().and_then(|low| {
                        invalid_type_parameter_type_declaration_in_expr(low, scopes)
                    })
                })
                .or_else(|| {
                    slice.high.as_ref().and_then(|high| {
                        invalid_type_parameter_type_declaration_in_expr(high, scopes)
                    })
                })
                .or_else(|| {
                    slice.max.as_ref().and_then(|max| {
                        invalid_type_parameter_type_declaration_in_expr(max, scopes)
                    })
                })
        }
        ast::Expr::StarExpr(star) => {
            invalid_type_parameter_type_declaration_in_expr(&star.x, scopes)
        }
        ast::Expr::StructType(_) => None,
        ast::Expr::TypeAssertExpr(assert) => {
            invalid_type_parameter_type_declaration_in_expr(&assert.x, scopes).or_else(|| {
                assert.type_.as_ref().and_then(|type_| {
                    invalid_type_parameter_type_declaration_in_expr(type_, scopes)
                })
            })
        }
        ast::Expr::UnaryExpr(unary) => {
            invalid_type_parameter_type_declaration_in_expr(&unary.x, scopes)
        }
    }
}

fn type_parameter_names(type_params: Option<&ast::FieldList<'_>>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Some(type_params) = type_params else {
        return names;
    };
    for field in &type_params.list {
        if let Some(field_names) = &field.names {
            names.extend(
                field_names
                    .iter()
                    .filter(|name| name.name != "_")
                    .map(|name| name.name.to_string()),
            );
        }
    }
    names
}

fn receiver_type_parameter_names(recv: &ast::FieldList<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for field in &recv.list {
        if let Some(type_) = &field.type_ {
            collect_receiver_type_parameter_names(type_, &mut names);
        }
    }
    names
}

fn collect_receiver_type_parameter_names(expr: &ast::Expr<'_>, names: &mut BTreeSet<String>) {
    match expr {
        ast::Expr::IndexExpr(index) => {
            if let ast::Expr::Ident(ident) = index.index.as_ref()
                && ident.name != "_"
            {
                names.insert(ident.name.to_string());
            }
        }
        ast::Expr::IndexListExpr(index) => {
            for expr in &index.indices {
                if let ast::Expr::Ident(ident) = expr
                    && ident.name != "_"
                {
                    names.insert(ident.name.to_string());
                }
            }
        }
        ast::Expr::ParenExpr(paren) => collect_receiver_type_parameter_names(&paren.x, names),
        ast::Expr::StarExpr(star) => collect_receiver_type_parameter_names(&star.x, names),
        _ => {}
    }
}

fn single_ident_type_name(expr: &ast::Expr<'_>) -> Option<String> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::ParenExpr(paren) => single_ident_type_name(&paren.x),
        _ => None,
    }
}

fn invalid_short_var_redeclaration_in_decl(decl: &ast::Decl<'_>) -> Option<InvalidStatement> {
    match decl {
        ast::Decl::FuncDecl(func) => invalid_short_var_redeclaration_in_func_decl(func),
        ast::Decl::GenDecl(gen_decl) => invalid_short_var_redeclaration_in_gen_decl(gen_decl),
    }
}

fn invalid_short_var_redeclaration_in_func_decl(
    func: &ast::FuncDecl<'_>,
) -> Option<InvalidStatement> {
    let body = func.body.as_ref()?;
    let mut scopes = ShortVarScopes::new();
    if let Some(recv) = &func.recv {
        seed_field_names_in_short_var_scope(recv, &mut scopes);
    }
    seed_field_names_in_short_var_scope(&func.type_.params, &mut scopes);
    if let Some(results) = &func.type_.results {
        seed_field_names_in_short_var_scope(results, &mut scopes);
    }
    invalid_short_var_redeclaration_in_stmt_list(&body.list, &mut scopes)
}

fn seed_field_names_in_short_var_scope(fields: &ast::FieldList<'_>, scopes: &mut ShortVarScopes) {
    for field in &fields.list {
        if let Some(names) = &field.names {
            for name in names {
                scopes.declare(name.name);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ShortVarScopes {
    scopes: Vec<BTreeSet<String>>,
}

impl ShortVarScopes {
    fn new() -> Self {
        Self {
            scopes: vec![BTreeSet::new()],
        }
    }

    fn contains_current(&self, name: &str) -> bool {
        self.scopes.last().is_some_and(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if name != "_"
            && let Some(scope) = self.scopes.last_mut()
        {
            scope.insert(name.to_string());
        }
    }

    fn with_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(BTreeSet::new());
        let out = f(self);
        self.scopes.pop();
        out
    }
}

fn invalid_short_var_redeclaration_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
) -> Option<InvalidStatement> {
    let mut scopes = ShortVarScopes::new();
    for spec in &gen_decl.specs {
        if let Some(invalid) = invalid_short_var_redeclaration_in_spec(spec, &mut scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_short_var_redeclaration_in_spec(
    spec: &ast::Spec<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => {
            invalid_short_var_redeclaration_in_expr(&type_spec.type_, scopes)
        }
        ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_) = &value_spec.type_
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(type_, scopes)
            {
                return Some(invalid);
            }
            if let Some(values) = &value_spec.values {
                for value in values {
                    if let Some(invalid) = invalid_short_var_redeclaration_in_expr(value, scopes) {
                        return Some(invalid);
                    }
                }
            }
            for name in &value_spec.names {
                scopes.declare(name.name);
            }
            None
        }
    }
}

fn invalid_short_var_redeclaration_in_block(
    block: &ast::BlockStmt<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    scopes.with_scope(|scopes| invalid_short_var_redeclaration_in_stmt_list(&block.list, scopes))
}

fn invalid_short_var_redeclaration_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    for stmt in stmts {
        if let Some(invalid) = invalid_short_var_redeclaration_in_stmt(stmt, scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_short_var_redeclaration_in_stmt(
    stmt: &ast::Stmt<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => invalid_short_var_redeclaration_in_assign(assign, scopes),
        ast::Stmt::BlockStmt(block) => invalid_short_var_redeclaration_in_block(block, scopes),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::CaseClause(case) => scopes.with_scope(|scopes| {
            if let Some(list) = &case.list {
                for expr in list {
                    if let Some(invalid) = invalid_short_var_redeclaration_in_expr(expr, scopes) {
                        return Some(invalid);
                    }
                }
            }
            invalid_short_var_redeclaration_in_stmt_list(&case.body, scopes)
        }),
        ast::Stmt::CommClause(comm) => scopes.with_scope(|scopes| {
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(comm, scopes)
            {
                return Some(invalid);
            }
            invalid_short_var_redeclaration_in_stmt_list(&comm.body, scopes)
        }),
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let Some(invalid) = invalid_short_var_redeclaration_in_spec(spec, scopes) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::DeferStmt(defer) => invalid_short_var_redeclaration_in_call(&defer.call, scopes),
        ast::Stmt::ExprStmt(expr) => invalid_short_var_redeclaration_in_expr(&expr.x, scopes),
        ast::Stmt::ForStmt(for_stmt) => scopes.with_scope(|scopes| {
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(cond, scopes)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && is_short_var_decl_stmt(post)
            {
                return Some(InvalidStatement::ForPostShortVarDecl);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(post, scopes)
            {
                return Some(invalid);
            }
            invalid_short_var_redeclaration_in_block(&for_stmt.body, scopes)
        }),
        ast::Stmt::GoStmt(go) => invalid_short_var_redeclaration_in_call(&go.call, scopes),
        ast::Stmt::IfStmt(if_stmt) => scopes.with_scope(|scopes| {
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&if_stmt.cond, scopes) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_short_var_redeclaration_in_block(&if_stmt.body, scopes) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                return invalid_short_var_redeclaration_in_stmt(else_branch, scopes);
            }
            None
        }),
        ast::Stmt::IncDecStmt(inc_dec) => {
            invalid_short_var_redeclaration_in_expr(&inc_dec.x, scopes)
        }
        ast::Stmt::LabeledStmt(labeled) => {
            invalid_short_var_redeclaration_in_stmt(&labeled.stmt, scopes)
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&range.x, scopes) {
                return Some(invalid);
            }
            scopes.with_scope(|scopes| {
                if range.tok == Some(token::Token::DEFINE) {
                    if let Some(reason) = invalid_range_short_var_decl_names(range) {
                        return Some(InvalidStatement::ShortVarDecl { reason });
                    }
                    if !range_short_var_decl_has_new_name(range) {
                        return Some(InvalidStatement::ShortVarDecl {
                            reason: InvalidShortVarDeclReason::NoNewVariables,
                        });
                    }
                    declare_range_short_var_names(range, scopes);
                }
                invalid_short_var_redeclaration_in_block(&range.body, scopes)
            })
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_short_var_redeclaration_in_expr(expr, scopes) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SelectStmt(select) => {
            invalid_short_var_redeclaration_in_block(&select.body, scopes)
        }
        ast::Stmt::SendStmt(send) => invalid_short_var_redeclaration_in_expr(&send.chan, scopes)
            .or_else(|| invalid_short_var_redeclaration_in_expr(&send.value, scopes)),
        ast::Stmt::SwitchStmt(switch) => scopes.with_scope(|scopes| {
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(tag, scopes)
            {
                return Some(invalid);
            }
            invalid_short_var_redeclaration_in_block(&switch.body, scopes)
        }),
        ast::Stmt::TypeSwitchStmt(type_switch) => scopes.with_scope(|scopes| {
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_short_var_redeclaration_in_stmt(init, scopes)
            {
                return Some(invalid);
            }
            if let Some(invalid) =
                invalid_short_var_redeclaration_in_stmt(&type_switch.assign, scopes)
            {
                return Some(invalid);
            }
            invalid_short_var_redeclaration_in_block(&type_switch.body, scopes)
        }),
    }
}

fn invalid_short_var_redeclaration_in_assign(
    assign: &ast::AssignStmt<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
        if let Some(invalid) = invalid_short_var_redeclaration_in_expr(expr, scopes) {
            return Some(invalid);
        }
    }
    if assign.tok != token::Token::DEFINE {
        return None;
    }
    if let Some(reason) = invalid_short_var_decl_names(&assign.lhs) {
        return Some(InvalidStatement::ShortVarDecl { reason });
    }
    let mut has_new = false;
    for expr in &assign.lhs {
        let Some(name) = short_var_decl_ident_name(expr) else {
            continue;
        };
        if name == "_" {
            continue;
        }
        if !scopes.contains_current(name) {
            has_new = true;
        }
    }
    if !has_new {
        return Some(InvalidStatement::ShortVarDecl {
            reason: InvalidShortVarDeclReason::NoNewVariables,
        });
    }
    for expr in &assign.lhs {
        if let Some(name) = short_var_decl_ident_name(expr) {
            scopes.declare(name);
        }
    }
    None
}

fn range_short_var_decl_has_new_name(range: &ast::RangeStmt<'_>) -> bool {
    [range.key.as_ref(), range.value.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(short_var_decl_ident_name)
        .any(|name| name != "_")
}

fn declare_range_short_var_names(range: &ast::RangeStmt<'_>, scopes: &mut ShortVarScopes) {
    if let Some(key) = &range.key
        && let Some(name) = short_var_decl_ident_name(key)
    {
        scopes.declare(name);
    }
    if let Some(value) = &range.value
        && let Some(name) = short_var_decl_ident_name(value)
    {
        scopes.declare(name);
    }
}

fn invalid_short_var_redeclaration_in_call(
    call: &ast::CallExpr<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&call.fun, scopes) {
        return Some(invalid);
    }
    if let Some(args) = &call.args {
        for arg in args {
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(arg, scopes) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_short_var_redeclaration_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(len, scopes)
            {
                return Some(invalid);
            }
            invalid_short_var_redeclaration_in_expr(&array.elt, scopes)
        }
        ast::Expr::BinaryExpr(binary) => invalid_short_var_redeclaration_in_expr(&binary.x, scopes)
            .or_else(|| invalid_short_var_redeclaration_in_expr(&binary.y, scopes)),
        ast::Expr::CallExpr(call) => invalid_short_var_redeclaration_in_call(call, scopes),
        ast::Expr::ChanType(chan) => invalid_short_var_redeclaration_in_expr(&chan.value, scopes),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(type_, scopes)
            {
                return Some(invalid);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    if let Some(invalid) = invalid_short_var_redeclaration_in_expr(elt, scopes) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|expr| invalid_short_var_redeclaration_in_expr(expr, scopes)),
        ast::Expr::FuncLit(func_lit) => {
            let mut func_scopes = ShortVarScopes::new();
            seed_field_names_in_short_var_scope(&func_lit.type_.params, &mut func_scopes);
            if let Some(results) = &func_lit.type_.results {
                seed_field_names_in_short_var_scope(results, &mut func_scopes);
            }
            invalid_short_var_redeclaration_in_stmt_list(&func_lit.body.list, &mut func_scopes)
        }
        ast::Expr::FuncType(func_type) => {
            for field in &func_type.params.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_short_var_redeclaration_in_expr(type_, scopes)
                {
                    return Some(invalid);
                }
            }
            if let Some(results) = &func_type.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_
                        && let Some(invalid) =
                            invalid_short_var_redeclaration_in_expr(type_, scopes)
                    {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::IndexExpr(index) => invalid_short_var_redeclaration_in_expr(&index.x, scopes)
            .or_else(|| invalid_short_var_redeclaration_in_expr(&index.index, scopes)),
        ast::Expr::IndexListExpr(index) => {
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&index.x, scopes) {
                return Some(invalid);
            }
            for index in &index.indices {
                if let Some(invalid) = invalid_short_var_redeclaration_in_expr(index, scopes) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Expr::InterfaceType(interface) => interface.methods.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_short_var_redeclaration_in_expr(type_, scopes)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::KeyValueExpr(kv) => invalid_short_var_redeclaration_in_expr(&kv.key, scopes)
            .or_else(|| invalid_short_var_redeclaration_in_expr(&kv.value, scopes)),
        ast::Expr::MapType(map) => invalid_short_var_redeclaration_in_expr(&map.key, scopes)
            .or_else(|| invalid_short_var_redeclaration_in_expr(&map.value, scopes)),
        ast::Expr::ParenExpr(paren) => invalid_short_var_redeclaration_in_expr(&paren.x, scopes),
        ast::Expr::SelectorExpr(selector) => {
            invalid_short_var_redeclaration_in_expr(&selector.x, scopes)
        }
        ast::Expr::SliceExpr(slice) => {
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&slice.x, scopes) {
                return Some(invalid);
            }
            if let Some(low) = &slice.low
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(low, scopes)
            {
                return Some(invalid);
            }
            if let Some(high) = &slice.high
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(high, scopes)
            {
                return Some(invalid);
            }
            if let Some(max) = &slice.max
                && let Some(invalid) = invalid_short_var_redeclaration_in_expr(max, scopes)
            {
                return Some(invalid);
            }
            None
        }
        ast::Expr::StarExpr(star) => invalid_short_var_redeclaration_in_expr(&star.x, scopes),
        ast::Expr::StructType(struct_type) => struct_type.fields.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_short_var_redeclaration_in_expr(type_, scopes)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::TypeAssertExpr(assert) => {
            if let Some(invalid) = invalid_short_var_redeclaration_in_expr(&assert.x, scopes) {
                return Some(invalid);
            }
            assert
                .type_
                .as_ref()
                .and_then(|ty| invalid_short_var_redeclaration_in_expr(ty, scopes))
        }
        ast::Expr::UnaryExpr(unary) => invalid_short_var_redeclaration_in_expr(&unary.x, scopes),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

#[derive(Default)]
struct BranchContext {
    breakable_depth: usize,
    loop_depth: usize,
    labels: Vec<(String, BranchLabelTarget)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BranchLabelTarget {
    Breakable,
    Loop,
}

impl BranchContext {
    fn with_breakable<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.breakable_depth += 1;
        let out = f(self);
        self.breakable_depth -= 1;
        out
    }

    fn with_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.breakable_depth += 1;
        self.loop_depth += 1;
        let out = f(self);
        self.loop_depth -= 1;
        self.breakable_depth -= 1;
        out
    }

    fn with_labels<T>(
        &mut self,
        labels: Vec<String>,
        target: Option<BranchLabelTarget>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let original_len = self.labels.len();
        if let Some(target) = target {
            self.labels
                .extend(labels.into_iter().map(|label| (label, target)));
        }
        let out = f(self);
        self.labels.truncate(original_len);
        out
    }

    fn label_target(&self, label: &str) -> Option<BranchLabelTarget> {
        self.labels
            .iter()
            .rev()
            .find_map(|(name, target)| (name == label).then_some(*target))
    }
}

fn invalid_branch_in_block(
    block: &ast::BlockStmt<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    invalid_branch_in_stmt_list(&block.list, context)
}

fn invalid_branch_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    for stmt in stmts {
        if let Some(invalid) = invalid_branch_in_stmt(stmt, context) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_branch_in_stmt(
    stmt: &ast::Stmt<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                if let Some(invalid) = invalid_branch_in_expr(expr, context) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::BranchStmt(branch) => invalid_branch_stmt(branch, context),
        ast::Stmt::BlockStmt(block) => invalid_branch_in_block(block, context),
        ast::Stmt::CaseClause(case) => invalid_branch_in_case_body(&case.body, context),
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_branch_in_stmt(comm, context)
            {
                return Some(invalid);
            }
            invalid_branch_in_stmt_list(&comm.body, context)
        }
        ast::Stmt::DeclStmt(_) => None,
        ast::Stmt::DeferStmt(defer) => invalid_branch_in_call(&defer.call, context),
        ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::ExprStmt(expr) => invalid_branch_in_expr(&expr.x, context),
        ast::Stmt::ForStmt(for_stmt) => context.with_loop(|context| {
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_branch_in_stmt(init, context)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_branch_in_expr(cond, context)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_branch_in_stmt(post, context)
            {
                return Some(invalid);
            }
            invalid_branch_in_block(&for_stmt.body, context)
        }),
        ast::Stmt::GoStmt(go) => invalid_branch_in_call(&go.call, context),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_branch_in_stmt(init, context)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_branch_in_expr(&if_stmt.cond, context) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_branch_in_block(&if_stmt.body, context) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                return invalid_branch_in_stmt(else_branch, context);
            }
            None
        }
        ast::Stmt::IncDecStmt(inc_dec) => invalid_branch_in_expr(&inc_dec.x, context),
        ast::Stmt::LabeledStmt(labeled) => invalid_branch_in_labeled_stmt(labeled, context),
        ast::Stmt::RangeStmt(range) => context.with_loop(|context| {
            if let Some(key) = &range.key
                && let Some(invalid) = invalid_branch_in_expr(key, context)
            {
                return Some(invalid);
            }
            if let Some(value) = &range.value
                && let Some(invalid) = invalid_branch_in_expr(value, context)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_branch_in_expr(&range.x, context) {
                return Some(invalid);
            }
            invalid_branch_in_block(&range.body, context)
        }),
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_branch_in_expr(expr, context) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SelectStmt(select) => context.with_breakable(|context| {
            for stmt in &select.body.list {
                if let Some(invalid) = invalid_branch_in_stmt(stmt, context) {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Stmt::SendStmt(send) => invalid_branch_in_expr(&send.chan, context)
            .or_else(|| invalid_branch_in_expr(&send.value, context)),
        ast::Stmt::SwitchStmt(switch) => context.with_breakable(|context| {
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_branch_in_stmt(init, context)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_branch_in_expr(tag, context)
            {
                return Some(invalid);
            }
            invalid_branch_in_expression_switch_cases(&switch.body.list, context)
        }),
        ast::Stmt::TypeSwitchStmt(type_switch) => context.with_breakable(|context| {
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_branch_in_stmt(init, context)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_branch_in_stmt(&type_switch.assign, context) {
                return Some(invalid);
            }
            invalid_branch_in_type_switch_cases(&type_switch.body.list, context)
        }),
    }
}

fn invalid_branch_stmt(
    branch: &ast::BranchStmt<'_>,
    context: &BranchContext,
) -> Option<InvalidBranch> {
    match branch.tok {
        token::Token::BREAK => {
            let Some(label) = branch.label.as_ref() else {
                return (context.breakable_depth == 0).then_some(InvalidBranch::BreakOutside);
            };
            match context.label_target(label.name) {
                Some(BranchLabelTarget::Breakable | BranchLabelTarget::Loop) => None,
                None => Some(InvalidBranch::BreakLabel {
                    label: label.name.to_string(),
                }),
            }
        }
        token::Token::CONTINUE => {
            let Some(label) = branch.label.as_ref() else {
                return (context.loop_depth == 0).then_some(InvalidBranch::ContinueOutside);
            };
            match context.label_target(label.name) {
                Some(BranchLabelTarget::Loop) => None,
                Some(BranchLabelTarget::Breakable) | None => Some(InvalidBranch::ContinueLabel {
                    label: label.name.to_string(),
                }),
            }
        }
        token::Token::FALLTHROUGH => Some(InvalidBranch::FallthroughOutsideSwitch),
        _ => None,
    }
}

fn invalid_branch_in_labeled_stmt(
    labeled: &ast::LabeledStmt<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    let mut labels: Vec<String> = non_blank_label_name(labeled.label.name)
        .into_iter()
        .collect();
    let mut inner = labeled.stmt.as_ref();
    while let ast::Stmt::LabeledStmt(next) = inner {
        if let Some(label) = non_blank_label_name(next.label.name) {
            labels.push(label);
        }
        inner = &next.stmt;
    }
    let target = branch_label_target(inner);
    context.with_labels(labels, target, |context| {
        invalid_branch_in_stmt(inner, context)
    })
}

fn branch_label_target(stmt: &ast::Stmt<'_>) -> Option<BranchLabelTarget> {
    match stmt {
        ast::Stmt::ForStmt(_) | ast::Stmt::RangeStmt(_) => Some(BranchLabelTarget::Loop),
        ast::Stmt::SelectStmt(_) | ast::Stmt::SwitchStmt(_) | ast::Stmt::TypeSwitchStmt(_) => {
            Some(BranchLabelTarget::Breakable)
        }
        _ => None,
    }
}

fn invalid_branch_in_expression_switch_cases(
    stmts: &[ast::Stmt<'_>],
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    let case_indices: Vec<_> = stmts
        .iter()
        .enumerate()
        .filter_map(|(idx, stmt)| matches!(stmt, ast::Stmt::CaseClause(_)).then_some(idx))
        .collect();

    for (case_order, stmt_idx) in case_indices.iter().copied().enumerate() {
        let Some(ast::Stmt::CaseClause(case)) = stmts.get(stmt_idx) else {
            continue;
        };
        if let Some(invalid) = invalid_branch_in_case_exprs(case, context) {
            return Some(invalid);
        }
        let final_idx = final_non_empty_stmt_idx(&case.body);
        for (idx, stmt) in case.body.iter().enumerate() {
            let allowed_fallthrough = final_idx == Some(idx) && case_order + 1 < case_indices.len();
            if allowed_fallthrough && is_fallthrough_stmt(stmt) {
                continue;
            }
            if contains_fallthrough_for_current_switch(stmt) {
                return Some(
                    if final_idx == Some(idx) && case_order + 1 == case_indices.len() {
                        InvalidBranch::FallthroughInFinalCase
                    } else {
                        InvalidBranch::FallthroughNotFinal
                    },
                );
            }
            if let Some(invalid) = invalid_branch_in_stmt(stmt, context) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_branch_in_type_switch_cases(
    stmts: &[ast::Stmt<'_>],
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    for stmt in stmts {
        let ast::Stmt::CaseClause(case) = stmt else {
            continue;
        };
        if let Some(invalid) = invalid_branch_in_case_exprs(case, context) {
            return Some(invalid);
        }
        for stmt in &case.body {
            if contains_fallthrough_for_current_switch(stmt) {
                return Some(InvalidBranch::FallthroughInTypeSwitch);
            }
            if let Some(invalid) = invalid_branch_in_stmt(stmt, context) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_branch_in_case_exprs(
    case: &ast::CaseClause<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    if let Some(exprs) = &case.list {
        for expr in exprs {
            if let Some(invalid) = invalid_branch_in_expr(expr, context) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_branch_in_case_body(
    body: &[ast::Stmt<'_>],
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    invalid_branch_in_stmt_list(body, context)
}

fn final_non_empty_stmt_idx(stmts: &[ast::Stmt<'_>]) -> Option<usize> {
    stmts
        .iter()
        .enumerate()
        .rev()
        .find_map(|(idx, stmt)| (!matches!(stmt, ast::Stmt::EmptyStmt(_))).then_some(idx))
}

fn is_fallthrough_stmt(stmt: &ast::Stmt<'_>) -> bool {
    matches!(
        stmt,
        ast::Stmt::BranchStmt(branch) if branch.tok == token::Token::FALLTHROUGH
    )
}

fn contains_fallthrough_for_current_switch(stmt: &ast::Stmt<'_>) -> bool {
    match stmt {
        ast::Stmt::BranchStmt(branch) => branch.tok == token::Token::FALLTHROUGH,
        ast::Stmt::BlockStmt(block) => block
            .list
            .iter()
            .any(contains_fallthrough_for_current_switch),
        ast::Stmt::CaseClause(case) => case
            .body
            .iter()
            .any(contains_fallthrough_for_current_switch),
        ast::Stmt::CommClause(comm) => {
            comm.comm
                .as_ref()
                .is_some_and(|stmt| contains_fallthrough_for_current_switch(stmt))
                || comm
                    .body
                    .iter()
                    .any(contains_fallthrough_for_current_switch)
        }
        ast::Stmt::ForStmt(for_stmt) => {
            for_stmt
                .init
                .as_ref()
                .is_some_and(|stmt| contains_fallthrough_for_current_switch(stmt))
                || for_stmt
                    .post
                    .as_ref()
                    .is_some_and(|stmt| contains_fallthrough_for_current_switch(stmt))
                || for_stmt
                    .body
                    .list
                    .iter()
                    .any(contains_fallthrough_for_current_switch)
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt
                .init
                .as_ref()
                .as_ref()
                .is_some_and(|stmt| contains_fallthrough_for_current_switch(stmt))
                || if_stmt
                    .body
                    .list
                    .iter()
                    .any(contains_fallthrough_for_current_switch)
                || if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .is_some_and(|stmt| contains_fallthrough_for_current_switch(stmt))
        }
        ast::Stmt::LabeledStmt(labeled) => contains_fallthrough_for_current_switch(&labeled.stmt),
        ast::Stmt::RangeStmt(range) => range
            .body
            .list
            .iter()
            .any(contains_fallthrough_for_current_switch),
        ast::Stmt::SelectStmt(select) => select
            .body
            .list
            .iter()
            .any(contains_fallthrough_for_current_switch),
        ast::Stmt::SwitchStmt(_) | ast::Stmt::TypeSwitchStmt(_) => false,
        ast::Stmt::AssignStmt(_)
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

fn invalid_branch_in_call(
    call: &ast::CallExpr<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    if let Some(invalid) = invalid_branch_in_expr(&call.fun, context) {
        return Some(invalid);
    }
    if let Some(args) = &call.args {
        for arg in args {
            if let Some(invalid) = invalid_branch_in_expr(arg, context) {
                return Some(invalid);
            }
        }
    }
    None
}

fn invalid_branch_in_expr(
    expr: &ast::Expr<'_>,
    context: &mut BranchContext,
) -> Option<InvalidBranch> {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                if let Some(invalid) = invalid_branch_in_expr(len, context) {
                    return Some(invalid);
                }
            }
            invalid_branch_in_expr(&array.elt, context)
        }
        ast::Expr::BinaryExpr(binary) => invalid_branch_in_expr(&binary.x, context)
            .or_else(|| invalid_branch_in_expr(&binary.y, context)),
        ast::Expr::CallExpr(call) => invalid_branch_in_call(call, context),
        ast::Expr::ChanType(chan) => invalid_branch_in_expr(&chan.value, context),
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                if let Some(invalid) = invalid_branch_in_expr(ty, context) {
                    return Some(invalid);
                }
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    if let Some(invalid) = invalid_branch_in_expr(elt, context) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|expr| invalid_branch_in_expr(expr, context)),
        ast::Expr::FuncLit(func_lit) => invalid_branch_in_func(&func_lit.body),
        ast::Expr::IndexExpr(index) => invalid_branch_in_expr(&index.x, context)
            .or_else(|| invalid_branch_in_expr(&index.index, context)),
        ast::Expr::IndexListExpr(index) => {
            if let Some(invalid) = invalid_branch_in_expr(&index.x, context) {
                return Some(invalid);
            }
            for index in &index.indices {
                if let Some(invalid) = invalid_branch_in_expr(index, context) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Expr::KeyValueExpr(kv) => invalid_branch_in_expr(&kv.key, context)
            .or_else(|| invalid_branch_in_expr(&kv.value, context)),
        ast::Expr::MapType(map) => invalid_branch_in_expr(&map.key, context)
            .or_else(|| invalid_branch_in_expr(&map.value, context)),
        ast::Expr::ParenExpr(paren) => invalid_branch_in_expr(&paren.x, context),
        ast::Expr::SelectorExpr(selector) => invalid_branch_in_expr(&selector.x, context),
        ast::Expr::SliceExpr(slice) => {
            if let Some(invalid) = invalid_branch_in_expr(&slice.x, context) {
                return Some(invalid);
            }
            if let Some(low) = &slice.low
                && let Some(invalid) = invalid_branch_in_expr(low, context)
            {
                return Some(invalid);
            }
            if let Some(high) = &slice.high
                && let Some(invalid) = invalid_branch_in_expr(high, context)
            {
                return Some(invalid);
            }
            if let Some(max) = &slice.max
                && let Some(invalid) = invalid_branch_in_expr(max, context)
            {
                return Some(invalid);
            }
            None
        }
        ast::Expr::StarExpr(star) => invalid_branch_in_expr(&star.x, context),
        ast::Expr::TypeAssertExpr(assert) => {
            if let Some(invalid) = invalid_branch_in_expr(&assert.x, context) {
                return Some(invalid);
            }
            assert
                .type_
                .as_ref()
                .and_then(|ty| invalid_branch_in_expr(ty, context))
        }
        ast::Expr::UnaryExpr(unary) => invalid_branch_in_expr(&unary.x, context),
        ast::Expr::BasicLit(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::Ident(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => None,
    }
}

fn invalid_statement_in_block(
    block: &ast::BlockStmt<'_>,
    env: &mut TypeEnv,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    for stmt in &block.list {
        if let Some(invalid) = invalid_statement_in_stmt(stmt, env, scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_statement_in_nested_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    let mut block_env = env.clone();
    scopes.with_scope(|scopes| invalid_statement_in_block(block, &mut block_env, scopes))
}

fn invalid_statement_in_stmt(
    stmt: &ast::Stmt<'_>,
    env: &mut TypeEnv,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            if assign.tok == token::Token::DEFINE
                && let Some(reason) = invalid_short_var_decl_names(&assign.lhs)
            {
                return Some(InvalidStatement::ShortVarDecl { reason });
            }
            if let Some(reason) = assign
                .lhs
                .iter()
                .find_map(|expr| invalid_receive_expr(expr, env))
                .or_else(|| {
                    assign
                        .rhs
                        .iter()
                        .find_map(|expr| invalid_receive_expr(expr, env))
                })
            {
                return Some(InvalidStatement::Receive { reason });
            }
            if let Some(reason) = assign
                .lhs
                .iter()
                .find_map(|expr| invalid_expression_in_assignment_lhs(expr, env))
                .or_else(|| {
                    assign
                        .rhs
                        .iter()
                        .find_map(|expr| invalid_expression_in_expr(expr, env))
                })
            {
                return Some(InvalidStatement::Expression { reason });
            }
            if let Some(reason) = invalid_assignment(assign, env, scopes) {
                return Some(InvalidStatement::Assignment { reason });
            }
            record_define_bindings(assign, env);
            declare_define_names(assign, scopes);
            None
        }
        ast::Stmt::BlockStmt(block) => invalid_statement_in_nested_block(block, env, scopes),
        ast::Stmt::BranchStmt(_) => None,
        ast::Stmt::CaseClause(case) => {
            let mut case_env = env.clone();
            scopes.with_scope(|scopes| {
                invalid_statement_in_stmt_list(&case.body, &mut case_env, scopes)
            })
        }
        ast::Stmt::CommClause(comm) => {
            let mut comm_env = env.clone();
            scopes.with_scope(|scopes| {
                if let Some(comm_stmt) = &comm.comm {
                    if let Some(reason) = invalid_select_comm_stmt(comm_stmt) {
                        return Some(InvalidStatement::SelectComm { reason });
                    }
                    if let Some(invalid) =
                        invalid_statement_in_stmt(comm_stmt, &mut comm_env, scopes)
                    {
                        return Some(invalid);
                    }
                }
                invalid_statement_in_stmt_list(&comm.body, &mut comm_env, scopes)
            })
        }
        ast::Stmt::DeclStmt(decl) => {
            if let Some(reason) = invalid_receive_in_gen_decl(&decl.decl, env) {
                return Some(InvalidStatement::Receive { reason });
            }
            if let Some(reason) = invalid_expression_in_gen_decl(&decl.decl, env) {
                return Some(InvalidStatement::Expression { reason });
            }
            if let Some(reason) = invalid_value_declaration_in_gen_decl(&decl.decl, env) {
                return Some(InvalidStatement::Declaration { reason });
            }
            record_decl_bindings(&decl.decl, env);
            declare_gen_decl_names(&decl.decl, scopes);
            None
        }
        ast::Stmt::DeferStmt(defer) => {
            if let Some(reason) = invalid_call_statement(&defer.call, env) {
                return Some(InvalidStatement::Defer { reason });
            }
            invalid_expression_in_call_statement(&defer.call, env)
                .map(|reason| InvalidStatement::Expression { reason })
        }
        ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::ExprStmt(expr) => {
            if let Some(reason) = invalid_receive_expr(&expr.x, env) {
                return Some(InvalidStatement::Receive { reason });
            }
            if let Some(reason) = invalid_expression_statement(&expr.x, env) {
                return Some(InvalidStatement::Expr { reason });
            }
            invalid_expression_in_statement_expr(&expr.x, env)
                .map(|reason| InvalidStatement::Expression { reason })
        }
        ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            scopes.with_scope(|scopes| {
                if let Some(init) = &for_stmt.init
                    && let Some(invalid) = invalid_statement_in_stmt(init, &mut loop_env, scopes)
                {
                    return Some(invalid);
                }
                if let Some(cond) = &for_stmt.cond
                    && let Some(reason) = invalid_receive_expr(cond, &loop_env)
                {
                    return Some(InvalidStatement::Receive { reason });
                }
                if let Some(cond) = &for_stmt.cond
                    && let Some(reason) = invalid_expression_in_expr(cond, &loop_env)
                {
                    return Some(InvalidStatement::Expression { reason });
                }
                if let Some(cond) = &for_stmt.cond
                    && let Some(reason) = invalid_condition(cond, &loop_env, ConditionKind::For)
                {
                    return Some(InvalidStatement::Condition { reason });
                }
                if let Some(post) = &for_stmt.post
                    && is_short_var_decl_stmt(post)
                {
                    return Some(InvalidStatement::ForPostShortVarDecl);
                }
                if let Some(post) = &for_stmt.post
                    && let Some(invalid) = invalid_statement_in_stmt(post, &mut loop_env, scopes)
                {
                    return Some(invalid);
                }
                invalid_statement_in_nested_block(&for_stmt.body, &loop_env, scopes)
            })
        }
        ast::Stmt::GoStmt(go) => {
            if let Some(reason) = invalid_call_statement(&go.call, env) {
                return Some(InvalidStatement::Go { reason });
            }
            invalid_expression_in_call_statement(&go.call, env)
                .map(|reason| InvalidStatement::Expression { reason })
        }
        ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            scopes.with_scope(|scopes| {
                if let Some(init) = if_stmt.init.as_ref().as_ref()
                    && let Some(invalid) = invalid_statement_in_stmt(init, &mut if_env, scopes)
                {
                    return Some(invalid);
                }
                if let Some(reason) = invalid_receive_expr(&if_stmt.cond, &if_env) {
                    return Some(InvalidStatement::Receive { reason });
                }
                if let Some(reason) = invalid_expression_in_expr(&if_stmt.cond, &if_env) {
                    return Some(InvalidStatement::Expression { reason });
                }
                if let Some(reason) = invalid_condition(&if_stmt.cond, &if_env, ConditionKind::If) {
                    return Some(InvalidStatement::Condition { reason });
                }
                if let Some(invalid) =
                    invalid_statement_in_nested_block(&if_stmt.body, &if_env, scopes)
                {
                    return Some(invalid);
                }
                if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                    let mut else_env = if_env;
                    return invalid_statement_in_stmt(else_branch, &mut else_env, scopes);
                }
                None
            })
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            if let Some(reason) = invalid_expression_in_assignment_lhs(&inc_dec.x, env) {
                return Some(InvalidStatement::Expression { reason });
            }
            invalid_inc_dec(inc_dec, env).map(|reason| InvalidStatement::IncDec { reason })
        }
        ast::Stmt::LabeledStmt(labeled) => invalid_statement_in_stmt(&labeled.stmt, env, scopes),
        ast::Stmt::RangeStmt(range) => {
            if range.tok == Some(token::Token::DEFINE)
                && let Some(reason) = invalid_range_short_var_decl_names(range)
            {
                return Some(InvalidStatement::ShortVarDecl { reason });
            }
            if matches!(range.tok, Some(token::Token::ASSIGN)) {
                if let Some(key) = &range.key
                    && let Some(reason) = invalid_expression_in_assignment_lhs(key, env)
                {
                    return Some(InvalidStatement::Expression { reason });
                }
                if let Some(value) = &range.value
                    && let Some(reason) = invalid_expression_in_assignment_lhs(value, env)
                {
                    return Some(InvalidStatement::Expression { reason });
                }
            }
            if let Some(reason) = invalid_range_clause(range, env) {
                return Some(InvalidStatement::Range { reason });
            }
            if let Some(reason) = invalid_expression_in_expr(&range.x, env) {
                return Some(InvalidStatement::Expression { reason });
            }
            let mut range_env = env.clone();
            record_range_bindings(range, &mut range_env);
            declare_range_names(range, scopes);
            invalid_statement_in_nested_block(&range.body, &range_env, scopes)
        }
        ast::Stmt::ReturnStmt(_) => None,
        ast::Stmt::SelectStmt(select) => {
            if has_duplicate_select_default(&select.body) {
                return Some(InvalidStatement::DuplicateDefault {
                    kind: DefaultClauseKind::Select,
                });
            }
            let mut select_env = env.clone();
            scopes.with_scope(|scopes| {
                for stmt in &select.body.list {
                    if let Some(invalid) = invalid_statement_in_stmt(stmt, &mut select_env, scopes)
                    {
                        return Some(invalid);
                    }
                }
                None
            })
        }
        ast::Stmt::SendStmt(send) => {
            if let Some(reason) = invalid_expression_in_expr(&send.chan, env)
                .or_else(|| invalid_expression_in_expr(&send.value, env))
            {
                return Some(InvalidStatement::Expression { reason });
            }
            invalid_send(send, env).map(|reason| InvalidStatement::Send { reason })
        }
        ast::Stmt::SwitchStmt(switch) => {
            if has_duplicate_case_default(&switch.body) {
                return Some(InvalidStatement::DuplicateDefault {
                    kind: DefaultClauseKind::Switch,
                });
            }
            let mut switch_env = env.clone();
            scopes.with_scope(|scopes| {
                if let Some(init) = &switch.init
                    && let Some(invalid) = invalid_statement_in_stmt(init, &mut switch_env, scopes)
                {
                    return Some(invalid);
                }
                if let Some(tag) = &switch.tag
                    && let Some(reason) = invalid_expression_in_expr(tag, &switch_env)
                {
                    return Some(InvalidStatement::Expression { reason });
                }
                if let Some(reason) = invalid_expression_switch(switch, &switch_env) {
                    return Some(InvalidStatement::Switch { reason });
                }
                invalid_statement_in_case_block(&switch.body, &switch_env, scopes)
            })
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if has_duplicate_case_default(&type_switch.body) {
                return Some(InvalidStatement::DuplicateDefault {
                    kind: DefaultClauseKind::TypeSwitch,
                });
            }
            if let Some(reason) = invalid_type_switch_guard(&type_switch.assign) {
                return Some(InvalidStatement::TypeSwitchGuard { reason });
            }
            let mut switch_env = env.clone();
            scopes.with_scope(|scopes| {
                if let Some(init) = &type_switch.init
                    && let Some(invalid) = invalid_statement_in_stmt(init, &mut switch_env, scopes)
                {
                    return Some(invalid);
                }
                if let Some(reason) = invalid_type_switch_stmt(type_switch, &switch_env) {
                    return Some(InvalidStatement::TypeSwitch { reason });
                }
                invalid_statement_in_case_block(&type_switch.body, &switch_env, scopes)
            })
        }
    }
}

fn is_short_var_decl_stmt(stmt: &ast::Stmt<'_>) -> bool {
    matches!(stmt, ast::Stmt::AssignStmt(assign) if assign.tok == token::Token::DEFINE)
}

fn has_duplicate_case_default(block: &ast::BlockStmt<'_>) -> bool {
    block
        .list
        .iter()
        .filter(|stmt| matches!(stmt, ast::Stmt::CaseClause(case) if case.list.is_none()))
        .nth(1)
        .is_some()
}

fn has_duplicate_select_default(block: &ast::BlockStmt<'_>) -> bool {
    block
        .list
        .iter()
        .filter(|stmt| matches!(stmt, ast::Stmt::CommClause(comm) if comm.comm.is_none()))
        .nth(1)
        .is_some()
}

fn invalid_assignment(
    assign: &ast::AssignStmt<'_>,
    env: &TypeEnv,
    scopes: &ShortVarScopes,
) -> Option<InvalidAssignmentReason> {
    if assign.tok.is_assign_op() {
        if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
            return Some(InvalidAssignmentReason::CompoundOperandCount {
                lhs: assign.lhs.len(),
                rhs: assign.rhs.len(),
            });
        }
        if assign.lhs.first().is_some_and(is_blank_ident) {
            return Some(InvalidAssignmentReason::CompoundBlankIdentifier);
        }
        if assign
            .lhs
            .first()
            .is_some_and(|lhs| !assignment_lhs_is_valid(lhs, env))
        {
            return Some(InvalidAssignmentReason::InvalidLeftOperand);
        }
        if let Some(values) = expression_value_count(
            assign.rhs.first()?,
            env,
            TupleAssignmentMode::SingleValueContext,
        )
        .filter(|values| *values != 1)
        {
            return Some(InvalidAssignmentReason::CountMismatch { lhs: 1, values });
        }
        if let Some(reason) = invalid_compound_assignment(assign, env) {
            return Some(reason);
        }
        return None;
    }

    if assign.tok == token::Token::ASSIGN
        && assign
            .lhs
            .iter()
            .any(|lhs| !assignment_lhs_is_valid(lhs, env))
    {
        return Some(InvalidAssignmentReason::InvalidLeftOperand);
    }

    if assign.rhs.len() == 1 {
        let mode = if assign.lhs.len() == 1 {
            TupleAssignmentMode::SingleValueContext
        } else {
            TupleAssignmentMode::AllowTupleOperations
        };
        if let Some(values) = expression_value_count(assign.rhs.first()?, env, mode)
            && values != assign.lhs.len()
        {
            return Some(InvalidAssignmentReason::CountMismatch {
                lhs: assign.lhs.len(),
                values,
            });
        }
        if let Some(reason) = invalid_assignment_type_mismatch(assign, env, scopes) {
            return Some(reason);
        }
        return None;
    }

    for expr in &assign.rhs {
        if let Some(values) =
            expression_value_count(expr, env, TupleAssignmentMode::SingleValueContext)
            && values != 1
        {
            return Some(InvalidAssignmentReason::MultiValueInSingleValueContext);
        }
    }

    if assign.lhs.len() != assign.rhs.len() {
        return Some(InvalidAssignmentReason::CountMismatch {
            lhs: assign.lhs.len(),
            values: assign.rhs.len(),
        });
    }

    if let Some(reason) = invalid_assignment_type_mismatch(assign, env, scopes) {
        return Some(reason);
    }

    None
}

fn invalid_assignment_type_mismatch(
    assign: &ast::AssignStmt<'_>,
    env: &TypeEnv,
    scopes: &ShortVarScopes,
) -> Option<InvalidAssignmentReason> {
    if !matches!(assign.tok, token::Token::ASSIGN | token::Token::DEFINE) {
        return None;
    }
    if let Some(reason) = invalid_assignment_untyped_nil(assign, env, scopes) {
        return Some(reason);
    }
    let actual_types = assignment_rhs_types(assign, env)?;
    if actual_types.len() != assign.lhs.len() {
        return None;
    }
    if assign.rhs.len() == assign.lhs.len() {
        return assign
            .lhs
            .iter()
            .zip(assign.rhs.iter())
            .find_map(|(lhs, rhs)| {
                let expected = assignment_lhs_expected_type(assign.tok, lhs, env, scopes)?;
                let actual = env.resolve_alias(&GoType::infer_expr(rhs, env));
                (!expr_is_assignable_for_validation(&expected, rhs, env)).then(|| {
                    InvalidAssignmentReason::TypeMismatch {
                        expected: go_type_display_name(&expected),
                        actual: go_type_display_name(&actual),
                    }
                })
            });
    }
    assign
        .lhs
        .iter()
        .zip(actual_types.iter())
        .find_map(|(lhs, actual)| {
            let expected = assignment_lhs_expected_type(assign.tok, lhs, env, scopes)?;
            if matches!(expected, GoType::Unknown | GoType::Named(_))
                || matches!(actual, GoType::Unknown | GoType::Named(_))
            {
                return None;
            }
            let actual = env.resolve_alias(actual);
            (!types_are_assignable_for_validation(&expected, &actual)).then(|| {
                InvalidAssignmentReason::TypeMismatch {
                    expected: go_type_display_name(&expected),
                    actual: go_type_display_name(&actual),
                }
            })
        })
}

fn invalid_assignment_untyped_nil(
    assign: &ast::AssignStmt<'_>,
    env: &TypeEnv,
    scopes: &ShortVarScopes,
) -> Option<InvalidAssignmentReason> {
    if assign.lhs.len() != assign.rhs.len() {
        return None;
    }
    assign
        .lhs
        .iter()
        .zip(assign.rhs.iter())
        .find_map(|(lhs, rhs)| {
            if !expr_is_nil(rhs) {
                return None;
            }
            let Some(expected) = assignment_lhs_expected_type(assign.tok, lhs, env, scopes) else {
                return Some(InvalidAssignmentReason::UntypedNil);
            };
            (!type_can_compare_to_nil(&expected, env)).then(|| {
                InvalidAssignmentReason::TypeMismatch {
                    expected: go_type_display_name(&expected),
                    actual: "nil".to_string(),
                }
            })
        })
}

fn assignment_lhs_expected_type(
    tok: token::Token,
    lhs: &ast::Expr<'_>,
    env: &TypeEnv,
    scopes: &ShortVarScopes,
) -> Option<GoType> {
    if is_blank_ident(lhs) {
        return None;
    }
    match tok {
        token::Token::ASSIGN => Some(env.resolve_alias(&GoType::infer_expr(lhs, env))),
        token::Token::DEFINE => short_var_decl_ident_name(lhs)
            .filter(|name| scopes.contains_current(name))
            .and_then(|name| env.get_var(name))
            .map(|ty| env.resolve_alias(&ty)),
        _ => None,
    }
}

fn invalid_compound_assignment(
    assign: &ast::AssignStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidAssignmentReason> {
    let lhs = assign.lhs.first()?;
    let rhs = assign.rhs.first()?;
    let lhs_ty = env.resolve_alias(&GoType::infer_expr(lhs, env));
    let rhs_ty = env.resolve_alias(&GoType::infer_expr(rhs, env));
    if !matches!(
        assign.tok,
        token::Token::SHL_ASSIGN | token::Token::SHR_ASSIGN
    ) && !expr_is_assignable_for_validation(&lhs_ty, rhs, env)
    {
        return Some(InvalidAssignmentReason::TypeMismatch {
            expected: go_type_display_name(&lhs_ty),
            actual: go_type_display_name(&rhs_ty),
        });
    }
    if matches!(lhs_ty, GoType::Unknown | GoType::Named(_))
        || matches!(rhs_ty, GoType::Unknown | GoType::Named(_))
    {
        return None;
    }
    let op = compound_assignment_op_name(assign.tok).to_string();
    match assign.tok {
        token::Token::ADD_ASSIGN if go_type_is_numeric(&lhs_ty) || lhs_ty.is_string() => None,
        token::Token::SUB_ASSIGN | token::Token::MUL_ASSIGN | token::Token::QUO_ASSIGN
            if go_type_is_numeric(&lhs_ty) =>
        {
            None
        }
        token::Token::REM_ASSIGN
        | token::Token::AND_ASSIGN
        | token::Token::OR_ASSIGN
        | token::Token::XOR_ASSIGN
        | token::Token::AND_NOT_ASSIGN
            if lhs_ty.is_integer() =>
        {
            None
        }
        token::Token::SHL_ASSIGN | token::Token::SHR_ASSIGN if lhs_ty.is_integer() => {
            if binary_operand_is_integer(&rhs_ty, rhs) {
                None
            } else {
                Some(invalid_compound_operand(&op, "right", &rhs_ty))
            }
        }
        _ => Some(invalid_compound_operand(&op, "left", &lhs_ty)),
    }
}

fn invalid_compound_operand(op: &str, side: &str, ty: &GoType) -> InvalidAssignmentReason {
    InvalidAssignmentReason::CompoundInvalidOperand {
        op: op.to_string(),
        side: side.to_string(),
        type_name: go_type_display_name(ty),
    }
}

fn compound_assignment_op_name(tok: token::Token) -> &'static str {
    match tok {
        token::Token::ADD_ASSIGN => "+=",
        token::Token::SUB_ASSIGN => "-=",
        token::Token::MUL_ASSIGN => "*=",
        token::Token::QUO_ASSIGN => "/=",
        token::Token::REM_ASSIGN => "%=",
        token::Token::AND_ASSIGN => "&=",
        token::Token::OR_ASSIGN => "|=",
        token::Token::XOR_ASSIGN => "^=",
        token::Token::SHL_ASSIGN => "<<=",
        token::Token::SHR_ASSIGN => ">>=",
        token::Token::AND_NOT_ASSIGN => "&^=",
        _ => "compound assignment",
    }
}

fn assignment_rhs_types(assign: &ast::AssignStmt<'_>, env: &TypeEnv) -> Option<Vec<GoType>> {
    if assign.rhs.len() == 1 && assign.lhs.len() > 1 {
        let ast::Expr::CallExpr(call) = unparen_expr(assign.rhs.first()?) else {
            return None;
        };
        return call_result_types(call, env).filter(|types| types.len() == assign.lhs.len());
    }
    (assign.rhs.len() == assign.lhs.len()).then(|| {
        assign
            .rhs
            .iter()
            .map(|expr| GoType::infer_expr(expr, env))
            .collect()
    })
}

fn assignment_lhs_is_valid(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    is_blank_ident(expr)
        || is_map_index_expr(expr, env)
        || expr_addressability(expr, env) == Addressability::Addressable
}

fn is_map_index_expr(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    let ast::Expr::IndexExpr(index) = unparen_expr(expr) else {
        return false;
    };
    matches!(
        env.resolve_alias(&GoType::infer_expr(&index.x, env)),
        GoType::Map(_, _)
    )
}

fn invalid_condition(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
    kind: ConditionKind,
) -> Option<InvalidConditionReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(expr, env));
    if matches!(ty, GoType::Bool | GoType::Unknown | GoType::Named(_)) {
        return None;
    }
    Some(InvalidConditionReason {
        kind,
        type_name: go_type_display_name(&ty),
    })
}

fn invalid_send(send: &ast::SendStmt<'_>, env: &TypeEnv) -> Option<InvalidSendReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(&send.chan, env));
    match ty {
        GoType::Chan { elem, direction } if direction.can_send() => {
            invalid_send_value_type(&elem, &send.value, env)
        }
        GoType::Chan { .. } => Some(InvalidSendReason::ReceiveOnlyChannel),
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(InvalidSendReason::NonChannel {
            type_name: go_type_display_name(&other),
        }),
    }
}

fn invalid_send_value_type(
    expected: &GoType,
    value: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidSendReason> {
    let expected = env.resolve_alias(expected);
    if expr_is_nil(value) && !type_can_compare_to_nil(&expected, env) {
        return Some(InvalidSendReason::ValueTypeMismatch {
            expected: go_type_display_name(&expected),
            actual: "nil".to_string(),
        });
    }
    let actual = env.resolve_alias(&GoType::infer_expr(value, env));
    if expr_is_assignable_for_validation(&expected, value, env) {
        return None;
    }
    Some(InvalidSendReason::ValueTypeMismatch {
        expected: go_type_display_name(&expected),
        actual: go_type_display_name(&actual),
    })
}

fn types_are_assignable_for_validation(expected: &GoType, actual: &GoType) -> bool {
    match (expected, actual) {
        (GoType::Unknown | GoType::Named(_), _) | (_, GoType::Unknown | GoType::Named(_)) => true,
        (_, GoType::Any) => true,
        (GoType::Any | GoType::Interface(_) | GoType::Error, _) => true,
        (expected, actual) if go_type_is_numeric(expected) && go_type_is_numeric(actual) => true,
        (GoType::Bool, GoType::Bool) | (GoType::String, GoType::String) => true,
        (GoType::Bool, _) | (_, GoType::Bool) | (GoType::String, _) | (_, GoType::String) => false,
        _ => true,
    }
}

fn expr_is_assignable_for_validation(
    expected: &GoType,
    value: &ast::Expr<'_>,
    env: &TypeEnv,
) -> bool {
    let expected = env.resolve_alias(expected);
    let actual = env.resolve_alias(&GoType::infer_expr(value, env));
    if matches!(expected, GoType::Unknown | GoType::Named(_))
        || matches!(actual, GoType::Unknown | GoType::Named(_))
    {
        return true;
    }
    comparison_operand_is_assignable_to(value, &actual, &expected, env)
}

fn go_type_is_numeric(ty: &GoType) -> bool {
    ty.is_numeric() || matches!(ty, GoType::Complex64 | GoType::Complex128)
}

fn go_type_is_ordered_numeric(ty: &GoType) -> bool {
    ty.is_integer() || ty.is_float()
}

fn invalid_select_comm_stmt(stmt: &ast::Stmt<'_>) -> Option<InvalidSelectCommReason> {
    match stmt {
        ast::Stmt::SendStmt(_) => None,
        ast::Stmt::ExprStmt(expr) if expr_is_receive_operation(&expr.x) => None,
        ast::Stmt::AssignStmt(assign) => invalid_select_recv_assignment(assign),
        _ => Some(InvalidSelectCommReason::NonCommunication),
    }
}

fn invalid_select_recv_assignment(assign: &ast::AssignStmt<'_>) -> Option<InvalidSelectCommReason> {
    if !matches!(assign.tok, token::Token::ASSIGN | token::Token::DEFINE) {
        return Some(InvalidSelectCommReason::InvalidAssignmentToken);
    }
    if assign.rhs.len() != 1 || !assign.rhs.first().is_some_and(expr_is_receive_operation) {
        return Some(InvalidSelectCommReason::MissingReceiveExpression);
    }
    if assign.tok == token::Token::DEFINE
        && !assign
            .lhs
            .iter()
            .all(|expr| matches!(expr, ast::Expr::Ident(_)))
    {
        return Some(InvalidSelectCommReason::ShortReceiveDeclarationLhs);
    }
    None
}

fn invalid_receive_expr(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<InvalidReceiveReason> {
    match expr {
        ast::Expr::ArrayType(array) => array
            .len
            .as_ref()
            .and_then(|len| invalid_receive_expr(len, env))
            .or_else(|| invalid_receive_expr(&array.elt, env)),
        ast::Expr::BinaryExpr(binary) => {
            invalid_receive_expr(&binary.x, env).or_else(|| invalid_receive_expr(&binary.y, env))
        }
        ast::Expr::CallExpr(call) => invalid_receive_expr(&call.fun, env).or_else(|| {
            call.args
                .as_ref()
                .and_then(|args| args.iter().find_map(|arg| invalid_receive_expr(arg, env)))
        }),
        ast::Expr::ChanType(chan) => invalid_receive_expr(&chan.value, env),
        ast::Expr::CompositeLit(comp) => comp
            .type_
            .as_ref()
            .and_then(|type_| invalid_receive_expr(type_, env))
            .or_else(|| {
                comp.elts
                    .as_ref()
                    .and_then(|elts| elts.iter().find_map(|elt| invalid_receive_expr(elt, env)))
            }),
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|elt| invalid_receive_expr(elt, env)),
        ast::Expr::FuncLit(func_lit) => invalid_receive_in_block(&func_lit.body, env),
        ast::Expr::FuncType(func_type) => invalid_receive_in_field_list(&func_type.params, env)
            .or_else(|| {
                func_type
                    .results
                    .as_ref()
                    .and_then(|results| invalid_receive_in_field_list(results, env))
            }),
        ast::Expr::IndexExpr(index) => {
            invalid_receive_expr(&index.x, env).or_else(|| invalid_receive_expr(&index.index, env))
        }
        ast::Expr::IndexListExpr(index) => invalid_receive_expr(&index.x, env).or_else(|| {
            index
                .indices
                .iter()
                .find_map(|index| invalid_receive_expr(index, env))
        }),
        ast::Expr::InterfaceType(interface) => interface
            .methods
            .as_ref()
            .and_then(|methods| invalid_receive_in_field_list(methods, env)),
        ast::Expr::KeyValueExpr(kv) => {
            invalid_receive_expr(&kv.key, env).or_else(|| invalid_receive_expr(&kv.value, env))
        }
        ast::Expr::MapType(map) => {
            invalid_receive_expr(&map.key, env).or_else(|| invalid_receive_expr(&map.value, env))
        }
        ast::Expr::ParenExpr(paren) => invalid_receive_expr(&paren.x, env),
        ast::Expr::SelectorExpr(selector) => invalid_receive_expr(&selector.x, env),
        ast::Expr::SliceExpr(slice) => invalid_receive_expr(&slice.x, env)
            .or_else(|| {
                slice
                    .low
                    .as_ref()
                    .and_then(|low| invalid_receive_expr(low, env))
            })
            .or_else(|| {
                slice
                    .high
                    .as_ref()
                    .and_then(|high| invalid_receive_expr(high, env))
            })
            .or_else(|| {
                slice
                    .max
                    .as_ref()
                    .and_then(|max| invalid_receive_expr(max, env))
            }),
        ast::Expr::StarExpr(star) => invalid_receive_expr(&star.x, env),
        ast::Expr::StructType(struct_type) => struct_type
            .fields
            .as_ref()
            .and_then(|fields| invalid_receive_in_field_list(fields, env)),
        ast::Expr::TypeAssertExpr(assert) => invalid_receive_expr(&assert.x, env).or_else(|| {
            assert
                .type_
                .as_ref()
                .and_then(|type_| invalid_receive_expr(type_, env))
        }),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ARROW => {
            let ty = env.resolve_alias(&GoType::infer_expr(&unary.x, env));
            match ty {
                GoType::Chan { direction, .. } if direction.can_receive() => {
                    invalid_receive_expr(&unary.x, env)
                }
                GoType::Chan { .. } => Some(InvalidReceiveReason::SendOnlyChannel),
                GoType::Unknown | GoType::Named(_) => invalid_receive_expr(&unary.x, env),
                other => Some(InvalidReceiveReason::NonChannel {
                    type_name: go_type_display_name(&other),
                }),
            }
        }
        ast::Expr::UnaryExpr(unary) => invalid_receive_expr(&unary.x, env),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_receive_in_field_list(
    fields: &ast::FieldList<'_>,
    env: &TypeEnv,
) -> Option<InvalidReceiveReason> {
    fields.list.iter().find_map(|field| {
        field
            .type_
            .as_ref()
            .and_then(|type_| invalid_receive_expr(type_, env))
    })
}

fn invalid_receive_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidReceiveReason> {
    let mut block_env = env.clone();
    for stmt in &block.list {
        if let Some(reason) = invalid_receive_in_stmt(stmt, &mut block_env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_receive_in_stmt(
    stmt: &ast::Stmt<'_>,
    env: &mut TypeEnv,
) -> Option<InvalidReceiveReason> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            let invalid = assign
                .lhs
                .iter()
                .chain(assign.rhs.iter())
                .find_map(|expr| invalid_receive_expr(expr, env));
            record_define_bindings(assign, env);
            invalid
        }
        ast::Stmt::BlockStmt(block) => invalid_receive_in_block(block, env),
        ast::Stmt::DeclStmt(decl) => {
            let invalid = invalid_receive_in_gen_decl(&decl.decl, env);
            record_decl_bindings(&decl.decl, env);
            invalid
        }
        ast::Stmt::ExprStmt(expr) => invalid_receive_expr(&expr.x, env),
        _ => None,
    }
}

fn invalid_receive_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidReceiveReason> {
    gen_decl.specs.iter().find_map(|spec| match spec {
        ast::Spec::ValueSpec(value_spec) => value_spec
            .type_
            .as_ref()
            .and_then(|type_| invalid_receive_expr(type_, env))
            .or_else(|| {
                value_spec.values.as_ref().and_then(|values| {
                    values
                        .iter()
                        .find_map(|value| invalid_receive_expr(value, env))
                })
            }),
        ast::Spec::TypeSpec(type_spec) => invalid_receive_expr(&type_spec.type_, env),
        ast::Spec::ImportSpec(_) => None,
    })
}

fn invalid_inc_dec(inc_dec: &ast::IncDecStmt<'_>, env: &TypeEnv) -> Option<InvalidIncDecReason> {
    if is_blank_ident(&inc_dec.x) || !assignment_lhs_is_valid(&inc_dec.x, env) {
        return Some(InvalidIncDecReason::InvalidOperand);
    }
    let ty = env.resolve_alias(&GoType::infer_expr(&inc_dec.x, env));
    if matches!(ty, GoType::Unknown | GoType::Named(_)) || inc_dec_operand_is_numeric(&ty) {
        return None;
    }
    Some(InvalidIncDecReason::NonNumericOperand)
}

fn inc_dec_operand_is_numeric(ty: &GoType) -> bool {
    ty.is_integer() || ty.is_float() || matches!(ty, GoType::Complex64 | GoType::Complex128)
}

#[derive(Clone, Copy)]
enum TupleAssignmentMode {
    AllowTupleOperations,
    SingleValueContext,
}

fn expression_value_count(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
    mode: TupleAssignmentMode,
) -> Option<usize> {
    match expr {
        ast::Expr::CallExpr(call) => call_result_count(call, env),
        ast::Expr::IndexExpr(index) => {
            match env.resolve_alias(&GoType::infer_expr(&index.x, env)) {
                GoType::Map(_, _) if matches!(mode, TupleAssignmentMode::AllowTupleOperations) => {
                    Some(2)
                }
                GoType::Unknown if matches!(mode, TupleAssignmentMode::AllowTupleOperations) => {
                    None
                }
                _ => Some(1),
            }
        }
        ast::Expr::ParenExpr(paren) => expression_value_count(&paren.x, env, mode),
        ast::Expr::TypeAssertExpr(type_assert) => {
            if type_assert.type_.is_some()
                && matches!(mode, TupleAssignmentMode::AllowTupleOperations)
            {
                Some(2)
            } else {
                Some(1)
            }
        }
        ast::Expr::UnaryExpr(unary)
            if unary.op == token::Token::ARROW
                && matches!(mode, TupleAssignmentMode::AllowTupleOperations) =>
        {
            Some(2)
        }
        _ => Some(1),
    }
}

fn call_result_count(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<usize> {
    if call_is_type_conversion(call, env) {
        return Some(1);
    }
    call_result_count_for_fun(&call.fun, env)
}

fn call_result_types(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<Vec<GoType>> {
    if call_is_type_conversion(call, env) {
        return None;
    }
    call_result_types_for_fun(&call.fun, env)
}

fn call_result_count_for_fun(fun: &ast::Expr<'_>, env: &TypeEnv) -> Option<usize> {
    match fun {
        ast::Expr::Ident(id) => {
            if env.has_func(id.name) {
                return Some(env.get_func_returns(id.name).len());
            }
            match env.get_var(id.name) {
                Some(GoType::Func { results, .. }) => Some(results.len()),
                Some(_) => None,
                None => builtin_result_count(id.name),
            }
        }
        ast::Expr::SelectorExpr(sel) => {
            if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                if env.has_func(&package_key) {
                    return Some(env.get_func_returns(&package_key).len());
                }

                if let Some(GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                    let method_key = format!("{}.{}", name, sel.sel.name);
                    if env.has_func(&method_key) {
                        return Some(env.get_func_returns(&method_key).len());
                    }
                }
            }

            match GoType::infer_expr(fun, env) {
                GoType::Func { results, .. } => Some(results.len()),
                _ => None,
            }
        }
        ast::Expr::FuncLit(func_lit) => {
            Some(field_list_binding_count(func_lit.type_.results.as_ref()))
        }
        ast::Expr::ParenExpr(paren) => call_result_count_for_fun(&paren.x, env),
        other => match GoType::infer_expr(other, env) {
            GoType::Func { results, .. } => Some(results.len()),
            _ => None,
        },
    }
}

fn call_result_types_for_fun(fun: &ast::Expr<'_>, env: &TypeEnv) -> Option<Vec<GoType>> {
    match fun {
        ast::Expr::Ident(id) => {
            if env.has_func(id.name) {
                return Some(env.get_func_returns(id.name));
            }
            match env.get_var(id.name) {
                Some(GoType::Func { results, .. }) => Some(results),
                Some(_) => None,
                None => builtin_result_types(id.name),
            }
        }
        ast::Expr::SelectorExpr(sel) => {
            if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                if env.has_func(&package_key) {
                    return Some(env.get_func_returns(&package_key));
                }

                if let Some(GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                    let method_key = format!("{}.{}", name, sel.sel.name);
                    if env.has_func(&method_key) {
                        return Some(env.get_func_returns(&method_key));
                    }
                }
            }

            match GoType::infer_expr(fun, env) {
                GoType::Func { results, .. } => Some(results),
                _ => None,
            }
        }
        ast::Expr::FuncLit(func_lit) => Some(field_list_types(func_lit.type_.results.as_ref())),
        ast::Expr::ParenExpr(paren) => call_result_types_for_fun(&paren.x, env),
        other => match GoType::infer_expr(other, env) {
            GoType::Func { results, .. } => Some(results),
            _ => None,
        },
    }
}

fn builtin_result_count(name: &str) -> Option<usize> {
    match name {
        "append" | "cap" | "complex" | "copy" | "imag" | "len" | "make" | "max" | "min" | "new"
        | "real" | "recover" => Some(1),
        "clear" | "close" | "delete" | "panic" | "print" | "println" => Some(0),
        _ => None,
    }
}

fn builtin_result_types(name: &str) -> Option<Vec<GoType>> {
    match name {
        "cap" | "copy" | "len" => Some(vec![GoType::Int]),
        "imag" | "real" => Some(vec![GoType::Float64]),
        "complex" => Some(vec![GoType::Complex128]),
        "recover" => Some(vec![GoType::Any]),
        "append" | "make" | "max" | "min" | "new" => Some(vec![GoType::Unknown]),
        "clear" | "close" | "delete" | "panic" | "print" | "println" => Some(Vec::new()),
        _ => None,
    }
}

fn field_list_binding_count(fields: Option<&ast::FieldList<'_>>) -> usize {
    fields.map_or(0, |fields| {
        fields
            .list
            .iter()
            .map(|field| field.names.as_ref().map_or(1, Vec::len))
            .sum()
    })
}

#[derive(Clone)]
struct ReturnSignature {
    count: usize,
    named: bool,
    types: Vec<GoType>,
}

fn return_signature(func_type: &ast::FuncType<'_>) -> ReturnSignature {
    let types = field_list_types(func_type.results.as_ref());
    let count = types.len();
    let named = func_type.results.as_ref().is_some_and(|results| {
        results
            .list
            .iter()
            .any(|field| field.names.as_ref().is_some_and(|names| !names.is_empty()))
    });
    ReturnSignature {
        count,
        named,
        types,
    }
}

fn field_list_types(fields: Option<&ast::FieldList<'_>>) -> Vec<GoType> {
    fields.map_or_else(Vec::new, |fields| {
        fields
            .list
            .iter()
            .flat_map(|field| {
                let ty = field
                    .type_
                    .as_ref()
                    .map(GoType::from_expr)
                    .unwrap_or(GoType::Unknown);
                let count = field.names.as_ref().map_or(1, Vec::len);
                std::iter::repeat_n(ty, count)
            })
            .collect()
    })
}

fn record_func_type_bindings(func_type: &ast::FuncType<'_>, env: &mut TypeEnv) {
    record_field_list_bindings(Some(&func_type.params), env);
    record_field_list_bindings(func_type.results.as_ref(), env);
}

fn record_field_list_bindings(fields: Option<&ast::FieldList<'_>>, env: &mut TypeEnv) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        let ty = field
            .type_
            .as_ref()
            .map(GoType::from_expr)
            .unwrap_or(GoType::Unknown);
        if let Some(names) = &field.names {
            for name in names {
                if name.name != "_" {
                    env.set_var(name.name, ty.clone());
                }
            }
        }
    }
}

fn invalid_return_in_block(
    block: &ast::BlockStmt<'_>,
    signature: &ReturnSignature,
    env: &mut TypeEnv,
) -> Option<InvalidStatement> {
    for stmt in &block.list {
        if let Some(invalid) = invalid_return_in_stmt(stmt, signature, env) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_return_in_stmt(
    stmt: &ast::Stmt<'_>,
    signature: &ReturnSignature,
    env: &mut TypeEnv,
) -> Option<InvalidStatement> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                if let Some(invalid) = invalid_return_in_expr(expr, env) {
                    return Some(invalid);
                }
            }
            record_define_bindings(assign, env);
            None
        }
        ast::Stmt::BlockStmt(block) => {
            let mut block_env = env.clone();
            invalid_return_in_block(block, signature, &mut block_env)
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::CaseClause(case) => {
            let mut case_env = env.clone();
            if let Some(list) = &case.list {
                for expr in list {
                    if let Some(invalid) = invalid_return_in_expr(expr, &case_env) {
                        return Some(invalid);
                    }
                }
            }
            invalid_return_in_stmt_list(&case.body, signature, &mut case_env)
        }
        ast::Stmt::CommClause(comm) => {
            let mut comm_env = env.clone();
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_return_in_stmt(comm, signature, &mut comm_env)
            {
                return Some(invalid);
            }
            invalid_return_in_stmt_list(&comm.body, signature, &mut comm_env)
        }
        ast::Stmt::DeclStmt(decl) => {
            if let Some(invalid) = invalid_return_in_gen_decl(&decl.decl, env) {
                return Some(invalid);
            }
            record_decl_bindings(&decl.decl, env);
            None
        }
        ast::Stmt::DeferStmt(defer) => invalid_return_in_call_statement(&defer.call, env),
        ast::Stmt::ExprStmt(expr) => invalid_return_in_statement_expr(&expr.x, env),
        ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_return_in_stmt(init, signature, &mut loop_env)
            {
                return Some(invalid);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(invalid) = invalid_return_in_expr(cond, &loop_env)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_return_in_stmt(post, signature, &mut loop_env)
            {
                return Some(invalid);
            }
            invalid_return_in_block(&for_stmt.body, signature, &mut loop_env)
        }
        ast::Stmt::GoStmt(go) => invalid_return_in_call_statement(&go.call, env),
        ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_return_in_stmt(init, signature, &mut if_env)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_return_in_expr(&if_stmt.cond, &if_env) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_return_in_block(&if_stmt.body, signature, &mut if_env) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                let mut else_env = if_env;
                return invalid_return_in_stmt(else_branch, signature, &mut else_env);
            }
            None
        }
        ast::Stmt::IncDecStmt(inc_dec) => invalid_return_in_expr(&inc_dec.x, env),
        ast::Stmt::LabeledStmt(labeled) => invalid_return_in_stmt(&labeled.stmt, signature, env),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key
                && let Some(invalid) = invalid_return_in_expr(key, env)
            {
                return Some(invalid);
            }
            if let Some(value) = &range.value
                && let Some(invalid) = invalid_return_in_expr(value, env)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_return_in_expr(&range.x, env) {
                return Some(invalid);
            }
            let mut range_env = env.clone();
            record_range_bindings(range, &mut range_env);
            invalid_return_in_block(&range.body, signature, &mut range_env)
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                if let Some(invalid) = invalid_return_in_expr(expr, env) {
                    return Some(invalid);
                }
            }
            invalid_return_stmt(ret, signature, env)
                .map(|reason| InvalidStatement::Return { reason })
        }
        ast::Stmt::SelectStmt(select) => {
            let mut select_env = env.clone();
            invalid_return_in_block(&select.body, signature, &mut select_env)
        }
        ast::Stmt::SendStmt(send) => invalid_return_in_expr(&send.chan, env)
            .or_else(|| invalid_return_in_expr(&send.value, env)),
        ast::Stmt::SwitchStmt(switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_return_in_stmt(init, signature, &mut switch_env)
            {
                return Some(invalid);
            }
            if let Some(tag) = &switch.tag
                && let Some(invalid) = invalid_return_in_expr(tag, &switch_env)
            {
                return Some(invalid);
            }
            invalid_return_in_block(&switch.body, signature, &mut switch_env)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_return_in_stmt(init, signature, &mut switch_env)
            {
                return Some(invalid);
            }
            if let Some(invalid) =
                invalid_return_in_stmt(&type_switch.assign, signature, &mut switch_env)
            {
                return Some(invalid);
            }
            invalid_return_in_block(&type_switch.body, signature, &mut switch_env)
        }
    }
}

fn invalid_return_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    signature: &ReturnSignature,
    env: &mut TypeEnv,
) -> Option<InvalidStatement> {
    for stmt in stmts {
        if let Some(invalid) = invalid_return_in_stmt(stmt, signature, env) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_return_stmt(
    ret: &ast::ReturnStmt<'_>,
    signature: &ReturnSignature,
    env: &TypeEnv,
) -> Option<InvalidReturnReason> {
    if ret.results.is_empty() {
        if signature.count == 0 || signature.named {
            return None;
        }
        return Some(InvalidReturnReason::CountMismatch {
            expected: signature.count,
            values: 0,
        });
    }

    if signature.count == 0 {
        let values = if ret.results.len() == 1 {
            expression_value_count(
                ret.results.first()?,
                env,
                TupleAssignmentMode::AllowTupleOperations,
            )
            .unwrap_or(1)
            .max(1)
        } else {
            ret.results.len()
        };
        return Some(InvalidReturnReason::CountMismatch {
            expected: 0,
            values,
        });
    }

    if ret.results.len() == 1 {
        let mode = if signature.count == 1 {
            TupleAssignmentMode::SingleValueContext
        } else {
            TupleAssignmentMode::AllowTupleOperations
        };
        if let Some(values) = expression_value_count(ret.results.first()?, env, mode)
            && values != signature.count
        {
            return Some(InvalidReturnReason::CountMismatch {
                expected: signature.count,
                values,
            });
        }
        return invalid_return_type_mismatch(&ret.results, signature, env);
    }

    for expr in &ret.results {
        if let Some(values) =
            expression_value_count(expr, env, TupleAssignmentMode::SingleValueContext)
            && values != 1
        {
            return Some(InvalidReturnReason::MultiValueInSingleValueContext);
        }
    }

    if ret.results.len() != signature.count {
        return Some(InvalidReturnReason::CountMismatch {
            expected: signature.count,
            values: ret.results.len(),
        });
    }

    invalid_return_type_mismatch(&ret.results, signature, env)
}

fn invalid_return_type_mismatch(
    results: &[ast::Expr<'_>],
    signature: &ReturnSignature,
    env: &TypeEnv,
) -> Option<InvalidReturnReason> {
    if let Some(reason) = invalid_return_nil_mismatch(results, signature, env) {
        return Some(reason);
    }
    let actual_types = return_result_types(results, signature, env)?;
    if actual_types.len() != signature.types.len() {
        return None;
    }
    if results.len() != signature.types.len() {
        return signature
            .types
            .iter()
            .zip(actual_types.iter())
            .find_map(|(expected, actual)| {
                let expected = env.resolve_alias(expected);
                let actual = env.resolve_alias(actual);
                (!types_are_assignable_for_validation(&expected, &actual)).then(|| {
                    InvalidReturnReason::TypeMismatch {
                        expected: go_type_display_name(&expected),
                        actual: go_type_display_name(&actual),
                    }
                })
            });
    }
    signature
        .types
        .iter()
        .zip(results.iter())
        .zip(actual_types.iter())
        .find_map(|((expected, result), actual)| {
            let expected = env.resolve_alias(expected);
            let actual = env.resolve_alias(actual);
            (!expr_is_assignable_for_validation(&expected, result, env)).then(|| {
                InvalidReturnReason::TypeMismatch {
                    expected: go_type_display_name(&expected),
                    actual: go_type_display_name(&actual),
                }
            })
        })
}

fn invalid_return_nil_mismatch(
    results: &[ast::Expr<'_>],
    signature: &ReturnSignature,
    env: &TypeEnv,
) -> Option<InvalidReturnReason> {
    if results.len() != signature.types.len() {
        return None;
    }
    signature
        .types
        .iter()
        .zip(results.iter())
        .filter(|(_, result)| expr_is_nil(result))
        .find_map(|(expected, _)| {
            let expected = env.resolve_alias(expected);
            (!type_can_compare_to_nil(&expected, env)).then(|| InvalidReturnReason::TypeMismatch {
                expected: go_type_display_name(&expected),
                actual: "nil".to_string(),
            })
        })
}

fn return_result_types(
    results: &[ast::Expr<'_>],
    signature: &ReturnSignature,
    env: &TypeEnv,
) -> Option<Vec<GoType>> {
    if results.len() == 1 && signature.count > 1 {
        let ast::Expr::CallExpr(call) = unparen_expr(results.first()?) else {
            return None;
        };
        return call_result_types(call, env).filter(|types| types.len() == signature.count);
    }
    (results.len() == signature.count).then(|| {
        results
            .iter()
            .map(|expr| GoType::infer_expr(expr, env))
            .collect()
    })
}

fn invalid_return_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    for spec in &gen_decl.specs {
        if let Some(invalid) = invalid_return_in_spec(spec, env) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_return_in_spec(spec: &ast::Spec<'_>, env: &TypeEnv) -> Option<InvalidStatement> {
    match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => invalid_return_in_expr(&type_spec.type_, env),
        ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_) = &value_spec.type_
                && let Some(invalid) = invalid_return_in_expr(type_, env)
            {
                return Some(invalid);
            }
            if let Some(values) = &value_spec.values {
                for value in values {
                    if let Some(invalid) = invalid_return_in_expr(value, env) {
                        return Some(invalid);
                    }
                }
            }
            None
        }
    }
}

fn invalid_return_in_call(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<InvalidStatement> {
    if let Some(reason) = invalid_builtin_call_expression(call, env) {
        return Some(InvalidStatement::Expression { reason });
    }
    if let Some(invalid) = invalid_return_in_expr(&call.fun, env) {
        return Some(invalid);
    }
    if let Some(args) = &call.args {
        for arg in args {
            if let Some(invalid) = invalid_return_in_expr(arg, env) {
                return Some(invalid);
            }
        }
    }
    if let Some(reason) = invalid_type_conversion_call(call, env) {
        return Some(InvalidStatement::Expression { reason });
    }
    if let Some(reason) = invalid_ordinary_call(call, env) {
        return Some(InvalidStatement::Expression { reason });
    }
    None
}

fn invalid_return_in_call_statement(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    invalid_expression_in_call_statement(call, env)
        .map(|reason| InvalidStatement::Expression { reason })
}

fn invalid_return_in_statement_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    match unparen_expr(expr) {
        ast::Expr::CallExpr(call) => invalid_return_in_call_statement(call, env),
        _ => invalid_return_in_expr(expr, env),
    }
}

fn invalid_return_in_expr(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<InvalidStatement> {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len
                && let Some(invalid) = invalid_return_in_expr(len, env)
            {
                return Some(invalid);
            }
            invalid_return_in_expr(&array.elt, env)
        }
        ast::Expr::BinaryExpr(binary) => invalid_return_in_expr(&binary.x, env)
            .or_else(|| invalid_return_in_expr(&binary.y, env))
            .or_else(|| {
                invalid_binary_expr(binary, env)
                    .map(|reason| InvalidStatement::Expression { reason })
            }),
        ast::Expr::CallExpr(call) => invalid_return_in_call(call, env),
        ast::Expr::ChanType(chan) => invalid_return_in_expr(&chan.value, env),
        ast::Expr::CompositeLit(comp) => {
            if let Some(type_) = &comp.type_
                && let Some(invalid) = invalid_return_in_expr(type_, env)
            {
                return Some(invalid);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    if let Some(invalid) = invalid_return_in_expr(elt, env) {
                        return Some(invalid);
                    }
                }
            }
            invalid_composite_lit(comp, env).map(|reason| InvalidStatement::Expression { reason })
        }
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|expr| invalid_return_in_expr(expr, env)),
        ast::Expr::FuncLit(func_lit) => {
            invalid_return_in_func(&func_lit.type_, &func_lit.body, env)
                .or_else(|| invalid_body_completion_in_func(&func_lit.type_, &func_lit.body, env))
        }
        ast::Expr::FuncType(_) => None,
        ast::Expr::IndexExpr(index) => invalid_return_in_expr(&index.x, env)
            .or_else(|| invalid_return_in_expr(&index.index, env))
            .or_else(|| {
                invalid_index_expr(index, env).map(|reason| InvalidStatement::Expression { reason })
            }),
        ast::Expr::IndexListExpr(index) => {
            if let Some(invalid) = invalid_return_in_expr(&index.x, env) {
                return Some(invalid);
            }
            for index in &index.indices {
                if let Some(invalid) = invalid_return_in_expr(index, env) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Expr::InterfaceType(interface) => interface.methods.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_return_in_expr(type_, env)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::KeyValueExpr(kv) => {
            invalid_return_in_expr(&kv.key, env).or_else(|| invalid_return_in_expr(&kv.value, env))
        }
        ast::Expr::MapType(map) => invalid_return_in_expr(&map.key, env)
            .or_else(|| invalid_return_in_expr(&map.value, env)),
        ast::Expr::ParenExpr(paren) => invalid_return_in_expr(&paren.x, env),
        ast::Expr::SelectorExpr(selector) => invalid_return_in_expr(&selector.x, env),
        ast::Expr::SliceExpr(slice) => {
            if let Some(invalid) = invalid_return_in_expr(&slice.x, env) {
                return Some(invalid);
            }
            if let Some(low) = &slice.low
                && let Some(invalid) = invalid_return_in_expr(low, env)
            {
                return Some(invalid);
            }
            if let Some(high) = &slice.high
                && let Some(invalid) = invalid_return_in_expr(high, env)
            {
                return Some(invalid);
            }
            if let Some(max) = &slice.max
                && let Some(invalid) = invalid_return_in_expr(max, env)
            {
                return Some(invalid);
            }
            invalid_slice_expr(slice, env).map(|reason| InvalidStatement::Expression { reason })
        }
        ast::Expr::StarExpr(star) => invalid_return_in_expr(&star.x, env).or_else(|| {
            invalid_star_expr(star, env).map(|reason| InvalidStatement::Expression { reason })
        }),
        ast::Expr::StructType(struct_type) => struct_type.fields.as_ref().and_then(|fields| {
            for field in &fields.list {
                if let Some(type_) = &field.type_
                    && let Some(invalid) = invalid_return_in_expr(type_, env)
                {
                    return Some(invalid);
                }
            }
            None
        }),
        ast::Expr::TypeAssertExpr(assert) => {
            if let Some(invalid) = invalid_return_in_expr(&assert.x, env) {
                return Some(invalid);
            }
            assert
                .type_
                .as_ref()
                .and_then(|ty| invalid_return_in_expr(ty, env))
        }
        ast::Expr::UnaryExpr(unary) => invalid_return_in_expr(&unary.x, env).or_else(|| {
            invalid_unary_expr(unary, env).map(|reason| InvalidStatement::Expression { reason })
        }),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_type_switch_guard(stmt: &ast::Stmt<'_>) -> Option<InvalidTypeSwitchGuardReason> {
    match stmt {
        ast::Stmt::ExprStmt(expr) => (!is_type_switch_guard_expr(&expr.x))
            .then_some(InvalidTypeSwitchGuardReason::InvalidExpression),
        ast::Stmt::AssignStmt(assign) => {
            if assign.tok != token::Token::DEFINE {
                return Some(InvalidTypeSwitchGuardReason::InvalidAssignmentToken);
            }
            if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
                return Some(InvalidTypeSwitchGuardReason::InvalidIdentifierCount);
            }
            if !assign.rhs.first().is_some_and(is_type_switch_guard_expr) {
                return Some(InvalidTypeSwitchGuardReason::InvalidExpression);
            }
            match assign.lhs.first() {
                Some(ast::Expr::Ident(ident)) if ident.name == "_" => {
                    Some(InvalidTypeSwitchGuardReason::BlankIdentifier)
                }
                Some(ast::Expr::Ident(_)) => None,
                _ => Some(InvalidTypeSwitchGuardReason::InvalidIdentifierCount),
            }
        }
        _ => Some(InvalidTypeSwitchGuardReason::InvalidExpression),
    }
}

fn invalid_type_switch_stmt(
    type_switch: &ast::TypeSwitchStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidTypeSwitchReason> {
    let guard = type_switch_guard_operand(&type_switch.assign)?;
    let guard_type = env.resolve_alias(&GoType::infer_expr(guard, env));
    let interface_name = match interface_operand_name(&guard_type, env) {
        InterfaceOperand::Interface(name) => name,
        InterfaceOperand::Empty => None,
        InterfaceOperand::Unknown => None,
        InterfaceOperand::NonInterface { type_name } => {
            return Some(InvalidTypeSwitchReason::NonInterfaceGuard { type_name });
        }
    };

    let mut seen = BTreeSet::new();
    let mut saw_nil = false;
    for stmt in &type_switch.body.list {
        let ast::Stmt::CaseClause(case) = stmt else {
            continue;
        };
        let Some(exprs) = &case.list else {
            continue;
        };
        for expr in exprs {
            if expr_is_nil(expr) {
                if saw_nil {
                    return Some(InvalidTypeSwitchReason::DuplicateNil);
                }
                saw_nil = true;
                continue;
            }
            let Some(key) = type_switch_case_key(expr, env) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                return Some(InvalidTypeSwitchReason::DuplicateCase { type_name: key });
            }
            if let Some(interface_name) = interface_name.as_deref()
                && let Some(reason) =
                    invalid_type_switch_case_implementation(interface_name, expr, env)
            {
                return Some(reason);
            }
        }
    }
    None
}

fn type_switch_guard_operand<'a>(stmt: &'a ast::Stmt<'a>) -> Option<&'a ast::Expr<'a>> {
    match stmt {
        ast::Stmt::ExprStmt(expr) => type_switch_guard_operand_expr(&expr.x),
        ast::Stmt::AssignStmt(assign) => {
            assign.rhs.first().and_then(type_switch_guard_operand_expr)
        }
        _ => None,
    }
}

fn type_switch_guard_operand_expr<'a>(expr: &'a ast::Expr<'a>) -> Option<&'a ast::Expr<'a>> {
    match unparen_expr(expr) {
        ast::Expr::TypeAssertExpr(assert) if assert.type_.is_none() => Some(&assert.x),
        _ => None,
    }
}

enum InterfaceOperand {
    Empty,
    Interface(Option<String>),
    NonInterface { type_name: String },
    Unknown,
}

fn interface_operand_name(ty: &GoType, env: &TypeEnv) -> InterfaceOperand {
    match ty {
        GoType::Any | GoType::Error | GoType::Interface(_) => InterfaceOperand::Empty,
        GoType::Named(name) if env.is_interface(name) => {
            InterfaceOperand::Interface(Some(name.clone()))
        }
        GoType::Unknown => InterfaceOperand::Unknown,
        other => InterfaceOperand::NonInterface {
            type_name: go_type_display_name(other),
        },
    }
}

fn type_switch_case_key(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    let ty = env.resolve_alias(&GoType::from_expr(expr));
    (!matches!(ty, GoType::Unknown)).then(|| go_type_display_name(&ty))
}

fn invalid_type_switch_case_implementation(
    interface_name: &str,
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidTypeSwitchReason> {
    if env
        .get_interface_methods(interface_name)
        .is_none_or(|methods| methods.is_empty())
    {
        return None;
    }
    let case_type_name = type_expr_named_type_for_validation(expr)?;
    if named_type_implements_interface_for_validation(&case_type_name, interface_name, env) {
        return None;
    }
    Some(InvalidTypeSwitchReason::CaseDoesNotImplement {
        case_type: case_type_name,
        interface_type: interface_name.to_string(),
    })
}

fn invalid_value_declaration_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    match gen_decl.tok {
        token::Token::CONST => invalid_const_declaration(gen_decl, env),
        token::Token::VAR => {
            for spec in &gen_decl.specs {
                let ast::Spec::ValueSpec(value_spec) = spec else {
                    continue;
                };
                if let Some(invalid) = invalid_var_value_spec(value_spec, env) {
                    return Some(invalid);
                }
            }
            None
        }
        _ => None,
    }
}

fn invalid_const_declaration(
    gen_decl: &ast::GenDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    let mut previous_values = None;
    let mut saw_value_spec = false;
    for spec in &gen_decl.specs {
        let ast::Spec::ValueSpec(value_spec) = spec else {
            continue;
        };
        saw_value_spec = true;
        let names = value_spec.names.len();
        if let Some(values) = &value_spec.values {
            let values = values.len();
            if names != values {
                return Some(InvalidDeclaration::ConstValueCount { names, values });
            }
            if let Some(invalid) = invalid_const_initializer(value_spec, env) {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_const_type_mismatch(value_spec, env) {
                return Some(invalid);
            }
            previous_values = Some(values);
        } else {
            let Some(values) = previous_values else {
                return Some(InvalidDeclaration::MissingConstInitializer);
            };
            if names != values {
                return Some(InvalidDeclaration::ConstValueCount { names, values });
            }
        }
    }
    if saw_value_spec && previous_values.is_none() {
        Some(InvalidDeclaration::MissingConstInitializer)
    } else {
        None
    }
}

fn invalid_const_initializer(
    value_spec: &ast::ValueSpec<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    value_spec
        .values
        .as_ref()?
        .iter()
        .find_map(|value| invalid_const_initializer_expr(value, env))
}

fn invalid_const_initializer_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    if expr_is_known_non_constant(expr, env) {
        return Some(InvalidDeclaration::ConstNonConstantInitializer);
    }
    invalid_const_expression(expr, env).map(|reason| InvalidDeclaration::ConstInvalidInitializer {
        reason: reason.to_string(),
    })
}

fn invalid_const_expression<'a>(expr: &ast::Expr<'a>, env: &TypeEnv) -> Option<&'static str> {
    match unparen_expr(expr) {
        ast::Expr::ParenExpr(paren) => invalid_const_expression(&paren.x, env),
        ast::Expr::UnaryExpr(unary)
            if matches!(
                unary.op,
                token::Token::ADD | token::Token::SUB | token::Token::NOT | token::Token::XOR
            ) =>
        {
            invalid_const_expression(&unary.x, env)
        }
        ast::Expr::BinaryExpr(binary) => invalid_const_expression(&binary.x, env)
            .or_else(|| invalid_const_expression(&binary.y, env))
            .or_else(|| {
                binary_divisor_is_constant_zero(binary, env).then_some("division by zero constant")
            }),
        _ => None,
    }
}

fn expr_is_known_non_constant(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(_) => false,
        ast::Expr::Ident(ident) if matches!(ident.name, "true" | "false" | "iota") => false,
        ast::Expr::Ident(ident) => ident.name == "nil" || known_ident_is_runtime_value(ident, env),
        ast::Expr::SelectorExpr(selector) => selector_is_known_runtime_value(selector, env),
        ast::Expr::ParenExpr(paren) => expr_is_known_non_constant(&paren.x, env),
        ast::Expr::UnaryExpr(unary) => match unary.op {
            token::Token::ADD | token::Token::SUB | token::Token::NOT | token::Token::XOR => {
                expr_is_known_non_constant(&unary.x, env)
            }
            _ => true,
        },
        ast::Expr::BinaryExpr(binary) => {
            expr_is_known_non_constant(&binary.x, env) || expr_is_known_non_constant(&binary.y, env)
        }
        ast::Expr::CallExpr(call) => const_call_is_known_non_constant(call, env),
        ast::Expr::CompositeLit(_)
        | ast::Expr::Ellipsis(_)
        | ast::Expr::FuncLit(_)
        | ast::Expr::IndexExpr(_)
        | ast::Expr::IndexListExpr(_)
        | ast::Expr::KeyValueExpr(_)
        | ast::Expr::SliceExpr(_)
        | ast::Expr::StarExpr(_)
        | ast::Expr::TypeAssertExpr(_) => true,
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => false,
    }
}

fn known_ident_is_runtime_value(ident: &ast::Ident<'_>, env: &TypeEnv) -> bool {
    if env.is_const(ident.name) {
        return false;
    }
    env.get_var(ident.name).is_some() || env.has_func(ident.name)
}

fn selector_is_known_runtime_value(selector: &ast::SelectorExpr<'_>, env: &TypeEnv) -> bool {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return expr_is_known_non_constant(&selector.x, env);
    };
    let key = format!("{}.{}", base.name, selector.sel.name);
    if env.is_const(&key) {
        return false;
    }
    env.get_var(&key).is_some() || env.has_func(&key)
}

fn const_call_is_known_non_constant(call: &ast::CallExpr<'_>, env: &TypeEnv) -> bool {
    if call.ellipsis.is_some() {
        return true;
    }
    if call_is_type_conversion(call, env) {
        let target = env.resolve_alias(&GoType::from_expr(&call.fun));
        if !type_conversion_result_can_be_constant(&target) {
            return true;
        }
        let Some(args) = &call.args else {
            return true;
        };
        let [arg] = args.as_slice() else {
            return true;
        };
        return expr_is_known_non_constant(arg, env);
    }

    if let Some(kind) = unshadowed_builtin_call_kind(call, env) {
        let Some(args) = &call.args else {
            return true;
        };
        return match kind {
            BuiltinCallKind::Complex => {
                args.len() != 2 || args.iter().any(|arg| expr_is_known_non_constant(arg, env))
            }
            BuiltinCallKind::Imag | BuiltinCallKind::Real => {
                let [arg] = args.as_slice() else {
                    return true;
                };
                expr_is_known_non_constant(arg, env)
            }
            BuiltinCallKind::Len | BuiltinCallKind::Cap => {
                let [arg] = args.as_slice() else {
                    return true;
                };
                expr_is_known_non_constant(arg, env)
            }
            BuiltinCallKind::Max | BuiltinCallKind::Min => {
                args.is_empty() || args.iter().any(|arg| expr_is_known_non_constant(arg, env))
            }
            BuiltinCallKind::Append
            | BuiltinCallKind::Clear
            | BuiltinCallKind::Close
            | BuiltinCallKind::Copy
            | BuiltinCallKind::Delete
            | BuiltinCallKind::Make
            | BuiltinCallKind::New
            | BuiltinCallKind::Panic
            | BuiltinCallKind::Print
            | BuiltinCallKind::Println
            | BuiltinCallKind::Recover => true,
        };
    }

    call_fun_is_known_runtime_value(&call.fun, env)
}

fn type_conversion_result_can_be_constant(target: &GoType) -> bool {
    matches!(
        target,
        GoType::Bool | GoType::String | GoType::Unknown | GoType::Named(_)
    ) || go_type_is_numeric(target)
}

fn call_fun_is_known_runtime_value(fun: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(fun) {
        ast::Expr::Ident(ident) => known_ident_is_runtime_value(ident, env),
        ast::Expr::SelectorExpr(selector) => selector_is_known_runtime_value(selector, env),
        ast::Expr::FuncLit(_) => true,
        _ => expr_is_known_non_constant(fun, env),
    }
}

fn invalid_const_type_mismatch(
    value_spec: &ast::ValueSpec<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    let expected = value_spec.type_.as_ref().map(GoType::from_expr)?;
    let values = value_spec.values.as_ref()?;
    values.iter().find_map(|value| {
        let expected = env.resolve_alias(&expected);
        let actual = env.resolve_alias(&GoType::infer_expr(value, env));
        (!expr_is_assignable_for_validation(&expected, value, env)).then(|| {
            InvalidDeclaration::ConstTypeMismatch {
                expected: go_type_display_name(&expected),
                actual: go_type_display_name(&actual),
            }
        })
    })
}

fn invalid_var_value_spec(
    value_spec: &ast::ValueSpec<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    let names = value_spec.names.len();
    let Some(values) = &value_spec.values else {
        return value_spec
            .type_
            .is_none()
            .then_some(InvalidDeclaration::VarMissingTypeOrInitializer);
    };

    if values.len() == 1 {
        if let Some(values) = expression_value_count(
            values.first()?,
            env,
            TupleAssignmentMode::AllowTupleOperations,
        ) && values != names
        {
            return Some(InvalidDeclaration::VarValueCount { names, values });
        }
        if let Some(invalid) = invalid_var_type_mismatch(value_spec, env) {
            return Some(invalid);
        }
        if let Some(invalid) = invalid_var_nil_initializer(value_spec, env) {
            return Some(invalid);
        }
        return None;
    }

    for expr in values {
        if let Some(values) =
            expression_value_count(expr, env, TupleAssignmentMode::SingleValueContext)
            && values != 1
        {
            return Some(InvalidDeclaration::VarMultiValueInSingleValueContext);
        }
    }

    if names != values.len() {
        return Some(InvalidDeclaration::VarValueCount {
            names,
            values: values.len(),
        });
    }

    if let Some(invalid) = invalid_var_type_mismatch(value_spec, env) {
        return Some(invalid);
    }
    if let Some(invalid) = invalid_var_nil_initializer(value_spec, env) {
        return Some(invalid);
    }

    None
}

fn invalid_var_nil_initializer(
    value_spec: &ast::ValueSpec<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    let values = value_spec.values.as_ref()?;
    if values.len() != value_spec.names.len() {
        return None;
    }
    let Some(type_expr) = &value_spec.type_ else {
        return values
            .iter()
            .any(expr_is_nil)
            .then_some(InvalidDeclaration::VarUntypedNil);
    };
    let expected = env.resolve_alias(&GoType::from_expr(type_expr));
    values
        .iter()
        .filter(|value| expr_is_nil(value))
        .find_map(|_| {
            (!type_can_compare_to_nil(&expected, env)).then(|| {
                InvalidDeclaration::VarTypeMismatch {
                    expected: go_type_display_name(&expected),
                    actual: "nil".to_string(),
                }
            })
        })
}

fn invalid_var_type_mismatch(
    value_spec: &ast::ValueSpec<'_>,
    env: &TypeEnv,
) -> Option<InvalidDeclaration> {
    let expected = value_spec.type_.as_ref().map(GoType::from_expr)?;
    let actual_types = var_initializer_types(value_spec, env)?;
    if actual_types.len() != value_spec.names.len() {
        return None;
    }
    let Some(values) = &value_spec.values else {
        return None;
    };
    if values.len() != value_spec.names.len() {
        return actual_types.iter().find_map(|actual| {
            let expected = env.resolve_alias(&expected);
            let actual = env.resolve_alias(actual);
            (!types_are_assignable_for_validation(&expected, &actual)).then(|| {
                InvalidDeclaration::VarTypeMismatch {
                    expected: go_type_display_name(&expected),
                    actual: go_type_display_name(&actual),
                }
            })
        });
    }
    values.iter().find_map(|value| {
        let expected = env.resolve_alias(&expected);
        let actual = env.resolve_alias(&GoType::infer_expr(value, env));
        (!expr_is_assignable_for_validation(&expected, value, env)).then(|| {
            InvalidDeclaration::VarTypeMismatch {
                expected: go_type_display_name(&expected),
                actual: go_type_display_name(&actual),
            }
        })
    })
}

fn var_initializer_types(value_spec: &ast::ValueSpec<'_>, env: &TypeEnv) -> Option<Vec<GoType>> {
    let values = value_spec.values.as_ref()?;
    if values.len() == 1 && value_spec.names.len() > 1 {
        let ast::Expr::CallExpr(call) = unparen_expr(values.first()?) else {
            return None;
        };
        return call_result_types(call, env).filter(|types| types.len() == value_spec.names.len());
    }
    (values.len() == value_spec.names.len()).then(|| {
        values
            .iter()
            .map(|expr| GoType::infer_expr(expr, env))
            .collect()
    })
}

fn is_type_switch_guard_expr(expr: &ast::Expr<'_>) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => is_type_switch_guard_expr(&paren.x),
        ast::Expr::TypeAssertExpr(assert) => assert.type_.is_none(),
        _ => false,
    }
}

fn invalid_range_clause(range: &ast::RangeStmt<'_>, env: &TypeEnv) -> Option<InvalidRangeReason> {
    let got = effective_range_binding_count(range);
    let ty = env.resolve_alias(&GoType::infer_expr(&range.x, env));
    let Some((kind, max)) = max_range_binding_count_for_type(&ty) else {
        if matches!(ty, GoType::Unknown | GoType::Named(_)) {
            return None;
        }
        return Some(InvalidRangeReason::NonRangeable {
            type_name: go_type_display_name(&ty),
        });
    };
    if got > max {
        return Some(InvalidRangeReason::BindingCount { kind, max, got });
    }
    invalid_range_assignment_type_mismatch(range, env)
}

fn invalid_range_assignment_type_mismatch(
    range: &ast::RangeStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidRangeReason> {
    if !matches!(range.tok, Some(token::Token::ASSIGN)) {
        return None;
    }
    let iteration_types = range_iteration_types(range, env)?;
    [range.key.as_ref(), range.value.as_ref()]
        .into_iter()
        .enumerate()
        .filter_map(|(idx, target)| {
            let target = target?;
            if is_blank_ident(target) {
                return None;
            }
            let expected = env.resolve_alias(&GoType::infer_expr(target, env));
            let actual = env.resolve_alias(iteration_types.get(idx)?);
            if range_iteration_value_is_assignable(range, idx, &expected, &actual, env) {
                return None;
            }
            Some(InvalidRangeReason::TypeMismatch {
                expected: go_type_display_name(&expected),
                actual: go_type_display_name(&actual),
            })
        })
        .next()
}

fn range_iteration_value_is_assignable(
    range: &ast::RangeStmt<'_>,
    idx: usize,
    expected: &GoType,
    actual: &GoType,
    env: &TypeEnv,
) -> bool {
    if matches!(expected, GoType::Unknown | GoType::Named(_))
        || matches!(actual, GoType::Unknown | GoType::Named(_))
    {
        return true;
    }
    if range_integer_constant_iteration_uses_target_type(range, idx, expected, env) {
        return true;
    }
    match (expected, actual) {
        (expected, actual) if expected == actual => true,
        (GoType::Any | GoType::Interface(_) | GoType::Error, _) => true,
        (expected, actual) if go_type_is_numeric(expected) && go_type_is_numeric(actual) => false,
        (GoType::Bool, _) | (_, GoType::Bool) | (GoType::String, _) | (_, GoType::String) => false,
        _ => true,
    }
}

fn range_integer_constant_iteration_uses_target_type(
    range: &ast::RangeStmt<'_>,
    idx: usize,
    expected: &GoType,
    env: &TypeEnv,
) -> bool {
    idx == 0
        && expected.is_integer()
        && expr_is_untyped_integer_constant_for_comparison(&range.x, env)
        && integer_constant_value_i128(&range.x)
            .is_none_or(|value| integer_constant_fits_type(value, expected))
}

fn range_iteration_types(range: &ast::RangeStmt<'_>, env: &TypeEnv) -> Option<Vec<GoType>> {
    match env.resolve_alias(&GoType::infer_expr(&range.x, env)) {
        GoType::String => Some(vec![GoType::Int, GoType::Int32]),
        GoType::Slice(elem) | GoType::Array(elem) => Some(vec![GoType::Int, *elem]),
        GoType::Pointer(inner) => match *inner {
            GoType::Array(elem) => Some(vec![GoType::Int, *elem]),
            _ => None,
        },
        GoType::Map(key, value) => Some(vec![*key, *value]),
        GoType::Chan { elem, direction } if direction.can_receive() => Some(vec![*elem]),
        ty if ty.is_integer() => Some(vec![ty]),
        GoType::Func { params, .. } => range_function_yield_params(&params),
        GoType::Unknown | GoType::Named(_) => None,
        _ => None,
    }
}

fn range_function_yield_params(params: &[GoType]) -> Option<Vec<GoType>> {
    let [yield_param] = params else {
        return None;
    };
    let GoType::Func {
        params: yield_params,
        results,
    } = yield_param
    else {
        return None;
    };
    matches!(results.as_slice(), [GoType::Bool]).then(|| yield_params.clone())
}

fn invalid_expression_switch(
    switch: &ast::SwitchStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidSwitchReason> {
    let tag = switch.tag.as_ref();
    let tag_type = match tag {
        Some(tag) if expr_is_nil(tag) => return Some(InvalidSwitchReason::NilTag),
        Some(tag) => env.resolve_alias(&GoType::infer_expr(tag, env)),
        None => GoType::Bool,
    };
    if !type_is_comparable_for_validation(&tag_type, env) {
        return Some(InvalidSwitchReason::NonComparableTag {
            type_name: go_type_display_name(&tag_type),
        });
    }
    let mut seen_constant_cases = BTreeSet::new();
    for stmt in &switch.body.list {
        let ast::Stmt::CaseClause(case) = stmt else {
            continue;
        };
        let Some(exprs) = &case.list else {
            continue;
        };
        for expr in exprs {
            if let Some(values) =
                expression_value_count(expr, env, TupleAssignmentMode::SingleValueContext)
                    .filter(|values| *values != 1)
            {
                return Some(InvalidSwitchReason::CaseMultiValue { values });
            }
            if let Some(reason) = invalid_expression_switch_case(&tag_type, expr, env) {
                return Some(reason);
            }
            if let Some(fingerprint) = literal_constant_key_fingerprint(expr, env)
                && !seen_constant_cases.insert(fingerprint)
            {
                return Some(InvalidSwitchReason::DuplicateConstantCase {
                    value: constant_case_display(expr),
                });
            }
        }
    }
    None
}

fn constant_case_display(expr: &ast::Expr<'_>) -> String {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => lit.value.to_string(),
        ast::Expr::Ident(ident) => ident.name.to_string(),
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            format!(
                "{}{}",
                unary_op_name(unary.op),
                constant_case_display(&unary.x)
            )
        }
        _ => "constant".to_string(),
    }
}

fn invalid_expression_switch_case(
    tag_type: &GoType,
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidSwitchReason> {
    let case_type = env.resolve_alias(&GoType::infer_expr(expr, env));
    if expr_is_nil(expr) {
        return (!type_can_compare_to_nil(tag_type, env)).then(|| {
            InvalidSwitchReason::CaseTypeMismatch {
                expected: go_type_display_name(tag_type),
                actual: "nil".to_string(),
            }
        });
    }
    if !type_is_comparable_for_validation(&case_type, env) {
        return Some(InvalidSwitchReason::NonComparableCase {
            type_name: go_type_display_name(&case_type),
        });
    }
    if expr_is_assignable_for_validation(tag_type, expr, env) {
        return None;
    }
    Some(InvalidSwitchReason::CaseTypeMismatch {
        expected: go_type_display_name(tag_type),
        actual: go_type_display_name(&case_type),
    })
}

fn invalid_array_type(array: &ast::ArrayType<'_>) -> Option<InvalidStatementReason> {
    let len = array.len.as_ref()?;
    invalid_array_length(len).map(|reason| InvalidStatementReason::InvalidArrayType { reason })
}

fn invalid_array_length(expr: &ast::Expr<'_>) -> Option<String> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => invalid_array_length_lit(lit),
        ast::Expr::Ident(ident) if ident.name == "nil" => {
            Some("length must be a numeric constant".to_string())
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            invalid_array_length(&unary.x)
        }
        ast::Expr::UnaryExpr(unary)
            if unary.op == token::Token::SUB && array_length_constant_is_numeric(&unary.x) =>
        {
            (!array_length_constant_is_zero(&unary.x))
                .then(|| "length must be non-negative".to_string())
        }
        _ => None,
    }
}

fn invalid_array_length_lit(lit: &ast::BasicLit<'_>) -> Option<String> {
    match lit.kind {
        token::Token::INT => None,
        token::Token::FLOAT if basic_lit_float_is_int_representable(lit) => None,
        token::Token::FLOAT => Some("length must be representable by int".to_string()),
        token::Token::CHAR | token::Token::IMAG | token::Token::STRING => {
            Some("length must be a numeric constant".to_string())
        }
        _ => None,
    }
}

fn basic_lit_float_is_int_representable(lit: &ast::BasicLit<'_>) -> bool {
    decimal_float_literal_is_integer(lit.value)
}

fn array_length_constant_is_numeric(expr: &ast::Expr<'_>) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => matches!(
            lit.kind,
            token::Token::INT | token::Token::FLOAT | token::Token::IMAG
        ),
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            array_length_constant_is_numeric(&unary.x)
        }
        _ => false,
    }
}

fn array_length_constant_is_zero(expr: &ast::Expr<'_>) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) if matches!(lit.kind, token::Token::INT) => {
            integer_literal_is_zero(lit.value)
        }
        ast::Expr::BasicLit(lit) if matches!(lit.kind, token::Token::FLOAT) => {
            decimal_float_literal_is_zero(lit.value)
        }
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            array_length_constant_is_zero(&unary.x)
        }
        _ => false,
    }
}

fn integer_literal_is_zero(value: &str) -> bool {
    value.chars().filter(|ch| *ch != '_').all(|ch| {
        ch == '0' || ch == 'x' || ch == 'X' || ch == 'o' || ch == 'O' || ch == 'b' || ch == 'B'
    })
}

fn decimal_float_literal_is_zero(value: &str) -> bool {
    let value = value.replace('_', "").to_ascii_lowercase();
    if value.starts_with("0x") || value.contains('p') {
        return false;
    }
    let mantissa = value
        .split_once('e')
        .map_or(value.as_str(), |(mantissa, _)| mantissa);
    mantissa
        .chars()
        .filter(|ch| *ch != '.' && *ch != '+' && *ch != '-')
        .all(|ch| ch == '0')
}

fn invalid_map_type(map: &ast::MapType<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    if type_expr_is_comparable_for_validation(&map.key, env) {
        return None;
    }
    Some(InvalidStatementReason::InvalidMapType {
        reason: format!(
            "key type {} is not comparable",
            type_expr_display_name(&map.key, env)
        ),
    })
}

fn invalid_type_assert_expr(
    assert: &ast::TypeAssertExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let Some(target_type) = &assert.type_ else {
        return None;
    };
    let operand_type = env.resolve_alias(&GoType::infer_expr(&assert.x, env));
    let interface_name = match interface_operand_name(&operand_type, env) {
        InterfaceOperand::Interface(name) => name,
        InterfaceOperand::Empty | InterfaceOperand::Unknown => None,
        InterfaceOperand::NonInterface { type_name } => {
            return Some(invalid_type_assert_reason(format!(
                "operand must have interface type, got {type_name}"
            )));
        }
    };
    let interface_name = interface_name?;
    if type_expr_is_interface_for_validation(target_type, env)
        || type_expr_implements_interface_for_validation(target_type, &interface_name, env)
    {
        return None;
    }
    Some(invalid_type_assert_reason(format!(
        "{} does not implement {}",
        type_expr_display_name(target_type, env),
        interface_name
    )))
}

fn invalid_type_assert_reason(reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidTypeAssert {
        reason: reason.into(),
    }
}

fn expr_is_nil(expr: &ast::Expr<'_>) -> bool {
    matches!(unparen_expr(expr), ast::Expr::Ident(ident) if ident.name == "nil")
}

fn type_can_compare_to_nil(ty: &GoType, env: &TypeEnv) -> bool {
    let ty = env.resolve_alias(ty);
    match &ty {
        GoType::Any
        | GoType::Error
        | GoType::Interface(_)
        | GoType::Pointer(_)
        | GoType::Func { .. }
        | GoType::Slice(_)
        | GoType::Map(_, _)
        | GoType::Chan { .. }
        | GoType::Unknown => true,
        GoType::Named(name) => env
            .get_type_kind(name)
            .is_none_or(|kind| matches!(kind, TypeKind::Interface)),
        _ => false,
    }
}

fn invalid_short_var_decl_names(lhs: &[ast::Expr<'_>]) -> Option<InvalidShortVarDeclReason> {
    let mut names = BTreeSet::new();
    for expr in lhs {
        let Some(name) = short_var_decl_ident_name(expr) else {
            return Some(InvalidShortVarDeclReason::NonIdentifier);
        };
        if name == "_" {
            continue;
        }
        if !names.insert(name) {
            return Some(InvalidShortVarDeclReason::DuplicateName(name.to_string()));
        }
    }
    None
}

fn invalid_range_short_var_decl_names(
    range: &ast::RangeStmt<'_>,
) -> Option<InvalidShortVarDeclReason> {
    let mut names = BTreeSet::new();
    if let Some(key) = &range.key {
        if let Some(reason) = record_short_var_decl_name(key, &mut names) {
            return Some(reason);
        }
    }
    if let Some(value) = &range.value {
        if let Some(reason) = record_short_var_decl_name(value, &mut names) {
            return Some(reason);
        }
    }
    None
}

fn record_short_var_decl_name<'src>(
    expr: &'src ast::Expr<'src>,
    names: &mut BTreeSet<&'src str>,
) -> Option<InvalidShortVarDeclReason> {
    let Some(name) = short_var_decl_ident_name(expr) else {
        return Some(InvalidShortVarDeclReason::NonIdentifier);
    };
    if name == "_" {
        return None;
    }
    if !names.insert(name) {
        return Some(InvalidShortVarDeclReason::DuplicateName(name.to_string()));
    }
    None
}

fn short_var_decl_ident_name<'src>(expr: &'src ast::Expr<'src>) -> Option<&'src str> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name),
        ast::Expr::ParenExpr(paren) => short_var_decl_ident_name(&paren.x),
        _ => None,
    }
}

fn effective_range_binding_count(range: &ast::RangeStmt<'_>) -> usize {
    match (&range.key, &range.value) {
        (None, None) => 0,
        (Some(_), None) => 1,
        (None, Some(value)) => {
            if is_blank_ident(value) {
                0
            } else {
                1
            }
        }
        (Some(_), Some(value)) => {
            if is_blank_ident(value) {
                1
            } else {
                2
            }
        }
    }
}

fn max_range_binding_count_for_type(ty: &GoType) -> Option<(RangeKind, usize)> {
    match ty {
        GoType::String => Some((RangeKind::String, 2)),
        GoType::Slice(_) | GoType::Array(_) => Some((RangeKind::Indexed, 2)),
        GoType::Pointer(inner) if matches!(inner.as_ref(), GoType::Array(_)) => {
            Some((RangeKind::Indexed, 2))
        }
        GoType::Map(_, _) => Some((RangeKind::Map, 2)),
        GoType::Chan { direction, .. } if direction.can_receive() => Some((RangeKind::Channel, 1)),
        GoType::Chan { .. } => None,
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
        | GoType::Uintptr => Some((RangeKind::Integer, 1)),
        GoType::Func { params, .. } => {
            range_function_max_binding_count(params).map(|max| (RangeKind::Function, max))
        }
        _ => None,
    }
}

fn go_type_display_name(ty: &GoType) -> String {
    match ty {
        GoType::Bool => "bool".to_string(),
        GoType::Float32 => "float32".to_string(),
        GoType::Float64 => "float64".to_string(),
        GoType::Complex64 => "complex64".to_string(),
        GoType::Complex128 => "complex128".to_string(),
        GoType::Pointer(_) => "pointer".to_string(),
        GoType::Func { .. } => "function".to_string(),
        GoType::Chan { direction, .. } => channel_type_display_name(*direction).to_string(),
        GoType::Any | GoType::Interface(_) | GoType::Error => "interface".to_string(),
        GoType::Unknown => "unknown".to_string(),
        GoType::Named(name) => name.clone(),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn channel_type_display_name(direction: GoChannelDirection) -> &'static str {
    match direction {
        GoChannelDirection::Bidirectional => "channel",
        GoChannelDirection::Send => "send-only channel",
        GoChannelDirection::Receive => "receive-only channel",
    }
}

fn range_function_max_binding_count(params: &[GoType]) -> Option<usize> {
    let [yield_param] = params else {
        return None;
    };
    let GoType::Func {
        params: yield_params,
        results,
    } = yield_param
    else {
        return None;
    };
    if !matches!(results.as_slice(), [GoType::Bool]) {
        return None;
    }
    Some(yield_params.len())
}

fn is_blank_ident(expr: &ast::Expr<'_>) -> bool {
    matches!(expr, ast::Expr::Ident(ident) if ident.name == "_")
}

fn invalid_statement_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    env: &mut TypeEnv,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    for stmt in stmts {
        if let Some(invalid) = invalid_statement_in_stmt(stmt, env, scopes) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_statement_in_case_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
    scopes: &mut ShortVarScopes,
) -> Option<InvalidStatement> {
    for stmt in &block.list {
        let ast::Stmt::CaseClause(case) = stmt else {
            continue;
        };
        let mut case_env = env.clone();
        if let Some(invalid) = scopes
            .with_scope(|scopes| invalid_statement_in_stmt_list(&case.body, &mut case_env, scopes))
        {
            return Some(invalid);
        }
    }
    None
}

fn invalid_expression_statement(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match unparen_expr(expr) {
        ast::Expr::CallExpr(call) => invalid_call_statement(call, env),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ARROW => None,
        _ => Some(InvalidStatementReason::NonCallOrReceive),
    }
}

fn invalid_expression_in_gen_decl(
    gen_decl: &ast::GenDecl<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    for spec in &gen_decl.specs {
        if let Some(reason) = invalid_expression_in_spec(spec, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_expression_in_spec(
    spec: &ast::Spec<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => invalid_expression_in_expr(&type_spec.type_, env),
        ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_) = &value_spec.type_
                && let Some(reason) = invalid_expression_in_expr(type_, env)
            {
                return Some(reason);
            }
            value_spec.values.as_ref().and_then(|values| {
                values
                    .iter()
                    .find_map(|value| invalid_expression_in_expr(value, env))
            })
        }
    }
}

fn invalid_expression_in_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(reason) = invalid_builtin_call_expression(call, env) {
        return Some(reason);
    }
    invalid_expression_in_call_operands(call, env)
        .or_else(|| invalid_expression_after_call_operands(call, env))
}

fn invalid_expression_in_call_statement(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(reason) = invalid_call_statement(call, env) {
        return Some(reason);
    }
    if let Some(kind) = unshadowed_builtin_call_kind(call, env)
        && !builtin_call_produces_value(kind)
    {
        return invalid_expression_in_call_operands(call, env);
    }
    invalid_expression_in_call(call, env)
}

fn invalid_expression_in_statement_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match unparen_expr(expr) {
        ast::Expr::CallExpr(call) => invalid_expression_in_call_statement(call, env),
        _ => invalid_expression_in_expr(expr, env),
    }
}

fn invalid_expression_in_call_operands(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(reason) = invalid_expression_in_expr(&call.fun, env).or_else(|| {
        call.args.as_ref().and_then(|args| {
            args.iter()
                .find_map(|arg| invalid_expression_in_expr(arg, env))
        })
    }) {
        return Some(reason);
    }
    None
}

fn invalid_expression_after_call_operands(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call_is_type_conversion(call, env) {
        return invalid_type_conversion_call(call, env);
    }
    invalid_ordinary_call(call, env)
}

fn invalid_expression_in_assignment_lhs(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match expr {
        ast::Expr::Ident(_) => None,
        ast::Expr::ParenExpr(paren) => invalid_expression_in_assignment_lhs(&paren.x, env),
        ast::Expr::SelectorExpr(selector) => invalid_expression_in_expr(&selector.x, env),
        ast::Expr::IndexExpr(index) => invalid_expression_in_expr(&index.x, env)
            .or_else(|| invalid_expression_in_expr(&index.index, env)),
        ast::Expr::IndexListExpr(index) => {
            invalid_expression_in_expr(&index.x, env).or_else(|| {
                index
                    .indices
                    .iter()
                    .find_map(|index| invalid_expression_in_expr(index, env))
            })
        }
        ast::Expr::StarExpr(star) => invalid_expression_in_expr(&star.x, env),
        ast::Expr::TypeAssertExpr(assert) => {
            invalid_expression_in_expr(&assert.x, env).or_else(|| {
                assert
                    .type_
                    .as_ref()
                    .and_then(|type_| invalid_expression_in_expr(type_, env))
            })
        }
        _ => invalid_expression_in_expr(expr, env),
    }
}

fn invalid_expression_in_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match expr {
        ast::Expr::Ident(ident) if ident.name == "_" => {
            Some(InvalidStatementReason::BlankIdentifier)
        }
        ast::Expr::ArrayType(array) => array
            .len
            .as_ref()
            .and_then(|len| invalid_expression_in_expr(len, env))
            .or_else(|| invalid_expression_in_expr(&array.elt, env))
            .or_else(|| invalid_array_type(array)),
        ast::Expr::BinaryExpr(binary) => invalid_expression_in_expr(&binary.x, env)
            .or_else(|| invalid_expression_in_expr(&binary.y, env))
            .or_else(|| invalid_binary_expr(binary, env)),
        ast::Expr::CallExpr(call) => invalid_expression_in_call(call, env),
        ast::Expr::ChanType(chan) => invalid_expression_in_expr(&chan.value, env),
        ast::Expr::CompositeLit(comp) => comp
            .type_
            .as_ref()
            .and_then(|type_| invalid_expression_in_expr(type_, env))
            .or_else(|| {
                comp.elts.as_ref().and_then(|elts| {
                    elts.iter()
                        .find_map(|elt| invalid_expression_in_expr(elt, env))
                })
            })
            .or_else(|| invalid_composite_lit(comp, env)),
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|elt| invalid_expression_in_expr(elt, env)),
        ast::Expr::FuncLit(func_lit) => invalid_expression_in_func_lit(func_lit, env),
        ast::Expr::FuncType(func_type) => invalid_expression_in_field_list(&func_type.params, env)
            .or_else(|| {
                func_type
                    .results
                    .as_ref()
                    .and_then(|results| invalid_expression_in_field_list(results, env))
            }),
        ast::Expr::IndexExpr(index) => invalid_expression_in_expr(&index.x, env)
            .or_else(|| invalid_expression_in_expr(&index.index, env))
            .or_else(|| invalid_index_expr(index, env)),
        ast::Expr::IndexListExpr(index) => {
            invalid_expression_in_expr(&index.x, env).or_else(|| {
                index
                    .indices
                    .iter()
                    .find_map(|index| invalid_expression_in_expr(index, env))
            })
        }
        ast::Expr::InterfaceType(interface) => interface
            .methods
            .as_ref()
            .and_then(|methods| invalid_expression_in_field_list(methods, env)),
        ast::Expr::KeyValueExpr(kv) => invalid_expression_in_expr(&kv.key, env)
            .or_else(|| invalid_expression_in_expr(&kv.value, env)),
        ast::Expr::MapType(map) => invalid_expression_in_expr(&map.key, env)
            .or_else(|| invalid_expression_in_expr(&map.value, env))
            .or_else(|| invalid_map_type(map, env)),
        ast::Expr::ParenExpr(paren) => invalid_expression_in_expr(&paren.x, env),
        ast::Expr::SelectorExpr(selector) => invalid_expression_in_expr(&selector.x, env),
        ast::Expr::SliceExpr(slice) => invalid_expression_in_expr(&slice.x, env)
            .or_else(|| {
                slice
                    .low
                    .as_ref()
                    .and_then(|low| invalid_expression_in_expr(low, env))
            })
            .or_else(|| {
                slice
                    .high
                    .as_ref()
                    .and_then(|high| invalid_expression_in_expr(high, env))
            })
            .or_else(|| {
                slice
                    .max
                    .as_ref()
                    .and_then(|max| invalid_expression_in_expr(max, env))
            })
            .or_else(|| invalid_slice_expr(slice, env)),
        ast::Expr::StarExpr(star) => {
            invalid_expression_in_expr(&star.x, env).or_else(|| invalid_star_expr(star, env))
        }
        ast::Expr::StructType(struct_type) => struct_type
            .fields
            .as_ref()
            .and_then(|fields| invalid_expression_in_field_list(fields, env)),
        ast::Expr::TypeAssertExpr(assert) => invalid_expression_in_expr(&assert.x, env)
            .or_else(|| {
                assert
                    .type_
                    .as_ref()
                    .and_then(|type_| invalid_expression_in_expr(type_, env))
            })
            .or_else(|| invalid_type_assert_expr(assert, env)),
        ast::Expr::UnaryExpr(unary) => {
            invalid_expression_in_expr(&unary.x, env).or_else(|| invalid_unary_expr(unary, env))
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn invalid_expression_in_func_lit(
    func_lit: &ast::FuncLit<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let mut func_env = env.clone();
    record_func_type_bindings(&func_lit.type_, &mut func_env);
    invalid_expression_in_block(&func_lit.body, &mut func_env)
}

fn invalid_expression_in_block(
    block: &ast::BlockStmt<'_>,
    env: &mut TypeEnv,
) -> Option<InvalidStatementReason> {
    for stmt in &block.list {
        if let Some(reason) = invalid_expression_in_stmt(stmt, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_expression_in_stmt(
    stmt: &ast::Stmt<'_>,
    env: &mut TypeEnv,
) -> Option<InvalidStatementReason> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            let invalid = assign
                .lhs
                .iter()
                .find_map(|expr| invalid_expression_in_assignment_lhs(expr, env))
                .or_else(|| {
                    assign
                        .rhs
                        .iter()
                        .find_map(|expr| invalid_expression_in_expr(expr, env))
                });
            record_define_bindings(assign, env);
            invalid
        }
        ast::Stmt::BlockStmt(block) => {
            let mut block_env = env.clone();
            invalid_expression_in_block(block, &mut block_env)
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::CaseClause(case) => {
            let mut case_env = env.clone();
            if let Some(list) = &case.list
                && let Some(reason) = list
                    .iter()
                    .find_map(|expr| invalid_expression_in_expr(expr, &case_env))
            {
                return Some(reason);
            }
            invalid_expression_in_stmt_list(&case.body, &mut case_env)
        }
        ast::Stmt::CommClause(comm) => {
            let mut comm_env = env.clone();
            if let Some(comm_stmt) = &comm.comm
                && let Some(reason) = invalid_expression_in_stmt(comm_stmt, &mut comm_env)
            {
                return Some(reason);
            }
            invalid_expression_in_stmt_list(&comm.body, &mut comm_env)
        }
        ast::Stmt::DeclStmt(decl) => {
            let invalid = invalid_expression_in_gen_decl(&decl.decl, env);
            record_decl_bindings(&decl.decl, env);
            invalid
        }
        ast::Stmt::DeferStmt(defer) => invalid_expression_in_call_statement(&defer.call, env),
        ast::Stmt::ExprStmt(expr) => invalid_expression_in_statement_expr(&expr.x, env),
        ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            if let Some(init) = &for_stmt.init
                && let Some(reason) = invalid_expression_in_stmt(init, &mut loop_env)
            {
                return Some(reason);
            }
            if let Some(cond) = &for_stmt.cond
                && let Some(reason) = invalid_expression_in_expr(cond, &loop_env)
            {
                return Some(reason);
            }
            if let Some(post) = &for_stmt.post
                && let Some(reason) = invalid_expression_in_stmt(post, &mut loop_env)
            {
                return Some(reason);
            }
            invalid_expression_in_block(&for_stmt.body, &mut loop_env)
        }
        ast::Stmt::GoStmt(go) => invalid_expression_in_call_statement(&go.call, env),
        ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(reason) = invalid_expression_in_stmt(init, &mut if_env)
            {
                return Some(reason);
            }
            if let Some(reason) = invalid_expression_in_expr(&if_stmt.cond, &if_env) {
                return Some(reason);
            }
            if let Some(reason) = invalid_expression_in_block(&if_stmt.body, &mut if_env) {
                return Some(reason);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                let mut else_env = if_env;
                return invalid_expression_in_stmt(else_branch, &mut else_env);
            }
            None
        }
        ast::Stmt::IncDecStmt(inc_dec) => invalid_expression_in_assignment_lhs(&inc_dec.x, env),
        ast::Stmt::LabeledStmt(labeled) => invalid_expression_in_stmt(&labeled.stmt, env),
        ast::Stmt::RangeStmt(range) => {
            if matches!(range.tok, Some(token::Token::ASSIGN)) {
                if let Some(key) = &range.key
                    && let Some(reason) = invalid_expression_in_assignment_lhs(key, env)
                {
                    return Some(reason);
                }
                if let Some(value) = &range.value
                    && let Some(reason) = invalid_expression_in_assignment_lhs(value, env)
                {
                    return Some(reason);
                }
            }
            if let Some(reason) = invalid_expression_in_expr(&range.x, env) {
                return Some(reason);
            }
            let mut range_env = env.clone();
            record_range_bindings(range, &mut range_env);
            invalid_expression_in_block(&range.body, &mut range_env)
        }
        ast::Stmt::ReturnStmt(ret) => ret
            .results
            .iter()
            .find_map(|expr| invalid_expression_in_expr(expr, env)),
        ast::Stmt::SelectStmt(select) => {
            let mut select_env = env.clone();
            invalid_expression_in_block(&select.body, &mut select_env)
        }
        ast::Stmt::SendStmt(send) => invalid_expression_in_expr(&send.chan, env)
            .or_else(|| invalid_expression_in_expr(&send.value, env)),
        ast::Stmt::SwitchStmt(switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = switch.init.as_ref().as_ref()
                && let Some(reason) = invalid_expression_in_stmt(init, &mut switch_env)
            {
                return Some(reason);
            }
            if let Some(tag) = &switch.tag
                && let Some(reason) = invalid_expression_in_expr(tag, &switch_env)
            {
                return Some(reason);
            }
            invalid_expression_in_block(&switch.body, &mut switch_env)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = type_switch.init.as_ref().as_ref()
                && let Some(reason) = invalid_expression_in_stmt(init, &mut switch_env)
            {
                return Some(reason);
            }
            if let Some(reason) = invalid_expression_in_stmt(&type_switch.assign, &mut switch_env) {
                return Some(reason);
            }
            invalid_expression_in_block(&type_switch.body, &mut switch_env)
        }
    }
}

fn invalid_expression_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    env: &mut TypeEnv,
) -> Option<InvalidStatementReason> {
    for stmt in stmts {
        if let Some(reason) = invalid_expression_in_stmt(stmt, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_expression_in_field_list(
    fields: &ast::FieldList<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    fields.list.iter().find_map(|field| {
        field
            .type_
            .as_ref()
            .and_then(|type_| invalid_expression_in_expr(type_, env))
    })
}

fn expr_is_receive_operation(expr: &ast::Expr<'_>) -> bool {
    matches!(unparen_expr(expr), ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ARROW)
}

fn invalid_call_statement(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(name) = disallowed_builtin_statement_name(call, env) {
        return Some(InvalidStatementReason::DisallowedBuiltin(name));
    }
    if let Some(reason) = invalid_builtin_call_statement(call, env) {
        return Some(reason);
    }
    call_is_type_conversion(call, env).then_some(InvalidStatementReason::TypeConversion)
}

fn invalid_builtin_call_expression(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let kind = unshadowed_builtin_call_kind(call, env)?;
    let invalid_call = match kind {
        BuiltinCallKind::Append => invalid_builtin_append_call(call, env),
        BuiltinCallKind::Cap | BuiltinCallKind::Len => invalid_builtin_len_cap_call(call, env),
        BuiltinCallKind::Clear => invalid_builtin_clear_call(call, env),
        BuiltinCallKind::Close => invalid_builtin_close_call(call, env),
        BuiltinCallKind::Complex => invalid_builtin_complex_call(call, env),
        BuiltinCallKind::Copy => invalid_builtin_copy_call(call, env),
        BuiltinCallKind::Delete => invalid_builtin_delete_call(call, env),
        BuiltinCallKind::Imag | BuiltinCallKind::Real => invalid_builtin_real_imag_call(call, env),
        BuiltinCallKind::Make => invalid_builtin_make_call(call, env),
        BuiltinCallKind::Max | BuiltinCallKind::Min => invalid_builtin_min_max_call(call, env),
        BuiltinCallKind::New => invalid_builtin_new_call(call, env),
        BuiltinCallKind::Panic => invalid_builtin_panic_call(call),
        BuiltinCallKind::Print => invalid_builtin_print_call(call, BuiltinCallKind::Print),
        BuiltinCallKind::Println => invalid_builtin_print_call(call, BuiltinCallKind::Println),
        BuiltinCallKind::Recover => invalid_builtin_recover_call(call),
    };
    if invalid_call.is_some() {
        return invalid_call;
    }
    (!builtin_call_produces_value(kind)).then_some(invalid_builtin_call_reason(
        kind,
        "does not produce a value",
    ))
}

fn builtin_call_produces_value(kind: BuiltinCallKind) -> bool {
    !matches!(
        kind,
        BuiltinCallKind::Clear
            | BuiltinCallKind::Close
            | BuiltinCallKind::Delete
            | BuiltinCallKind::Panic
            | BuiltinCallKind::Print
            | BuiltinCallKind::Println
    )
}

fn invalid_builtin_call_statement(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    match unshadowed_builtin_call_kind(call, env)? {
        BuiltinCallKind::Clear => invalid_builtin_clear_call(call, env),
        BuiltinCallKind::Close => invalid_builtin_close_call(call, env),
        BuiltinCallKind::Delete => invalid_builtin_delete_call(call, env),
        BuiltinCallKind::Panic => invalid_builtin_panic_call(call),
        BuiltinCallKind::Print => invalid_builtin_print_call(call, BuiltinCallKind::Print),
        BuiltinCallKind::Println => invalid_builtin_print_call(call, BuiltinCallKind::Println),
        BuiltinCallKind::Recover => invalid_builtin_recover_call(call),
        _ => None,
    }
}

#[derive(Clone)]
struct CallSignature {
    target: String,
    params: Vec<GoType>,
    variadic_start: Option<usize>,
}

fn invalid_ordinary_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if unshadowed_builtin_call_kind(call, env).is_some() || call_is_type_conversion(call, env) {
        return None;
    }
    let signature = call_signature(call, env)?;
    if let Some(variadic_start) = signature.variadic_start {
        invalid_variadic_call_args(call, &signature, variadic_start, env)
    } else {
        invalid_fixed_call_args(call, &signature, env)
    }
}

fn invalid_index_expr(index: &ast::IndexExpr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let target = env.resolve_alias(&GoType::infer_expr(&index.x, env));
    match target {
        GoType::String | GoType::Slice(_) | GoType::Array(_) => {
            invalid_integer_index(&index.index, env)
        }
        GoType::Pointer(inner) if matches!(inner.as_ref(), GoType::Array(_)) => {
            invalid_integer_index(&index.index, env)
        }
        GoType::Map(key, _) => invalid_map_index_key(&key, &index.index, env),
        GoType::Unknown | GoType::Named(_) | GoType::Func { .. } => None,
        other => Some(invalid_index_reason(format!(
            "cannot index {}",
            go_type_display_name(&other)
        ))),
    }
}

fn invalid_integer_index(index: &ast::Expr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(index, env));
    if matches!(ty, GoType::Unknown | GoType::Named(_)) || binary_operand_is_integer(&ty, index) {
        return None;
    }
    Some(invalid_index_reason(format!(
        "index must have integer type, got {}",
        go_type_display_name(&ty)
    )))
}

fn invalid_map_index_key(
    expected: &GoType,
    index: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let expected = env.resolve_alias(expected);
    let actual = env.resolve_alias(&GoType::infer_expr(index, env));
    if expr_is_assignable_for_validation(&expected, index, env) {
        return None;
    }
    Some(invalid_index_reason(format!(
        "key must be assignable to {}, got {}",
        go_type_display_name(&expected),
        go_type_display_name(&actual)
    )))
}

fn invalid_index_reason(reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidIndex {
        reason: reason.into(),
    }
}

fn invalid_star_expr(star: &ast::StarExpr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let operand = env.resolve_alias(&GoType::infer_expr(&star.x, env));
    match operand {
        GoType::Pointer(_) | GoType::Unknown | GoType::Named(_) => None,
        other => Some(invalid_unary_reason(
            "*",
            format!(
                "operand must be pointer, got {}",
                go_type_display_name(&other)
            ),
        )),
    }
}

fn invalid_unary_expr(unary: &ast::UnaryExpr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let operand = env.resolve_alias(&GoType::infer_expr(&unary.x, env));
    match unary.op {
        token::Token::ADD | token::Token::SUB => invalid_unary_numeric_operand(unary.op, &operand),
        token::Token::NOT => invalid_unary_bool_operand(&operand),
        token::Token::XOR => invalid_unary_integer_operand(unary, &operand),
        token::Token::AND => invalid_unary_address_operand(unary, env),
        token::Token::ARROW => invalid_unary_receive_operand(&operand),
        _ => None,
    }
}

fn invalid_unary_numeric_operand(
    op: token::Token,
    operand: &GoType,
) -> Option<InvalidStatementReason> {
    if go_type_is_numeric(operand) || matches!(operand, GoType::Unknown | GoType::Named(_)) {
        return None;
    }
    Some(invalid_unary_reason(
        unary_op_name(op),
        format!(
            "operand must be numeric, got {}",
            go_type_display_name(operand)
        ),
    ))
}

fn invalid_unary_bool_operand(operand: &GoType) -> Option<InvalidStatementReason> {
    if matches!(operand, GoType::Bool | GoType::Unknown | GoType::Named(_)) {
        return None;
    }
    Some(invalid_unary_reason(
        "!",
        format!(
            "operand must be bool, got {}",
            go_type_display_name(operand)
        ),
    ))
}

fn invalid_unary_integer_operand(
    unary: &ast::UnaryExpr<'_>,
    operand: &GoType,
) -> Option<InvalidStatementReason> {
    if matches!(operand, GoType::Unknown | GoType::Named(_))
        || binary_operand_is_integer(operand, &unary.x)
    {
        return None;
    }
    Some(invalid_unary_reason(
        "^",
        format!(
            "operand must be integer, got {}",
            go_type_display_name(operand)
        ),
    ))
}

fn invalid_unary_address_operand(
    unary: &ast::UnaryExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if matches!(unparen_expr(&unary.x), ast::Expr::CompositeLit(_))
        || (!is_blank_ident(&unary.x)
            && expr_addressability(&unary.x, env) == Addressability::Addressable)
    {
        return None;
    }
    Some(invalid_unary_reason("&", "operand must be addressable"))
}

fn invalid_unary_receive_operand(operand: &GoType) -> Option<InvalidStatementReason> {
    match operand {
        GoType::Chan { direction, .. } if direction.can_receive() => None,
        GoType::Unknown | GoType::Named(_) => None,
        GoType::Chan { .. } => Some(invalid_unary_reason(
            "<-",
            "operand must be receive-capable channel, got send-only channel",
        )),
        other => Some(invalid_unary_reason(
            "<-",
            format!(
                "operand must be receive-capable channel, got {}",
                go_type_display_name(other)
            ),
        )),
    }
}

fn invalid_unary_reason(
    op: impl Into<String>,
    reason: impl Into<String>,
) -> InvalidStatementReason {
    InvalidStatementReason::InvalidUnary {
        op: op.into(),
        reason: reason.into(),
    }
}

fn unary_op_name(op: token::Token) -> &'static str {
    match op {
        token::Token::ADD => "+",
        token::Token::SUB => "-",
        token::Token::NOT => "!",
        token::Token::XOR => "^",
        token::Token::AND => "&",
        token::Token::ARROW => "<-",
        _ => "unary operator",
    }
}

fn invalid_binary_expr(
    binary: &ast::BinaryExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let left = env.resolve_alias(&GoType::infer_expr(&binary.x, env));
    let right = env.resolve_alias(&GoType::infer_expr(&binary.y, env));
    if expr_is_nil(&binary.x) || expr_is_nil(&binary.y) {
        return binary_nil_operands(binary, &left, &right, env);
    }
    if binary_divisor_is_constant_zero(binary, env) {
        return Some(invalid_binary_reason(
            binary.op,
            "division by zero constant",
        ));
    }
    if binary_type_should_skip_validation(&left, binary.op)
        || binary_type_should_skip_validation(&right, binary.op)
    {
        return None;
    }
    match binary.op {
        token::Token::LAND | token::Token::LOR => binary_bool_operands(binary.op, &left, &right),
        token::Token::ADD => binary_add_operands(binary, &left, &right, env),
        token::Token::SUB | token::Token::MUL | token::Token::QUO => {
            binary_numeric_operands(binary, &left, &right, env)
        }
        token::Token::REM
        | token::Token::AND
        | token::Token::OR
        | token::Token::XOR
        | token::Token::AND_NOT => binary_integer_operands(&left, &right, binary, env),
        token::Token::SHL | token::Token::SHR => {
            binary_shift_operands(binary.op, &left, &right, binary, env)
        }
        token::Token::EQL | token::Token::NEQ => {
            binary_equality_operands(binary, &left, &right, env)
        }
        token::Token::LSS | token::Token::LEQ | token::Token::GTR | token::Token::GEQ => {
            binary_ordered_operands(binary, &left, &right, env)
        }
        _ => None,
    }
}

fn binary_divisor_is_constant_zero(binary: &ast::BinaryExpr<'_>, env: &TypeEnv) -> bool {
    matches!(binary.op, token::Token::QUO | token::Token::REM)
        && expr_is_untyped_numeric_constant_for_comparison(&binary.y, env)
        && numeric_constant_expr_is_zero(&binary.y)
}

fn numeric_constant_expr_is_zero(expr: &ast::Expr<'_>) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => match lit.kind {
            token::Token::INT => integer_literal_is_zero(lit.value),
            token::Token::FLOAT => decimal_float_literal_is_zero(lit.value),
            token::Token::IMAG => imaginary_literal_is_zero(lit.value),
            token::Token::CHAR => rune_literal_value_i128(lit.value) == Some(0),
            _ => false,
        },
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            numeric_constant_expr_is_zero(&unary.x)
        }
        _ => false,
    }
}

fn binary_type_should_skip_validation(ty: &GoType, op: token::Token) -> bool {
    matches!(ty, GoType::Unknown)
        || (matches!(ty, GoType::Named(_)) && !matches!(op, token::Token::EQL | token::Token::NEQ))
}

fn binary_bool_operands(
    op: token::Token,
    left: &GoType,
    right: &GoType,
) -> Option<InvalidStatementReason> {
    if matches!((left, right), (GoType::Bool, GoType::Bool)) {
        return None;
    }
    Some(invalid_binary_reason(
        op,
        format!(
            "operands must both be bool, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn binary_add_operands(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if (go_type_is_numeric(left) && go_type_is_numeric(right))
        || matches!((left, right), (GoType::String, GoType::String))
    {
        if binary_operator_operands_are_compatible(binary, left, right, env) {
            return None;
        }
        return Some(binary_operator_type_mismatch(binary.op, left, right, env));
    }
    Some(invalid_binary_reason(
        token::Token::ADD,
        format!(
            "operands must both be numeric or both be string, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn binary_numeric_operands(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if go_type_is_numeric(left) && go_type_is_numeric(right) {
        if binary_operator_operands_are_compatible(binary, left, right, env) {
            return None;
        }
        return Some(binary_operator_type_mismatch(binary.op, left, right, env));
    }
    Some(invalid_binary_reason(
        binary.op,
        format!(
            "operands must both be numeric, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn binary_integer_operands(
    left: &GoType,
    right: &GoType,
    binary: &ast::BinaryExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if binary_operand_is_integer(left, &binary.x) && binary_operand_is_integer(right, &binary.y) {
        if binary_operator_operands_are_compatible(binary, left, right, env) {
            return None;
        }
        return Some(binary_operator_type_mismatch(binary.op, left, right, env));
    }
    Some(invalid_binary_reason(
        binary.op,
        format!(
            "operands must both be integer, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn binary_operator_operands_are_compatible(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> bool {
    let inferred_left = env.resolve_alias(left);
    let inferred_right = env.resolve_alias(right);
    let left = comparison_operand_effective_type(&binary.x, &inferred_left, &inferred_right, env);
    let right = comparison_operand_effective_type(&binary.y, &inferred_right, &inferred_left, env);
    if matches!(left, GoType::Unknown) || matches!(right, GoType::Unknown) {
        return true;
    }
    if left == right {
        return true;
    }
    comparison_constant_is_assignable_to(&binary.x, &left, &right, env)
        || comparison_constant_is_assignable_to(&binary.y, &right, &left, env)
}

fn binary_operator_type_mismatch(
    op: token::Token,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> InvalidStatementReason {
    let left = env.resolve_alias(left);
    let right = env.resolve_alias(right);
    invalid_binary_reason(
        op,
        format!(
            "operands have mismatched types {} and {}",
            go_type_display_name(&left),
            go_type_display_name(&right)
        ),
    )
}

fn binary_shift_operands(
    op: token::Token,
    left: &GoType,
    right: &GoType,
    binary: &ast::BinaryExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if binary_shift_left_operand_is_integer(left, &binary.x, &binary.y, env)
        && binary_operand_is_integer(right, &binary.y)
    {
        return None;
    }
    Some(invalid_binary_reason(
        op,
        format!(
            "shift operands must be integer, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn binary_shift_left_operand_is_integer(
    ty: &GoType,
    left_expr: &ast::Expr<'_>,
    right_expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> bool {
    ty.is_integer()
        || (ty.is_float()
            && expr_is_integer_constant(left_expr)
            && expr_is_untyped_constant_for_comparison(right_expr, env))
}

fn binary_operand_is_integer(ty: &GoType, expr: &ast::Expr<'_>) -> bool {
    ty.is_integer() || (ty.is_float() && expr_is_integer_constant(expr))
}

fn expr_is_integer_constant(expr: &ast::Expr<'_>) -> bool {
    match expr {
        ast::Expr::BasicLit(lit) => basic_lit_is_integer_constant(lit),
        ast::Expr::ParenExpr(paren) => expr_is_integer_constant(&paren.x),
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            expr_is_integer_constant(&unary.x)
        }
        _ => false,
    }
}

fn basic_lit_is_integer_constant(lit: &ast::BasicLit<'_>) -> bool {
    match lit.kind {
        token::Token::INT => true,
        token::Token::FLOAT => decimal_float_literal_is_integer(lit.value),
        _ => false,
    }
}

fn decimal_float_literal_is_integer(value: &str) -> bool {
    let value = value.replace('_', "").to_ascii_lowercase();
    if value.starts_with("0x") || value.contains('p') {
        return false;
    }

    let (mantissa, exponent) = value
        .split_once('e')
        .map_or((value.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap_or(0))
        });
    let mantissa = mantissa.strip_prefix(['+', '-']).unwrap_or(mantissa);
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int_part}{frac_part}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let shift = exponent - frac_part.len() as i32;
    if shift >= 0 {
        return true;
    }
    digits
        .bytes()
        .rev()
        .take((-shift) as usize)
        .all(|byte| byte == b'0')
}

fn binary_equality_operands(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if !type_is_comparable_for_validation(left, env)
        || !type_is_comparable_for_validation(right, env)
    {
        return Some(invalid_binary_reason(
            binary.op,
            format!(
                "operands must be comparable, got {} and {}",
                go_type_display_name(left),
                go_type_display_name(right)
            ),
        ));
    }
    if comparison_operands_are_assignable(binary, left, right, env) {
        return None;
    }
    let left = env.resolve_alias(left);
    let right = env.resolve_alias(right);
    Some(invalid_binary_reason(
        binary.op,
        format!(
            "operands have mismatched types {} and {}",
            go_type_display_name(&left),
            go_type_display_name(&right)
        ),
    ))
}

fn binary_nil_operands(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if !matches!(binary.op, token::Token::EQL | token::Token::NEQ) {
        return Some(invalid_binary_reason(
            binary.op,
            "operator not defined on untyped nil",
        ));
    }
    let left_is_nil = expr_is_nil(&binary.x);
    let right_is_nil = expr_is_nil(&binary.y);
    if left_is_nil && right_is_nil {
        return Some(invalid_binary_reason(
            binary.op,
            "operator not defined on untyped nil",
        ));
    }
    let other = if left_is_nil { right } else { left };
    if type_can_compare_to_nil(other, env) {
        return None;
    }
    Some(invalid_binary_reason(
        binary.op,
        format!(
            "operand must be comparable to nil, got {}",
            go_type_display_name(other)
        ),
    ))
}

fn binary_ordered_operands(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if (go_type_is_ordered_numeric(left) && go_type_is_ordered_numeric(right))
        || matches!((left, right), (GoType::String, GoType::String))
    {
        if comparison_operands_are_assignable(binary, left, right, env) {
            return None;
        }
        return Some(binary_comparison_type_mismatch(binary.op, left, right, env));
    }
    Some(invalid_binary_reason(
        binary.op,
        format!(
            "operands must both be ordered numeric values or strings, got {} and {}",
            go_type_display_name(left),
            go_type_display_name(right)
        ),
    ))
}

fn comparison_operands_are_assignable(
    binary: &ast::BinaryExpr<'_>,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> bool {
    comparison_exprs_are_assignable(&binary.x, left, &binary.y, right, env)
}

fn comparison_exprs_are_assignable(
    left_expr: &ast::Expr<'_>,
    left: &GoType,
    right_expr: &ast::Expr<'_>,
    right: &GoType,
    env: &TypeEnv,
) -> bool {
    comparison_operand_is_assignable_to(left_expr, left, right, env)
        || comparison_operand_is_assignable_to(right_expr, right, left, env)
}

fn comparison_operand_is_assignable_to(
    expr: &ast::Expr<'_>,
    actual: &GoType,
    expected: &GoType,
    env: &TypeEnv,
) -> bool {
    let expected = env.resolve_alias(expected);
    let actual =
        comparison_operand_effective_type(expr, &env.resolve_alias(actual), &expected, env);
    if matches!(actual, GoType::Unknown) || matches!(expected, GoType::Unknown) {
        return true;
    }
    if expr_is_untyped_constant_for_comparison(expr, env)
        && target_needs_constant_representability_check(&expected)
    {
        return comparison_constant_is_assignable_to(expr, &actual, &expected, env);
    }
    if actual == expected {
        return true;
    }
    if comparison_constant_is_assignable_to(expr, &actual, &expected, env) {
        return true;
    }
    match (&expected, &actual) {
        (GoType::Any | GoType::Interface(_) | GoType::Error, _) => true,
        (GoType::Named(expected), GoType::Named(actual)) => expected == actual,
        (GoType::Named(_), _) | (_, GoType::Named(_)) => true,
        (expected, actual) if go_type_is_numeric(expected) && go_type_is_numeric(actual) => false,
        (GoType::Bool, _) | (_, GoType::Bool) | (GoType::String, _) | (_, GoType::String) => false,
        _ => true,
    }
}

fn comparison_operand_effective_type(
    expr: &ast::Expr<'_>,
    inferred: &GoType,
    expected: &GoType,
    env: &TypeEnv,
) -> GoType {
    match unparen_expr(expr) {
        ast::Expr::UnaryExpr(unary)
            if matches!(
                unary.op,
                token::Token::ADD | token::Token::SUB | token::Token::XOR
            ) =>
        {
            comparison_operand_effective_type(&unary.x, inferred, expected, env)
        }
        ast::Expr::BinaryExpr(binary)
            if binary.op == token::Token::SHL || binary.op == token::Token::SHR =>
        {
            if expr_is_untyped_integer_constant_for_comparison(&binary.x, env)
                && expected.is_integer()
            {
                return expected.clone();
            }
            inferred.clone()
        }
        ast::Expr::BinaryExpr(binary) if binary_op_is_numeric_for_type_inference(binary.op) => {
            let left = env.resolve_alias(&GoType::infer_expr(&binary.x, env));
            let right = env.resolve_alias(&GoType::infer_expr(&binary.y, env));
            let left_const = expr_is_untyped_numeric_constant_for_comparison(&binary.x, env);
            let right_const = expr_is_untyped_numeric_constant_for_comparison(&binary.y, env);
            match (left_const, right_const) {
                (true, false) if go_type_is_numeric(&right) => right,
                (true, false)
                    if matches!(right, GoType::Unknown) && go_type_is_numeric(expected) =>
                {
                    expected.clone()
                }
                (false, true) if go_type_is_numeric(&left) => left,
                (false, true)
                    if matches!(left, GoType::Unknown) && go_type_is_numeric(expected) =>
                {
                    expected.clone()
                }
                _ => inferred.clone(),
            }
        }
        _ => inferred.clone(),
    }
}

fn binary_op_is_numeric_for_type_inference(op: token::Token) -> bool {
    matches!(
        op,
        token::Token::ADD
            | token::Token::SUB
            | token::Token::MUL
            | token::Token::QUO
            | token::Token::REM
            | token::Token::AND
            | token::Token::OR
            | token::Token::XOR
            | token::Token::AND_NOT
    )
}

fn comparison_constant_is_assignable_to(
    expr: &ast::Expr<'_>,
    actual: &GoType,
    expected: &GoType,
    env: &TypeEnv,
) -> bool {
    if !expr_is_untyped_constant_for_comparison(expr, env) {
        return false;
    }
    match expected {
        GoType::Any | GoType::Interface(_) | GoType::Error => {
            untyped_constant_default_type_is_representable(expr, actual)
        }
        GoType::Unknown | GoType::Named(_) => true,
        GoType::Bool => matches!(actual, GoType::Bool),
        GoType::String => matches!(actual, GoType::String),
        expected if expected.is_integer() => {
            if let Some(value) = integer_constant_value_i128(expr) {
                return integer_constant_fits_type(value, expected);
            }
            actual.is_integer() || (actual.is_float() && expr_is_integer_constant(expr))
        }
        expected if expected.is_float() => float_constant_is_assignable_to(expr, actual, expected),
        GoType::Complex64 | GoType::Complex128 => go_type_is_numeric(actual),
        _ => false,
    }
}

fn untyped_constant_default_type_is_representable(expr: &ast::Expr<'_>, actual: &GoType) -> bool {
    match actual {
        GoType::Float64 => float_constant_is_assignable_to(expr, actual, &GoType::Float64),
        GoType::Int => integer_constant_value_i128(expr)
            .is_none_or(|value| integer_constant_fits_type(value, &GoType::Int)),
        GoType::Int32 => integer_constant_value_i128(expr)
            .is_none_or(|value| integer_constant_fits_type(value, &GoType::Int32)),
        GoType::Bool | GoType::String | GoType::Complex128 => true,
        GoType::Unknown | GoType::Named(_) => true,
        _ => true,
    }
}

fn target_needs_constant_representability_check(expected: &GoType) -> bool {
    matches!(
        expected,
        GoType::Any
            | GoType::Interface(_)
            | GoType::Error
            | GoType::Bool
            | GoType::String
            | GoType::Complex64
            | GoType::Complex128
    ) || go_type_is_numeric(expected)
}

fn float_constant_is_assignable_to(
    expr: &ast::Expr<'_>,
    actual: &GoType,
    expected: &GoType,
) -> bool {
    if let Some(representable) = float_constant_is_representable_by_type(expr, expected) {
        return representable;
    }
    go_type_is_ordered_numeric(actual) || integer_constant_value_i128(expr).is_some()
}

fn float_constant_is_representable_by_type(
    expr: &ast::Expr<'_>,
    expected: &GoType,
) -> Option<bool> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => match lit.kind {
            token::Token::INT | token::Token::FLOAT => {
                numeric_literal_is_finite_for_float_type(lit.value, expected)
            }
            token::Token::CHAR => Some(true),
            token::Token::IMAG => Some(imaginary_literal_is_zero(lit.value)),
            _ => Some(false),
        },
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            float_constant_is_representable_by_type(&unary.x, expected)
        }
        _ => None,
    }
}

fn numeric_literal_is_finite_for_float_type(value: &str, expected: &GoType) -> Option<bool> {
    let value = value.replace('_', "");
    if value.starts_with("0x") || value.starts_with("0X") || value.contains(['p', 'P']) {
        return None;
    }
    match expected {
        GoType::Float32 => Some(value.parse::<f32>().map(f32::is_finite).unwrap_or(false)),
        GoType::Float64 => Some(value.parse::<f64>().map(f64::is_finite).unwrap_or(false)),
        _ => None,
    }
}

fn expr_is_untyped_integer_constant_for_comparison(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    expr_is_untyped_numeric_constant_for_comparison(expr, env)
        && matches!(
            env.resolve_alias(&GoType::infer_expr(expr, env)),
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

fn expr_is_untyped_numeric_constant_for_comparison(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    if !expr_is_untyped_constant_for_comparison(expr, env) {
        return false;
    }
    go_type_is_numeric(&env.resolve_alias(&GoType::infer_expr(expr, env)))
}

fn expr_is_untyped_constant_for_comparison(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(_) => true,
        ast::Expr::Ident(ident) if matches!(ident.name, "true" | "false" | "iota") => true,
        ast::Expr::Ident(ident) => env.is_const(ident.name),
        ast::Expr::SelectorExpr(selector) => selector_expr_is_const(selector, env),
        ast::Expr::UnaryExpr(unary)
            if matches!(
                unary.op,
                token::Token::ADD | token::Token::SUB | token::Token::NOT | token::Token::XOR
            ) =>
        {
            expr_is_untyped_constant_for_comparison(&unary.x, env)
        }
        ast::Expr::BinaryExpr(binary) => {
            expr_is_untyped_constant_for_comparison(&binary.x, env)
                && expr_is_untyped_constant_for_comparison(&binary.y, env)
        }
        ast::Expr::CallExpr(call) => !const_call_is_known_non_constant(call, env),
        _ => false,
    }
}

fn selector_expr_is_const(selector: &ast::SelectorExpr<'_>, env: &TypeEnv) -> bool {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return false;
    };
    env.is_const(&format!("{}.{}", base.name, selector.sel.name))
}

fn binary_comparison_type_mismatch(
    op: token::Token,
    left: &GoType,
    right: &GoType,
    env: &TypeEnv,
) -> InvalidStatementReason {
    let left = env.resolve_alias(left);
    let right = env.resolve_alias(right);
    invalid_binary_reason(
        op,
        format!(
            "operands have mismatched types {} and {}",
            go_type_display_name(&left),
            go_type_display_name(&right)
        ),
    )
}

fn type_is_comparable_for_validation(ty: &GoType, env: &TypeEnv) -> bool {
    let mut visiting = BTreeSet::new();
    type_is_comparable_for_validation_inner(ty, env, &mut visiting)
}

fn type_is_comparable_for_validation_inner(
    ty: &GoType,
    env: &TypeEnv,
    visiting: &mut BTreeSet<String>,
) -> bool {
    let ty = env.resolve_alias(ty);
    match &ty {
        GoType::Slice(_) | GoType::Map(_, _) | GoType::Func { .. } => false,
        GoType::Array(elem) => type_is_comparable_for_validation_inner(elem, env, visiting),
        GoType::Named(name) if matches!(env.get_type_kind(name), Some(TypeKind::Struct)) => {
            if !visiting.insert(name.clone()) {
                return true;
            }
            let comparable = env
                .get_struct_fields(name)
                .iter()
                .all(|(_, field)| type_is_comparable_for_validation_inner(field, env, visiting));
            visiting.remove(name);
            comparable
        }
        GoType::Unknown | GoType::Named(_) => true,
        GoType::Bool
        | GoType::Int
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
        | GoType::Float32
        | GoType::Float64
        | GoType::Complex64
        | GoType::Complex128
        | GoType::String
        | GoType::Pointer(_)
        | GoType::Chan { .. }
        | GoType::Interface(_)
        | GoType::Any
        | GoType::Error => true,
    }
}

fn type_expr_is_comparable_for_validation(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(expr) {
        ast::Expr::ArrayType(array) => {
            array.len.is_some() && type_expr_is_comparable_for_validation(&array.elt, env)
        }
        ast::Expr::MapType(_) | ast::Expr::FuncType(_) => false,
        ast::Expr::StructType(struct_type) => struct_type.fields.as_ref().is_none_or(|fields| {
            fields.list.iter().all(|field| {
                field
                    .type_
                    .as_ref()
                    .is_some_and(|type_| type_expr_is_comparable_for_validation(type_, env))
            })
        }),
        ast::Expr::Ident(_) | ast::Expr::SelectorExpr(_) => {
            type_is_comparable_for_validation(&GoType::from_expr(expr), env)
        }
        ast::Expr::StarExpr(_) | ast::Expr::ChanType(_) | ast::Expr::InterfaceType(_) => true,
        _ => true,
    }
}

fn type_expr_is_interface_for_validation(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    if matches!(unparen_expr(expr), ast::Expr::InterfaceType(_)) {
        return true;
    }
    match env.resolve_alias(&GoType::from_expr(expr)) {
        GoType::Any | GoType::Error | GoType::Interface(_) => true,
        GoType::Named(name) => env.is_interface(&name),
        _ => false,
    }
}

fn type_expr_implements_interface_for_validation(
    expr: &ast::Expr<'_>,
    interface_name: &str,
    env: &TypeEnv,
) -> bool {
    let Some(type_name) = type_expr_named_type_for_validation(expr) else {
        return false;
    };
    named_type_implements_interface_for_validation(&type_name, interface_name, env)
}

fn named_type_implements_interface_for_validation(
    type_name: &str,
    interface_name: &str,
    env: &TypeEnv,
) -> bool {
    env.get_interface_methods(interface_name)
        .is_none_or(|methods| {
            methods
                .iter()
                .all(|method| env.has_func(&format!("{type_name}.{method}")))
        })
}

fn type_expr_named_type_for_validation(expr: &ast::Expr<'_>) -> Option<String> {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => Some(match unparen_expr(&selector.x) {
            ast::Expr::Ident(package) => format!("{}.{}", package.name, selector.sel.name),
            _ => selector.sel.name.to_string(),
        }),
        ast::Expr::StarExpr(star) => type_expr_named_type_for_validation(&star.x),
        _ => None,
    }
}

fn type_expr_display_name(expr: &ast::Expr<'_>, env: &TypeEnv) -> String {
    match unparen_expr(expr) {
        ast::Expr::FuncType(_) => "func".to_string(),
        ast::Expr::MapType(map) => format!(
            "map[{}]{}",
            type_expr_display_name(&map.key, env),
            type_expr_display_name(&map.value, env)
        ),
        ast::Expr::StructType(_) => "struct".to_string(),
        _ => go_type_display_name(&env.resolve_alias(&GoType::from_expr(expr))),
    }
}

fn invalid_binary_reason(op: token::Token, reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidBinary {
        op: binary_op_name(op).to_string(),
        reason: reason.into(),
    }
}

fn binary_op_name(op: token::Token) -> &'static str {
    match op {
        token::Token::ADD => "+",
        token::Token::SUB => "-",
        token::Token::MUL => "*",
        token::Token::QUO => "/",
        token::Token::REM => "%",
        token::Token::AND => "&",
        token::Token::OR => "|",
        token::Token::XOR => "^",
        token::Token::SHL => "<<",
        token::Token::SHR => ">>",
        token::Token::AND_NOT => "&^",
        token::Token::LAND => "&&",
        token::Token::LOR => "||",
        token::Token::EQL => "==",
        token::Token::NEQ => "!=",
        token::Token::LSS => "<",
        token::Token::LEQ => "<=",
        token::Token::GTR => ">",
        token::Token::GEQ => ">=",
        _ => "binary operator",
    }
}

fn invalid_slice_expr(slice: &ast::SliceExpr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let target = env.resolve_alias(&GoType::infer_expr(&slice.x, env));
    match target {
        GoType::String if slice.max.is_some() => Some(invalid_slice_reason(
            "full slice expression is not valid for strings",
        )),
        GoType::String | GoType::Slice(_) | GoType::Array(_) => invalid_slice_bounds(slice, env),
        GoType::Pointer(inner) if matches!(inner.as_ref(), GoType::Array(_)) => {
            invalid_slice_bounds(slice, env)
        }
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(invalid_slice_reason(format!(
            "cannot slice {}",
            go_type_display_name(&other)
        ))),
    }
}

fn invalid_slice_bounds(
    slice: &ast::SliceExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    [slice.low.as_ref(), slice.high.as_ref(), slice.max.as_ref()]
        .into_iter()
        .flatten()
        .find_map(|bound| invalid_integer_bound(bound, env))
}

fn invalid_integer_bound(bound: &ast::Expr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(bound, env));
    if matches!(ty, GoType::Unknown | GoType::Named(_)) || binary_operand_is_integer(&ty, bound) {
        return None;
    }
    Some(invalid_slice_reason(format!(
        "bound must have integer type, got {}",
        go_type_display_name(&ty)
    )))
}

fn invalid_slice_reason(reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidSlice {
        reason: reason.into(),
    }
}

enum CompositeLiteralKind {
    Struct {
        fields: Vec<(String, GoType)>,
        known: bool,
    },
    Array {
        elem: GoType,
        len: Option<usize>,
    },
    Slice {
        elem: GoType,
    },
    Map {
        key: GoType,
        value: GoType,
    },
    Unknown,
}

fn invalid_composite_lit(
    comp: &ast::CompositeLit<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let type_expr = comp.type_.as_deref()?;
    let elems = comp.elts.as_deref().unwrap_or(&[]);
    match composite_literal_kind(type_expr, env) {
        CompositeLiteralKind::Struct { fields, known } => {
            invalid_struct_composite_lit(&fields, known, elems, env)
        }
        CompositeLiteralKind::Array { elem, len } => {
            invalid_array_composite_lit(&elem, len, elems, env)
        }
        CompositeLiteralKind::Slice { elem } => invalid_slice_composite_lit(&elem, elems, env),
        CompositeLiteralKind::Map { key, value } => {
            invalid_map_composite_lit(&key, &value, elems, env)
        }
        CompositeLiteralKind::Unknown => None,
    }
}

fn composite_literal_kind(type_expr: &ast::Expr<'_>, env: &TypeEnv) -> CompositeLiteralKind {
    match unparen_expr(type_expr) {
        ast::Expr::ArrayType(array) => {
            let elem = GoType::from_expr(&array.elt);
            if array.len.is_some() {
                CompositeLiteralKind::Array {
                    elem,
                    len: array_literal_len(array.len.as_deref()),
                }
            } else {
                CompositeLiteralKind::Slice { elem }
            }
        }
        ast::Expr::MapType(map) => CompositeLiteralKind::Map {
            key: GoType::from_expr(&map.key),
            value: GoType::from_expr(&map.value),
        },
        ast::Expr::StructType(struct_type) => CompositeLiteralKind::Struct {
            fields: struct_literal_fields(struct_type),
            known: true,
        },
        _ => composite_literal_kind_from_type(type_expr, env),
    }
}

fn composite_literal_kind_from_type(
    type_expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> CompositeLiteralKind {
    match env.resolve_alias(&GoType::from_expr(type_expr)) {
        GoType::Array(elem) => CompositeLiteralKind::Array {
            elem: *elem,
            len: None,
        },
        GoType::Slice(elem) => CompositeLiteralKind::Slice { elem: *elem },
        GoType::Map(key, value) => CompositeLiteralKind::Map {
            key: *key,
            value: *value,
        },
        GoType::Named(name) => named_struct_composite_kind(&name, env),
        _ => composite_type_name(type_expr)
            .map(|name| named_struct_composite_kind(&name, env))
            .unwrap_or(CompositeLiteralKind::Unknown),
    }
}

fn named_struct_composite_kind(name: &str, env: &TypeEnv) -> CompositeLiteralKind {
    let fields = env.get_struct_fields(name);
    if matches!(env.get_type_kind(name), Some(TypeKind::Struct)) || !fields.is_empty() {
        CompositeLiteralKind::Struct {
            fields,
            known: true,
        }
    } else {
        CompositeLiteralKind::Unknown
    }
}

fn composite_type_name(type_expr: &ast::Expr<'_>) -> Option<String> {
    match unparen_expr(type_expr) {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(package) = selector.x.as_ref() {
                Some(format!("{}.{}", package.name, selector.sel.name))
            } else {
                Some(selector.sel.name.to_string())
            }
        }
        ast::Expr::IndexExpr(index) => composite_type_name(&index.x),
        ast::Expr::IndexListExpr(index) => composite_type_name(&index.x),
        _ => None,
    }
}

fn struct_literal_fields(struct_type: &ast::StructType<'_>) -> Vec<(String, GoType)> {
    let Some(fields) = &struct_type.fields else {
        return Vec::new();
    };
    fields
        .list
        .iter()
        .flat_map(|field| {
            let ty = field
                .type_
                .as_ref()
                .map(GoType::from_expr)
                .unwrap_or(GoType::Unknown);
            struct_field_names(field)
                .into_iter()
                .map(move |name| (name, ty.clone()))
        })
        .collect()
}

fn invalid_struct_composite_lit(
    fields: &[(String, GoType)],
    known: bool,
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if !known {
        return None;
    }
    let keyed = elems.iter().any(elt_key_value);
    if keyed {
        return invalid_keyed_struct_composite_lit(fields, elems, env);
    }
    invalid_unkeyed_struct_composite_lit(fields, elems, env)
}

fn invalid_keyed_struct_composite_lit(
    fields: &[(String, GoType)],
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let field_types: BTreeMap<_, _> = fields
        .iter()
        .map(|(name, ty)| (name.as_str(), ty))
        .collect();
    let mut seen = BTreeSet::new();
    for elem in elems {
        let Some((key, value)) = elt_key_value_exprs(elem) else {
            return Some(invalid_composite_literal_reason(
                "all struct literal elements must be keyed when any element is keyed",
            ));
        };
        let Some(field) = struct_literal_key_name(key) else {
            return Some(invalid_composite_literal_reason(
                "struct literal key must be a field name",
            ));
        };
        let Some(expected) = field_types.get(field).copied() else {
            return Some(invalid_composite_literal_reason(format!(
                "unknown field {field}"
            )));
        };
        if !seen.insert(field.to_string()) {
            return Some(invalid_composite_literal_reason(format!(
                "duplicate field {field}"
            )));
        }
        if let Some(reason) = invalid_composite_element_type("field", expected, value, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_unkeyed_struct_composite_lit(
    fields: &[(String, GoType)],
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if elems.is_empty() {
        return None;
    }
    if elems.len() != fields.len() {
        return Some(invalid_composite_literal_reason(format!(
            "struct literal expects {} field value(s), got {}",
            fields.len(),
            elems.len()
        )));
    }
    fields
        .iter()
        .zip(elems.iter())
        .find_map(|((_, expected), elem)| {
            invalid_composite_element_type("field", expected, elem, env)
        })
}

fn invalid_array_composite_lit(
    elem: &GoType,
    len: Option<usize>,
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    invalid_indexed_composite_lit(elem, len, elems, env)
}

fn invalid_slice_composite_lit(
    elem: &GoType,
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    invalid_indexed_composite_lit(elem, None, elems, env)
}

fn invalid_indexed_composite_lit(
    elem: &GoType,
    len: Option<usize>,
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let mut seen_keys = BTreeSet::new();
    let mut next_index = 0usize;
    for elem_expr in elems {
        let value = match elt_key_value_exprs(elem_expr) {
            Some((key, value)) => {
                if let Some(reason) = invalid_array_slice_literal_key(key, len, env) {
                    return Some(reason);
                }
                if let Some(index) = integer_constant_index_value(key) {
                    next_index = index.saturating_add(1);
                } else {
                    next_index = next_index.saturating_add(1);
                }
                if let Some(fingerprint) = indexed_literal_key_fingerprint(key, env)
                    && !seen_keys.insert(fingerprint)
                {
                    return Some(invalid_composite_literal_reason("duplicate index key"));
                }
                value
            }
            None => {
                let index = next_index;
                next_index = next_index.saturating_add(1);
                if let Some(array_len) = len
                    && index >= array_len
                {
                    return Some(invalid_composite_literal_reason(format!(
                        "array literal index {index} out of bounds for length {array_len}"
                    )));
                }
                if !seen_keys.insert(format!("index:{index}")) {
                    return Some(invalid_composite_literal_reason("duplicate index key"));
                }
                elem_expr
            }
        };
        if let Some(reason) = invalid_composite_element_type("element", elem, value, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_array_slice_literal_key(
    key: &ast::Expr<'_>,
    len: Option<usize>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(key, env));
    if !(matches!(ty, GoType::Unknown | GoType::Named(_))
        || ty.is_integer()
        || (ty.is_float() && expr_is_integer_constant(key)))
    {
        return Some(invalid_composite_literal_reason(format!(
            "index key must be an integer constant, got {}",
            go_type_display_name(&ty)
        )));
    }
    if expr_is_known_non_constant(key, env) {
        return Some(invalid_composite_literal_reason(
            "index key must be a constant",
        ));
    }
    if expr_is_negative_integer_constant(key) {
        return Some(invalid_composite_literal_reason(
            "index key must be non-negative",
        ));
    }
    if let (Some(array_len), Some(index)) = (len, integer_constant_index_value(key))
        && index >= array_len
    {
        return Some(invalid_composite_literal_reason(format!(
            "array literal index {index} out of bounds for length {array_len}"
        )));
    }
    None
}

fn invalid_map_composite_lit(
    key: &GoType,
    value: &GoType,
    elems: &[ast::Expr<'_>],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let mut seen_keys = BTreeSet::new();
    for elem in elems {
        let Some((key_expr, value_expr)) = elt_key_value_exprs(elem) else {
            return Some(invalid_composite_literal_reason(
                "map literal elements must be keyed",
            ));
        };
        if let Some(reason) = invalid_composite_element_type("key", key, key_expr, env) {
            return Some(reason);
        }
        if let Some(fingerprint) = literal_constant_key_fingerprint(key_expr, env)
            && !seen_keys.insert(fingerprint)
        {
            return Some(invalid_composite_literal_reason("duplicate map key"));
        }
        if let Some(reason) = invalid_composite_element_type("value", value, value_expr, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_composite_element_type(
    role: &str,
    expected: &GoType,
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let expected = env.resolve_alias(expected);
    let actual = env.resolve_alias(&GoType::infer_expr(expr, env));
    if expr_is_assignable_for_validation(&expected, expr, env) {
        return None;
    }
    Some(invalid_composite_literal_reason(format!(
        "{role} must be assignable to {}, got {}",
        go_type_display_name(&expected),
        go_type_display_name(&actual)
    )))
}

fn elt_key_value(elem: &ast::Expr<'_>) -> bool {
    matches!(unparen_expr(elem), ast::Expr::KeyValueExpr(_))
}

fn elt_key_value_exprs<'a>(
    elem: &'a ast::Expr<'a>,
) -> Option<(&'a ast::Expr<'a>, &'a ast::Expr<'a>)> {
    match unparen_expr(elem) {
        ast::Expr::KeyValueExpr(kv) => Some((&kv.key, &kv.value)),
        _ => None,
    }
}

fn struct_literal_key_name<'a>(key: &'a ast::Expr<'a>) -> Option<&'a str> {
    match unparen_expr(key) {
        ast::Expr::Ident(ident) => Some(ident.name),
        _ => None,
    }
}

fn array_literal_len(len: Option<&ast::Expr<'_>>) -> Option<usize> {
    integer_constant_index_value(len?)
}

fn integer_constant_index_value(expr: &ast::Expr<'_>) -> Option<usize> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            parse_integer_literal_usize(lit.value)
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            integer_constant_index_value(&unary.x)
        }
        _ => None,
    }
}

fn parse_integer_literal_usize(value: &str) -> Option<usize> {
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
    usize::from_str_radix(if digits.is_empty() { "0" } else { digits }, radix).ok()
}

fn integer_constant_value_i128(expr: &ast::Expr<'_>) -> Option<i128> {
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            parse_integer_literal_i128(lit.value)
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::CHAR => {
            rune_literal_value_i128(lit.value)
        }
        ast::Expr::BasicLit(lit)
            if lit.kind == token::Token::IMAG && imaginary_literal_is_zero(lit.value) =>
        {
            Some(0)
        }
        ast::Expr::BasicLit(lit)
            if lit.kind == token::Token::FLOAT && decimal_float_literal_is_integer(lit.value) =>
        {
            parse_decimal_float_integer_i128(lit.value)
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            integer_constant_value_i128(&unary.x)
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::SUB => {
            integer_constant_value_i128(&unary.x).and_then(i128::checked_neg)
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

fn rune_literal_value_i128(value: &str) -> Option<i128> {
    let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut chars = inner.chars();
    let first = chars.next()?;
    let value = if first == '\\' {
        rune_escape_value(&mut chars)?
    } else {
        first as u32
    };
    chars.next().is_none().then_some(i128::from(value))
}

fn rune_escape_value(chars: &mut std::str::Chars<'_>) -> Option<u32> {
    match chars.next()? {
        'a' => Some(0x07),
        'b' => Some(0x08),
        'f' => Some(0x0c),
        'n' => Some(0x0a),
        'r' => Some(0x0d),
        't' => Some(0x09),
        'v' => Some(0x0b),
        '\\' => Some('\\' as u32),
        '\'' => Some('\'' as u32),
        'x' => parse_fixed_radix_escape(chars, 2, 16).filter(|value| *value <= 0xff),
        'u' => parse_unicode_escape(chars, 4),
        'U' => parse_unicode_escape(chars, 8),
        first @ '0'..='7' => parse_octal_escape(chars, first),
        _ => None,
    }
}

fn parse_fixed_radix_escape(
    chars: &mut std::str::Chars<'_>,
    count: usize,
    radix: u32,
) -> Option<u32> {
    let mut digits = String::with_capacity(count);
    for _ in 0..count {
        let ch = chars.next()?;
        if !ch.is_digit(radix) {
            return None;
        }
        digits.push(ch);
    }
    u32::from_str_radix(&digits, radix).ok()
}

fn parse_unicode_escape(chars: &mut std::str::Chars<'_>, count: usize) -> Option<u32> {
    let value = parse_fixed_radix_escape(chars, count, 16)?;
    char::from_u32(value).map(|ch| ch as u32)
}

fn parse_octal_escape(chars: &mut std::str::Chars<'_>, first: char) -> Option<u32> {
    let mut digits = String::with_capacity(3);
    digits.push(first);
    for _ in 0..2 {
        let ch = chars.next()?;
        if !matches!(ch, '0'..='7') {
            return None;
        }
        digits.push(ch);
    }
    u32::from_str_radix(&digits, 8)
        .ok()
        .filter(|value| *value <= 0xff)
}

fn imaginary_literal_is_zero(value: &str) -> bool {
    let Some(value) = value.strip_suffix('i') else {
        return false;
    };
    numeric_literal_mantissa_is_zero(value)
}

fn numeric_literal_mantissa_is_zero(value: &str) -> bool {
    let value = value.replace('_', "").to_ascii_lowercase();
    let mantissa = value
        .split_once(['e', 'p'])
        .map_or(value.as_str(), |(mantissa, _)| mantissa);
    let mantissa = mantissa
        .strip_prefix("0x")
        .or_else(|| mantissa.strip_prefix("0o"))
        .or_else(|| mantissa.strip_prefix("0b"))
        .unwrap_or(mantissa);
    let mut saw_digit = false;
    for ch in mantissa.chars().filter(|ch| *ch != '.') {
        if ch != '0' {
            return false;
        }
        saw_digit = true;
    }
    saw_digit
}

fn parse_decimal_float_integer_i128(value: &str) -> Option<i128> {
    let value = value.replace('_', "").to_ascii_lowercase();
    if value.starts_with("0x") || value.contains('p') {
        return None;
    }
    let (mantissa, exponent) = if let Some((mantissa, exponent)) = value.split_once('e') {
        (mantissa, exponent.parse::<i32>().ok()?)
    } else {
        (value.as_str(), 0)
    };
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa.strip_prefix('+').unwrap_or(mantissa)),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = format!("{int_part}{frac_part}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let shift = exponent - frac_part.len() as i32;
    if shift >= 0 {
        digits.push_str(&"0".repeat(shift as usize));
    } else {
        let trim = (-shift) as usize;
        if trim > digits.len() {
            digits.clear();
        } else {
            digits.truncate(digits.len() - trim);
        }
    }
    let magnitude = digits.trim_start_matches('0');
    let magnitude = if magnitude.is_empty() { "0" } else { magnitude };
    let value = magnitude.parse::<i128>().ok()?;
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

fn integer_constant_fits_type(value: i128, ty: &GoType) -> bool {
    match ty {
        GoType::Int => (isize::MIN as i128..=isize::MAX as i128).contains(&value),
        GoType::Int8 => (i8::MIN as i128..=i8::MAX as i128).contains(&value),
        GoType::Int16 => (i16::MIN as i128..=i16::MAX as i128).contains(&value),
        GoType::Int32 => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
        GoType::Int64 => (i64::MIN as i128..=i64::MAX as i128).contains(&value),
        GoType::Uint | GoType::Uintptr => (0..=usize::MAX as i128).contains(&value),
        GoType::Uint8 => (0..=u8::MAX as i128).contains(&value),
        GoType::Uint16 => (0..=u16::MAX as i128).contains(&value),
        GoType::Uint32 => (0..=u32::MAX as i128).contains(&value),
        GoType::Uint64 => (0..=u64::MAX as i128).contains(&value),
        _ => false,
    }
}

fn expr_is_negative_integer_constant(expr: &ast::Expr<'_>) -> bool {
    match unparen_expr(expr) {
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::SUB => {
            expr_is_integer_constant(&unary.x)
        }
        _ => false,
    }
}

fn indexed_literal_key_fingerprint(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    integer_constant_index_value(expr)
        .map(|index| format!("index:{index}"))
        .or_else(|| literal_constant_key_fingerprint(expr, env))
}

fn literal_constant_key_fingerprint(expr: &ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    if expr_is_known_non_constant(expr, env) {
        return None;
    }
    match unparen_expr(expr) {
        ast::Expr::BasicLit(lit) => Some(format!("{:?}:{}", lit.kind, lit.value)),
        ast::Expr::Ident(ident) if matches!(ident.name, "true" | "false") => {
            Some(format!("bool:{}", ident.name))
        }
        ast::Expr::Ident(ident) if env.is_const(ident.name) => {
            Some(format!("const:{}", ident.name))
        }
        ast::Expr::UnaryExpr(unary)
            if matches!(unary.op, token::Token::ADD | token::Token::SUB) =>
        {
            literal_constant_key_fingerprint(&unary.x, env)
                .map(|inner| format!("{}{}", unary_op_name(unary.op), inner))
        }
        _ => None,
    }
}

fn invalid_composite_literal_reason(reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidCompositeLiteral {
        reason: reason.into(),
    }
}

fn invalid_type_conversion_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if !call_is_type_conversion(call, env) {
        return None;
    }
    let target = type_conversion_target_name(&call.fun, env);
    if call.ellipsis.is_some() {
        return Some(invalid_type_conversion_reason(
            &target,
            "cannot use spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_type_conversion_reason(
            &target,
            format!("expects 1 argument, got {}", args.len()),
        ));
    }
    if let Some(values) =
        expression_value_count(args.first()?, env, TupleAssignmentMode::SingleValueContext)
            .filter(|values| *values != 1)
    {
        return Some(invalid_type_conversion_reason(
            &target,
            format!("expects 1 argument, got {values} values"),
        ));
    }
    if let Some(reason) = invalid_type_conversion_value(&call.fun, args.first()?, &target, env) {
        return Some(reason);
    }
    None
}

fn invalid_type_conversion_value(
    target_expr: &ast::Expr<'_>,
    value: &ast::Expr<'_>,
    target_name: &str,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let target = env.resolve_alias(&GoType::from_expr(target_expr));
    let actual = env.resolve_alias(&GoType::infer_expr(value, env));
    if conversion_is_valid_for_validation(value, &actual, &target, env) {
        return None;
    }
    Some(invalid_type_conversion_reason(
        target_name,
        format!(
            "cannot convert {} to {}",
            go_type_display_name(&actual),
            go_type_display_name(&target)
        ),
    ))
}

fn conversion_is_valid_for_validation(
    value: &ast::Expr<'_>,
    actual: &GoType,
    target: &GoType,
    env: &TypeEnv,
) -> bool {
    if matches!(target, GoType::Unknown | GoType::Named(_))
        || matches!(actual, GoType::Unknown | GoType::Named(_))
    {
        return true;
    }
    if string_slice_conversion_is_valid(actual, target) {
        return true;
    }
    if expr_is_untyped_constant_for_comparison(value, env) {
        if expr_is_assignable_for_validation(target, value, env) {
            return true;
        }
        return matches!((target, actual), (GoType::String, actual) if actual.is_integer());
    }
    if actual == target || expr_is_assignable_for_validation(target, value, env) {
        return true;
    }
    match (target, actual) {
        (target, actual) if go_type_is_numeric(target) && go_type_is_numeric(actual) => true,
        (GoType::String, actual) if actual.is_integer() => true,
        _ => false,
    }
}

fn string_slice_conversion_is_valid(actual: &GoType, target: &GoType) -> bool {
    matches!(
        (target, actual),
        (GoType::String, GoType::Slice(elem)) | (GoType::Slice(elem), GoType::String)
            if matches!(elem.as_ref(), GoType::Uint8 | GoType::Int32)
    )
}

fn type_conversion_target_name(fun: &ast::Expr<'_>, env: &TypeEnv) -> String {
    match unparen_expr(fun) {
        ast::Expr::Ident(ident) => ident.name.to_string(),
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(pkg) = selector.x.as_ref() {
                return format!("{}.{}", pkg.name, selector.sel.name);
            }
            selector.sel.name.to_string()
        }
        other => go_type_display_name(&env.resolve_alias(&GoType::from_expr(other))),
    }
}

fn invalid_type_conversion_reason(
    target: &str,
    reason: impl Into<String>,
) -> InvalidStatementReason {
    InvalidStatementReason::InvalidTypeConversion {
        target: target.to_string(),
        reason: reason.into(),
    }
}

fn call_signature(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<CallSignature> {
    call_signature_for_fun(&call.fun, env)
}

fn call_signature_for_fun(fun: &ast::Expr<'_>, env: &TypeEnv) -> Option<CallSignature> {
    match unparen_expr(fun) {
        ast::Expr::Ident(ident) => call_signature_for_ident(ident.name, env),
        ast::Expr::SelectorExpr(selector) => call_signature_for_selector(selector, env)
            .or_else(|| call_signature_from_inferred_type(fun, "function value", env)),
        ast::Expr::FuncLit(func_lit) => Some(call_signature_from_func_type(
            "function literal".to_string(),
            &func_lit.type_,
        )),
        ast::Expr::ParenExpr(paren) => call_signature_for_fun(&paren.x, env),
        _ => call_signature_from_inferred_type(fun, "function value", env),
    }
}

fn call_signature_for_ident(name: &str, env: &TypeEnv) -> Option<CallSignature> {
    if env.has_func(name) {
        return Some(CallSignature {
            target: name.to_string(),
            params: env.get_func_params(name),
            variadic_start: env.get_func_variadic_start(name),
        });
    }
    match env.get_var(name) {
        Some(GoType::Func { params, .. }) => Some(CallSignature {
            target: name.to_string(),
            params,
            variadic_start: None,
        }),
        _ => None,
    }
}

fn call_signature_for_selector(
    selector: &ast::SelectorExpr<'_>,
    env: &TypeEnv,
) -> Option<CallSignature> {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return None;
    };
    let package_key = format!("{}.{}", base.name, selector.sel.name);
    if env.has_func(&package_key) {
        return Some(CallSignature {
            target: package_key.clone(),
            params: env.get_func_params(&package_key),
            variadic_start: env.get_func_variadic_start(&package_key),
        });
    }
    let receiver_name = env
        .get_var(base.name)
        .and_then(|ty| method_receiver_type_name(ty, env))?;
    let method_key = format!("{}.{}", receiver_name, selector.sel.name);
    env.has_func(&method_key).then(|| CallSignature {
        target: method_key.clone(),
        params: env.get_func_params(&method_key),
        variadic_start: env.get_func_variadic_start(&method_key),
    })
}

fn method_receiver_type_name(ty: GoType, env: &TypeEnv) -> Option<String> {
    match env.resolve_alias(&ty) {
        GoType::Named(name) => Some(name),
        GoType::Pointer(inner) => method_receiver_type_name(*inner, env),
        _ => None,
    }
}

fn call_signature_from_inferred_type(
    fun: &ast::Expr<'_>,
    target: &str,
    env: &TypeEnv,
) -> Option<CallSignature> {
    match env.resolve_alias(&GoType::infer_expr(fun, env)) {
        GoType::Func { params, .. } => Some(CallSignature {
            target: target.to_string(),
            params,
            variadic_start: None,
        }),
        _ => None,
    }
}

fn call_signature_from_func_type(target: String, func_type: &ast::FuncType<'_>) -> CallSignature {
    CallSignature {
        target,
        params: field_list_types(Some(&func_type.params)),
        variadic_start: func_type_variadic_start(func_type),
    }
}

fn func_type_variadic_start(func_type: &ast::FuncType<'_>) -> Option<usize> {
    let mut param_count = 0;
    for field in &func_type.params.list {
        if matches!(field.type_, Some(ast::Expr::Ellipsis(_))) {
            return Some(param_count);
        }
        param_count += field.names.as_ref().map_or(1, Vec::len);
    }
    None
}

fn invalid_fixed_call_args(
    call: &ast::CallExpr<'_>,
    signature: &CallSignature,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_call_reason(
            &signature.target,
            "cannot use spread arguments with non-variadic call",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if let Some(return_types) = single_call_result_types(args, env)
        && (return_types.len() != 1 || signature.params.len() != 1)
    {
        if return_types.len() != signature.params.len() {
            return Some(invalid_call_reason(
                &signature.target,
                format!(
                    "expects {} argument(s), got {} return value(s)",
                    signature.params.len(),
                    return_types.len()
                ),
            ));
        }
        return invalid_call_arg_types(
            &signature.target,
            signature.params.iter(),
            &return_types,
            env,
        );
    }
    if args.len() != signature.params.len() {
        return Some(invalid_call_reason(
            &signature.target,
            format!(
                "expects {} argument(s), got {}",
                signature.params.len(),
                args.len()
            ),
        ));
    }
    invalid_call_arg_exprs(&signature.target, signature.params.iter(), args.iter(), env)
}

fn invalid_variadic_call_args(
    call: &ast::CallExpr<'_>,
    signature: &CallSignature,
    variadic_start: usize,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let args = call.args.as_deref().unwrap_or(&[]);
    let variadic_param = signature.params.get(variadic_start)?;
    let variadic_elem = variadic_elem_type(variadic_param, env);
    if call.ellipsis.is_some() {
        return invalid_variadic_spread_call_args(call, signature, variadic_start, env);
    }
    if let Some(return_types) = single_call_result_types(args, env)
        && (return_types.len() != 1 || variadic_start != 0)
    {
        return invalid_variadic_forwarded_call_args(
            signature,
            variadic_start,
            variadic_elem.as_ref(),
            &return_types,
            env,
        );
    }
    if args.len() < variadic_start {
        return Some(invalid_call_reason(
            &signature.target,
            format!(
                "expects at least {} argument(s), got {}",
                variadic_start,
                args.len()
            ),
        ));
    }
    for (idx, (expected, arg)) in signature
        .params
        .iter()
        .take(variadic_start)
        .zip(args.iter())
        .enumerate()
    {
        if let Some(reason) = invalid_call_arg_expr(&signature.target, idx + 1, expected, arg, env)
        {
            return Some(reason);
        }
    }
    if let Some(elem) = variadic_elem {
        for (idx, arg) in args.iter().skip(variadic_start).enumerate() {
            if let Some(reason) =
                invalid_call_arg_expr(&signature.target, variadic_start + idx + 1, &elem, arg, env)
            {
                return Some(reason);
            }
        }
    }
    None
}

fn invalid_variadic_spread_call_args(
    call: &ast::CallExpr<'_>,
    signature: &CallSignature,
    variadic_start: usize,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let args = call.args.as_deref().unwrap_or(&[]);
    let expected_count = variadic_start + 1;
    if args.len() != expected_count {
        return Some(invalid_call_reason(
            &signature.target,
            format!(
                "spread call expects {} argument(s), got {}",
                expected_count,
                args.len()
            ),
        ));
    }
    for (idx, (expected, arg)) in signature
        .params
        .iter()
        .take(variadic_start)
        .zip(args.iter())
        .enumerate()
    {
        if let Some(reason) = invalid_call_arg_expr(&signature.target, idx + 1, expected, arg, env)
        {
            return Some(reason);
        }
    }
    let spread_arg = args.last()?;
    let spread_param = signature.params.get(variadic_start)?;
    invalid_call_arg_expr(
        &signature.target,
        expected_count,
        spread_param,
        spread_arg,
        env,
    )
}

fn invalid_variadic_forwarded_call_args(
    signature: &CallSignature,
    variadic_start: usize,
    variadic_elem: Option<&GoType>,
    return_types: &[GoType],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if return_types.len() < variadic_start {
        return Some(invalid_call_reason(
            &signature.target,
            format!(
                "expects at least {} argument(s), got {} return value(s)",
                variadic_start,
                return_types.len()
            ),
        ));
    }
    for (idx, (expected, actual)) in signature
        .params
        .iter()
        .take(variadic_start)
        .zip(return_types.iter())
        .enumerate()
    {
        if let Some(reason) =
            invalid_call_arg_type(&signature.target, idx + 1, expected, actual, env)
        {
            return Some(reason);
        }
    }
    if let Some(elem) = variadic_elem {
        for (idx, actual) in return_types.iter().skip(variadic_start).enumerate() {
            if let Some(reason) = invalid_call_arg_type(
                &signature.target,
                variadic_start + idx + 1,
                elem,
                actual,
                env,
            ) {
                return Some(reason);
            }
        }
    }
    None
}

fn variadic_elem_type(param: &GoType, env: &TypeEnv) -> Option<GoType> {
    match env.resolve_alias(param) {
        GoType::Slice(elem) => Some(*elem),
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(other),
    }
}

fn single_call_result_types(args: &[ast::Expr<'_>], env: &TypeEnv) -> Option<Vec<GoType>> {
    let [arg] = args else {
        return None;
    };
    let ast::Expr::CallExpr(call) = unparen_expr(arg) else {
        return None;
    };
    call_result_types(call, env)
}

fn invalid_call_arg_exprs<'a>(
    target: &str,
    params: impl Iterator<Item = &'a GoType>,
    args: impl Iterator<Item = &'a ast::Expr<'a>>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    for (idx, (expected, arg)) in params.zip(args).enumerate() {
        if let Some(reason) = invalid_call_arg_expr(target, idx + 1, expected, arg, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_call_arg_expr(
    target: &str,
    position: usize,
    expected: &GoType,
    arg: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(values) = expression_value_count(arg, env, TupleAssignmentMode::SingleValueContext)
        .filter(|values| *values != 1)
    {
        return Some(invalid_call_reason(
            target,
            format!("argument {position} must be single-valued, got {values} value(s)"),
        ));
    }
    let expected = env.resolve_alias(expected);
    if expr_is_nil(arg) && !type_can_compare_to_nil(&expected, env) {
        return Some(invalid_call_reason(
            target,
            format!(
                "argument {position} must be assignable to {}, got nil",
                go_type_display_name(&expected)
            ),
        ));
    }
    let actual = env.resolve_alias(&GoType::infer_expr(arg, env));
    if expr_is_assignable_for_validation(&expected, arg, env) {
        return None;
    }
    Some(invalid_call_reason(
        target,
        format!(
            "argument {position} must be assignable to {}, got {}",
            go_type_display_name(&expected),
            go_type_display_name(&actual)
        ),
    ))
}

fn invalid_call_arg_types<'a>(
    target: &str,
    params: impl Iterator<Item = &'a GoType>,
    actual_types: &'a [GoType],
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    for (idx, (expected, actual)) in params.zip(actual_types.iter()).enumerate() {
        if let Some(reason) = invalid_call_arg_type(target, idx + 1, expected, actual, env) {
            return Some(reason);
        }
    }
    None
}

fn invalid_call_arg_type(
    target: &str,
    position: usize,
    expected: &GoType,
    actual: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let expected = env.resolve_alias(expected);
    let actual = env.resolve_alias(actual);
    if types_are_assignable_for_validation(&expected, &actual) {
        return None;
    }
    Some(invalid_call_reason(
        target,
        format!(
            "argument {position} must be assignable to {}, got {}",
            go_type_display_name(&expected),
            go_type_display_name(&actual)
        ),
    ))
}

fn invalid_call_reason(target: &str, reason: impl Into<String>) -> InvalidStatementReason {
    InvalidStatementReason::InvalidCall {
        target: target.to_string(),
        reason: reason.into(),
    }
}

fn invalid_builtin_append_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.is_empty() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Append,
            "expects at least one argument",
        ));
    }
    let (dst_arg, values) = args.split_first()?;
    let dst = env.resolve_alias(&GoType::infer_expr(dst_arg, env));
    let dst_elem = match dst {
        GoType::Slice(elem) => *elem,
        GoType::Unknown | GoType::Named(_) => return None,
        other => {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Append,
                format!(
                    "first argument must have slice type, got {}",
                    go_type_display_name(&other)
                ),
            ));
        }
    };

    if call.ellipsis.is_some() {
        return invalid_builtin_append_spread_call(args, &dst_elem, env);
    }

    let expected = env.resolve_alias(&dst_elem);
    for value in values {
        if expr_is_nil(value) && !type_can_compare_to_nil(&expected, env) {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Append,
                format!(
                    "argument must be assignable to {}, got nil",
                    go_type_display_name(&expected)
                ),
            ));
        }
        let actual = env.resolve_alias(&GoType::infer_expr(value, env));
        if !expr_is_assignable_for_validation(&expected, value, env) {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Append,
                format!(
                    "argument must be assignable to {}, got {}",
                    go_type_display_name(&expected),
                    go_type_display_name(&actual)
                ),
            ));
        }
    }
    None
}

fn invalid_builtin_append_spread_call(
    args: &[ast::Expr<'_>],
    dst_elem: &GoType,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let [_, src_arg] = args else {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Append,
            "spread form expects exactly two arguments",
        ));
    };
    let src = env.resolve_alias(&GoType::infer_expr(src_arg, env));
    if matches!((dst_elem, &src), (GoType::Uint8, GoType::String)) {
        return None;
    }
    let src_elem = match src {
        GoType::Slice(elem) => *elem,
        GoType::Unknown | GoType::Named(_) => return None,
        other => {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Append,
                format!(
                    "spread argument must have slice type or string for []byte, got {}",
                    go_type_display_name(&other)
                ),
            ));
        }
    };
    let expected = env.resolve_alias(dst_elem);
    let actual = env.resolve_alias(&src_elem);
    if types_are_identical_for_validation(&expected, &actual) {
        return None;
    }
    Some(invalid_builtin_call_reason(
        BuiltinCallKind::Append,
        format!(
            "spread argument element type must match {}, got {}",
            go_type_display_name(&expected),
            go_type_display_name(&actual)
        ),
    ))
}

fn invalid_builtin_len_cap_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let kind = unshadowed_builtin_call_kind(call, env)?;
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            kind,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            kind,
            "expects exactly one argument",
        ));
    }
    let [arg] = args else {
        return None;
    };
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    let valid = match kind {
        BuiltinCallKind::Len => {
            matches!(
                &ty,
                GoType::String
                    | GoType::Array(_)
                    | GoType::Slice(_)
                    | GoType::Map(_, _)
                    | GoType::Chan { .. }
                    | GoType::Unknown
                    | GoType::Named(_)
            ) || matches!(&ty, GoType::Pointer(inner) if matches!(&**inner, GoType::Array(_)))
        }
        BuiltinCallKind::Cap => {
            matches!(
                &ty,
                GoType::Array(_)
                    | GoType::Slice(_)
                    | GoType::Chan { .. }
                    | GoType::Unknown
                    | GoType::Named(_)
            ) || matches!(&ty, GoType::Pointer(inner) if matches!(&**inner, GoType::Array(_)))
        }
        _ => true,
    };
    if valid {
        return None;
    }
    let expected = match kind {
        BuiltinCallKind::Len => "argument must have string, array, slice, map, or channel type",
        BuiltinCallKind::Cap => "argument must have array, slice, or channel type",
        _ => "invalid argument type",
    };
    Some(invalid_builtin_call_reason(
        kind,
        format!("{expected}, got {}", go_type_display_name(&ty)),
    ))
}

fn invalid_builtin_complex_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Complex,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 2 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Complex,
            "expects exactly two arguments",
        ));
    }
    let [left_arg, right_arg] = args else {
        return None;
    };
    let left = complex_arg_float_type(left_arg, env);
    let right = complex_arg_float_type(right_arg, env);
    if let Err(ty) = left {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Complex,
            format!(
                "arguments must have floating-point type, got {}",
                go_type_display_name(&ty)
            ),
        ));
    }
    if let Err(ty) = right {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Complex,
            format!(
                "arguments must have floating-point type, got {}",
                go_type_display_name(&ty)
            ),
        ));
    }
    match (left.ok().flatten(), right.ok().flatten()) {
        (Some(left), Some(right)) if left != right => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Complex,
            format!(
                "arguments must have the same floating-point type, got {} and {}",
                go_type_display_name(&left),
                go_type_display_name(&right)
            ),
        )),
        _ => None,
    }
}

fn complex_arg_float_type(arg: &ast::Expr<'_>, env: &TypeEnv) -> Result<Option<GoType>, GoType> {
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    match unparen_expr(arg) {
        ast::Expr::BasicLit(lit)
            if matches!(
                lit.kind,
                token::Token::INT | token::Token::FLOAT | token::Token::CHAR
            ) =>
        {
            Ok(None)
        }
        ast::Expr::Ident(ident) if env.is_const(ident.name) && go_type_is_numeric(&ty) => Ok(None),
        _ => match ty {
            GoType::Float32 | GoType::Float64 => Ok(Some(ty)),
            GoType::Unknown | GoType::Named(_) => Ok(None),
            other => Err(other),
        },
    }
}

fn invalid_builtin_real_imag_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let kind = unshadowed_builtin_call_kind(call, env)?;
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            kind,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            kind,
            "expects exactly one argument",
        ));
    }
    let [arg] = args else {
        return None;
    };
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    if matches!(
        ty,
        GoType::Complex64 | GoType::Complex128 | GoType::Unknown | GoType::Named(_)
    ) {
        return None;
    }
    Some(invalid_builtin_call_reason(
        kind,
        format!(
            "argument must have complex type, got {}",
            go_type_display_name(&ty)
        ),
    ))
}

fn invalid_builtin_min_max_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    let kind = unshadowed_builtin_call_kind(call, env)?;
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            kind,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.is_empty() {
        return Some(invalid_builtin_call_reason(
            kind,
            "expects at least one argument",
        ));
    }

    let mut saw_string = false;
    let mut saw_numeric = false;
    let mut checked_args = Vec::new();
    for arg in args {
        let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
        let arg_kind = min_max_arg_kind(arg, &ty, env);
        match arg_kind {
            MinMaxArgKind::Numeric => saw_numeric = true,
            MinMaxArgKind::String => saw_string = true,
            MinMaxArgKind::Unknown => {}
            MinMaxArgKind::Invalid => {
                return Some(invalid_builtin_call_reason(
                    kind,
                    format!(
                        "arguments must have ordered type, got {}",
                        go_type_display_name(&ty)
                    ),
                ));
            }
        }
        checked_args.push((arg, ty, arg_kind));
    }
    if saw_string && saw_numeric {
        return Some(invalid_builtin_call_reason(
            kind,
            "arguments must be all numeric or all string",
        ));
    }
    for (index, (left_expr, left, left_kind)) in checked_args.iter().enumerate() {
        if matches!(left_kind, MinMaxArgKind::Unknown) {
            continue;
        }
        for (right_expr, right, right_kind) in checked_args.iter().skip(index + 1) {
            if matches!(right_kind, MinMaxArgKind::Unknown) {
                continue;
            }
            if comparison_exprs_are_assignable(left_expr, left, right_expr, right, env) {
                continue;
            }
            return Some(invalid_builtin_call_reason(
                kind,
                format!(
                    "arguments have mismatched types {} and {}",
                    go_type_display_name(left),
                    go_type_display_name(right)
                ),
            ));
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MinMaxArgKind {
    Numeric,
    String,
    Unknown,
    Invalid,
}

fn min_max_arg_kind(arg: &ast::Expr<'_>, ty: &GoType, env: &TypeEnv) -> MinMaxArgKind {
    match unparen_expr(arg) {
        ast::Expr::BasicLit(lit)
            if matches!(
                lit.kind,
                token::Token::INT | token::Token::FLOAT | token::Token::CHAR
            ) =>
        {
            MinMaxArgKind::Numeric
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::STRING => MinMaxArgKind::String,
        ast::Expr::Ident(ident) if env.is_const(ident.name) && go_type_is_ordered_numeric(ty) => {
            MinMaxArgKind::Numeric
        }
        _ if ty.is_numeric() => MinMaxArgKind::Numeric,
        _ if matches!(ty, GoType::String) => MinMaxArgKind::String,
        _ if matches!(ty, GoType::Unknown | GoType::Named(_)) => MinMaxArgKind::Unknown,
        _ => MinMaxArgKind::Invalid,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MakeTypeKind {
    Slice,
    Map,
    Channel,
}

enum MakeTypeArg {
    Type(MakeTypeKind),
    NonMakeType(GoType),
    NonType,
    Unknown,
}

fn invalid_builtin_make_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.is_empty() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "expects a type argument",
        ));
    }

    let (type_arg, size_args) = args.split_first()?;
    let kind = match make_type_arg(type_arg, env) {
        MakeTypeArg::Type(kind) => kind,
        MakeTypeArg::NonMakeType(ty) => {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Make,
                format!(
                    "first argument must have slice, map, or channel type, got {}",
                    go_type_display_name(&ty)
                ),
            ));
        }
        MakeTypeArg::NonType => {
            return Some(invalid_builtin_call_reason(
                BuiltinCallKind::Make,
                "first argument must be a type",
            ));
        }
        MakeTypeArg::Unknown => return None,
    };

    if let Some(reason) = invalid_make_arg_count(kind, args.len()) {
        return Some(reason);
    }
    if let Some(reason) = invalid_make_size_order(kind, size_args) {
        return Some(reason);
    }
    size_args
        .iter()
        .find_map(|arg| invalid_make_size_arg(arg, env))
}

fn invalid_make_arg_count(kind: MakeTypeKind, count: usize) -> Option<InvalidStatementReason> {
    match kind {
        MakeTypeKind::Slice if !(2..=3).contains(&count) => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "slice make expects length and optional capacity",
        )),
        MakeTypeKind::Map if !(1..=2).contains(&count) => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "map make expects optional size hint",
        )),
        MakeTypeKind::Channel if !(1..=2).contains(&count) => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "channel make expects optional buffer size",
        )),
        _ => None,
    }
}

fn invalid_make_size_order(
    kind: MakeTypeKind,
    size_args: &[ast::Expr<'_>],
) -> Option<InvalidStatementReason> {
    if kind != MakeTypeKind::Slice || size_args.len() != 2 {
        return None;
    }
    let len = integer_constant_index_value(size_args.first()?);
    let cap = integer_constant_index_value(size_args.get(1)?);
    match (len, cap) {
        (Some(len), Some(cap)) if len > cap => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "length must not exceed capacity",
        )),
        _ => None,
    }
}

fn invalid_make_size_arg(arg: &ast::Expr<'_>, env: &TypeEnv) -> Option<InvalidStatementReason> {
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    if expr_is_negative_integer_constant(arg) {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Make,
            "size argument must be non-negative",
        ));
    }
    if make_size_arg_is_valid(arg, &ty, env) {
        return None;
    }
    Some(invalid_builtin_call_reason(
        BuiltinCallKind::Make,
        format!(
            "size argument must have integer type, got {}",
            go_type_display_name(&ty)
        ),
    ))
}

fn make_size_arg_is_valid(arg: &ast::Expr<'_>, ty: &GoType, env: &TypeEnv) -> bool {
    match unparen_expr(arg) {
        ast::Expr::BasicLit(lit) => match lit.kind {
            token::Token::INT | token::Token::CHAR => true,
            token::Token::FLOAT => expr_is_integer_constant(arg),
            _ => false,
        },
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::ADD => {
            make_size_arg_is_valid(&unary.x, ty, env)
        }
        ast::Expr::Ident(ident) if env.is_const(ident.name) && go_type_is_numeric(ty) => true,
        _ => ty.is_integer() || matches!(ty, GoType::Unknown | GoType::Named(_)),
    }
}

fn make_type_arg(expr: &ast::Expr<'_>, env: &TypeEnv) -> MakeTypeArg {
    match unparen_expr(expr) {
        ast::Expr::ArrayType(array) if array.len.is_none() => {
            MakeTypeArg::Type(MakeTypeKind::Slice)
        }
        ast::Expr::ArrayType(_) => {
            MakeTypeArg::NonMakeType(GoType::Array(Box::new(GoType::Unknown)))
        }
        ast::Expr::MapType(_) => MakeTypeArg::Type(MakeTypeKind::Map),
        ast::Expr::ChanType(_) => MakeTypeArg::Type(MakeTypeKind::Channel),
        ast::Expr::Ident(ident) => make_ident_type_arg(ident, env),
        ast::Expr::SelectorExpr(selector) => make_selector_type_arg(selector, env),
        ast::Expr::ParenExpr(paren) => make_type_arg(&paren.x, env),
        ast::Expr::StarExpr(_) => {
            MakeTypeArg::NonMakeType(GoType::Pointer(Box::new(GoType::Unknown)))
        }
        ast::Expr::FuncType(_) => MakeTypeArg::NonMakeType(GoType::Func {
            params: Vec::new(),
            results: Vec::new(),
        }),
        ast::Expr::InterfaceType(_) => MakeTypeArg::NonMakeType(GoType::Any),
        ast::Expr::StructType(_) => MakeTypeArg::NonMakeType(GoType::Named("struct".to_string())),
        _ => MakeTypeArg::NonType,
    }
}

fn make_ident_type_arg(ident: &ast::Ident<'_>, env: &TypeEnv) -> MakeTypeArg {
    if matches!(ident.name, "nil" | "true" | "false")
        || env.get_var(ident.name).is_some()
        || env.has_func(ident.name)
    {
        return MakeTypeArg::NonType;
    }
    if predeclared_type_name(ident.name) || env.get_type_kind(ident.name).is_some() {
        return make_type_arg_from_type(GoType::Named(ident.name.to_string()), env);
    }
    MakeTypeArg::Unknown
}

fn make_selector_type_arg(selector: &ast::SelectorExpr<'_>, env: &TypeEnv) -> MakeTypeArg {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return MakeTypeArg::Unknown;
    };
    let name = format!("{}.{}", base.name, selector.sel.name);
    if env.get_type_kind(&name).is_some() {
        make_type_arg_from_type(GoType::Named(name), env)
    } else {
        MakeTypeArg::Unknown
    }
}

fn make_type_arg_from_type(ty: GoType, env: &TypeEnv) -> MakeTypeArg {
    match env.resolve_alias(&ty) {
        GoType::Slice(_) => MakeTypeArg::Type(MakeTypeKind::Slice),
        GoType::Map(_, _) => MakeTypeArg::Type(MakeTypeKind::Map),
        GoType::Chan { .. } => MakeTypeArg::Type(MakeTypeKind::Channel),
        other => MakeTypeArg::NonMakeType(other),
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

fn invalid_builtin_new_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::New,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::New,
            "expects exactly one argument",
        ));
    }
    let [arg] = args else {
        return None;
    };
    if matches!(unparen_expr(arg), ast::Expr::Ident(ident) if ident.name == "nil") {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::New,
            "argument must not be nil",
        ));
    }
    if matches!(new_arg_kind(arg, env), NewArgKind::Value) {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::New,
            "argument must be a type",
        ));
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NewArgKind {
    Type,
    Value,
    Unknown,
}

fn new_arg_kind(expr: &ast::Expr<'_>, env: &TypeEnv) -> NewArgKind {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) => {
            if env.get_var(ident.name).is_some()
                || env.has_func(ident.name)
                || env.is_const(ident.name)
                || matches!(ident.name, "true" | "false" | "nil")
            {
                NewArgKind::Value
            } else if is_predeclared_type_name(ident.name)
                || env.get_type_kind(ident.name).is_some()
            {
                NewArgKind::Type
            } else {
                NewArgKind::Unknown
            }
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(pkg) = selector.x.as_ref() {
                if pkg.name == "unsafe" && selector.sel.name == "Pointer" {
                    return NewArgKind::Type;
                }
                let key = format!("{}.{}", pkg.name, selector.sel.name);
                if env.get_type_kind(&key).is_some() && !env.has_func(&key) {
                    return NewArgKind::Type;
                }
                if env.get_var(&key).is_some() || env.has_func(&key) || env.is_const(&key) {
                    return NewArgKind::Value;
                }
            }
            NewArgKind::Unknown
        }
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => NewArgKind::Type,
        ast::Expr::StarExpr(star) => new_arg_kind(&star.x, env),
        ast::Expr::IndexExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name))
            .map_or(NewArgKind::Unknown, |_| NewArgKind::Type),
        ast::Expr::IndexListExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name))
            .map_or(NewArgKind::Unknown, |_| NewArgKind::Type),
        ast::Expr::BasicLit(_)
        | ast::Expr::BinaryExpr(_)
        | ast::Expr::CallExpr(_)
        | ast::Expr::CompositeLit(_)
        | ast::Expr::Ellipsis(_)
        | ast::Expr::FuncLit(_)
        | ast::Expr::KeyValueExpr(_)
        | ast::Expr::SliceExpr(_)
        | ast::Expr::TypeAssertExpr(_)
        | ast::Expr::UnaryExpr(_) => NewArgKind::Value,
        ast::Expr::ParenExpr(_) => NewArgKind::Unknown,
    }
}

fn invalid_builtin_panic_call(call: &ast::CallExpr<'_>) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Panic,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Panic,
            "expects exactly one argument",
        ));
    }
    None
}

fn invalid_builtin_recover_call(call: &ast::CallExpr<'_>) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Recover,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if !args.is_empty() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Recover,
            "expects no arguments",
        ));
    }
    None
}

fn invalid_builtin_print_call(
    call: &ast::CallExpr<'_>,
    kind: BuiltinCallKind,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            kind,
            "does not accept spread arguments",
        ));
    }
    None
}

fn invalid_builtin_clear_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Clear,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Clear,
            "expects exactly one argument",
        ));
    }
    let [arg] = args else {
        return None;
    };
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    match ty {
        GoType::Map(_, _) | GoType::Slice(_) => None,
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Clear,
            format!(
                "argument must have map or slice type, got {}",
                go_type_display_name(&other)
            ),
        )),
    }
}

fn invalid_builtin_close_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Close,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 1 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Close,
            "expects exactly one argument",
        ));
    }
    let [arg] = args else {
        return None;
    };
    let ty = env.resolve_alias(&GoType::infer_expr(arg, env));
    match ty {
        GoType::Chan { direction, .. } if direction.can_send() => None,
        GoType::Chan { .. } => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Close,
            "cannot close receive-only channel",
        )),
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Close,
            format!(
                "argument must have channel type, got {}",
                go_type_display_name(&other)
            ),
        )),
    }
}

fn invalid_builtin_copy_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Copy,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 2 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Copy,
            "expects exactly two arguments",
        ));
    }
    let [dst_arg, src_arg] = args else {
        return None;
    };
    let dst = env.resolve_alias(&GoType::infer_expr(dst_arg, env));
    let dst_elem = match dst {
        GoType::Slice(dst_elem) => dst_elem,
        other => {
            return match other {
                GoType::Unknown | GoType::Named(_) => None,
                other => Some(invalid_builtin_call_reason(
                    BuiltinCallKind::Copy,
                    format!(
                        "first argument must have slice type, got {}",
                        go_type_display_name(&other)
                    ),
                )),
            };
        }
    };
    let src = env.resolve_alias(&GoType::infer_expr(src_arg, env));
    if matches!((&*dst_elem, &src), (GoType::Uint8, GoType::String)) {
        return None;
    }
    let src_elem = match src {
        GoType::Slice(src_elem) => src_elem,
        other => {
            return match other {
                GoType::Unknown | GoType::Named(_) => None,
                other => Some(invalid_builtin_call_reason(
                    BuiltinCallKind::Copy,
                    format!(
                        "second argument must have slice type, got {}",
                        go_type_display_name(&other)
                    ),
                )),
            };
        }
    };
    let expected = env.resolve_alias(&dst_elem);
    let actual = env.resolve_alias(&src_elem);
    if types_are_identical_for_validation(&expected, &actual) {
        return None;
    }
    Some(invalid_builtin_call_reason(
        BuiltinCallKind::Copy,
        format!(
            "source element type must match {}, got {}",
            go_type_display_name(&expected),
            go_type_display_name(&actual)
        ),
    ))
}

fn invalid_builtin_delete_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if call.ellipsis.is_some() {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Delete,
            "does not accept spread arguments",
        ));
    }
    let args = call.args.as_deref().unwrap_or(&[]);
    if args.len() != 2 {
        return Some(invalid_builtin_call_reason(
            BuiltinCallKind::Delete,
            "expects exactly two arguments",
        ));
    }
    let [map_arg, key_arg] = args else {
        return None;
    };
    let ty = env.resolve_alias(&GoType::infer_expr(map_arg, env));
    match ty {
        GoType::Map(key, _) => {
            let expected = env.resolve_alias(&key);
            if expr_is_nil(key_arg) && !type_can_compare_to_nil(&expected, env) {
                return Some(invalid_builtin_call_reason(
                    BuiltinCallKind::Delete,
                    format!(
                        "key must be assignable to {}, got nil",
                        go_type_display_name(&expected)
                    ),
                ));
            }
            let actual = env.resolve_alias(&GoType::infer_expr(key_arg, env));
            if expr_is_assignable_for_validation(&expected, key_arg, env) {
                return None;
            }
            Some(invalid_builtin_call_reason(
                BuiltinCallKind::Delete,
                format!(
                    "key must be assignable to {}, got {}",
                    go_type_display_name(&expected),
                    go_type_display_name(&actual)
                ),
            ))
        }
        GoType::Unknown | GoType::Named(_) => None,
        other => Some(invalid_builtin_call_reason(
            BuiltinCallKind::Delete,
            format!(
                "first argument must have map type, got {}",
                go_type_display_name(&other)
            ),
        )),
    }
}

fn types_are_identical_for_validation(expected: &GoType, actual: &GoType) -> bool {
    matches!(expected, GoType::Unknown | GoType::Named(_))
        || matches!(actual, GoType::Unknown | GoType::Named(_))
        || expected == actual
}

fn invalid_builtin_call_reason(
    kind: BuiltinCallKind,
    reason: impl Into<String>,
) -> InvalidStatementReason {
    InvalidStatementReason::InvalidBuiltinCall {
        name: kind.name().to_string(),
        reason: reason.into(),
    }
}

fn disallowed_builtin_statement_name(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Option<String> {
    if let Some(kind) = unshadowed_builtin_call_kind(call, env)
        && builtin_disallowed_in_statement(kind)
    {
        return Some(kind.name().to_string());
    }
    unsafe_disallowed_builtin_statement_name(call)
}

fn unshadowed_builtin_call_kind(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<BuiltinCallKind> {
    let ast::Expr::Ident(ident) = call.fun.as_ref() else {
        return None;
    };
    if env.get_var(ident.name).is_some()
        || env.has_func(ident.name)
        || env.get_type_kind(ident.name).is_some()
    {
        return None;
    }
    builtin_call_kind(call)
}

fn builtin_disallowed_in_statement(kind: BuiltinCallKind) -> bool {
    matches!(
        kind,
        BuiltinCallKind::Append
            | BuiltinCallKind::Cap
            | BuiltinCallKind::Complex
            | BuiltinCallKind::Imag
            | BuiltinCallKind::Len
            | BuiltinCallKind::Make
            | BuiltinCallKind::New
            | BuiltinCallKind::Real
    )
}

fn unsafe_disallowed_builtin_statement_name(call: &ast::CallExpr<'_>) -> Option<String> {
    let ast::Expr::SelectorExpr(selector) = call.fun.as_ref() else {
        return None;
    };
    let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
        return None;
    };
    if pkg.name != "unsafe" {
        return None;
    }
    matches!(
        selector.sel.name,
        "Add" | "Alignof" | "Offsetof" | "Sizeof" | "Slice" | "SliceData" | "String" | "StringData"
    )
    .then(|| format!("unsafe.{}", selector.sel.name))
}

fn call_is_type_conversion(call: &ast::CallExpr<'_>, env: &TypeEnv) -> bool {
    let fun = unparen_expr(&call.fun);
    match fun {
        ast::Expr::Ident(ident) => {
            env.get_var(ident.name).is_none()
                && !env.has_func(ident.name)
                && (is_predeclared_type_name(ident.name) || env.get_type_kind(ident.name).is_some())
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(pkg) = selector.x.as_ref() {
                let name = format!("{}.{}", pkg.name, selector.sel.name);
                return env.get_type_kind(&name).is_some() && !env.has_func(&name);
            }
            false
        }
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StarExpr(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::IndexExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name).cloned())
            .is_some(),
        ast::Expr::IndexListExpr(index) => type_name(&index.x)
            .and_then(|name| env.get_type_kind(&name).cloned())
            .is_some(),
        _ => false,
    }
}

fn unparen_expr<'a>(expr: &'a ast::Expr<'a>) -> &'a ast::Expr<'a> {
    match expr {
        ast::Expr::ParenExpr(paren) => unparen_expr(&paren.x),
        _ => expr,
    }
}

fn collect_labels_in_block(block: &ast::BlockStmt<'_>, labels: &mut Vec<String>) {
    for stmt in &block.list {
        collect_labels_in_stmt(stmt, labels);
    }
}

fn collect_labels_in_stmt(stmt: &ast::Stmt<'_>, labels: &mut Vec<String>) {
    match stmt {
        ast::Stmt::BlockStmt(block) => collect_labels_in_block(block, labels),
        ast::Stmt::CaseClause(case) => {
            for stmt in &case.body {
                collect_labels_in_stmt(stmt, labels);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm {
                collect_labels_in_stmt(comm, labels);
            }
            for stmt in &comm.body {
                collect_labels_in_stmt(stmt, labels);
            }
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_labels_in_stmt(init, labels);
            }
            if let Some(post) = &for_stmt.post {
                collect_labels_in_stmt(post, labels);
            }
            collect_labels_in_block(&for_stmt.body, labels);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_labels_in_stmt(init, labels);
            }
            collect_labels_in_block(&if_stmt.body, labels);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_labels_in_stmt(else_branch, labels);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => {
            if let Some(label) = non_blank_label_name(labeled.label.name) {
                labels.push(label);
            }
            collect_labels_in_stmt(&labeled.stmt, labels);
        }
        ast::Stmt::RangeStmt(range) => collect_labels_in_block(&range.body, labels),
        ast::Stmt::SelectStmt(select) => collect_labels_in_block(&select.body, labels),
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_labels_in_stmt(init, labels);
            }
            collect_labels_in_block(&switch.body, labels);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_labels_in_stmt(init, labels);
            }
            collect_labels_in_block(&type_switch.body, labels);
        }
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => {}
    }
}

fn collect_label_uses_in_block(block: &ast::BlockStmt<'_>, labels: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_label_uses_in_stmt(stmt, labels);
    }
}

fn collect_label_uses_in_stmt(stmt: &ast::Stmt<'_>, labels: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::BranchStmt(branch)
            if matches!(
                branch.tok,
                token::Token::BREAK | token::Token::CONTINUE | token::Token::GOTO
            ) =>
        {
            if let Some(label) = &branch.label {
                labels.insert(label.name.to_string());
            }
        }
        ast::Stmt::BlockStmt(block) => collect_label_uses_in_block(block, labels),
        ast::Stmt::CaseClause(case) => {
            for stmt in &case.body {
                collect_label_uses_in_stmt(stmt, labels);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(comm) = &comm.comm {
                collect_label_uses_in_stmt(comm, labels);
            }
            for stmt in &comm.body {
                collect_label_uses_in_stmt(stmt, labels);
            }
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_label_uses_in_stmt(init, labels);
            }
            if let Some(post) = &for_stmt.post {
                collect_label_uses_in_stmt(post, labels);
            }
            collect_label_uses_in_block(&for_stmt.body, labels);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_label_uses_in_stmt(init, labels);
            }
            collect_label_uses_in_block(&if_stmt.body, labels);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_label_uses_in_stmt(else_branch, labels);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => collect_label_uses_in_stmt(&labeled.stmt, labels),
        ast::Stmt::RangeStmt(range) => collect_label_uses_in_block(&range.body, labels),
        ast::Stmt::SelectStmt(select) => collect_label_uses_in_block(&select.body, labels),
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_label_uses_in_stmt(init, labels);
            }
            collect_label_uses_in_block(&switch.body, labels);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_label_uses_in_stmt(init, labels);
            }
            collect_label_uses_in_block(&type_switch.body, labels);
        }
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => {}
    }
}

fn child_path(path: &[usize], idx: usize, child: usize) -> Vec<usize> {
    let mut next = path.to_vec();
    next.push(idx);
    next.push(child);
    next
}

fn collect_label_paths_in_block(
    block: &ast::BlockStmt<'_>,
    path: &[usize],
    labels: &mut BTreeMap<String, Vec<usize>>,
) {
    for (idx, stmt) in block.list.iter().enumerate() {
        collect_label_paths_in_stmt(stmt, path, idx, labels);
    }
}

fn collect_label_paths_in_stmt(
    stmt: &ast::Stmt<'_>,
    path: &[usize],
    idx: usize,
    labels: &mut BTreeMap<String, Vec<usize>>,
) {
    match stmt {
        ast::Stmt::BlockStmt(block) => {
            collect_label_paths_in_block(block, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::CaseClause(case) => {
            collect_label_paths_in_stmt_list(&case.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::CommClause(comm) => {
            collect_label_paths_in_stmt_list(&comm.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::ForStmt(for_stmt) => {
            collect_label_paths_in_block(&for_stmt.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            collect_label_paths_in_block(&if_stmt.body, &child_path(path, idx, 0), labels);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_label_paths_in_stmt(else_branch, &child_path(path, idx, 1), 0, labels);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => {
            if let Some(label) = non_blank_label_name(labeled.label.name) {
                labels.entry(label).or_insert_with(|| path.to_vec());
            }
            collect_label_paths_in_stmt(&labeled.stmt, path, idx, labels);
        }
        ast::Stmt::RangeStmt(range) => {
            collect_label_paths_in_block(&range.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::SelectStmt(select) => {
            collect_label_paths_in_block(&select.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::SwitchStmt(switch) => {
            collect_label_paths_in_block(&switch.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            collect_label_paths_in_block(&type_switch.body, &child_path(path, idx, 0), labels);
        }
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => {}
    }
}

fn collect_label_paths_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    path: &[usize],
    labels: &mut BTreeMap<String, Vec<usize>>,
) {
    for (idx, stmt) in stmts.iter().enumerate() {
        collect_label_paths_in_stmt(stmt, path, idx, labels);
    }
}

fn collect_goto_paths_in_block(
    block: &ast::BlockStmt<'_>,
    path: &[usize],
    gotos: &mut Vec<(String, Vec<usize>)>,
) {
    for (idx, stmt) in block.list.iter().enumerate() {
        collect_goto_paths_in_stmt(stmt, path, idx, gotos);
    }
}

fn collect_goto_paths_in_stmt(
    stmt: &ast::Stmt<'_>,
    path: &[usize],
    idx: usize,
    gotos: &mut Vec<(String, Vec<usize>)>,
) {
    match stmt {
        ast::Stmt::BranchStmt(branch) if branch.tok == token::Token::GOTO => {
            if let Some(label) = &branch.label {
                gotos.push((label.name.to_string(), path.to_vec()));
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_goto_paths_in_block(block, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::CaseClause(case) => {
            collect_goto_paths_in_stmt_list(&case.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::CommClause(comm) => {
            collect_goto_paths_in_stmt_list(&comm.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::ForStmt(for_stmt) => {
            collect_goto_paths_in_block(&for_stmt.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            collect_goto_paths_in_block(&if_stmt.body, &child_path(path, idx, 0), gotos);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_goto_paths_in_stmt(else_branch, &child_path(path, idx, 1), 0, gotos);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => {
            collect_goto_paths_in_stmt(&labeled.stmt, path, idx, gotos);
        }
        ast::Stmt::RangeStmt(range) => {
            collect_goto_paths_in_block(&range.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::SelectStmt(select) => {
            collect_goto_paths_in_block(&select.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::SwitchStmt(switch) => {
            collect_goto_paths_in_block(&switch.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            collect_goto_paths_in_block(&type_switch.body, &child_path(path, idx, 0), gotos);
        }
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => {}
    }
}

fn collect_goto_paths_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    path: &[usize],
    gotos: &mut Vec<(String, Vec<usize>)>,
) {
    for (idx, stmt) in stmts.iter().enumerate() {
        collect_goto_paths_in_stmt(stmt, path, idx, gotos);
    }
}

pub fn goto_state_plan_for_block(block: &ast::BlockStmt<'_>) -> Option<GotoStatePlan> {
    let labels = direct_label_names_in_block(block);
    if labels.is_empty() {
        return None;
    }

    let label_set: BTreeSet<_> = labels.iter().cloned().collect();
    let mut goto_targets = BTreeSet::new();
    collect_goto_targets_in_block(block, &mut goto_targets);
    if label_set.is_disjoint(&goto_targets) {
        return None;
    }

    let hoisted_names = goto_state_hoisted_names(block);
    Some(GotoStatePlan {
        labels,
        hoisted_names,
    })
}

pub fn direct_label_names_in_stmt(stmt: &ast::Stmt<'_>) -> Vec<String> {
    let mut labels = Vec::new();
    let mut current = stmt;
    while let ast::Stmt::LabeledStmt(label) = current {
        if let Some(label) = non_blank_label_name(label.label.name) {
            labels.push(label);
        }
        current = &label.stmt;
    }
    labels
}

fn non_blank_label_name(name: &str) -> Option<String> {
    (name != "_").then(|| name.to_string())
}

fn direct_label_names_in_block(block: &ast::BlockStmt<'_>) -> Vec<String> {
    let mut labels = Vec::new();
    for stmt in &block.list {
        labels.extend(direct_label_names_in_stmt(stmt));
    }
    labels
}

fn collect_goto_targets_in_block(block: &ast::BlockStmt<'_>, targets: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_goto_targets_in_stmt(stmt, targets);
    }
}

fn collect_goto_targets_in_stmt(stmt: &ast::Stmt<'_>, targets: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::BranchStmt(branch) if branch.tok == token::Token::GOTO => {
            if let Some(label) = &branch.label {
                targets.insert(label.name.to_string());
            }
        }
        ast::Stmt::BlockStmt(block) => collect_goto_targets_in_block(block, targets),
        ast::Stmt::CaseClause(case) => {
            for stmt in &case.body {
                collect_goto_targets_in_stmt(stmt, targets);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_goto_targets_in_stmt(stmt, targets);
            }
            for stmt in &comm.body {
                collect_goto_targets_in_stmt(stmt, targets);
            }
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_goto_targets_in_stmt(init, targets);
            }
            if let Some(post) = &for_stmt.post {
                collect_goto_targets_in_stmt(post, targets);
            }
            collect_goto_targets_in_block(&for_stmt.body, targets);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_goto_targets_in_stmt(init, targets);
            }
            collect_goto_targets_in_block(&if_stmt.body, targets);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_goto_targets_in_stmt(else_branch, targets);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => collect_goto_targets_in_stmt(&labeled.stmt, targets),
        ast::Stmt::RangeStmt(range) => collect_goto_targets_in_block(&range.body, targets),
        ast::Stmt::SelectStmt(select) => collect_goto_targets_in_block(&select.body, targets),
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_goto_targets_in_stmt(init, targets);
            }
            collect_goto_targets_in_block(&switch.body, targets);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_goto_targets_in_stmt(init, targets);
            }
            collect_goto_targets_in_stmt(&type_switch.assign, targets);
            collect_goto_targets_in_block(&type_switch.body, targets);
        }
        ast::Stmt::AssignStmt(_)
        | ast::Stmt::BranchStmt(_)
        | ast::Stmt::DeclStmt(_)
        | ast::Stmt::DeferStmt(_)
        | ast::Stmt::EmptyStmt(_)
        | ast::Stmt::ExprStmt(_)
        | ast::Stmt::GoStmt(_)
        | ast::Stmt::IncDecStmt(_)
        | ast::Stmt::ReturnStmt(_)
        | ast::Stmt::SendStmt(_) => {}
    }
}

fn invalid_in_func_lits_in_block<T>(
    block: &ast::BlockStmt<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    invalid_in_func_lits_in_stmt_list(&block.list, check)
}

fn invalid_in_func_lits_in_stmt_list<T>(
    stmts: &[ast::Stmt<'_>],
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    stmts
        .iter()
        .find_map(|stmt| invalid_in_func_lits_in_stmt(stmt, check))
}

fn invalid_in_func_lits_in_stmt<T>(
    stmt: &ast::Stmt<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => assign
            .lhs
            .iter()
            .chain(&assign.rhs)
            .find_map(|expr| invalid_in_func_lits_in_expr(expr, check)),
        ast::Stmt::BlockStmt(block) => invalid_in_func_lits_in_block(block, check),
        ast::Stmt::CaseClause(case) => case
            .list
            .as_ref()
            .and_then(|list| {
                list.iter()
                    .find_map(|expr| invalid_in_func_lits_in_expr(expr, check))
            })
            .or_else(|| invalid_in_func_lits_in_stmt_list(&case.body, check)),
        ast::Stmt::CommClause(comm) => comm
            .comm
            .as_deref()
            .and_then(|stmt| invalid_in_func_lits_in_stmt(stmt, check))
            .or_else(|| invalid_in_func_lits_in_stmt_list(&comm.body, check)),
        ast::Stmt::DeclStmt(decl) => invalid_in_func_lits_in_gen_decl(&decl.decl, check),
        ast::Stmt::DeferStmt(defer) => invalid_in_func_lits_in_call(&defer.call, check),
        ast::Stmt::ExprStmt(expr) => invalid_in_func_lits_in_expr(&expr.x, check),
        ast::Stmt::ForStmt(for_stmt) => for_stmt
            .init
            .as_deref()
            .and_then(|init| invalid_in_func_lits_in_stmt(init, check))
            .or_else(|| {
                for_stmt
                    .cond
                    .as_ref()
                    .and_then(|cond| invalid_in_func_lits_in_expr(cond, check))
            })
            .or_else(|| {
                for_stmt
                    .post
                    .as_deref()
                    .and_then(|post| invalid_in_func_lits_in_stmt(post, check))
            })
            .or_else(|| invalid_in_func_lits_in_block(&for_stmt.body, check)),
        ast::Stmt::GoStmt(go) => invalid_in_func_lits_in_call(&go.call, check),
        ast::Stmt::IfStmt(if_stmt) => if_stmt
            .init
            .as_ref()
            .as_ref()
            .and_then(|init| invalid_in_func_lits_in_stmt(init, check))
            .or_else(|| invalid_in_func_lits_in_expr(&if_stmt.cond, check))
            .or_else(|| invalid_in_func_lits_in_block(&if_stmt.body, check))
            .or_else(|| {
                if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .and_then(|else_branch| invalid_in_func_lits_in_stmt(else_branch, check))
            }),
        ast::Stmt::IncDecStmt(inc_dec) => invalid_in_func_lits_in_expr(&inc_dec.x, check),
        ast::Stmt::LabeledStmt(labeled) => invalid_in_func_lits_in_stmt(&labeled.stmt, check),
        ast::Stmt::RangeStmt(range) => range
            .key
            .as_ref()
            .and_then(|key| invalid_in_func_lits_in_expr(key, check))
            .or_else(|| {
                range
                    .value
                    .as_ref()
                    .and_then(|value| invalid_in_func_lits_in_expr(value, check))
            })
            .or_else(|| invalid_in_func_lits_in_expr(&range.x, check))
            .or_else(|| invalid_in_func_lits_in_block(&range.body, check)),
        ast::Stmt::ReturnStmt(ret) => ret
            .results
            .iter()
            .find_map(|expr| invalid_in_func_lits_in_expr(expr, check)),
        ast::Stmt::SelectStmt(select) => invalid_in_func_lits_in_block(&select.body, check),
        ast::Stmt::SendStmt(send) => invalid_in_func_lits_in_expr(&send.chan, check)
            .or_else(|| invalid_in_func_lits_in_expr(&send.value, check)),
        ast::Stmt::SwitchStmt(switch) => switch
            .init
            .as_deref()
            .and_then(|init| invalid_in_func_lits_in_stmt(init, check))
            .or_else(|| {
                switch
                    .tag
                    .as_ref()
                    .and_then(|tag| invalid_in_func_lits_in_expr(tag, check))
            })
            .or_else(|| invalid_in_func_lits_in_block(&switch.body, check)),
        ast::Stmt::TypeSwitchStmt(type_switch) => type_switch
            .init
            .as_deref()
            .and_then(|init| invalid_in_func_lits_in_stmt(init, check))
            .or_else(|| invalid_in_func_lits_in_stmt(&type_switch.assign, check))
            .or_else(|| invalid_in_func_lits_in_block(&type_switch.body, check)),
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => None,
    }
}

fn invalid_in_func_lits_in_gen_decl<T>(
    decl: &ast::GenDecl<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    decl.specs.iter().find_map(|spec| match spec {
        ast::Spec::ImportSpec(_) => None,
        ast::Spec::TypeSpec(type_spec) => invalid_in_func_lits_in_expr(&type_spec.type_, check)
            .or_else(|| {
                type_spec
                    .type_params
                    .as_ref()
                    .and_then(|fields| invalid_in_func_lits_in_field_list(fields, check))
            }),
        ast::Spec::ValueSpec(value) => value
            .type_
            .as_ref()
            .and_then(|type_| invalid_in_func_lits_in_expr(type_, check))
            .or_else(|| {
                value.values.as_ref().and_then(|values| {
                    values
                        .iter()
                        .find_map(|expr| invalid_in_func_lits_in_expr(expr, check))
                })
            }),
    })
}

fn invalid_in_func_lits_in_field_list<T>(
    fields: &ast::FieldList<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    fields.list.iter().find_map(|field| {
        field
            .type_
            .as_ref()
            .and_then(|type_| invalid_in_func_lits_in_expr(type_, check))
    })
}

fn invalid_in_func_lits_in_call<T>(
    call: &ast::CallExpr<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    invalid_in_func_lits_in_expr(&call.fun, check).or_else(|| {
        call.args.as_ref().and_then(|args| {
            args.iter()
                .find_map(|arg| invalid_in_func_lits_in_expr(arg, check))
        })
    })
}

fn invalid_in_func_lits_in_expr<T>(
    expr: &ast::Expr<'_>,
    check: &mut impl FnMut(&ast::BlockStmt<'_>) -> Option<T>,
) -> Option<T> {
    match expr {
        ast::Expr::ArrayType(array) => array
            .len
            .as_ref()
            .and_then(|len| invalid_in_func_lits_in_expr(len, check))
            .or_else(|| invalid_in_func_lits_in_expr(&array.elt, check)),
        ast::Expr::BinaryExpr(binary) => invalid_in_func_lits_in_expr(&binary.x, check)
            .or_else(|| invalid_in_func_lits_in_expr(&binary.y, check)),
        ast::Expr::CallExpr(call) => invalid_in_func_lits_in_call(call, check),
        ast::Expr::ChanType(chan) => invalid_in_func_lits_in_expr(&chan.value, check),
        ast::Expr::CompositeLit(comp) => comp
            .type_
            .as_ref()
            .and_then(|type_| invalid_in_func_lits_in_expr(type_, check))
            .or_else(|| {
                comp.elts.as_ref().and_then(|elts| {
                    elts.iter()
                        .find_map(|elt| invalid_in_func_lits_in_expr(elt, check))
                })
            }),
        ast::Expr::Ellipsis(ellipsis) => ellipsis
            .elt
            .as_ref()
            .and_then(|elt| invalid_in_func_lits_in_expr(elt, check)),
        ast::Expr::FuncLit(func_lit) => check(&func_lit.body),
        ast::Expr::FuncType(func_type) => func_type
            .type_params
            .as_ref()
            .and_then(|fields| invalid_in_func_lits_in_field_list(fields, check))
            .or_else(|| invalid_in_func_lits_in_field_list(&func_type.params, check))
            .or_else(|| {
                func_type
                    .results
                    .as_ref()
                    .and_then(|results| invalid_in_func_lits_in_field_list(results, check))
            }),
        ast::Expr::IndexExpr(index) => invalid_in_func_lits_in_expr(&index.x, check)
            .or_else(|| invalid_in_func_lits_in_expr(&index.index, check)),
        ast::Expr::IndexListExpr(index) => {
            invalid_in_func_lits_in_expr(&index.x, check).or_else(|| {
                index
                    .indices
                    .iter()
                    .find_map(|index| invalid_in_func_lits_in_expr(index, check))
            })
        }
        ast::Expr::InterfaceType(interface) => interface
            .methods
            .as_ref()
            .and_then(|fields| invalid_in_func_lits_in_field_list(fields, check)),
        ast::Expr::KeyValueExpr(kv) => invalid_in_func_lits_in_expr(&kv.key, check)
            .or_else(|| invalid_in_func_lits_in_expr(&kv.value, check)),
        ast::Expr::MapType(map) => invalid_in_func_lits_in_expr(&map.key, check)
            .or_else(|| invalid_in_func_lits_in_expr(&map.value, check)),
        ast::Expr::ParenExpr(paren) => invalid_in_func_lits_in_expr(&paren.x, check),
        ast::Expr::SelectorExpr(selector) => invalid_in_func_lits_in_expr(&selector.x, check),
        ast::Expr::SliceExpr(slice) => invalid_in_func_lits_in_expr(&slice.x, check)
            .or_else(|| {
                slice
                    .low
                    .as_ref()
                    .and_then(|low| invalid_in_func_lits_in_expr(low, check))
            })
            .or_else(|| {
                slice
                    .high
                    .as_ref()
                    .and_then(|high| invalid_in_func_lits_in_expr(high, check))
            })
            .or_else(|| {
                slice
                    .max
                    .as_ref()
                    .and_then(|max| invalid_in_func_lits_in_expr(max, check))
            }),
        ast::Expr::StarExpr(star) => invalid_in_func_lits_in_expr(&star.x, check),
        ast::Expr::StructType(struct_type) => struct_type
            .fields
            .as_ref()
            .and_then(|fields| invalid_in_func_lits_in_field_list(fields, check)),
        ast::Expr::TypeAssertExpr(assert) => invalid_in_func_lits_in_expr(&assert.x, check)
            .or_else(|| {
                assert
                    .type_
                    .as_ref()
                    .and_then(|type_| invalid_in_func_lits_in_expr(type_, check))
            }),
        ast::Expr::UnaryExpr(unary) => invalid_in_func_lits_in_expr(&unary.x, check),
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => None,
    }
}

fn goto_state_hoisted_names(block: &ast::BlockStmt<'_>) -> Vec<String> {
    let mut declared_by_segment = vec![BTreeSet::new()];
    let mut referenced_by_segment = vec![BTreeSet::new()];
    for stmt in &block.list {
        if !direct_label_names_in_stmt(stmt).is_empty() {
            declared_by_segment.push(BTreeSet::new());
            referenced_by_segment.push(BTreeSet::new());
        }
        if let Some(declared) = declared_by_segment.last_mut() {
            collect_direct_declared_names_in_stmt(stmt, declared);
        }
        if let Some(referenced) = referenced_by_segment.last_mut() {
            collect_referenced_names_in_stmt(stmt, referenced);
        }
    }

    let mut hoisted_names = BTreeSet::new();
    for idx in 0..declared_by_segment.len() {
        let Some(declared) = declared_by_segment.get(idx) else {
            continue;
        };
        if declared.is_empty() {
            continue;
        }
        let mut later_references = BTreeSet::new();
        for referenced in referenced_by_segment.iter().skip(idx + 1) {
            later_references.extend(referenced.iter().cloned());
        }
        if !declared.is_disjoint(&later_references) {
            hoisted_names.extend(declared.intersection(&later_references).cloned());
        }
    }
    hoisted_names.into_iter().collect()
}

fn collect_direct_declared_names_in_stmt(stmt: &ast::Stmt<'_>, names: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::AssignStmt(assign) if assign.tok == token::Token::DEFINE => {
            names.extend(assign.lhs.iter().filter_map(ident_name));
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    names.extend(value.names.iter().map(|name| name.name.to_string()));
                }
            }
        }
        ast::Stmt::LabeledStmt(label) => collect_direct_declared_names_in_stmt(&label.stmt, names),
        _ => {}
    }
}

fn ast_block_has_goto_to_label(block: &ast::BlockStmt<'_>, label: &str) -> bool {
    block
        .list
        .iter()
        .any(|stmt| ast_stmt_has_goto_to_label(stmt, label))
}

fn stmt_completion_with_label(stmt: &Stmt, label: Option<&str>) -> Completion {
    match stmt {
        Stmt::Return(_)
        | Stmt::Branch {
            kind: BranchKind::Goto,
            ..
        } => Completion::Terminates,
        Stmt::Block(block) => block_completion(block),
        Stmt::For { cond, body, .. } => for_completion(cond.as_ref(), body, label),
        Stmt::If {
            body, else_branch, ..
        } => {
            if block_completion(body) != Completion::Terminates {
                return Completion::MayComplete;
            }
            let Some(else_branch) = else_branch.as_deref() else {
                return Completion::MayComplete;
            };
            stmt_completion(else_branch)
        }
        Stmt::Expr(expr) if expr_is_builtin_panic_call(expr) => Completion::Terminates,
        Stmt::Label { name, stmt } => stmt_completion_with_label(stmt, Some(name)),
        Stmt::Select { cases } => select_completion(cases, label),
        Stmt::Switch { cases, .. } | Stmt::TypeSwitch { cases, .. } => {
            switch_completion(cases, label)
        }
        _ => Completion::MayComplete,
    }
}

fn expr_is_builtin_panic_call(expr: &Expr) -> bool {
    let ExprKind::Call(call) = &expr.kind else {
        return false;
    };
    matches!(&call.fun.kind, ExprKind::Ident(name) if name == "panic")
}

fn for_completion(cond: Option<&Expr>, body: &Block, label: Option<&str>) -> Completion {
    if cond.is_some() || block_has_break_referring_to_current(body, label) {
        return Completion::MayComplete;
    }
    Completion::Terminates
}

fn select_completion(cases: &[CommCase], label: Option<&str>) -> Completion {
    if comm_cases_have_break_referring_to_current(cases, label) {
        return Completion::MayComplete;
    }
    if cases.is_empty() {
        return Completion::Terminates;
    }
    for case in cases {
        if stmts_completion(&case.body) != Completion::Terminates {
            return Completion::MayComplete;
        }
    }
    Completion::Terminates
}

fn switch_completion(cases: &[Case], label: Option<&str>) -> Completion {
    if cases.is_empty()
        || !cases.iter().any(|case| case.is_default)
        || cases_have_break_referring_to_current(cases, label)
    {
        return Completion::MayComplete;
    }
    for idx in 0..cases.len() {
        if case_completion(cases, idx) != Completion::Terminates {
            return Completion::MayComplete;
        }
    }
    Completion::Terminates
}

fn case_completion(cases: &[Case], idx: usize) -> Completion {
    let Some(case) = cases.get(idx) else {
        return Completion::MayComplete;
    };
    if case_ends_with_fallthrough(case) {
        return case_completion(cases, idx + 1);
    }
    stmts_completion(&case.body)
}

fn case_ends_with_fallthrough(case: &Case) -> bool {
    case.body.last().is_some_and(stmt_is_fallthrough)
}

fn stmt_is_fallthrough(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Branch {
            kind: BranchKind::Fallthrough,
            ..
        } => true,
        Stmt::Label { stmt, .. } => stmt_is_fallthrough(stmt),
        _ => false,
    }
}

fn block_has_break_referring_to_current(block: &Block, label: Option<&str>) -> bool {
    block
        .stmts
        .iter()
        .any(|stmt| stmt_has_break_referring_to_current(stmt, label))
}

fn stmt_has_break_referring_to_current(stmt: &Stmt, label: Option<&str>) -> bool {
    match stmt {
        Stmt::Branch {
            kind: BranchKind::Break,
            label: break_label,
        } => match (break_label.as_deref(), label) {
            (None, _) => true,
            (Some(break_label), Some(current_label)) => break_label == current_label,
            (Some(_), None) => false,
        },
        Stmt::Block(block) => block_has_break_referring_to_current(block, label),
        Stmt::If {
            init,
            body,
            else_branch,
            ..
        } => {
            init.as_deref()
                .is_some_and(|stmt| stmt_has_break_referring_to_current(stmt, label))
                || block_has_break_referring_to_current(body, label)
                || else_branch
                    .as_deref()
                    .is_some_and(|stmt| stmt_has_break_referring_to_current(stmt, label))
        }
        Stmt::Label { stmt, .. } => stmt_has_break_referring_to_current(stmt, label),
        Stmt::For { body, .. } | Stmt::Range { body, .. } => {
            block_has_labeled_break_referring_to_current(body, label)
        }
        Stmt::Select { cases } => comm_cases_have_labeled_break_referring_to_current(cases, label),
        Stmt::Switch { cases, .. } | Stmt::TypeSwitch { cases, .. } => {
            cases_have_labeled_break_referring_to_current(cases, label)
        }
        _ => false,
    }
}

fn block_has_labeled_break_referring_to_current(block: &Block, label: Option<&str>) -> bool {
    block
        .stmts
        .iter()
        .any(|stmt| stmt_has_labeled_break_referring_to_current(stmt, label))
}

fn stmt_has_labeled_break_referring_to_current(stmt: &Stmt, label: Option<&str>) -> bool {
    let Some(current_label) = label else {
        return false;
    };
    match stmt {
        Stmt::Branch {
            kind: BranchKind::Break,
            label: Some(break_label),
        } => break_label == current_label,
        Stmt::Block(block) => block_has_labeled_break_referring_to_current(block, label),
        Stmt::If {
            init,
            body,
            else_branch,
            ..
        } => {
            init.as_deref()
                .is_some_and(|stmt| stmt_has_labeled_break_referring_to_current(stmt, label))
                || block_has_labeled_break_referring_to_current(body, label)
                || else_branch
                    .as_deref()
                    .is_some_and(|stmt| stmt_has_labeled_break_referring_to_current(stmt, label))
        }
        Stmt::Label { stmt, .. } => stmt_has_labeled_break_referring_to_current(stmt, label),
        Stmt::For { body, .. } | Stmt::Range { body, .. } => {
            block_has_labeled_break_referring_to_current(body, label)
        }
        Stmt::Select { cases } => comm_cases_have_labeled_break_referring_to_current(cases, label),
        Stmt::Switch { cases, .. } | Stmt::TypeSwitch { cases, .. } => {
            cases_have_labeled_break_referring_to_current(cases, label)
        }
        _ => false,
    }
}

fn cases_have_break_referring_to_current(cases: &[Case], label: Option<&str>) -> bool {
    cases.iter().any(|case| {
        case.body
            .iter()
            .any(|stmt| stmt_has_break_referring_to_current(stmt, label))
    })
}

fn cases_have_labeled_break_referring_to_current(cases: &[Case], label: Option<&str>) -> bool {
    cases.iter().any(|case| {
        case.body
            .iter()
            .any(|stmt| stmt_has_labeled_break_referring_to_current(stmt, label))
    })
}

fn comm_cases_have_break_referring_to_current(cases: &[CommCase], label: Option<&str>) -> bool {
    cases.iter().any(|case| {
        case.body
            .iter()
            .any(|stmt| stmt_has_break_referring_to_current(stmt, label))
    })
}

fn comm_cases_have_labeled_break_referring_to_current(
    cases: &[CommCase],
    label: Option<&str>,
) -> bool {
    cases.iter().any(|case| {
        case.body
            .iter()
            .any(|stmt| stmt_has_labeled_break_referring_to_current(stmt, label))
    })
}

fn lower_stmt(stmt: &ast::Stmt<'_>, env: &mut TypeEnv) -> Option<Stmt> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            let lowered = Stmt::Assign(Assign {
                lhs: assign
                    .lhs
                    .iter()
                    .map(|expr| lower_expr(expr, env))
                    .collect(),
                op: lower_assign_op(assign.tok),
                rhs: assign
                    .rhs
                    .iter()
                    .map(|expr| lower_expr(expr, env))
                    .collect(),
            });
            record_define_bindings(assign, env);
            Some(lowered)
        }
        ast::Stmt::BlockStmt(block) => Some(Stmt::Block(lower_block(block, env))),
        ast::Stmt::BranchStmt(branch) => Some(Stmt::Branch {
            kind: lower_branch_kind(branch.tok),
            label: branch.label.as_ref().map(|label| label.name.to_string()),
        }),
        ast::Stmt::CaseClause(case) => Some(Stmt::Case(lower_case(case, env))),
        ast::Stmt::CommClause(comm) => Some(Stmt::Comm(lower_comm_case(comm, env))),
        ast::Stmt::DeclStmt(decl) => {
            let lowered = lower_gen_decl(&decl.decl, env);
            record_decl_bindings(&decl.decl, env);
            Some(Stmt::Decl(lowered))
        }
        ast::Stmt::DeferStmt(defer) => Some(Stmt::Defer(lower_call(&defer.call, env))),
        ast::Stmt::EmptyStmt(_) => Some(Stmt::Empty),
        ast::Stmt::ExprStmt(expr) => Some(Stmt::Expr(lower_expr(&expr.x, env))),
        ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            Some(Stmt::For {
                init: for_stmt
                    .init
                    .as_ref()
                    .and_then(|init| lower_stmt(init, &mut loop_env).map(Box::new)),
                cond: for_stmt
                    .cond
                    .as_ref()
                    .map(|cond| lower_expr(cond, &loop_env)),
                post: for_stmt
                    .post
                    .as_ref()
                    .and_then(|post| lower_stmt(post, &mut loop_env).map(Box::new)),
                body: lower_block_with_env(&for_stmt.body, &mut loop_env),
            })
        }
        ast::Stmt::GoStmt(go) => Some(Stmt::Go(lower_call(&go.call, env))),
        ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            let init = if_stmt
                .init
                .as_ref()
                .as_ref()
                .and_then(|init| lower_stmt(init, &mut if_env).map(Box::new));
            let cond = lower_expr(&if_stmt.cond, &if_env);
            let mut body_env = if_env.clone();
            let body = lower_block_with_env(&if_stmt.body, &mut body_env);
            let mut else_env = if_env;
            let else_branch = if_stmt
                .else_
                .as_ref()
                .as_ref()
                .and_then(|else_branch| lower_stmt(else_branch, &mut else_env).map(Box::new));
            Some(Stmt::If {
                init,
                cond,
                body,
                else_branch,
            })
        }
        ast::Stmt::IncDecStmt(inc_dec) => Some(Stmt::IncDec {
            expr: lower_expr(&inc_dec.x, env),
            op: lower_inc_dec_op(inc_dec.tok),
        }),
        ast::Stmt::LabeledStmt(label) => lower_stmt(&label.stmt, env).map(|stmt| Stmt::Label {
            name: label.label.name.to_string(),
            stmt: Box::new(stmt),
        }),
        ast::Stmt::RangeStmt(range) => {
            let mut range_env = env.clone();
            record_range_bindings(range, &mut range_env);
            Some(Stmt::Range {
                key: range.key.as_ref().map(|key| lower_expr(key, env)),
                value: range.value.as_ref().map(|value| lower_expr(value, env)),
                define: matches!(range.tok, Some(token::Token::DEFINE)),
                expr: lower_expr(&range.x, env),
                body: lower_block_with_env(&range.body, &mut range_env),
            })
        }
        ast::Stmt::ReturnStmt(ret) => Some(Stmt::Return(
            ret.results
                .iter()
                .map(|expr| lower_expr(expr, env))
                .collect(),
        )),
        ast::Stmt::SelectStmt(select) => Some(Stmt::Select {
            cases: select
                .body
                .list
                .iter()
                .filter_map(|stmt| match stmt {
                    ast::Stmt::CommClause(comm) => Some(lower_comm_case(comm, env)),
                    _ => None,
                })
                .collect(),
        }),
        ast::Stmt::SendStmt(send) => Some(Stmt::Send {
            chan: lower_expr(&send.chan, env),
            value: lower_expr(&send.value, env),
        }),
        ast::Stmt::SwitchStmt(switch) => Some(Stmt::Switch {
            init: switch
                .init
                .as_ref()
                .and_then(|init| lower_stmt(init, env).map(Box::new)),
            tag: switch.tag.as_ref().map(|tag| lower_expr(tag, env)),
            cases: switch
                .body
                .list
                .iter()
                .filter_map(|stmt| match stmt {
                    ast::Stmt::CaseClause(case) => Some(lower_case(case, env)),
                    _ => None,
                })
                .collect(),
        }),
        ast::Stmt::TypeSwitchStmt(type_switch) => Some(Stmt::TypeSwitch {
            init: type_switch
                .init
                .as_ref()
                .and_then(|init| lower_stmt(init, env).map(Box::new)),
            assign: Box::new(lower_stmt(&type_switch.assign, env).unwrap_or(Stmt::Empty)),
            cases: type_switch
                .body
                .list
                .iter()
                .filter_map(|stmt| match stmt {
                    ast::Stmt::CaseClause(case) => Some(lower_case(case, env)),
                    _ => None,
                })
                .collect(),
        }),
    }
}

fn lower_case(case: &ast::CaseClause<'_>, env: &TypeEnv) -> Case {
    let mut body_env = env.clone();
    Case {
        exprs: case.list.as_ref().map_or_else(Vec::new, |exprs| {
            exprs.iter().map(|expr| lower_expr(expr, env)).collect()
        }),
        body: case
            .body
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, &mut body_env))
            .collect(),
        is_default: case.list.is_none(),
    }
}

fn lower_comm_case(comm: &ast::CommClause<'_>, env: &TypeEnv) -> CommCase {
    let mut comm_env = env.clone();
    let lowered_comm = comm
        .comm
        .as_ref()
        .and_then(|stmt| lower_stmt(stmt, &mut comm_env).map(Box::new));
    let mut body_env = comm_env;
    CommCase {
        comm: lowered_comm,
        body: comm
            .body
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, &mut body_env))
            .collect(),
        is_default: comm.comm.is_none(),
    }
}

fn lower_expr(expr: &ast::Expr<'_>, env: &TypeEnv) -> Expr {
    let ty = GoType::infer_expr(expr, env);
    let addressability = expr_addressability(expr, env);
    let kind = match expr {
        ast::Expr::ArrayType(array) => ExprKind::ArrayType {
            len: array.len.as_ref().map(|len| Box::new(lower_expr(len, env))),
            elem: Box::new(lower_expr(&array.elt, env)),
        },
        ast::Expr::BasicLit(lit) => ExprKind::BasicLit(lit.value.to_string()),
        ast::Expr::BinaryExpr(binary) => ExprKind::Binary {
            op: token_text(binary.op),
            left: Box::new(lower_expr(&binary.x, env)),
            right: Box::new(lower_expr(&binary.y, env)),
        },
        ast::Expr::CallExpr(call) => ExprKind::Call(lower_call(call, env)),
        ast::Expr::ChanType(chan) => ExprKind::ChannelType {
            elem: Box::new(lower_expr(&chan.value, env)),
            direction: lower_channel_direction(chan.dir),
        },
        ast::Expr::CompositeLit(comp) => ExprKind::CompositeLit {
            ty: comp.type_.as_ref().map(|ty| Box::new(lower_expr(ty, env))),
            elems: comp.elts.as_ref().map_or_else(Vec::new, |elts| {
                elts.iter().map(|elt| lower_expr(elt, env)).collect()
            }),
        },
        ast::Expr::Ellipsis(ellipsis) => ExprKind::Ellipsis(
            ellipsis
                .elt
                .as_ref()
                .map(|elt| Box::new(lower_expr(elt, env))),
        ),
        ast::Expr::FuncLit(func_lit) => ExprKind::FuncLit(Box::new(lower_func_lit(func_lit, env))),
        ast::Expr::FuncType(func_type) => ExprKind::FuncType(lower_signature(func_type)),
        ast::Expr::Ident(ident) => ExprKind::Ident(ident.name.to_string()),
        ast::Expr::IndexExpr(index) => ExprKind::Index {
            target: Box::new(lower_expr(&index.x, env)),
            index: Box::new(lower_expr(&index.index, env)),
        },
        ast::Expr::IndexListExpr(index) => ExprKind::IndexList {
            target: Box::new(lower_expr(&index.x, env)),
            indices: index
                .indices
                .iter()
                .map(|index| lower_expr(index, env))
                .collect(),
        },
        ast::Expr::InterfaceType(_) => ExprKind::InterfaceType,
        ast::Expr::KeyValueExpr(kv) => ExprKind::KeyValue {
            key: Box::new(lower_expr(&kv.key, env)),
            value: Box::new(lower_expr(&kv.value, env)),
        },
        ast::Expr::MapType(map) => ExprKind::MapType {
            key: Box::new(lower_expr(&map.key, env)),
            value: Box::new(lower_expr(&map.value, env)),
        },
        ast::Expr::ParenExpr(paren) => ExprKind::Paren(Box::new(lower_expr(&paren.x, env))),
        ast::Expr::SelectorExpr(selector) => ExprKind::Selector {
            target: Box::new(lower_expr(&selector.x, env)),
            field: selector.sel.name.to_string(),
        },
        ast::Expr::SliceExpr(slice) => ExprKind::Slice {
            target: Box::new(lower_expr(&slice.x, env)),
            low: slice.low.as_ref().map(|low| Box::new(lower_expr(low, env))),
            high: slice
                .high
                .as_ref()
                .map(|high| Box::new(lower_expr(high, env))),
            max: slice.max.as_ref().map(|max| Box::new(lower_expr(max, env))),
        },
        ast::Expr::StarExpr(star) => ExprKind::Star(Box::new(lower_expr(&star.x, env))),
        ast::Expr::StructType(_) => ExprKind::StructType,
        ast::Expr::TypeAssertExpr(assert) => ExprKind::TypeAssert {
            target: Box::new(lower_expr(&assert.x, env)),
            ty: assert
                .type_
                .as_ref()
                .map(|ty| Box::new(lower_expr(ty, env))),
        },
        ast::Expr::UnaryExpr(unary) => ExprKind::Unary {
            op: token_text(unary.op),
            expr: Box::new(lower_expr(&unary.x, env)),
        },
    };
    Expr {
        kind,
        ty,
        addressability,
    }
}

fn lower_call(call: &ast::CallExpr<'_>, env: &TypeEnv) -> Call {
    Call {
        fun: Box::new(lower_expr(&call.fun, env)),
        args: call.args.as_ref().map_or_else(Vec::new, |args| {
            args.iter().map(|arg| lower_expr(arg, env)).collect()
        }),
        spread: call.ellipsis.is_some(),
    }
}

pub fn expr_addressability(expr: &ast::Expr<'_>, env: &TypeEnv) -> Addressability {
    match expr {
        ast::Expr::Ident(ident)
            if !env.is_const(ident.name)
                && (env.get_var(ident.name).is_some() || !is_predeclared_name(ident.name)) =>
        {
            Addressability::Addressable
        }
        ast::Expr::IndexExpr(index) => {
            let container = GoType::infer_expr(&index.x, env);
            match env.resolve_alias(&container) {
                GoType::Map(_, _) | GoType::String => Addressability::NotAddressable,
                GoType::Array(_) => expr_addressability(&index.x, env),
                _ => Addressability::Addressable,
            }
        }
        ast::Expr::ParenExpr(paren) => expr_addressability(&paren.x, env),
        ast::Expr::SelectorExpr(selector) => {
            let target_type = env.resolve_alias(&GoType::infer_expr(&selector.x, env));
            match target_type {
                GoType::Pointer(_) | GoType::Unknown => Addressability::Addressable,
                _ => expr_addressability(&selector.x, env),
            }
        }
        ast::Expr::StarExpr(_) => Addressability::Addressable,
        _ => Addressability::NotAddressable,
    }
}

pub fn is_string_concat_binary_expr(binary_expr: &ast::BinaryExpr<'_>, env: &TypeEnv) -> bool {
    binary_expr.op == token::Token::ADD
        && is_string_concat_operand(&binary_expr.x, env)
        && is_string_concat_operand(&binary_expr.y, env)
}

fn is_string_concat_operand(expr: &ast::Expr<'_>, env: &TypeEnv) -> bool {
    match expr {
        ast::Expr::BinaryExpr(binary) if binary.op == token::Token::ADD => {
            is_string_concat_binary_expr(binary, env)
        }
        _ => matches!(
            env.resolve_alias(&GoType::infer_expr(expr, env)),
            GoType::String
        ),
    }
}

fn lower_assign_op(tok: token::Token) -> AssignOp {
    match tok {
        token::Token::DEFINE => AssignOp::Define,
        token::Token::ASSIGN => AssignOp::Assign,
        token::Token::ADD_ASSIGN => AssignOp::Add,
        token::Token::SUB_ASSIGN => AssignOp::Sub,
        token::Token::MUL_ASSIGN => AssignOp::Mul,
        token::Token::QUO_ASSIGN => AssignOp::Quo,
        token::Token::REM_ASSIGN => AssignOp::Rem,
        token::Token::AND_ASSIGN => AssignOp::And,
        token::Token::OR_ASSIGN => AssignOp::Or,
        token::Token::XOR_ASSIGN => AssignOp::Xor,
        token::Token::SHL_ASSIGN => AssignOp::Shl,
        token::Token::SHR_ASSIGN => AssignOp::Shr,
        token::Token::AND_NOT_ASSIGN => AssignOp::AndNot,
        _ => AssignOp::Assign,
    }
}

fn lower_branch_kind(tok: token::Token) -> BranchKind {
    match tok {
        token::Token::CONTINUE => BranchKind::Continue,
        token::Token::FALLTHROUGH => BranchKind::Fallthrough,
        token::Token::GOTO => BranchKind::Goto,
        _ => BranchKind::Break,
    }
}

fn lower_inc_dec_op(tok: token::Token) -> IncDecOp {
    if tok == token::Token::DEC {
        IncDecOp::Dec
    } else {
        IncDecOp::Inc
    }
}

fn lower_channel_direction(dir: u8) -> ChannelDirection {
    match dir {
        1 => ChannelDirection::Send,
        2 => ChannelDirection::Receive,
        _ => ChannelDirection::Bidirectional,
    }
}

fn token_text(tok: token::Token) -> String {
    <&'static str>::from(&tok).to_string()
}

pub fn func_lit_captures(func_lit: &ast::FuncLit<'_>, env: &TypeEnv) -> Vec<Capture> {
    let uses = func_lit_free_name_uses(func_lit);
    uses.referenced
        .into_iter()
        .filter(|name| !is_predeclared_name(name))
        .map(|name| {
            let mode = if uses.mutated.contains(&name) {
                CaptureMode::BorrowMut
            } else {
                CaptureMode::Borrow
            };
            Capture {
                ty: env.get_var(&name).unwrap_or(GoType::Unknown),
                name,
                mode,
            }
        })
        .collect()
}

#[derive(Default)]
struct ScopedNameUses {
    referenced: BTreeSet<String>,
    mutated: BTreeSet<String>,
}

fn func_lit_free_name_uses(func_lit: &ast::FuncLit<'_>) -> ScopedNameUses {
    let mut scopes = vec![BTreeSet::new()];
    if let Some(scope) = scopes.last_mut() {
        collect_signature_bindings(&func_lit.type_, scope);
    }
    let mut uses = ScopedNameUses::default();
    collect_free_name_uses_in_stmt_list(&func_lit.body.list, &mut scopes, &mut uses);
    uses
}

fn collect_free_name_uses_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    for stmt in stmts {
        collect_free_name_uses_in_stmt(stmt, scopes, uses);
    }
}

fn collect_free_name_uses_in_nested_block(
    block: &ast::BlockStmt<'_>,
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    scopes.push(BTreeSet::new());
    collect_free_name_uses_in_stmt_list(&block.list, scopes, uses);
    scopes.pop();
}

fn scoped_name_is_bound(scopes: &[BTreeSet<String>], name: &str) -> bool {
    scopes.iter().rev().any(|scope| scope.contains(name))
}

fn scoped_name_is_bound_in_current_scope(scopes: &[BTreeSet<String>], name: &str) -> bool {
    scopes.last().is_some_and(|scope| scope.contains(name))
}

fn scoped_declare_name(scopes: &mut [BTreeSet<String>], name: String) {
    if let Some(scope) = scopes.last_mut() {
        scope.insert(name);
    }
}

fn scoped_record_reference(scopes: &[BTreeSet<String>], uses: &mut ScopedNameUses, name: &str) {
    if !scoped_name_is_bound(scopes, name) {
        uses.referenced.insert(name.to_string());
    }
}

fn scoped_record_mutation(scopes: &[BTreeSet<String>], uses: &mut ScopedNameUses, name: &str) {
    if !scoped_name_is_bound(scopes, name) {
        uses.referenced.insert(name.to_string());
        uses.mutated.insert(name.to_string());
    }
}

fn collect_free_name_uses_in_stmt(
    stmt: &ast::Stmt<'_>,
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            if assign.tok == token::Token::DEFINE {
                for expr in &assign.rhs {
                    collect_free_name_uses_in_expr(expr, scopes, uses);
                }
                for expr in &assign.lhs {
                    match ident_name(expr) {
                        Some(name) if scoped_name_is_bound_in_current_scope(scopes, &name) => {}
                        Some(name) => scoped_declare_name(scopes, name),
                        None => collect_free_name_uses_in_expr(expr, scopes, uses),
                    }
                }
            } else {
                for expr in &assign.lhs {
                    collect_free_name_uses_in_assignment_lhs(expr, scopes, uses);
                }
                for expr in &assign.rhs {
                    collect_free_name_uses_in_expr(expr, scopes, uses);
                }
            }
        }
        ast::Stmt::BlockStmt(block) => collect_free_name_uses_in_nested_block(block, scopes, uses),
        ast::Stmt::CaseClause(case_clause) => {
            if let Some(exprs) = &case_clause.list {
                for expr in exprs {
                    collect_free_name_uses_in_expr(expr, scopes, uses);
                }
            }
            collect_free_name_uses_in_nested_stmt_list(&case_clause.body, scopes, uses);
        }
        ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = &comm_clause.comm {
                collect_free_name_uses_in_stmt(comm, scopes, uses);
            }
            collect_free_name_uses_in_nested_stmt_list(&comm_clause.body, scopes, uses);
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    if let Some(values) = &value.values {
                        for expr in values {
                            collect_free_name_uses_in_expr(expr, scopes, uses);
                        }
                    }
                    for name in &value.names {
                        scoped_declare_name(scopes, name.name.to_string());
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer_stmt) => {
            collect_free_name_uses_in_call(&defer_stmt.call, scopes, uses);
        }
        ast::Stmt::ExprStmt(expr) => collect_free_name_uses_in_expr(&expr.x, scopes, uses),
        ast::Stmt::ForStmt(for_stmt) => {
            let has_clause_scope = for_stmt.init.is_some();
            if has_clause_scope {
                scopes.push(BTreeSet::new());
            }
            if let Some(init) = &for_stmt.init {
                collect_free_name_uses_in_stmt(init, scopes, uses);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_free_name_uses_in_expr(cond, scopes, uses);
            }
            if let Some(post) = &for_stmt.post {
                collect_free_name_uses_in_stmt(post, scopes, uses);
            }
            collect_free_name_uses_in_nested_block(&for_stmt.body, scopes, uses);
            if has_clause_scope {
                scopes.pop();
            }
        }
        ast::Stmt::GoStmt(go_stmt) => collect_free_name_uses_in_call(&go_stmt.call, scopes, uses),
        ast::Stmt::IfStmt(if_stmt) => {
            let has_clause_scope = if_stmt.init.as_ref().as_ref().is_some();
            if has_clause_scope {
                scopes.push(BTreeSet::new());
            }
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_free_name_uses_in_stmt(init, scopes, uses);
            }
            collect_free_name_uses_in_expr(&if_stmt.cond, scopes, uses);
            collect_free_name_uses_in_nested_block(&if_stmt.body, scopes, uses);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_free_name_uses_in_stmt(else_branch, scopes, uses);
            }
            if has_clause_scope {
                scopes.pop();
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            if let Some(name) = ident_name(&inc_dec.x) {
                scoped_record_mutation(scopes, uses, &name);
            } else {
                collect_free_name_uses_in_expr(&inc_dec.x, scopes, uses);
            }
        }
        ast::Stmt::LabeledStmt(label) => {
            collect_free_name_uses_in_stmt(&label.stmt, scopes, uses);
        }
        ast::Stmt::RangeStmt(range) => {
            collect_free_name_uses_in_expr(&range.x, scopes, uses);
            let has_range_scope = matches!(range.tok, Some(token::Token::DEFINE));
            if has_range_scope {
                scopes.push(BTreeSet::new());
                if let Some(key) = &range.key
                    && let Some(name) = ident_name(key)
                {
                    scoped_declare_name(scopes, name);
                }
                if let Some(value) = &range.value
                    && let Some(name) = ident_name(value)
                {
                    scoped_declare_name(scopes, name);
                }
            } else {
                if let Some(key) = &range.key {
                    collect_free_name_uses_in_assignment_lhs(key, scopes, uses);
                }
                if let Some(value) = &range.value {
                    collect_free_name_uses_in_assignment_lhs(value, scopes, uses);
                }
            }
            collect_free_name_uses_in_nested_block(&range.body, scopes, uses);
            if has_range_scope {
                scopes.pop();
            }
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_free_name_uses_in_expr(expr, scopes, uses);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_free_name_uses_in_expr(&send.chan, scopes, uses);
            collect_free_name_uses_in_expr(&send.value, scopes, uses);
        }
        ast::Stmt::SelectStmt(select_stmt) => {
            collect_free_name_uses_in_nested_block(&select_stmt.body, scopes, uses);
        }
        ast::Stmt::SwitchStmt(switch_stmt) => {
            let has_clause_scope = switch_stmt.init.is_some();
            if has_clause_scope {
                scopes.push(BTreeSet::new());
            }
            if let Some(init) = &switch_stmt.init {
                collect_free_name_uses_in_stmt(init, scopes, uses);
            }
            if let Some(tag) = &switch_stmt.tag {
                collect_free_name_uses_in_expr(tag, scopes, uses);
            }
            collect_free_name_uses_in_nested_block(&switch_stmt.body, scopes, uses);
            if has_clause_scope {
                scopes.pop();
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            let has_clause_scope = type_switch.init.is_some();
            if has_clause_scope {
                scopes.push(BTreeSet::new());
            }
            if let Some(init) = &type_switch.init {
                collect_free_name_uses_in_stmt(init, scopes, uses);
            }
            collect_free_name_uses_in_stmt(&type_switch.assign, scopes, uses);
            collect_free_name_uses_in_nested_block(&type_switch.body, scopes, uses);
            if has_clause_scope {
                scopes.pop();
            }
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_free_name_uses_in_nested_stmt_list(
    stmts: &[ast::Stmt<'_>],
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    scopes.push(BTreeSet::new());
    collect_free_name_uses_in_stmt_list(stmts, scopes, uses);
    scopes.pop();
}

fn collect_free_name_uses_in_assignment_lhs(
    expr: &ast::Expr<'_>,
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    if let Some(name) = ident_name(expr) {
        scoped_record_mutation(scopes, uses, &name);
    } else {
        collect_free_name_uses_in_expr(expr, scopes, uses);
    }
}

fn collect_free_name_uses_in_call(
    call: &ast::CallExpr<'_>,
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    collect_free_name_uses_in_expr(&call.fun, scopes, uses);
    if let Some(args) = &call.args {
        for arg in args {
            collect_free_name_uses_in_expr(arg, scopes, uses);
        }
    }
}

fn collect_free_name_uses_in_expr(
    expr: &ast::Expr<'_>,
    scopes: &mut Vec<BTreeSet<String>>,
    uses: &mut ScopedNameUses,
) {
    match expr {
        ast::Expr::Ident(ident) => scoped_record_reference(scopes, uses, ident.name),
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_free_name_uses_in_expr(len, scopes, uses);
            }
            collect_free_name_uses_in_expr(&array.elt, scopes, uses);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_free_name_uses_in_expr(&binary.x, scopes, uses);
            collect_free_name_uses_in_expr(&binary.y, scopes, uses);
        }
        ast::Expr::CallExpr(call) => collect_free_name_uses_in_call(call, scopes, uses),
        ast::Expr::ChanType(chan) => collect_free_name_uses_in_expr(&chan.value, scopes, uses),
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                collect_free_name_uses_in_expr(ty, scopes, uses);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_free_name_uses_in_expr(elt, scopes, uses);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_free_name_uses_in_expr(elt, scopes, uses);
            }
        }
        ast::Expr::FuncLit(func_lit) => {
            let nested_uses = func_lit_free_name_uses(func_lit);
            for name in nested_uses.referenced {
                if nested_uses.mutated.contains(&name) {
                    scoped_record_mutation(scopes, uses, &name);
                } else {
                    scoped_record_reference(scopes, uses, &name);
                }
            }
        }
        ast::Expr::IndexExpr(index) => {
            collect_free_name_uses_in_expr(&index.x, scopes, uses);
            collect_free_name_uses_in_expr(&index.index, scopes, uses);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_free_name_uses_in_expr(&index.x, scopes, uses);
            for index in &index.indices {
                collect_free_name_uses_in_expr(index, scopes, uses);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_free_name_uses_in_expr(&kv.key, scopes, uses);
            collect_free_name_uses_in_expr(&kv.value, scopes, uses);
        }
        ast::Expr::MapType(map) => {
            collect_free_name_uses_in_expr(&map.key, scopes, uses);
            collect_free_name_uses_in_expr(&map.value, scopes, uses);
        }
        ast::Expr::ParenExpr(paren) => collect_free_name_uses_in_expr(&paren.x, scopes, uses),
        ast::Expr::SelectorExpr(selector) => {
            collect_free_name_uses_in_expr(&selector.x, scopes, uses);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_free_name_uses_in_expr(&slice.x, scopes, uses);
            if let Some(low) = &slice.low {
                collect_free_name_uses_in_expr(low, scopes, uses);
            }
            if let Some(high) = &slice.high {
                collect_free_name_uses_in_expr(high, scopes, uses);
            }
            if let Some(max) = &slice.max {
                collect_free_name_uses_in_expr(max, scopes, uses);
            }
        }
        ast::Expr::StarExpr(star) => collect_free_name_uses_in_expr(&star.x, scopes, uses),
        ast::Expr::TypeAssertExpr(assert) => {
            collect_free_name_uses_in_expr(&assert.x, scopes, uses);
            if let Some(ty) = &assert.type_ {
                collect_free_name_uses_in_expr(ty, scopes, uses);
            }
        }
        ast::Expr::UnaryExpr(unary) => collect_free_name_uses_in_expr(&unary.x, scopes, uses),
        ast::Expr::BasicLit(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => {}
    }
}

pub fn mutable_func_lit_capture_names_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_mutable_func_lit_capture_names_in_block(block, env, &mut names);
    names
}

pub fn mutable_range_function_capture_names_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_mutable_range_function_capture_names_in_block(block, env, &mut names);
    names
}

pub fn mutable_range_function_capture_names(
    range: &ast::RangeStmt<'_>,
    env: &TypeEnv,
) -> BTreeSet<String> {
    if !matches!(range_kind(&range.x, env), RangeKind::Function) {
        return BTreeSet::new();
    }
    range_function_body_free_name_uses(range)
        .mutated
        .into_iter()
        .filter(|name| !is_predeclared_name(name))
        .collect()
}

pub fn address_taken_names_in_block(block: &ast::BlockStmt<'_>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_address_taken_names_in_block(block, &mut names);
    let mut declared = BTreeSet::new();
    collect_declared_names_in_block(block, &mut declared);
    names.retain(|name| declared.contains(name));
    names
}

fn collect_address_taken_names_in_block(block: &ast::BlockStmt<'_>, names: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_address_taken_names_in_stmt(stmt, names);
    }
}

fn collect_address_taken_names_in_stmt(stmt: &ast::Stmt<'_>, names: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_address_taken_names_in_expr(expr, names);
            }
        }
        ast::Stmt::BlockStmt(block) => collect_address_taken_names_in_block(block, names),
        ast::Stmt::CaseClause(case_clause) => {
            if let Some(exprs) = &case_clause.list {
                for expr in exprs {
                    collect_address_taken_names_in_expr(expr, names);
                }
            }
            for stmt in &case_clause.body {
                collect_address_taken_names_in_stmt(stmt, names);
            }
        }
        ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = &comm_clause.comm {
                collect_address_taken_names_in_stmt(comm, names);
            }
            for stmt in &comm_clause.body {
                collect_address_taken_names_in_stmt(stmt, names);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_address_taken_names_in_expr(expr, names);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer_stmt) => {
            collect_address_taken_names_in_expr(&defer_stmt.call.fun, names);
            if let Some(args) = &defer_stmt.call.args {
                for arg in args {
                    collect_address_taken_names_in_expr(arg, names);
                }
            }
        }
        ast::Stmt::ExprStmt(expr) => collect_address_taken_names_in_expr(&expr.x, names),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_address_taken_names_in_stmt(init, names);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_address_taken_names_in_expr(cond, names);
            }
            if let Some(post) = &for_stmt.post {
                collect_address_taken_names_in_stmt(post, names);
            }
            collect_address_taken_names_in_block(&for_stmt.body, names);
        }
        ast::Stmt::GoStmt(go_stmt) => {
            collect_address_taken_names_in_expr(&go_stmt.call.fun, names);
            if let Some(args) = &go_stmt.call.args {
                for arg in args {
                    collect_address_taken_names_in_expr(arg, names);
                }
            }
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_address_taken_names_in_stmt(init, names);
            }
            collect_address_taken_names_in_expr(&if_stmt.cond, names);
            collect_address_taken_names_in_block(&if_stmt.body, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_address_taken_names_in_stmt(else_branch, names);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => collect_address_taken_names_in_expr(&inc_dec.x, names),
        ast::Stmt::LabeledStmt(label) => collect_address_taken_names_in_stmt(&label.stmt, names),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_address_taken_names_in_expr(key, names);
            }
            if let Some(value) = &range.value {
                collect_address_taken_names_in_expr(value, names);
            }
            collect_address_taken_names_in_expr(&range.x, names);
            collect_address_taken_names_in_block(&range.body, names);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_address_taken_names_in_expr(expr, names);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_address_taken_names_in_expr(&send.chan, names);
            collect_address_taken_names_in_expr(&send.value, names);
        }
        ast::Stmt::SelectStmt(select_stmt) => {
            collect_address_taken_names_in_block(&select_stmt.body, names)
        }
        ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = &switch_stmt.init {
                collect_address_taken_names_in_stmt(init, names);
            }
            if let Some(tag) = &switch_stmt.tag {
                collect_address_taken_names_in_expr(tag, names);
            }
            collect_address_taken_names_in_block(&switch_stmt.body, names);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_address_taken_names_in_stmt(init, names);
            }
            collect_address_taken_names_in_stmt(&type_switch.assign, names);
            collect_address_taken_names_in_block(&type_switch.body, names);
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_address_taken_names_in_expr(expr: &ast::Expr<'_>, names: &mut BTreeSet<String>) {
    match expr {
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::AND => {
            if let ast::Expr::Ident(ident) = &*unary.x
                && ident.name != "_"
            {
                names.insert(ident.name.to_string());
            }
            collect_address_taken_names_in_expr(&unary.x, names);
        }
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_address_taken_names_in_expr(len, names);
            }
            collect_address_taken_names_in_expr(&array.elt, names);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_address_taken_names_in_expr(&binary.x, names);
            collect_address_taken_names_in_expr(&binary.y, names);
        }
        ast::Expr::CallExpr(call) => {
            collect_address_taken_names_in_expr(&call.fun, names);
            if let Some(args) = &call.args {
                for arg in args {
                    collect_address_taken_names_in_expr(arg, names);
                }
            }
        }
        ast::Expr::ChanType(chan) => collect_address_taken_names_in_expr(&chan.value, names),
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                collect_address_taken_names_in_expr(ty, names);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_address_taken_names_in_expr(elt, names);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_address_taken_names_in_expr(elt, names);
            }
        }
        ast::Expr::FuncLit(func_lit) => collect_address_taken_names_in_block(&func_lit.body, names),
        ast::Expr::IndexExpr(index) => {
            collect_address_taken_names_in_expr(&index.x, names);
            collect_address_taken_names_in_expr(&index.index, names);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_address_taken_names_in_expr(&index.x, names);
            for index in &index.indices {
                collect_address_taken_names_in_expr(index, names);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_address_taken_names_in_expr(&kv.key, names);
            collect_address_taken_names_in_expr(&kv.value, names);
        }
        ast::Expr::MapType(map) => {
            collect_address_taken_names_in_expr(&map.key, names);
            collect_address_taken_names_in_expr(&map.value, names);
        }
        ast::Expr::ParenExpr(paren) => collect_address_taken_names_in_expr(&paren.x, names),
        ast::Expr::SelectorExpr(selector) => {
            collect_address_taken_names_in_expr(&selector.x, names)
        }
        ast::Expr::SliceExpr(slice) => {
            collect_address_taken_names_in_expr(&slice.x, names);
            if let Some(low) = &slice.low {
                collect_address_taken_names_in_expr(low, names);
            }
            if let Some(high) = &slice.high {
                collect_address_taken_names_in_expr(high, names);
            }
            if let Some(max) = &slice.max {
                collect_address_taken_names_in_expr(max, names);
            }
        }
        ast::Expr::StarExpr(star) => collect_address_taken_names_in_expr(&star.x, names),
        ast::Expr::TypeAssertExpr(assert) => {
            collect_address_taken_names_in_expr(&assert.x, names);
            if let Some(ty) = &assert.type_ {
                collect_address_taken_names_in_expr(ty, names);
            }
        }
        ast::Expr::UnaryExpr(unary) => collect_address_taken_names_in_expr(&unary.x, names),
        ast::Expr::BasicLit(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::Ident(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => {}
    }
}

fn collect_mutable_func_lit_capture_names_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    for stmt in &block.list {
        collect_mutable_func_lit_capture_names_in_stmt(stmt, env, names);
    }
}

fn collect_mutable_func_lit_capture_names_in_stmt(
    stmt: &ast::Stmt<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_mutable_func_lit_capture_names_in_expr(expr, env, names);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_mutable_func_lit_capture_names_in_block(block, env, names);
        }
        ast::Stmt::CaseClause(case_clause) => {
            if let Some(exprs) = &case_clause.list {
                for expr in exprs {
                    collect_mutable_func_lit_capture_names_in_expr(expr, env, names);
                }
            }
            for stmt in &case_clause.body {
                collect_mutable_func_lit_capture_names_in_stmt(stmt, env, names);
            }
        }
        ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = &comm_clause.comm {
                collect_mutable_func_lit_capture_names_in_stmt(comm, env, names);
            }
            for stmt in &comm_clause.body {
                collect_mutable_func_lit_capture_names_in_stmt(stmt, env, names);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    if let Some(values) = &value.values {
                        for expr in values {
                            collect_mutable_func_lit_capture_names_in_expr(expr, env, names);
                        }
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer_stmt) => {
            collect_mutable_func_lit_capture_names_in_call(&defer_stmt.call, env, names);
        }
        ast::Stmt::ExprStmt(expr) => {
            collect_mutable_func_lit_capture_names_in_expr(&expr.x, env, names);
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_mutable_func_lit_capture_names_in_stmt(init, env, names);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_mutable_func_lit_capture_names_in_expr(cond, env, names);
            }
            if let Some(post) = &for_stmt.post {
                collect_mutable_func_lit_capture_names_in_stmt(post, env, names);
            }
            collect_mutable_func_lit_capture_names_in_block(&for_stmt.body, env, names);
        }
        ast::Stmt::GoStmt(go_stmt) => {
            collect_mutable_func_lit_capture_names_in_call(&go_stmt.call, env, names);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_mutable_func_lit_capture_names_in_stmt(init, env, names);
            }
            collect_mutable_func_lit_capture_names_in_expr(&if_stmt.cond, env, names);
            collect_mutable_func_lit_capture_names_in_block(&if_stmt.body, env, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_mutable_func_lit_capture_names_in_stmt(else_branch, env, names);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            collect_mutable_func_lit_capture_names_in_expr(&inc_dec.x, env, names);
        }
        ast::Stmt::LabeledStmt(label) => {
            collect_mutable_func_lit_capture_names_in_stmt(&label.stmt, env, names);
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_mutable_func_lit_capture_names_in_expr(key, env, names);
            }
            if let Some(value) = &range.value {
                collect_mutable_func_lit_capture_names_in_expr(value, env, names);
            }
            collect_mutable_func_lit_capture_names_in_expr(&range.x, env, names);
            collect_mutable_func_lit_capture_names_in_block(&range.body, env, names);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_mutable_func_lit_capture_names_in_expr(expr, env, names);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_mutable_func_lit_capture_names_in_expr(&send.chan, env, names);
            collect_mutable_func_lit_capture_names_in_expr(&send.value, env, names);
        }
        ast::Stmt::SelectStmt(select_stmt) => {
            collect_mutable_func_lit_capture_names_in_block(&select_stmt.body, env, names);
        }
        ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = &switch_stmt.init {
                collect_mutable_func_lit_capture_names_in_stmt(init, env, names);
            }
            if let Some(tag) = &switch_stmt.tag {
                collect_mutable_func_lit_capture_names_in_expr(tag, env, names);
            }
            collect_mutable_func_lit_capture_names_in_block(&switch_stmt.body, env, names);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_mutable_func_lit_capture_names_in_stmt(init, env, names);
            }
            collect_mutable_func_lit_capture_names_in_stmt(&type_switch.assign, env, names);
            collect_mutable_func_lit_capture_names_in_block(&type_switch.body, env, names);
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_mutable_func_lit_capture_names_in_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    collect_mutable_func_lit_capture_names_in_expr(&call.fun, env, names);
    if let Some(args) = &call.args {
        for arg in args {
            collect_mutable_func_lit_capture_names_in_expr(arg, env, names);
        }
    }
}

fn collect_mutable_func_lit_capture_names_in_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_mutable_func_lit_capture_names_in_expr(len, env, names);
            }
            collect_mutable_func_lit_capture_names_in_expr(&array.elt, env, names);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_mutable_func_lit_capture_names_in_expr(&binary.x, env, names);
            collect_mutable_func_lit_capture_names_in_expr(&binary.y, env, names);
        }
        ast::Expr::CallExpr(call) => {
            collect_mutable_func_lit_capture_names_in_call(call, env, names);
        }
        ast::Expr::ChanType(chan) => {
            collect_mutable_func_lit_capture_names_in_expr(&chan.value, env, names);
        }
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                collect_mutable_func_lit_capture_names_in_expr(ty, env, names);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_mutable_func_lit_capture_names_in_expr(elt, env, names);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_mutable_func_lit_capture_names_in_expr(elt, env, names);
            }
        }
        ast::Expr::FuncLit(func_lit) => {
            names.extend(
                func_lit_captures(func_lit, env)
                    .into_iter()
                    .filter(|capture| capture.mode == CaptureMode::BorrowMut)
                    .map(|capture| capture.name),
            );
            collect_mutable_func_lit_capture_names_in_block(&func_lit.body, env, names);
        }
        ast::Expr::IndexExpr(index) => {
            collect_mutable_func_lit_capture_names_in_expr(&index.x, env, names);
            collect_mutable_func_lit_capture_names_in_expr(&index.index, env, names);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_mutable_func_lit_capture_names_in_expr(&index.x, env, names);
            for index in &index.indices {
                collect_mutable_func_lit_capture_names_in_expr(index, env, names);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_mutable_func_lit_capture_names_in_expr(&kv.key, env, names);
            collect_mutable_func_lit_capture_names_in_expr(&kv.value, env, names);
        }
        ast::Expr::MapType(map) => {
            collect_mutable_func_lit_capture_names_in_expr(&map.key, env, names);
            collect_mutable_func_lit_capture_names_in_expr(&map.value, env, names);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_mutable_func_lit_capture_names_in_expr(&paren.x, env, names);
        }
        ast::Expr::SelectorExpr(selector) => {
            collect_mutable_func_lit_capture_names_in_expr(&selector.x, env, names);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_mutable_func_lit_capture_names_in_expr(&slice.x, env, names);
            if let Some(low) = &slice.low {
                collect_mutable_func_lit_capture_names_in_expr(low, env, names);
            }
            if let Some(high) = &slice.high {
                collect_mutable_func_lit_capture_names_in_expr(high, env, names);
            }
            if let Some(max) = &slice.max {
                collect_mutable_func_lit_capture_names_in_expr(max, env, names);
            }
        }
        ast::Expr::StarExpr(star) => {
            collect_mutable_func_lit_capture_names_in_expr(&star.x, env, names);
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_mutable_func_lit_capture_names_in_expr(&assert.x, env, names);
            if let Some(ty) = &assert.type_ {
                collect_mutable_func_lit_capture_names_in_expr(ty, env, names);
            }
        }
        ast::Expr::UnaryExpr(unary) => {
            collect_mutable_func_lit_capture_names_in_expr(&unary.x, env, names);
        }
        ast::Expr::BasicLit(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::Ident(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => {}
    }
}

fn collect_mutable_range_function_capture_names_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    for stmt in &block.list {
        collect_mutable_range_function_capture_names_in_stmt(stmt, env, names);
    }
}

fn collect_mutable_range_function_capture_names_in_stmt(
    stmt: &ast::Stmt<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_mutable_range_function_capture_names_in_expr(expr, env, names);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_mutable_range_function_capture_names_in_block(block, env, names);
        }
        ast::Stmt::CaseClause(case_clause) => {
            if let Some(exprs) = &case_clause.list {
                for expr in exprs {
                    collect_mutable_range_function_capture_names_in_expr(expr, env, names);
                }
            }
            for stmt in &case_clause.body {
                collect_mutable_range_function_capture_names_in_stmt(stmt, env, names);
            }
        }
        ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = &comm_clause.comm {
                collect_mutable_range_function_capture_names_in_stmt(comm, env, names);
            }
            for stmt in &comm_clause.body {
                collect_mutable_range_function_capture_names_in_stmt(stmt, env, names);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_mutable_range_function_capture_names_in_expr(expr, env, names);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer_stmt) => {
            collect_mutable_range_function_capture_names_in_call(&defer_stmt.call, env, names);
        }
        ast::Stmt::ExprStmt(expr) => {
            collect_mutable_range_function_capture_names_in_expr(&expr.x, env, names);
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_mutable_range_function_capture_names_in_stmt(init, env, names);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_mutable_range_function_capture_names_in_expr(cond, env, names);
            }
            if let Some(post) = &for_stmt.post {
                collect_mutable_range_function_capture_names_in_stmt(post, env, names);
            }
            collect_mutable_range_function_capture_names_in_block(&for_stmt.body, env, names);
        }
        ast::Stmt::GoStmt(go_stmt) => {
            collect_mutable_range_function_capture_names_in_call(&go_stmt.call, env, names);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_mutable_range_function_capture_names_in_stmt(init, env, names);
            }
            collect_mutable_range_function_capture_names_in_expr(&if_stmt.cond, env, names);
            collect_mutable_range_function_capture_names_in_block(&if_stmt.body, env, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_mutable_range_function_capture_names_in_stmt(else_branch, env, names);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            collect_mutable_range_function_capture_names_in_expr(&inc_dec.x, env, names);
        }
        ast::Stmt::LabeledStmt(label) => {
            collect_mutable_range_function_capture_names_in_stmt(&label.stmt, env, names);
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_mutable_range_function_capture_names_in_expr(key, env, names);
            }
            if let Some(value) = &range.value {
                collect_mutable_range_function_capture_names_in_expr(value, env, names);
            }
            collect_mutable_range_function_capture_names_in_expr(&range.x, env, names);
            names.extend(mutable_range_function_capture_names(range, env));
            collect_mutable_range_function_capture_names_in_block(&range.body, env, names);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_mutable_range_function_capture_names_in_expr(expr, env, names);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_mutable_range_function_capture_names_in_expr(&send.chan, env, names);
            collect_mutable_range_function_capture_names_in_expr(&send.value, env, names);
        }
        ast::Stmt::SelectStmt(select_stmt) => {
            collect_mutable_range_function_capture_names_in_block(&select_stmt.body, env, names);
        }
        ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = &switch_stmt.init {
                collect_mutable_range_function_capture_names_in_stmt(init, env, names);
            }
            if let Some(tag) = &switch_stmt.tag {
                collect_mutable_range_function_capture_names_in_expr(tag, env, names);
            }
            collect_mutable_range_function_capture_names_in_block(&switch_stmt.body, env, names);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_mutable_range_function_capture_names_in_stmt(init, env, names);
            }
            collect_mutable_range_function_capture_names_in_stmt(&type_switch.assign, env, names);
            collect_mutable_range_function_capture_names_in_block(&type_switch.body, env, names);
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_mutable_range_function_capture_names_in_call(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    collect_mutable_range_function_capture_names_in_expr(&call.fun, env, names);
    if let Some(args) = &call.args {
        for arg in args {
            collect_mutable_range_function_capture_names_in_expr(arg, env, names);
        }
    }
}

fn collect_mutable_range_function_capture_names_in_expr(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
    names: &mut BTreeSet<String>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_mutable_range_function_capture_names_in_expr(len, env, names);
            }
            collect_mutable_range_function_capture_names_in_expr(&array.elt, env, names);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_mutable_range_function_capture_names_in_expr(&binary.x, env, names);
            collect_mutable_range_function_capture_names_in_expr(&binary.y, env, names);
        }
        ast::Expr::CallExpr(call) => {
            collect_mutable_range_function_capture_names_in_call(call, env, names);
        }
        ast::Expr::ChanType(chan) => {
            collect_mutable_range_function_capture_names_in_expr(&chan.value, env, names);
        }
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                collect_mutable_range_function_capture_names_in_expr(ty, env, names);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_mutable_range_function_capture_names_in_expr(elt, env, names);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_mutable_range_function_capture_names_in_expr(elt, env, names);
            }
        }
        ast::Expr::FuncLit(func_lit) => {
            collect_mutable_range_function_capture_names_in_block(&func_lit.body, env, names);
        }
        ast::Expr::IndexExpr(index) => {
            collect_mutable_range_function_capture_names_in_expr(&index.x, env, names);
            collect_mutable_range_function_capture_names_in_expr(&index.index, env, names);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_mutable_range_function_capture_names_in_expr(&index.x, env, names);
            for index in &index.indices {
                collect_mutable_range_function_capture_names_in_expr(index, env, names);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_mutable_range_function_capture_names_in_expr(&kv.key, env, names);
            collect_mutable_range_function_capture_names_in_expr(&kv.value, env, names);
        }
        ast::Expr::MapType(map) => {
            collect_mutable_range_function_capture_names_in_expr(&map.key, env, names);
            collect_mutable_range_function_capture_names_in_expr(&map.value, env, names);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_mutable_range_function_capture_names_in_expr(&paren.x, env, names);
        }
        ast::Expr::SelectorExpr(selector) => {
            collect_mutable_range_function_capture_names_in_expr(&selector.x, env, names);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_mutable_range_function_capture_names_in_expr(&slice.x, env, names);
            if let Some(low) = &slice.low {
                collect_mutable_range_function_capture_names_in_expr(low, env, names);
            }
            if let Some(high) = &slice.high {
                collect_mutable_range_function_capture_names_in_expr(high, env, names);
            }
            if let Some(max) = &slice.max {
                collect_mutable_range_function_capture_names_in_expr(max, env, names);
            }
        }
        ast::Expr::StarExpr(star) => {
            collect_mutable_range_function_capture_names_in_expr(&star.x, env, names);
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_mutable_range_function_capture_names_in_expr(&assert.x, env, names);
            if let Some(ty) = &assert.type_ {
                collect_mutable_range_function_capture_names_in_expr(ty, env, names);
            }
        }
        ast::Expr::UnaryExpr(unary) => {
            collect_mutable_range_function_capture_names_in_expr(&unary.x, env, names);
        }
        ast::Expr::BasicLit(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::Ident(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => {}
    }
}

fn range_function_body_free_name_uses(range: &ast::RangeStmt<'_>) -> ScopedNameUses {
    let mut scopes = vec![BTreeSet::new()];
    if matches!(range.tok, Some(token::Token::DEFINE)) {
        if let Some(key) = &range.key
            && let Some(name) = ident_name(key)
        {
            scoped_declare_name(&mut scopes, name);
        }
        if let Some(value) = &range.value
            && let Some(name) = ident_name(value)
        {
            scoped_declare_name(&mut scopes, name);
        }
    }
    let mut uses = ScopedNameUses::default();
    collect_free_name_uses_in_stmt_list(&range.body.list, &mut scopes, &mut uses);
    uses
}

fn collect_signature_bindings(func_type: &ast::FuncType<'_>, names: &mut BTreeSet<String>) {
    collect_field_names(&func_type.params, names);
    if let Some(results) = &func_type.results {
        collect_field_names(results, names);
    }
}

fn collect_field_names(fields: &ast::FieldList<'_>, names: &mut BTreeSet<String>) {
    for field in &fields.list {
        if let Some(field_names) = &field.names {
            names.extend(field_names.iter().map(|name| name.name.to_string()));
        }
    }
}

fn collect_declared_names_in_block(block: &ast::BlockStmt<'_>, names: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_declared_names_in_stmt(stmt, names);
    }
}

fn collect_declared_names_in_stmt(stmt: &ast::Stmt<'_>, names: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::AssignStmt(assign) if assign.tok == token::Token::DEFINE => {
            names.extend(assign.lhs.iter().filter_map(ident_name));
        }
        ast::Stmt::BlockStmt(block) => collect_declared_names_in_block(block, names),
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    names.extend(value.names.iter().map(|name| name.name.to_string()));
                }
            }
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_declared_names_in_stmt(init, names);
            }
            collect_declared_names_in_block(&for_stmt.body, names);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_declared_names_in_stmt(init, names);
            }
            collect_declared_names_in_block(&if_stmt.body, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_declared_names_in_stmt(else_branch, names);
            }
        }
        ast::Stmt::RangeStmt(range) if matches!(range.tok, Some(token::Token::DEFINE)) => {
            if let Some(key) = &range.key
                && let Some(name) = ident_name(key)
            {
                names.insert(name);
            }
            if let Some(value) = &range.value
                && let Some(name) = ident_name(value)
            {
                names.insert(name);
            }
            collect_declared_names_in_block(&range.body, names);
        }
        ast::Stmt::RangeStmt(range) => collect_declared_names_in_block(&range.body, names),
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_declared_names_in_stmt(init, names);
            }
            collect_declared_names_in_block(&switch.body, names);
        }
        ast::Stmt::LabeledStmt(label) => collect_declared_names_in_stmt(&label.stmt, names),
        _ => {}
    }
}

fn collect_referenced_names_in_block(block: &ast::BlockStmt<'_>, names: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_referenced_names_in_stmt(stmt, names);
    }
}

fn collect_referenced_names_in_stmt(stmt: &ast::Stmt<'_>, names: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_referenced_names_in_expr(expr, names);
            }
        }
        ast::Stmt::BlockStmt(block) => collect_referenced_names_in_block(block, names),
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    if let Some(values) = &value.values {
                        for expr in values {
                            collect_referenced_names_in_expr(expr, names);
                        }
                    }
                }
            }
        }
        ast::Stmt::ExprStmt(expr) => collect_referenced_names_in_expr(&expr.x, names),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_referenced_names_in_stmt(init, names);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_referenced_names_in_expr(cond, names);
            }
            if let Some(post) = &for_stmt.post {
                collect_referenced_names_in_stmt(post, names);
            }
            collect_referenced_names_in_block(&for_stmt.body, names);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_referenced_names_in_stmt(init, names);
            }
            collect_referenced_names_in_expr(&if_stmt.cond, names);
            collect_referenced_names_in_block(&if_stmt.body, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_referenced_names_in_stmt(else_branch, names);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => collect_referenced_names_in_expr(&inc_dec.x, names),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_referenced_names_in_expr(key, names);
            }
            if let Some(value) = &range.value {
                collect_referenced_names_in_expr(value, names);
            }
            collect_referenced_names_in_expr(&range.x, names);
            collect_referenced_names_in_block(&range.body, names);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_referenced_names_in_expr(expr, names);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_referenced_names_in_expr(&send.chan, names);
            collect_referenced_names_in_expr(&send.value, names);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_referenced_names_in_stmt(init, names);
            }
            if let Some(tag) = &switch.tag {
                collect_referenced_names_in_expr(tag, names);
            }
            collect_referenced_names_in_block(&switch.body, names);
        }
        ast::Stmt::LabeledStmt(label) => collect_referenced_names_in_stmt(&label.stmt, names),
        _ => {}
    }
}

fn collect_referenced_names_in_expr(expr: &ast::Expr<'_>, names: &mut BTreeSet<String>) {
    match expr {
        ast::Expr::Ident(ident) => {
            names.insert(ident.name.to_string());
        }
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_referenced_names_in_expr(len, names);
            }
            collect_referenced_names_in_expr(&array.elt, names);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_referenced_names_in_expr(&binary.x, names);
            collect_referenced_names_in_expr(&binary.y, names);
        }
        ast::Expr::CallExpr(call) => {
            collect_referenced_names_in_expr(&call.fun, names);
            if let Some(args) = &call.args {
                for arg in args {
                    collect_referenced_names_in_expr(arg, names);
                }
            }
        }
        ast::Expr::CompositeLit(comp) => {
            if let Some(ty) = &comp.type_ {
                collect_referenced_names_in_expr(ty, names);
            }
            if let Some(elts) = &comp.elts {
                for elt in elts {
                    collect_referenced_names_in_expr(elt, names);
                }
            }
        }
        ast::Expr::FuncLit(_) => {}
        ast::Expr::IndexExpr(index) => {
            collect_referenced_names_in_expr(&index.x, names);
            collect_referenced_names_in_expr(&index.index, names);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_referenced_names_in_expr(&index.x, names);
            for index in &index.indices {
                collect_referenced_names_in_expr(index, names);
            }
        }
        ast::Expr::KeyValueExpr(kv) => {
            collect_referenced_names_in_expr(&kv.key, names);
            collect_referenced_names_in_expr(&kv.value, names);
        }
        ast::Expr::ParenExpr(paren) => collect_referenced_names_in_expr(&paren.x, names),
        ast::Expr::SelectorExpr(selector) => collect_referenced_names_in_expr(&selector.x, names),
        ast::Expr::SliceExpr(slice) => {
            collect_referenced_names_in_expr(&slice.x, names);
            if let Some(low) = &slice.low {
                collect_referenced_names_in_expr(low, names);
            }
            if let Some(high) = &slice.high {
                collect_referenced_names_in_expr(high, names);
            }
            if let Some(max) = &slice.max {
                collect_referenced_names_in_expr(max, names);
            }
        }
        ast::Expr::StarExpr(star) => collect_referenced_names_in_expr(&star.x, names),
        ast::Expr::TypeAssertExpr(assert) => collect_referenced_names_in_expr(&assert.x, names),
        ast::Expr::UnaryExpr(unary) => collect_referenced_names_in_expr(&unary.x, names),
        ast::Expr::BasicLit(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::Ellipsis(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => {}
    }
}

fn ident_name(expr: &ast::Expr<'_>) -> Option<String> {
    if let ast::Expr::Ident(ident) = expr
        && ident.name != "_"
    {
        return Some(ident.name.to_string());
    }
    None
}

fn is_predeclared_name(name: &str) -> bool {
    matches!(
        name,
        "_" | "nil"
            | "true"
            | "false"
            | "iota"
            | "append"
            | "cap"
            | "clear"
            | "close"
            | "complex"
            | "copy"
            | "delete"
            | "imag"
            | "len"
            | "make"
            | "max"
            | "min"
            | "new"
            | "panic"
            | "print"
            | "println"
            | "real"
            | "recover"
    )
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    use super::{Addressability, CaptureMode, Completion, ExprKind, Item, Stmt, lower_file};
    use crate::compiler::typeinfer::{GoType, TypeEnv};
    use crate::parser::parse_file;
    use std::collections::BTreeMap;

    fn lower(source: &str) -> super::File {
        let file = parse_file("test.go", source).unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        lower_file(&file, &env)
    }

    #[test]
    fn lower_file_records_function_signatures() {
        let ir = lower(
            r#"
                package main

                func sum(label string, nums ...int) int {
                    return 0
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        assert_eq!(func.name.as_deref(), Some("sum"));
        assert_eq!(func.signature.variadic_start, Some(1));
        assert_eq!(func.signature.params.len(), 2);
        assert_eq!(func.signature.results.len(), 1);
    }

    fn invalid_signature(source: &str) -> Option<super::InvalidSignature> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_signature_in_file(&file)
    }

    fn invalid_receiver_type(source: &str) -> Option<super::InvalidSignature> {
        let file = parse_file("test.go", source).unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        super::invalid_receiver_type_in_file(&file, &env)
    }

    #[test]
    fn rejects_invalid_function_signatures() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func dup(a int, a int) {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateName {
                name: "a".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func variadic(nums ...int, label string) {}
                "#,
            ),
            Some(super::InvalidSignature::VariadicNotFinal)
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func result() (...int) { return nil }
                "#,
            ),
            Some(super::InvalidSignature::VariadicResult)
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type T int
                    func (a, b T) bad() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverCount { count: 2 })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type T int
                    func (T) Generic[A any]() {}
                "#,
            ),
            Some(super::InvalidSignature::MethodTypeParams { count: 1 })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    var _ = func(a int, a int) {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateName {
                name: "a".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type I interface {
                        M()
                        M()
                    }
                "#,
            ),
            Some(super::InvalidSignature::DuplicateInterfaceMethod {
                name: "M".to_string(),
            })
        );
    }

    #[test]
    fn rejects_invalid_type_parameter_declarations() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func f[T any, T comparable]() {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateTypeParameterName {
                name: "T".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type Box[T any, T comparable] struct{}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateTypeParameterName {
                name: "T".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func f[T]() {}
                "#,
            ),
            Some(super::InvalidSignature::InvalidTypeParameterDecl)
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func f[T any](T T) {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateName {
                name: "T".to_string(),
            })
        );
    }

    #[test]
    fn validates_receiver_type_parameter_declarations() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair[A, A]) M() {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateTypeParameterName {
                name: "A".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair[*int, string]) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverTypeParameterNotIdentifier)
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair[A, B]) M(A int) {}
                "#,
            ),
            Some(super::InvalidSignature::DuplicateName {
                name: "A".to_string(),
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    type Pair[A, B any] struct{ a A }
                    func (p Pair[First, _]) First() First {
                        return p.a
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_init_declarations() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func init(x int) {}
                "#,
            ),
            Some(super::InvalidSignature::InitFunction {
                type_params: 0,
                params: 1,
                results: 0,
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func init[T any]() {}
                "#,
            ),
            Some(super::InvalidSignature::InitFunction {
                type_params: 1,
                params: 0,
                results: 0,
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func init() int {
                        return 0
                    }
                "#,
            ),
            Some(super::InvalidSignature::InitFunction {
                type_params: 0,
                params: 0,
                results: 1,
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    var init int
                "#,
            ),
            Some(super::InvalidDeclaration::InvalidInitIdentifier)
        );
    }

    #[test]
    fn accepts_multiple_init_functions() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func init() {}
                    func init() {}
                "#,
            ),
            None
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func init() {}
                    func init() {}
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_main_function_signatures() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func main(x int) {}
                "#,
            ),
            Some(super::InvalidSignature::MainFunction {
                type_params: 0,
                params: 1,
                results: 0,
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func main[T any]() {}
                "#,
            ),
            Some(super::InvalidSignature::MainFunction {
                type_params: 1,
                params: 0,
                results: 0,
            })
        );
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func main() int {
                        return 0
                    }
                "#,
            ),
            Some(super::InvalidSignature::MainFunction {
                type_params: 0,
                params: 0,
                results: 1,
            })
        );
    }

    #[test]
    fn accepts_main_named_function_in_non_main_package() {
        assert_eq!(
            invalid_signature(
                r#"
                    package helper

                    func main(x int) int {
                        return x
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_receiver_base_types() {
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    func (n int) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("int".to_string()),
                reason: super::InvalidReceiverTypeReason::Undefined,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    func (s []int) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: None,
                reason: super::InvalidReceiverTypeReason::Unnamed,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type I interface{}
                    func (I) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("I".to_string()),
                reason: super::InvalidReceiverTypeReason::Interface,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type P *int
                    func (P) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("P".to_string()),
                reason: super::InvalidReceiverTypeReason::Pointer,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair[A]) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverTypeParameterCount {
                base: "Pair".to_string(),
                expected: 2,
                got: 1,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverTypeParameterCount {
                base: "Pair".to_string(),
                expected: 2,
                got: 0,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Number int
                    func (n Number[T]) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverTypeParameterCount {
                base: "Number".to_string(),
                expected: 0,
                got: 1,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Point struct{}
                    type GenericAlias[P any] = Point
                    func (p GenericAlias[P]) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("GenericAlias".to_string()),
                reason: super::InvalidReceiverTypeReason::GenericAlias,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    type InstantiatedPair = Pair[int, string]
                    func (p InstantiatedPair) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("InstantiatedPair".to_string()),
                reason: super::InvalidReceiverTypeReason::InstantiatedAlias,
            })
        );
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    type InstantiatedPair = Pair[int, string]
                    type Indirect = *InstantiatedPair
                    func (p Indirect) M() {}
                "#,
            ),
            Some(super::InvalidSignature::ReceiverType {
                base: Some("Indirect".to_string()),
                reason: super::InvalidReceiverTypeReason::InstantiatedAlias,
            })
        );
    }

    #[test]
    fn accepts_defined_receiver_base_types() {
        assert_eq!(
            invalid_receiver_type(
                r#"
                    package main

                    func (S) M() {}
                    type S struct{}
                    type N int
                    func (*N) N() {}
                    type Pair[A, B any] struct{}
                    func (p Pair[A, B]) Pair() {}
                    type Alias = S
                    func (a Alias) Alias() {}
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_blank_signature_names_and_valid_variadic() {
        assert_eq!(
            invalid_signature(
                r#"
                    package main

                    func ok(_ int, _ string, nums ...int) (_ int, _ bool) {
                        return 0, true
                    }
                "#,
            ),
            None
        );
    }

    fn invalid_declaration(source: &str) -> Option<super::InvalidDeclaration> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_declaration_in_file(&file)
    }

    fn invalid_declaration_with_import_package_names(
        source: &str,
        import_package_names: BTreeMap<String, String>,
    ) -> Option<super::InvalidDeclaration> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_declaration_in_file_with_import_package_names(&file, &import_package_names)
    }

    fn invalid_unused_import(
        source: &str,
        import_package_names: BTreeMap<String, String>,
    ) -> Option<super::InvalidDeclaration> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_unused_import_in_file_with_import_package_names(&file, &import_package_names)
    }

    fn invalid_unused_local(source: &str) -> Option<super::InvalidDeclaration> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_unused_local_in_file(&file)
    }

    fn invalid_declaration_in_merged_files(
        first_path: &'static str,
        first_source: &'static str,
        second_path: &'static str,
        second_source: &'static str,
    ) -> Option<super::InvalidDeclaration> {
        let mut first = parse_file(first_path, first_source).unwrap();
        let second = parse_file(second_path, second_source).unwrap();
        first.decls.extend(second.decls);
        super::invalid_declaration_in_file(&first)
    }

    fn invalid_value_declaration(source: &str) -> Option<super::InvalidDeclaration> {
        let file = parse_file("test.go", source).unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        super::invalid_value_declaration_in_file(&file, &env)
    }

    fn invalid_main_statement(source: &str) -> Option<super::InvalidStatement> {
        let file = parse_file("test.go", source).unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env)
    }

    #[test]
    fn rejects_blank_package_name() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package _

                    func main() {}
                "#,
            ),
            Some(super::InvalidDeclaration::InvalidPackageName {
                name: "_".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_struct_fields_and_methods() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type S struct {
                        X int
                        X string
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateStructField {
                type_name: Some("S".to_string()),
                field: "X".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type Inner struct{}
                    type S struct {
                        Inner
                        *Inner
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateStructField {
                type_name: Some("S".to_string()),
                field: "Inner".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type S struct{}
                    func (S) M() {}
                    func (*S) M() {}
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateMethod {
                base: "S".to_string(),
                method: "M".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type S struct { M int }
                    func (S) M() {}
                "#,
            ),
            Some(super::InvalidDeclaration::MethodFieldConflict {
                base: "S".to_string(),
                name: "M".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_top_level_declarations() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    var X int
                    const X = 1
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateTopLevelName {
                name: "X".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type T int
                    func T() {}
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateTopLevelName {
                name: "T".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    var A, A int
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateTopLevelName {
                name: "A".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_names_in_declaration_groups() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        var x, x int
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateDeclarationName {
                name: "x".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        const (
                            A = 1
                            A = 2
                        )
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateDeclarationName {
                name: "A".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        type (
                            T int
                            T string
                        )
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateDeclarationName {
                name: "T".to_string(),
            })
        );
    }

    #[test]
    fn rejects_duplicate_local_declarations_in_same_block() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f(x int) {
                        var x int
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateLexicalName {
                name: "x".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        var x int
                        const x = 1
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateLexicalName {
                name: "x".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        _ = func(x int) {
                            type x int
                        }
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateLexicalName {
                name: "x".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f[T any]() {
                        var T int
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateLexicalName {
                name: "T".to_string(),
            })
        );
    }

    #[test]
    fn accepts_shadowed_and_short_redeclared_local_names() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f() {
                        var x int
                        {
                            var x int
                            _ = x
                        }
                        x, y := 1, 2
                        _, _ = x, y
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_type_declarations_defined_from_type_parameters() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type T[P any] P
                "#,
            ),
            Some(super::InvalidDeclaration::TypeDefinitionFromTypeParameter {
                name: "P".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type A[P any] = P
                "#,
            ),
            Some(super::InvalidDeclaration::AliasToOwnTypeParameter {
                name: "P".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f[P any]() {
                        type L P
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::TypeDefinitionFromTypeParameter {
                name: "P".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type Pair[A, B any] struct{}
                    func (p Pair[A, B]) M() {
                        type L A
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::TypeDefinitionFromTypeParameter {
                name: "A".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f[P any]() {
                        type A = P
                    }
                "#,
            ),
            None
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    func f[P any]() {
                        type S struct{ value P }
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_explicit_import_names() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import (
                        f "fmt"
                        f "math"
                    )
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateImportName {
                name: "f".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import f "fmt"
                    import f "math"
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateImportName {
                name: "f".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import f "fmt"

                    var f int
                "#,
            ),
            Some(super::InvalidDeclaration::ImportPackageBlockConflict {
                name: "f".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import f "fmt"

                    func f() {}
                "#,
            ),
            Some(super::InvalidDeclaration::ImportPackageBlockConflict {
                name: "f".to_string(),
            })
        );
    }

    #[test]
    fn rejects_invalid_implicit_import_names() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import "fmt"
                    import "fmt"
                "#,
            ),
            Some(super::InvalidDeclaration::DuplicateImportName {
                name: "fmt".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import "fmt"

                    var fmt int
                "#,
            ),
            Some(super::InvalidDeclaration::ImportPackageBlockConflict {
                name: "fmt".to_string(),
            })
        );
        assert_eq!(
            invalid_declaration_with_import_package_names(
                r#"
                    package main

                    import "crypto/rand"
                    import "math/rand/v2"
                "#,
                BTreeMap::from([
                    ("crypto/rand".to_string(), "rand".to_string()),
                    ("math/rand/v2".to_string(), "rand".to_string()),
                ]),
            ),
            Some(super::InvalidDeclaration::DuplicateImportName {
                name: "rand".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unused_normal_imports() {
        assert_eq!(
            invalid_unused_import(
                r#"
                    package main

                    import "fmt"

                    func main() {}
                "#,
                BTreeMap::new(),
            ),
            Some(super::InvalidDeclaration::UnusedImport {
                path: "fmt".to_string(),
                alias: None,
            })
        );
        assert_eq!(
            invalid_unused_import(
                r#"
                    package main

                    import f "fmt"

                    func main() {}
                "#,
                BTreeMap::new(),
            ),
            Some(super::InvalidDeclaration::UnusedImport {
                path: "fmt".to_string(),
                alias: Some("f".to_string()),
            })
        );
        assert_eq!(
            invalid_unused_import(
                r#"
                    package main

                    import "math/rand/v2"

                    func main() {}
                "#,
                BTreeMap::from([("math/rand/v2".to_string(), "rand".to_string())]),
            ),
            Some(super::InvalidDeclaration::UnusedImport {
                path: "math/rand/v2".to_string(),
                alias: Some("rand".to_string()),
            })
        );
        assert_eq!(
            invalid_unused_import(
                r#"
                    package main

                    import "fmt"

                    func main() {
                        fmt.Println("ok")
                    }
                "#,
                BTreeMap::new(),
            ),
            None
        );
    }

    #[test]
    fn rejects_blank_identifier_as_value_or_type() {
        for source in [
            r#"
                package main

                func main() {
                    println(_)
                }
            "#,
            r#"
                package main

                func main() {
                    var x _
                    _ = x
                }
            "#,
            r#"
                package main

                func main() {
                    xs := []int{1}
                    _ = xs[_]
                }
            "#,
        ] {
            assert_eq!(
                invalid_main_statement(source),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::BlankIdentifier,
                })
            );
        }
    }

    #[test]
    fn accepts_blank_identifier_assignment_targets() {
        assert_eq!(
            invalid_main_statement(
                r#"
                    package main

                    func main() {
                        _ = 1
                        _, x := 1, 2
                        _ = x
                        xs := []int{1}
                        for _ = range xs {
                        }
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_unused_local_variables() {
        assert_eq!(
            invalid_unused_local(
                r#"
                    package main

                    func main() {
                        var x int
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::UnusedVariable {
                name: "x".to_string(),
            })
        );
        assert_eq!(
            invalid_unused_local(
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        for i := range xs {
                        }
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::UnusedVariable {
                name: "i".to_string(),
            })
        );
        assert_eq!(
            invalid_unused_local(
                r#"
                    package main

                    func main() {
                        x := 1
                        x = 2
                    }
                "#,
            ),
            Some(super::InvalidDeclaration::UnusedVariable {
                name: "x".to_string(),
            })
        );
    }

    #[test]
    fn accepts_used_local_variables() {
        assert_eq!(
            invalid_unused_local(
                r#"
                    package main

                    func f(unusedParam int) {}

                    func main() {
                        xs := []int{1}
                        for i := range xs {
                            _ = i
                        }
                        n := 0
                        n++
                        total := 1
                        total += n
                        _ = func() int {
                            return total
                        }
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_same_explicit_import_alias_in_different_files() {
        assert_eq!(
            invalid_declaration_in_merged_files(
                "a.go",
                r#"
                    package main

                    import f "fmt"
                "#,
                "b.go",
                r#"
                    package main

                    import f "math"
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_blank_and_dot_import_names_for_file_block_validation() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    import (
                        _ "fmt"
                        _ "math"
                        . "strings"
                        . "bytes"
                    )
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_blank_and_method_names_in_top_level_declarations() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    var _ int
                    const _ = 1
                    type T int
                    func (T) T() {}
                    func (T) _() {}
                "#,
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_value_declaration_counts() {
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const A, B = 1
                "#,
            ),
            Some(super::InvalidDeclaration::ConstValueCount {
                names: 2,
                values: 1,
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const (
                        A = 1
                        B, C
                    )
                "#,
            ),
            Some(super::InvalidDeclaration::ConstValueCount {
                names: 2,
                values: 1,
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const (
                        A
                    )
                "#,
            ),
            Some(super::InvalidDeclaration::MissingConstInitializer)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    var X = pair()
                "#,
            ),
            Some(super::InvalidDeclaration::VarValueCount {
                names: 1,
                values: 2,
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    var X, Y, Z = pair()
                "#,
            ),
            Some(super::InvalidDeclaration::VarValueCount {
                names: 3,
                values: 2,
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    var X, Y, Z = pair(), 3
                "#,
            ),
            Some(super::InvalidDeclaration::VarMultiValueInSingleValueContext)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var X int = "go"
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "int".to_string(),
                actual: "string".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var I int
                    var F float64 = I
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "float64".to_string(),
                actual: "int".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var I int = 1.5
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "int".to_string(),
                actual: "float64".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    func pair() (int, string) { return 1, "go" }

                    var X, Y int = pair()
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "int".to_string(),
                actual: "string".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    func f() int { return 1 }

                    const X = f()
                "#,
            ),
            Some(super::InvalidDeclaration::ConstNonConstantInitializer)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const X = []byte("go")
                "#,
            ),
            Some(super::InvalidDeclaration::ConstNonConstantInitializer)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const X = 1 / 0
                "#,
            ),
            Some(super::InvalidDeclaration::ConstInvalidInitializer {
                reason: "division by zero constant".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const X = 1 % -0.0
                "#,
            ),
            Some(super::InvalidDeclaration::ConstInvalidInitializer {
                reason: "division by zero constant".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var X = nil
                "#,
            ),
            Some(super::InvalidDeclaration::VarUntypedNil)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var X int = nil
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "int".to_string(),
                actual: "nil".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    type S struct{}

                    var X S = nil
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "S".to_string(),
                actual: "nil".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var y = 1
                    const X = y
                "#,
            ),
            Some(super::InvalidDeclaration::ConstNonConstantInitializer)
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const X int = "go"
                "#,
            ),
            Some(super::InvalidDeclaration::ConstTypeMismatch {
                expected: "int".to_string(),
                actual: "string".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var B byte = 256
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "uint8".to_string(),
                actual: "int".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var B byte = 256.0
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "uint8".to_string(),
                actual: "float64".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var B byte = '\u0100'
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "uint8".to_string(),
                actual: "int32".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var F float64 = 1e1000
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "float64".to_string(),
                actual: "float64".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var F float32 = 1e1000
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "float32".to_string(),
                actual: "float64".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var A any = 1e1000
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "interface".to_string(),
                actual: "float64".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var I int = 1i
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "int".to_string(),
                actual: "complex128".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    var U uint = -1
                "#,
            ),
            Some(super::InvalidDeclaration::VarTypeMismatch {
                expected: "uint".to_string(),
                actual: "int".to_string(),
            })
        );
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    const X bool = 1
                "#,
            ),
            Some(super::InvalidDeclaration::ConstTypeMismatch {
                expected: "bool".to_string(),
                actual: "int".to_string(),
            })
        );
    }

    #[test]
    fn accepts_valid_value_declaration_counts() {
        assert_eq!(
            invalid_value_declaration(
                r#"
                    package main

                    type NamedIface interface{}

                    func pair() (int, int) { return 1, 2 }

                    const (
                        A, B = 1, 2
                        C, D
                    )
                    const U, V float32 = 0, 3
                    const Converted = int(1)

                    var X, Y = pair()
                    var Z int
                    var S string = "go"
                    var F float64 = 1
                    var I int = 1.0
                    var B byte = 255
                    var BFloat byte = 255.0
                    var BRune byte = '\xff'
                    var Thousand int = 1e3
                    var ZeroImagInt int = 0i
                    var ZeroImagFloat float64 = 0i
                    var UnderflowFloat float64 = -1e-1000
                    var UnderflowAny any = -1e-1000
                    var Small int8 = -128
                    var C complex128 = 1
                    var P *int = nil
                    var Slice []int = nil
                    var Map map[string]int = nil
                    var Ch chan int = nil
                    var Fn func() = nil
                    var Iface any = nil
                    var Named NamedIface = nil
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_blank_struct_fields() {
        assert_eq!(
            invalid_declaration(
                r#"
                    package main

                    type S struct {
                        _ int
                        _ string
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn lower_expr_records_addressability() {
        let ir = lower(
            r#"
                package main

                func main() {
                    xs := []int{1, 2}
                    _ = xs[0]
                    _ = 1 + 2
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected index assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected rhs");
        };
        assert_eq!(expr.addressability, Addressability::Addressable);
        assert!(matches!(expr.kind, ExprKind::Index { .. }));
    }

    #[test]
    fn lower_expr_marks_map_indexes_not_addressable() {
        let ir = lower(
            r#"
                package main

                var m map[string]int

                func main() {
                    _ = m["k"]
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.get(1) else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.first() else {
            panic!("expected assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected rhs");
        };
        assert_eq!(expr.addressability, Addressability::NotAddressable);
        assert!(matches!(expr.kind, ExprKind::Index { .. }));
    }

    #[test]
    fn lower_expr_uses_spec_addressability_for_selectors_and_arrays() {
        let ir = lower(
            r#"
                package main

                type S struct { X int }
                var arr [1]int

                func main() {
                    s := S{X: 1}
                    _ = s.X
                    _ = S{X: 1}.X
                    _ = arr[0]
                    _ = [1]int{1}[0]
                }
            "#,
        );
        let Some(func) = ir.items.iter().find_map(|item| match item {
            Item::Func(func) if func.name.as_deref() == Some("main") => Some(func),
            Item::Func(_) | Item::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };

        let addressability = |index| {
            let Some(Stmt::Assign(assign)) = body.stmts.get(index) else {
                panic!("expected assignment at {index}");
            };
            assign
                .rhs
                .first()
                .map(|expr| expr.addressability)
                .expect("expected rhs")
        };

        assert_eq!(addressability(1), Addressability::Addressable);
        assert_eq!(addressability(2), Addressability::NotAddressable);
        assert_eq!(addressability(3), Addressability::Addressable);
        assert_eq!(addressability(4), Addressability::NotAddressable);
    }

    #[test]
    fn lower_expr_marks_constants_and_builtins_not_addressable() {
        let ir = lower(
            r#"
                package main

                const c = 1

                func main() {
                    _ = c
                    _ = len
                }
            "#,
        );
        let Some(func) = ir.items.iter().find_map(|item| match item {
            Item::Func(func) if func.name.as_deref() == Some("main") => Some(func),
            Item::Func(_) | Item::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        for index in [0, 1] {
            let Some(Stmt::Assign(assign)) = body.stmts.get(index) else {
                panic!("expected assignment at {index}");
            };
            let Some(expr) = assign.rhs.first() else {
                panic!("expected rhs");
            };
            assert_eq!(expr.addressability, Addressability::NotAddressable);
        }
    }

    #[test]
    fn expr_addressability_allows_shadowed_predeclared_names() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = len
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(crate::ast::Stmt::AssignStmt(assign)) =
            func.body.as_ref().and_then(|body| body.list.first())
        else {
            panic!("expected assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected rhs");
        };

        let mut env = TypeEnv::new();
        assert_eq!(
            super::expr_addressability(expr, &env),
            Addressability::NotAddressable
        );
        env.set_var("len", GoType::Int);
        assert_eq!(
            super::expr_addressability(expr, &env),
            Addressability::Addressable
        );
    }

    #[test]
    fn lower_block_tracks_local_define_bindings_for_addressability() {
        let ir = lower(
            r#"
                package main

                func main() {
                    len := 1
                    _ = len
                }
            "#,
        );
        let Some(func) = ir.items.iter().find_map(|item| match item {
            Item::Func(func) if func.name.as_deref() == Some("main") => Some(func),
            Item::Func(_) | Item::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected rhs");
        };
        assert_eq!(expr.addressability, Addressability::Addressable);
    }

    #[test]
    fn classifies_string_concat_from_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                const prefix = "a"
                const suffix = "b"

                func main() {
                    _ = prefix + suffix
                    _ = 1 + 2
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(crate::ast::Stmt::AssignStmt(string_assign)) =
            func.body.as_ref().and_then(|body| body.list.first())
        else {
            panic!("expected string assignment");
        };
        let Some(crate::ast::Expr::BinaryExpr(string_binary)) = string_assign.rhs.first() else {
            panic!("expected string binary expression");
        };
        assert!(super::is_string_concat_binary_expr(string_binary, &env));

        let Some(crate::ast::Stmt::AssignStmt(numeric_assign)) =
            func.body.as_ref().and_then(|body| body.list.get(1))
        else {
            panic!("expected numeric assignment");
        };
        let Some(crate::ast::Expr::BinaryExpr(numeric_binary)) = numeric_assign.rhs.first() else {
            panic!("expected numeric binary expression");
        };
        assert!(!super::is_string_concat_binary_expr(numeric_binary, &env));
    }

    #[test]
    fn classifies_predeclared_builtin_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func helper() int { return 1 }

                func main() {
                    _ = len([]int{1})
                    _ = helper()
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let Some(crate::ast::Stmt::AssignStmt(builtin_assign)) =
            func.body.as_ref().and_then(|body| body.list.first())
        else {
            panic!("expected builtin assignment");
        };
        let Some(crate::ast::Expr::CallExpr(builtin_call)) = builtin_assign.rhs.first() else {
            panic!("expected builtin call");
        };
        assert_eq!(
            super::builtin_call_kind(builtin_call),
            Some(super::BuiltinCallKind::Len)
        );

        let Some(crate::ast::Stmt::AssignStmt(user_assign)) =
            func.body.as_ref().and_then(|body| body.list.get(1))
        else {
            panic!("expected user assignment");
        };
        let Some(crate::ast::Expr::CallExpr(user_call)) = user_assign.rhs.first() else {
            panic!("expected user call");
        };
        assert_eq!(super::builtin_call_kind(user_call), None);
    }

    #[test]
    fn classifies_variadic_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func sum(nums ...int) int { return 0 }
                func add(a int, b int) int { return a + b }

                func main() {
                    _ = sum(1, 2)
                    _ = add(1, 2)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let Some(crate::ast::Stmt::AssignStmt(variadic_assign)) =
            func.body.as_ref().and_then(|body| body.list.first())
        else {
            panic!("expected variadic assignment");
        };
        let Some(crate::ast::Expr::CallExpr(variadic_call)) = variadic_assign.rhs.first() else {
            panic!("expected variadic call");
        };
        assert_eq!(super::variadic_call_start(variadic_call, &env), Some(0));

        let Some(crate::ast::Stmt::AssignStmt(fixed_assign)) =
            func.body.as_ref().and_then(|body| body.list.get(1))
        else {
            panic!("expected fixed assignment");
        };
        let Some(crate::ast::Expr::CallExpr(fixed_call)) = fixed_assign.rhs.first() else {
            panic!("expected fixed call");
        };
        assert_eq!(super::variadic_call_start(fixed_call, &env), None);
    }

    #[test]
    fn classifies_range_kinds() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    nums := []int{1}
                    dict := map[string]int{"a": 1}
                    ch := make(chan int)
                    iter := func(yield func(int) bool) {}
                    text := "go"
                    for range nums {}
                    for range dict {}
                    for range ch {}
                    for range iter {}
                    for range text {}
                    for range 3 {}
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        for stmt in func
            .body
            .as_ref()
            .expect("expected body")
            .list
            .iter()
            .take(5)
        {
            if let crate::ast::Stmt::AssignStmt(assign) = stmt
                && let Some(crate::ast::Expr::Ident(ident)) = assign.lhs.first()
                && let Some(value) = assign.rhs.first()
            {
                env.set_var(ident.name, GoType::infer_expr(value, &env));
            }
        }
        let kinds: Vec<_> = func
            .body
            .as_ref()
            .expect("expected body")
            .list
            .iter()
            .filter_map(|stmt| match stmt {
                crate::ast::Stmt::RangeStmt(range) => Some(super::range_kind(&range.x, &env)),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                super::RangeKind::Indexed,
                super::RangeKind::Map,
                super::RangeKind::Channel,
                super::RangeKind::Function,
                super::RangeKind::String,
                super::RangeKind::Integer,
            ]
        );
    }

    #[test]
    fn classifies_type_conversions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type MyInt int

                func helper(x int) int { return x }

                func main() {
                    _ = string(65)
                    _ = []byte("go")
                    _ = MyInt(1)
                    _ = helper(1)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected main function");
        };
        let calls: Vec<&crate::ast::CallExpr> = func
            .body
            .as_ref()
            .map(|body| {
                body.list
                    .iter()
                    .filter_map(|stmt| match stmt {
                        crate::ast::Stmt::AssignStmt(assign) => assign.rhs.first(),
                        _ => None,
                    })
                    .filter_map(|expr| match expr {
                        crate::ast::Expr::CallExpr(call) => Some(call),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let Some(string_call) = calls.first() else {
            panic!("expected string call");
        };
        assert_eq!(
            super::special_type_conversion(string_call),
            Some(super::SpecialTypeConversionKind::String)
        );
        let Some(byte_slice_call) = calls.get(1) else {
            panic!("expected byte slice call");
        };
        assert_eq!(
            super::special_type_conversion(byte_slice_call),
            Some(super::SpecialTypeConversionKind::ByteSlice)
        );
        let Some(named_call) = calls.get(2) else {
            panic!("expected named conversion call");
        };
        assert!(super::is_general_type_conversion_fun(&named_call.fun, &env));
        let Some(helper_call) = calls.get(3) else {
            panic!("expected helper call");
        };
        assert!(!super::is_general_type_conversion_fun(
            &helper_call.fun,
            &env
        ));
    }

    #[test]
    fn lower_func_lit_records_mutable_captures() {
        let ir = lower(
            r#"
                package main

                func main() {
                    count := 0
                    next := func() int {
                        count++
                        return count
                    }
                    _ = next
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected closure assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected closure rhs");
        };
        let ExprKind::FuncLit(func_lit) = &expr.kind else {
            panic!("expected function literal");
        };
        let Some(capture) = func_lit.captures.first() else {
            panic!("expected capture");
        };
        assert_eq!(capture.name, "count");
        assert_eq!(capture.mode, CaptureMode::BorrowMut);
        assert_eq!(capture.ty, GoType::Int);
    }

    #[test]
    fn lower_func_lit_records_read_only_captures() {
        let ir = lower(
            r#"
                package main

                func main() {
                    base := 10
                    add := func(x int) int {
                        return base + x
                    }
                    _ = add
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected closure assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected closure rhs");
        };
        let ExprKind::FuncLit(func_lit) = &expr.kind else {
            panic!("expected function literal");
        };
        let Some(capture) = func_lit.captures.first() else {
            panic!("expected capture");
        };
        assert_eq!(capture.name, "base");
        assert_eq!(capture.mode, CaptureMode::Borrow);
    }

    #[test]
    fn lower_func_lit_keeps_outer_capture_after_nested_shadow() {
        let ir = lower(
            r#"
                package main

                func main() {
                    base := 10
                    f := func() int {
                        if true {
                            base := 1
                            _ = base
                        }
                        return base
                    }
                    _ = f
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected closure assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected closure rhs");
        };
        let ExprKind::FuncLit(func_lit) = &expr.kind else {
            panic!("expected function literal");
        };
        assert_eq!(func_lit.captures.len(), 1);
        assert_eq!(func_lit.captures[0].name, "base");
        assert_eq!(func_lit.captures[0].mode, CaptureMode::Borrow);
    }

    #[test]
    fn lower_func_lit_ignores_fully_shadowed_outer_names() {
        let ir = lower(
            r#"
                package main

                func main() {
                    base := 10
                    f := func() int {
                        if true {
                            base := 1
                            return base
                        }
                        return 0
                    }
                    _ = f
                    _ = base
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected closure assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected closure rhs");
        };
        let ExprKind::FuncLit(func_lit) = &expr.kind else {
            panic!("expected function literal");
        };
        assert!(func_lit.captures.is_empty());
    }

    #[test]
    fn lower_func_lit_propagates_nested_closure_captures() {
        let ir = lower(
            r#"
                package main

                func main() {
                    count := 0
                    f := func() func() int {
                        return func() int {
                            count++
                            return count
                        }
                    }
                    _ = f
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Assign(assign)) = body.stmts.get(1) else {
            panic!("expected closure assignment");
        };
        let Some(expr) = assign.rhs.first() else {
            panic!("expected closure rhs");
        };
        let ExprKind::FuncLit(func_lit) = &expr.kind else {
            panic!("expected function literal");
        };
        assert_eq!(func_lit.captures.len(), 1);
        assert_eq!(func_lit.captures[0].name, "count");
        assert_eq!(func_lit.captures[0].mode, CaptureMode::BorrowMut);
    }

    #[test]
    fn records_mutable_function_literal_captures() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    done := make(chan bool)
                    count := 0
                    label := "ready"
                    go func() {
                        count = 7
                        _ = label
                        done <- true
                    }()
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let names = super::mutable_func_lit_capture_names_in_block(
            func.body.as_ref().expect("expected body"),
            &env,
        );
        assert!(names.contains("count"));
        assert!(!names.contains("done"));
        assert!(!names.contains("label"));
    }

    #[test]
    fn records_mutable_function_literal_captures_in_composite_literals() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Runner struct {
                    Run func()
                }

                func main() {
                    count := 0
                    _ = Runner{Run: func() {
                        count++
                    }}
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let names = super::mutable_func_lit_capture_names_in_block(
            func.body.as_ref().expect("expected body"),
            &env,
        );
        assert!(names.contains("count"));
    }

    #[test]
    fn records_mutable_range_function_captures() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func ints(yield func(int) bool) {}

                func main() {
                    total := 0
                    for v := range ints {
                        total += v
                        inner := 0
                        inner++
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            _ => None,
        }) else {
            panic!("expected function");
        };
        let names = super::mutable_range_function_capture_names_in_block(
            func.body.as_ref().expect("expected body"),
            &env,
        );
        assert!(names.contains("total"));
        assert!(!names.contains("v"));
        assert!(!names.contains("inner"));
    }

    #[test]
    fn records_named_return_range_function_captures() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func ints(yield func(int) bool) {}

                func first() (out int) {
                    for v := range ints {
                        out = v
                        return
                    }
                    return
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "first" => Some(func),
            _ => None,
        }) else {
            panic!("expected function");
        };
        let names = super::mutable_range_function_capture_names_in_block(
            func.body.as_ref().expect("expected body"),
            &env,
        );
        assert!(names.contains("out"));
        assert!(!names.contains("v"));
    }

    #[test]
    fn detects_ast_goto_to_label() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                Loop:
                    if true {
                        goto Loop
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(crate::ast::Stmt::LabeledStmt(labeled)) =
            func.body.as_ref().and_then(|body| body.list.first())
        else {
            panic!("expected labeled statement");
        };
        assert!(super::ast_stmt_has_goto_to_label(
            &labeled.stmt,
            labeled.label.name
        ));
    }

    #[test]
    fn plans_scope_safe_forward_goto_state_blocks() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    if true {
                        goto Done
                    }
                Done:
                    println("done")
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(plan) = super::goto_state_plan_for_block(func.body.as_ref().expect("body")) else {
            panic!("expected forward goto plan");
        };
        assert_eq!(plan.labels, vec!["Done"]);
        assert!(plan.hoisted_names.is_empty());
    }

    #[test]
    fn plans_forward_goto_state_local_hoists() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    x := 1
                    goto Done
                Done:
                    println(x)
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(plan) = super::goto_state_plan_for_block(func.body.as_ref().expect("body")) else {
            panic!("expected forward goto plan");
        };
        assert_eq!(plan.labels, vec!["Done"]);
        assert_eq!(plan.hoisted_names, vec!["x"]);
    }

    #[test]
    fn rejects_forward_goto_over_same_block_decl() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    goto Done
                    x := 1
                Done:
                    println(x)
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(invalid) = super::invalid_forward_goto_in_block(func.body.as_ref().expect("body"))
        else {
            panic!("expected invalid goto");
        };
        assert_eq!(
            invalid,
            super::InvalidGoto::SkipsDeclarations {
                label: "Done".to_string(),
                skipped_names: vec!["x".to_string()]
            }
        );
    }

    #[test]
    fn rejects_forward_goto_over_function_literal_decl() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = func() {
                        goto Done
                        x := 1
                    Done:
                        _ = x
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_forward_goto_in_func(func.body.as_ref().expect("body")),
            Some(super::InvalidGoto::SkipsDeclarations {
                label: "Done".to_string(),
                skipped_names: vec!["x".to_string()]
            })
        );
    }

    #[test]
    fn rejects_forward_goto_over_switch_clause_decl() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    switch 1 {
                    case 1:
                        goto Done
                        x := 1
                    Done:
                        _ = x
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_forward_goto_in_func(func.body.as_ref().expect("body")),
            Some(super::InvalidGoto::SkipsDeclarations {
                label: "Done".to_string(),
                skipped_names: vec!["x".to_string()]
            })
        );
    }

    #[test]
    fn rejects_goto_into_nested_block() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    goto Inside
                    if true {
                    Inside:
                        println("inside")
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(invalid) = super::invalid_goto_target_in_func(func.body.as_ref().expect("body"))
        else {
            panic!("expected invalid goto");
        };
        assert_eq!(
            invalid,
            super::InvalidGoto::EntersBlock {
                label: "Inside".to_string()
            }
        );
    }

    #[test]
    fn rejects_goto_to_undefined_label() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    goto Missing
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(invalid) = super::invalid_goto_target_in_func(func.body.as_ref().expect("body"))
        else {
            panic!("expected invalid goto");
        };
        assert_eq!(
            invalid,
            super::InvalidGoto::UndefinedLabel {
                label: "Missing".to_string()
            }
        );
    }

    #[test]
    fn treats_blank_labels_as_non_targets() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                _:
                _:
                    println("blank labels are ignored")
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let body = func.body.as_ref().expect("body");
        assert_eq!(super::invalid_label_in_func(body), None);
        assert_eq!(super::invalid_goto_target_in_func(body), None);
        assert_eq!(
            super::direct_label_names_in_stmt(&body.list[0]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rejects_goto_to_blank_label() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    goto _
                _:
                    println("not a target")
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_goto_target_in_func(func.body.as_ref().expect("body")),
            Some(super::InvalidGoto::UndefinedLabel {
                label: "_".to_string(),
            })
        );
    }

    #[test]
    fn rejects_goto_to_undefined_label_in_function_literal() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = func() {
                        goto Missing
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_goto_target_in_func(func.body.as_ref().expect("body")),
            Some(super::InvalidGoto::UndefinedLabel {
                label: "Missing".to_string()
            })
        );
    }

    #[test]
    fn rejects_function_literal_goto_to_outer_label() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                Outer:
                    _ = func() {
                        goto Outer
                    }
                    goto Outer
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_goto_target_in_func(func.body.as_ref().expect("body")),
            Some(super::InvalidGoto::UndefinedLabel {
                label: "Outer".to_string()
            })
        );
    }

    #[test]
    fn rejects_invalid_branch_statements() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        break
                    }
                "#,
                super::InvalidBranch::BreakOutside,
            ),
            (
                r#"
                    package main

                    func main() {
                        continue
                    }
                "#,
                super::InvalidBranch::ContinueOutside,
            ),
            (
                r#"
                    package main

                    func main() {
                    Done:
                        println("done")
                        for {
                            break Done
                        }
                    }
                "#,
                super::InvalidBranch::BreakLabel {
                    label: "Done".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                    Switch:
                        switch 1 {
                        default:
                            continue Switch
                        }
                    }
                "#,
                super::InvalidBranch::ContinueLabel {
                    label: "Switch".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case 1:
                            fallthrough
                            println("unreachable")
                        default:
                        }
                    }
                "#,
                super::InvalidBranch::FallthroughNotFinal,
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        default:
                            fallthrough
                        }
                    }
                "#,
                super::InvalidBranch::FallthroughInFinalCase,
            ),
            (
                r#"
                    package main

                    func main() {
                        var x any
                        switch x.(type) {
                        case int:
                            fallthrough
                        default:
                        }
                    }
                "#,
                super::InvalidBranch::FallthroughInTypeSwitch,
            ),
            (
                r#"
                    package main

                    func main() {
                        fallthrough
                    }
                "#,
                super::InvalidBranch::FallthroughOutsideSwitch,
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = func() {
                            break
                        }
                    }
                "#,
                super::InvalidBranch::BreakOutside,
            ),
        ];

        for (source, expected) in cases {
            let file = parse_file("test.go", source).unwrap();
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_branch_in_func(func.body.as_ref().expect("body")),
                Some(expected)
            );
        }
    }

    #[test]
    fn accepts_valid_labeled_branches_and_fallthrough() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                Loop:
                    for i := 0; i < 2; i++ {
                        switch i {
                        case 0:
                            fallthrough
                        default:
                            continue Loop
                        }
                        break Loop
                    }
                    select {
                    default:
                        break
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_branch_in_func(func.body.as_ref().expect("body")),
            None
        );
    }

    #[test]
    fn rejects_invalid_statement_context_expressions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        1 + 2
                    }
                "#,
                super::InvalidStatement::Expr {
                    reason: super::InvalidStatementReason::NonCallOrReceive,
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        len("go")
                    }
                "#,
                super::InvalidStatement::Expr {
                    reason: super::InvalidStatementReason::DisallowedBuiltin("len".to_string()),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        string(65)
                    }
                "#,
                super::InvalidStatement::Expr {
                    reason: super::InvalidStatementReason::TypeConversion,
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        go len("go")
                    }
                "#,
                super::InvalidStatement::Go {
                    reason: super::InvalidStatementReason::DisallowedBuiltin("len".to_string()),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        defer int(1)
                    }
                "#,
                super::InvalidStatement::Defer {
                    reason: super::InvalidStatementReason::TypeConversion,
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        for i := 0; i < 3; i := i + 1 {
                            _ = i
                        }
                    }
                "#,
                super::InvalidStatement::ForPostShortVarDecl,
            ),
        ];

        for (source, expected) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(expected)
            );
        }
    }

    #[test]
    fn rejects_invalid_statement_builtin_calls() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        clear(1)
                    }
                "#,
                "clear",
                "argument must have map or slice type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        clear(xs, xs)
                    }
                "#,
                "clear",
                "expects exactly one argument",
            ),
            (
                r#"
                    package main

                    func main() {
                        close(1)
                    }
                "#,
                "close",
                "argument must have channel type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var ch <-chan int
                        close(ch)
                    }
                "#,
                "close",
                "cannot close receive-only channel",
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int)
                        close(ch, ch)
                    }
                "#,
                "close",
                "expects exactly one argument",
            ),
            (
                r#"
                    package main

                    func main() {
                        delete(1, 2)
                    }
                "#,
                "delete",
                "first argument must have map type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        m := map[string]int{"x": 1}
                        delete(m, 1)
                    }
                "#,
                "delete",
                "key must be assignable to string, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        m := map[int]int{1: 1}
                        var f float64
                        delete(m, f)
                    }
                "#,
                "delete",
                "key must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        delete(map[int]int{}, nil)
                    }
                "#,
                "delete",
                "key must be assignable to int, got nil",
            ),
            (
                r#"
                    package main

                    func main() {
                        m := map[string]int{"x": 1}
                        delete(m)
                    }
                "#,
                "delete",
                "expects exactly two arguments",
            ),
            (
                r#"
                    package main

                    func main() {
                        panic()
                    }
                "#,
                "panic",
                "expects exactly one argument",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        println(xs...)
                    }
                "#,
                "println",
                "does not accept spread arguments",
            ),
        ];

        for (source, name, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expr {
                    reason: super::InvalidStatementReason::InvalidBuiltinCall {
                        name: name.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn rejects_invalid_expression_builtin_calls() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = len(1)
                    }
                "#,
                "len",
                "argument must have string, array, slice, map, or channel type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = cap("go")
                    }
                "#,
                "cap",
                "argument must have array, slice, or channel type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = copy(1, []int{})
                    }
                "#,
                "copy",
                "first argument must have slice type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = copy([]int{}, []string{})
                    }
                "#,
                "copy",
                "source element type must match int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = append(1, 2)
                    }
                "#,
                "append",
                "first argument must have slice type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = append([]int{}, "x")
                    }
                "#,
                "append",
                "argument must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var f float64
                        _ = append([]int{}, f)
                    }
                "#,
                "append",
                "argument must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = append([]int{}, nil)
                    }
                "#,
                "append",
                "argument must be assignable to int, got nil",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = append([]int{}, []string{}...)
                    }
                "#,
                "append",
                "spread argument element type must match int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make(int)
                    }
                "#,
                "make",
                "first argument must have slice, map, or channel type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make([]int)
                    }
                "#,
                "make",
                "slice make expects length and optional capacity",
            ),
            (
                r#"
                    package main

                    func main() {
                        n := "bad"
                        _ = make([]int, n)
                    }
                "#,
                "make",
                "size argument must have integer type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make([]int, 1.5)
                    }
                "#,
                "make",
                "size argument must have integer type, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make([]int, -1)
                    }
                "#,
                "make",
                "size argument must be non-negative",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make([]int, 10, 0)
                    }
                "#,
                "make",
                "length must not exceed capacity",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = make(map[string]int, 1, 2)
                    }
                "#,
                "make",
                "map make expects optional size hint",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = new(nil)
                    }
                "#,
                "new",
                "argument must not be nil",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = new(123)
                    }
                "#,
                "new",
                "argument must be a type",
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        _ = new(x)
                    }
                "#,
                "new",
                "argument must be a type",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = complex("x", 1)
                    }
                "#,
                "complex",
                "arguments must have floating-point type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = complex(float32(1), float64(2))
                    }
                "#,
                "complex",
                "arguments must have the same floating-point type, got float32 and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = real(1)
                    }
                "#,
                "real",
                "argument must have complex type, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = imag("x")
                    }
                "#,
                "imag",
                "argument must have complex type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = min()
                    }
                "#,
                "min",
                "expects at least one argument",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = max(true, false)
                    }
                "#,
                "max",
                "arguments must have ordered type, got bool",
            ),
            (
                r#"
                    package main

                    func main() {
                        var z complex128
                        _ = min(z, z)
                    }
                "#,
                "min",
                "arguments must have ordered type, got complex128",
            ),
            (
                r#"
                    package main

                    const z = 1i

                    func main() {
                        _ = max(z, z)
                    }
                "#,
                "max",
                "arguments must have ordered type, got complex128",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = max("x", 1)
                    }
                "#,
                "max",
                "arguments must be all numeric or all string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        _ = max(i, f)
                    }
                "#,
                "max",
                "arguments have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        _ = min(i, 1.5)
                    }
                "#,
                "min",
                "arguments have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = recover(1)
                    }
                "#,
                "recover",
                "expects no arguments",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = clear([]int{})
                    }
                "#,
                "clear",
                "does not produce a value",
            ),
            (
                r#"
                    package main

                    func main() {
                        var ch chan int
                        _ = close(ch)
                    }
                "#,
                "close",
                "does not produce a value",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = delete(map[int]int{}, 1)
                    }
                "#,
                "delete",
                "does not produce a value",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = panic("boom")
                    }
                "#,
                "panic",
                "does not produce a value",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = print("boom")
                    }
                "#,
                "print",
                "does not produce a value",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = println("boom")
                    }
                "#,
                "println",
                "does not produce a value",
            ),
        ];

        for (source, name, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidBuiltinCall {
                        name: name.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn rejects_invalid_ordinary_function_calls() {
        let cases = vec![
            (
                r#"
                    package main

                    func takes(a int, b string) {}

                    func main() {
                        takes(1)
                    }
                "#,
                "takes",
                "expects 2 argument(s), got 1",
            ),
            (
                r#"
                    package main

                    func takes(a int, b string) {}

                    func main() {
                        takes(1, 2)
                    }
                "#,
                "takes",
                "argument 2 must be assignable to string, got int",
            ),
            (
                r#"
                    package main

                    func takes(a int) {}

                    func main() {
                        takes(nil)
                    }
                "#,
                "takes",
                "argument 1 must be assignable to int, got nil",
            ),
            (
                r#"
                    package main

                    func takes(a int) {}

                    func main() {
                        var f float64
                        takes(f)
                    }
                "#,
                "takes",
                "argument 1 must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func takes(a int) {}

                    func main() {
                        takes(1.5)
                    }
                "#,
                "takes",
                "argument 1 must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func takes(a any) {}

                    func main() {
                        takes(1e1000)
                    }
                "#,
                "takes",
                "argument 1 must be assignable to interface, got float64",
            ),
            (
                r#"
                    package main

                    func pair() (int, string) { return 1, "go" }
                    func takesInts(a int, b int) {}

                    func main() {
                        takesInts(pair())
                    }
                "#,
                "takesInts",
                "argument 2 must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    func sink(prefix string, values ...string) {}

                    func main() {
                        sink()
                    }
                "#,
                "sink",
                "expects at least 1 argument(s), got 0",
            ),
            (
                r#"
                    package main

                    func sink(prefix string, values ...string) {}

                    func main() {
                        sink("go", 1)
                    }
                "#,
                "sink",
                "argument 2 must be assignable to string, got int",
            ),
            (
                r#"
                    package main

                    func sink(prefix string, values ...string) {}

                    func main() {
                        sink("go", nil)
                    }
                "#,
                "sink",
                "argument 2 must be assignable to string, got nil",
            ),
            (
                r#"
                    package main

                    func takes(a int, b string) {}

                    func main() {
                        xs := []string{}
                        takes(1, xs...)
                    }
                "#,
                "takes",
                "cannot use spread arguments with non-variadic call",
            ),
            (
                r#"
                    package main

                    func sink(prefix string, values ...string) {}

                    func main() {
                        xs := []string{}
                        sink("go", "rs", xs...)
                    }
                "#,
                "sink",
                "spread call expects 2 argument(s), got 3",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = func(a int) int { return a }("go")
                    }
                "#,
                "function literal",
                "argument 1 must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = func(a int) int { return a }(nil)
                    }
                "#,
                "function literal",
                "argument 1 must be assignable to int, got nil",
            ),
        ];

        for (source, target, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidCall {
                        target: target.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_ordinary_function_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func pair() (int, string) { return 1, "go" }
                func source() (string, string) { return "go", "rs" }
                func takes(a int, b string) {}
                func sink(prefix string, values ...string) {}
                func noArgs() {}
                func len(a int) {}
                func takesNilables(p *int, xs []int, m map[string]int, ch chan int, fn func(), v any) {}

                func main() {
                    takes(pair())
                    sink("go")
                    sink("go", "rs", "lang")
                    xs := []string{}
                    sink("go", xs...)
                    sink(source())
                    noArgs()
                    len(1)
                    takesNilables(nil, nil, nil, nil, nil, nil)
                    f := func(a int) {}
                    f(1)
                    g := func(a *int) {}
                    g(nil)
                    _ = func(a int) int { return a }(1)
                    _ = func(a any) any { return a }(nil)
                    _ = func(a any) any { return a }(-1e-1000)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_type_conversion_calls() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = int()
                    }
                "#,
                "int",
                "expects 1 argument, got 0",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = int(1, 2)
                    }
                "#,
                "int",
                "expects 1 argument, got 2",
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func main() {
                        _ = int(pair())
                    }
                "#,
                "int",
                "expects 1 argument, got 2 values",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{}
                        _ = int(xs...)
                    }
                "#,
                "int",
                "cannot use spread arguments",
            ),
            (
                r#"
                    package main

                    func main() {
                        var s string
                        _ = int(s)
                    }
                "#,
                "int",
                "cannot convert string to int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = bool(1)
                    }
                "#,
                "bool",
                "cannot convert int to bool",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = string(true)
                    }
                "#,
                "string",
                "cannot convert bool to string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = byte(256)
                    }
                "#,
                "byte",
                "cannot convert int to uint8",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = byte(256.0)
                    }
                "#,
                "byte",
                "cannot convert float64 to uint8",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = uint(-1)
                    }
                "#,
                "uint",
                "cannot convert int to uint",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = float64(1e1000)
                    }
                "#,
                "float64",
                "cannot convert float64 to float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = int(1i)
                    }
                "#,
                "int",
                "cannot convert complex128 to int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = byte('\u0100')
                    }
                "#,
                "byte",
                "cannot convert int32 to uint8",
            ),
        ];

        for (source, target, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidTypeConversion {
                        target: target.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_type_conversion_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int

                func main() {
                    var i int
                    _ = float64(i)
                    _ = int32(i)
                    _ = byte(255)
                    _ = byte(255.0)
                    _ = byte('\xff')
                    _ = byte(0i)
                    _ = float64(0i)
                    _ = float64(-1e-1000)
                    _ = string(i)
                    _ = string(65)
                    _ = []byte("go")
                    _ = []rune("go")
                    _ = string([]byte{})
                    _ = string([]rune{})
                    var c Count
                    _ = Count(c)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn validates_calls_inside_return_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func takes(a int) int { return a }

                func badCall() int {
                    return takes("go")
                }

                func badConversion() int {
                    return int()
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let invalid_return = |name: &str| {
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function {name}");
            };
            super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env)
        };

        assert_eq!(
            invalid_return("badCall"),
            Some(super::InvalidStatement::Expression {
                reason: super::InvalidStatementReason::InvalidCall {
                    target: "takes".to_string(),
                    reason: "argument 1 must be assignable to int, got string".to_string(),
                },
            })
        );
        assert_eq!(
            invalid_return("badConversion"),
            Some(super::InvalidStatement::Expression {
                reason: super::InvalidStatementReason::InvalidTypeConversion {
                    target: "int".to_string(),
                    reason: "expects 1 argument, got 0".to_string(),
                },
            })
        );
    }

    #[test]
    fn rejects_invalid_index_expressions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = 1[0]
                    }
                "#,
                "cannot index int",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        _ = xs["0"]
                    }
                "#,
                "index must have integer type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        _ = xs[1.5]
                    }
                "#,
                "index must have integer type, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        m := map[string]int{}
                        _ = m[1]
                    }
                "#,
                "key must be assignable to string, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        m := map[int]int{}
                        var f float64
                        _ = m[f]
                    }
                "#,
                "key must be assignable to int, got float64",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidIndex {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_index_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    xs := []int{1}
                    _ = xs[0]
                    _ = xs[1.0]
                    s := "go"
                    _ = s[1]
                    m := map[string]int{"go": 1}
                    _ = m["go"]
                    ints := map[int]int{1: 1}
                    _ = ints[1.0]
                    a := [1]int{1}
                    p := &a
                    _ = p[0]
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_array_lengths() {
        let cases = vec![
            (
                r#"
                    package main

                    var _ [-1]int
                "#,
                "length must be non-negative",
            ),
            (
                r#"
                    package main

                    var _ [1.5]int
                "#,
                "length must be representable by int",
            ),
            (
                r#"
                    package main

                    var _ ["go"]int
                "#,
                "length must be a numeric constant",
            ),
            (
                r#"
                    package main

                    var _ [nil]int
                "#,
                "length must be a numeric constant",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            assert_eq!(
                super::invalid_expression_in_file(&file, &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidArrayType {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_array_lengths() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                const n = 2

                var _ [0]int
                var _ [-0]int
                var _ [1.0]int
                var _ [n]int
                var _ [2*n]int
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        assert_eq!(super::invalid_expression_in_file(&file, &env), None);
    }

    #[test]
    fn rejects_invalid_map_key_types() {
        let cases = vec![
            (
                r#"
                    package main

                    var _ map[[]int]int
                "#,
                "key type slice(int) is not comparable",
            ),
            (
                r#"
                    package main

                    var _ map[map[string]int]int
                "#,
                "key type map[string]int is not comparable",
            ),
            (
                r#"
                    package main

                    var _ map[func()]int
                "#,
                "key type func is not comparable",
            ),
            (
                r#"
                    package main

                    var _ map[struct { X []int }]int
                "#,
                "key type struct is not comparable",
            ),
            (
                r#"
                    package main

                    type T struct { X []int }
                    var _ map[T]int
                "#,
                "key type T is not comparable",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            assert_eq!(
                super::invalid_expression_in_file(&file, &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidMapType {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_map_key_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type T struct { X int }

                var _ map[int]int
                var _ map[[2]string]int
                var _ map[struct { X int; Y string }]int
                var _ map[T]int
                var _ map[chan int]int
                var _ map[any]int
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        assert_eq!(super::invalid_expression_in_file(&file, &env), None);
    }

    #[test]
    fn rejects_invalid_slice_expressions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = 1[0:1]
                    }
                "#,
                "cannot slice int",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        _ = xs["0":]
                    }
                "#,
                "bound must have integer type, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        _ = xs[:1.5]
                    }
                "#,
                "bound must have integer type, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        s := "go"
                        _ = s[0:1:1]
                    }
                "#,
                "full slice expression is not valid for strings",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidSlice {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_slice_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    xs := []int{1, 2}
                    _ = xs[0:]
                    _ = xs[:1]
                    _ = xs[:1.0]
                    _ = xs[0:1:1]
                    _ = xs[0:1.0:1.0]
                    s := "go"
                    _ = s[0:1]
                    a := [2]int{1, 2}
                    _ = a[0:1]
                    p := &a
                    _ = p[0:1:1]
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_binary_expressions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = true + false
                    }
                "#,
                "+",
                "operands must both be numeric or both be string, got bool and bool",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = "go" - "g"
                    }
                "#,
                "-",
                "operands must both be numeric, got string and string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 && 2
                    }
                "#,
                "&&",
                "operands must both be bool, got int and int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1.5 % 1.0
                    }
                "#,
                "%",
                "operands must both be integer, got float64 and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 / 0
                    }
                "#,
                "/",
                "division by zero constant",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1.0 / -0.0
                    }
                "#,
                "/",
                "division by zero constant",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 % 0
                    }
                "#,
                "%",
                "division by zero constant",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        _ = i + f
                    }
                "#,
                "+",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        _ = i + 1.5
                    }
                "#,
                "+",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var u uint
                        _ = i & u
                    }
                "#,
                "&",
                "operands have mismatched types int and uint",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 << 1.5
                    }
                "#,
                "<<",
                "shift operands must be integer, got int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        s := uint(1)
                        _ = 1.0 << s
                    }
                "#,
                "<<",
                "shift operands must be integer, got float64 and uint",
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{}
                        _ = xs == xs
                    }
                "#,
                "==",
                "operands must be comparable, got slice(int) and slice(int)",
            ),
            (
                r#"
                    package main

                    type T struct { X []int }

                    func main() {
                        var a T
                        var b T
                        _ = a == b
                    }
                "#,
                "==",
                "operands must be comparable, got T and T",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = true < false
                    }
                "#,
                "<",
                "operands must both be ordered numeric values or strings, got bool and bool",
            ),
            (
                r#"
                    package main

                    var z complex128

                    func main() {
                        _ = z < z
                    }
                "#,
                "<",
                "operands must both be ordered numeric values or strings, got complex128 and complex128",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 == "1"
                    }
                "#,
                "==",
                "operands have mismatched types int and string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        _ = i == f
                    }
                "#,
                "==",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        _ = i == 1.5
                    }
                "#,
                "==",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        _ = i < f
                    }
                "#,
                "<",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        _ = i < 1.5
                    }
                "#,
                "<",
                "operands have mismatched types int and float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = 1 == nil
                    }
                "#,
                "==",
                "operand must be comparable to nil, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = nil == nil
                    }
                "#,
                "==",
                "operator not defined on untyped nil",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = nil < nil
                    }
                "#,
                "<",
                "operator not defined on untyped nil",
            ),
            (
                r#"
                    package main

                    type S struct{}

                    func main() {
                        var s S
                        _ = s != nil
                    }
                "#,
                "!=",
                "operand must be comparable to nil, got S",
            ),
        ];

        for (source, op, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidBinary {
                        op: op.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_binary_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type I interface{}
                type P *int
                type Slice []int

                func main() {
                    _ = 1 + 2
                    _ = "go" + "rs"
                    _ = 1.5 / 2
                    _ = 3 % 2
                    var u uint64
                    _ = u % 1e9
                    _ = 1 << 2
                    _ = u >> 1.0
                    _ = true && false
                    _ = "go" < "rs"
                    _ = 1 == 2
                    var i int
                    _ = i + 1
                    _ = i + 1.0
                    _ = i < 1
                    _ = i <= 1.0
                    _ = i == 1
                    var f64 float64
                    _ = f64 + 1
                    _ = f64 + 1.5
                    _ = f64 >= 1
                    _ = f64 != 1.5
                    var n64 int64
                    _ = 2 * n64
                    _ = n64 % 1
                    _ = n64 & 3
                    _ = n64 != 2*n64
                    binBits := uint(i+1) * 8
                    _ = 1.0 << 2
                    _ = n64 >= -1<<binBits
                    _ = n64 < 1<<binBits
                    var c128 complex128
                    _ = c128 + 1
                    _ = c128 + 1i
                    _ = c128 == 1
                    xs := []int{}
                    _ = xs == nil
                    m := map[string]int{}
                    _ = m != nil
                    var f func()
                    _ = f == nil
                    var i I
                    _ = nil == i
                    var p P
                    _ = p == nil
                    var s Slice
                    _ = s != nil
                    ch := make(chan int)
                    _ = ch == ch
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_unary_expressions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = +"go"
                    }
                "#,
                "+",
                "operand must be numeric, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = !1
                    }
                "#,
                "!",
                "operand must be bool, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = ^1.5
                    }
                "#,
                "^",
                "operand must be integer, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = &1
                    }
                "#,
                "&",
                "operand must be addressable",
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        _ = *x
                    }
                "#,
                "*",
                "operand must be pointer, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var ch chan<- int
                        _ = func() int {
                            return <-ch
                        }
                    }
                "#,
                "<-",
                "operand must be receive-capable channel, got send-only channel",
            ),
        ];

        for (source, op, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidUnary {
                        op: op.to_string(),
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_unary_expressions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type T struct {}

                func main() {
                    x := 1
                    _ = +x
                    _ = -x
                    _ = ^1e9
                    _ = !false
                    _ = &x
                    _ = &T{}
                    p := &x
                    _ = *p
                    ch := make(chan int, 1)
                    _ = <-ch
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_composite_literals() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = map[string]int{"go"}
                    }
                "#,
                "map literal elements must be keyed",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = map[string]int{1: 2}
                    }
                "#,
                "key must be assignable to string, got int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var f float64
                        _ = map[int]int{f: 1}
                    }
                "#,
                "key must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = map[string]int{"go": "rs"}
                    }
                "#,
                "value must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var f float64
                        _ = map[string]int{"go": f}
                    }
                "#,
                "value must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = map[string]int{"go": 1, "go": 2}
                    }
                "#,
                "duplicate map key",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = []int{"go"}
                    }
                "#,
                "element must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var f float64
                        _ = []int{f}
                    }
                "#,
                "element must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = []int{"go": 1}
                    }
                "#,
                "index key must be an integer constant, got string",
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = [1]int{1: 2}
                    }
                "#,
                "array literal index 1 out of bounds for length 1",
            ),
            (
                r#"
                    package main

                    type T struct { A int }

                    func main() {
                        _ = T{B: 1}
                    }
                "#,
                "unknown field B",
            ),
            (
                r#"
                    package main

                    type T struct { A int }

                    func main() {
                        _ = T{A: "go"}
                    }
                "#,
                "field must be assignable to int, got string",
            ),
            (
                r#"
                    package main

                    type T struct { A int }

                    func main() {
                        var f float64
                        _ = T{A: f}
                    }
                "#,
                "field must be assignable to int, got float64",
            ),
            (
                r#"
                    package main

                    type T struct { A int }

                    func main() {
                        _ = T{A: 1, 2}
                    }
                "#,
                "all struct literal elements must be keyed when any element is keyed",
            ),
            (
                r#"
                    package main

                    type T struct { A int }

                    func main() {
                        _ = T{1, 2}
                    }
                "#,
                "struct literal expects 1 field value(s), got 2",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidCompositeLiteral {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_composite_literals() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type T struct {
                    A int
                    B string
                }
                type Ints []int
                type Dict map[string]int

                func main() {
                    _ = []int{1, 2}
                    _ = []int{1.0}
                    _ = Ints{0: 1, 2}
                    _ = [3]int{0: 1, 2: 3}
                    _ = map[string]int{"go": 1}
                    _ = map[int]int{1.0: 2.0}
                    _ = Dict{"go": 1}
                    _ = T{}
                    _ = T{A: 1.0}
                    _ = T{1, "go"}
                    _ = struct { A int }{A: 1}
                    _ = []T{{A: 1}, {1, "go"}}
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_top_level_expression_builtin_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                var N = len(1)
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        assert_eq!(
            super::invalid_expression_in_file(&file, &env),
            Some(super::InvalidStatement::Expression {
                reason: super::InvalidStatementReason::InvalidBuiltinCall {
                    name: "len".to_string(),
                    reason:
                        "argument must have string, array, slice, map, or channel type, got int"
                            .to_string(),
                },
            })
        );
    }

    #[test]
    fn rejects_invalid_expression_builtin_calls_inside_function_literals() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                var F = func() {
                    _ = len(1)
                }

                func main() {}
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        assert_eq!(
            super::invalid_expression_in_file(&file, &env),
            Some(super::InvalidStatement::Expression {
                reason: super::InvalidStatementReason::InvalidBuiltinCall {
                    name: "len".to_string(),
                    reason:
                        "argument must have string, array, slice, map, or channel type, got int"
                            .to_string(),
                },
            })
        );

        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = func() {
                        _ = copy([]int{}, []string{})
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            Some(super::InvalidStatement::Expression {
                reason: super::InvalidStatementReason::InvalidBuiltinCall {
                    name: "copy".to_string(),
                    reason: "source element type must match int, got string".to_string(),
                },
            })
        );
    }

    #[test]
    fn accepts_valid_expression_builtin_calls() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = len("go")
                    _ = cap([]int{1})
                    _ = copy([]byte{}, "go")
                    _ = append([]int{}, 1, 2)
                    _ = append([]int{}, 1.0)
                    _ = append([]*int{}, nil)
                    _ = append([]any{}, nil)
                    _ = append([]int{}, []int{}...)
                    _ = append([]byte{}, "go"...)
                    delete(map[*int]int{}, nil)
                    delete(map[int]int{}, 1.0)
                    _ = make([]int, N)
                    _ = make([]int, 1.0)
                    _ = make([]int, 1, 2)
                    _ = make(map[string]int)
                    _ = make(chan int, 1)
                    _ = make(Ints, 1)
                    _ = new(int)
                    _ = new(*int)
                    _ = new([]int)
                    _ = new(struct{ X int })
                    _ = new(Ints)
                    _ = complex(1, 2)
                    _ = complex(float32(1), float32(2))
                    _ = real(1i)
                    _ = imag(complex(1, 2))
                    _ = max(1, 2.0, 3)
                    _ = min("a", "b")
                    var i int
                    var f float64
                    var n64 int64
                    _ = max(i, 1)
                    _ = max(i, 1.0)
                    _ = min(f, 1.5)
                    _ = max(n64, 2*n64)
                    _ = max(n64, -1<<binBits)
                    _ = recover()
                    println("ok", 1)
                    _ = func(len func(int) int) {
                        _ = len(1)
                    }
                    len := func(int) int { return 1 }
                    _ = len(1)
                    takesInt(1.0)
                }

                func takesInt(x int) {}
                type Ints []int
                const N = 1e3
                const binBits = 5
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_inc_dec_on_non_numeric_operands() {
        let cases = vec![
            r#"
                package main

                func main() {
                    s := "go"
                    s++
                }
            "#,
            r#"
                package main

                func main() {
                    b := true
                    b--
                }
            "#,
            r#"
                package main

                func main() {
                    m := map[string]string{"go": "rs"}
                    m["go"]++
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::IncDec {
                    reason: super::InvalidIncDecReason::NonNumericOperand,
                })
            );
        }
    }

    #[test]
    fn rejects_inc_dec_on_invalid_operands() {
        let cases = vec![
            r#"
                package main

                func main() {
                    1++
                }
            "#,
            r#"
                package main

                func f() int { return 1 }

                func main() {
                    f()--
                }
            "#,
            r#"
                package main

                func main() {
                    _++
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::IncDec {
                    reason: super::InvalidIncDecReason::InvalidOperand,
                })
            );
        }
    }

    #[test]
    fn accepts_inc_dec_on_numeric_operands() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int

                func main() {
                    i := 0
                    i++
                    f := 1.5
                    f--
                    var c Count
                    c++
                    var z complex128
                    z--
                    m := map[string]int{"go": 1}
                    m["go"]++
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_non_boolean_conditions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        if 1 {
                        }
                    }
                "#,
                super::ConditionKind::If,
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        for "go" {
                        }
                    }
                "#,
                super::ConditionKind::For,
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        if x := 1; x {
                        }
                    }
                "#,
                super::ConditionKind::If,
                "int",
            ),
        ];

        for (source, kind, type_name) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Condition {
                    reason: super::InvalidConditionReason {
                        kind,
                        type_name: type_name.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_boolean_conditions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Flag bool

                func main() {
                    if true {
                    }
                    var flag Flag
                    if flag {
                    }
                    for false {
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_send_statements_with_non_channel_operands() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x <- 2
                    }
                "#,
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        s := "go"
                        s <- "rs"
                    }
                "#,
                "string",
            ),
        ];

        for (source, type_name) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Send {
                    reason: super::InvalidSendReason::NonChannel {
                        type_name: type_name.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_send_statements_with_channel_operands() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    ch := make(chan int, 1)
                    ch <- 1
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_send_to_receive_only_channels() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func send(ch <-chan int) {
                    ch <- 1
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            Some(super::InvalidStatement::Send {
                reason: super::InvalidSendReason::ReceiveOnlyChannel,
            })
        );
    }

    #[test]
    fn rejects_send_values_with_incompatible_known_types() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int, 1)
                        ch <- "go"
                    }
                "#,
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan bool, 1)
                        ch <- 1
                    }
                "#,
                "bool",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int, 1)
                        var f float64
                        ch <- f
                    }
                "#,
                "int",
                "float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int, 1)
                        ch <- nil
                    }
                "#,
                "int",
                "nil",
            ),
        ];

        for (source, expected, actual) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Send {
                    reason: super::InvalidSendReason::ValueTypeMismatch {
                        expected: expected.to_string(),
                        actual: actual.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_send_values_with_compatible_known_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int
                type I interface{}

                func main() {
                    ints := make(chan int, 1)
                    ints <- 1
                    ints <- 1.0
                    floats := make(chan float64, 1)
                    floats <- 1
                    strings := make(chan string, 1)
                    strings <- "go"
                    counts := make(chan Count, 1)
                    var count Count
                    counts <- count
                    slices := make(chan []int, 1)
                    slices <- nil
                    pointers := make(chan *int, 1)
                    pointers <- nil
                    values := make(chan any, 1)
                    values <- nil
                    interfaces := make(chan I, 1)
                    interfaces <- nil
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_receive_operations_with_non_channel_operands() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        <-1
                    }
                "#,
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        s := "go"
                        x := <-s
                        _ = x
                    }
                "#,
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        var x = <-true
                        _ = x
                    }
                "#,
                "bool",
            ),
            (
                r#"
                    package main

                    func main() {
                        if <-1 {
                        }
                    }
                "#,
                "int",
            ),
        ];

        for (source, type_name) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Receive {
                    reason: super::InvalidReceiveReason::NonChannel {
                        type_name: type_name.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_receive_operations_with_channel_operands() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    ch := make(chan bool, 1)
                    ch <- true
                    if <-ch {
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_receive_from_send_only_channels() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func recv(ch chan<- int) {
                    <-ch
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            Some(super::InvalidStatement::Receive {
                reason: super::InvalidReceiveReason::SendOnlyChannel,
            })
        );
    }

    #[test]
    fn accepts_directional_channel_operations() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func use(send chan<- int, recv <-chan bool) {
                    send <- 1
                    if <-recv {
                    }
                    for range recv {
                        break
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_select_communication_clauses() {
        let cases = vec![
            (
                r#"
                    package main

                    func f() {}

                    func main() {
                        select {
                        case f():
                        }
                    }
                "#,
                super::InvalidSelectCommReason::NonCommunication,
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int, 1)
                        x := 0
                        select {
                        case x += <-ch:
                        }
                    }
                "#,
                super::InvalidSelectCommReason::InvalidAssignmentToken,
            ),
            (
                r#"
                    package main

                    func f() int { return 1 }

                    func main() {
                        x := 0
                        select {
                        case x := f():
                        }
                    }
                "#,
                super::InvalidSelectCommReason::MissingReceiveExpression,
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int, 1)
                        a := []int{0}
                        select {
                        case a[0] := <-ch:
                        }
                    }
                "#,
                super::InvalidSelectCommReason::ShortReceiveDeclarationLhs,
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::SelectComm { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_select_communication_clauses() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    ch := make(chan int, 1)
                    x := 0
                    select {
                    case (<-ch):
                    case x = <-ch:
                    case y, ok := (<-ch):
                        _ = y
                        _ = ok
                    case ch <- x:
                    default:
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_duplicate_short_var_decl_names() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        x, x := 1, 2
                    }
                "#,
                "x",
            ),
            (
                r#"
                    package main

                    func main() {
                        for i, i := range []int{1} {
                            _ = i
                        }
                    }
                "#,
                "i",
            ),
        ];

        for (source, name) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::ShortVarDecl {
                    reason: super::InvalidShortVarDeclReason::DuplicateName(name.to_string()),
                })
            );
        }
    }

    #[test]
    fn rejects_duplicate_default_clauses() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        default:
                        default:
                        }
                    }
                "#,
                super::DefaultClauseKind::Switch,
            ),
            (
                r#"
                    package main

                    func main() {
                        var x any
                        switch x.(type) {
                        default:
                        default:
                        }
                    }
                "#,
                super::DefaultClauseKind::TypeSwitch,
            ),
            (
                r#"
                    package main

                    func main() {
                        select {
                        default:
                        default:
                        }
                    }
                "#,
                super::DefaultClauseKind::Select,
            ),
        ];

        for (source, kind) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::DuplicateDefault { kind })
            );
        }
    }

    #[test]
    fn rejects_invalid_expression_switch_cases() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        switch nil {
                        }
                    }
                "#,
                super::InvalidSwitchReason::NilTag,
            ),
            (
                r#"
                    package main

                    func main() {
                        switch []int{} {
                        }
                    }
                "#,
                super::InvalidSwitchReason::NonComparableTag {
                    type_name: "slice(int)".to_string(),
                },
            ),
            (
                r#"
                    package main

                    type T struct { X []int }

                    func main() {
                        var x T
                        switch x {
                        }
                    }
                "#,
                super::InvalidSwitchReason::NonComparableTag {
                    type_name: "T".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func main() {
                        switch 1 {
                        case pair():
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseMultiValue { values: 2 },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case "go":
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseTypeMismatch {
                    expected: "int".to_string(),
                    actual: "string".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        switch i {
                        case f:
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseTypeMismatch {
                    expected: "int".to_string(),
                    actual: "float64".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case 1.5:
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseTypeMismatch {
                    expected: "int".to_string(),
                    actual: "float64".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch {
                        case 1:
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseTypeMismatch {
                    expected: "bool".to_string(),
                    actual: "int".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case 1:
                        case 1:
                        }
                    }
                "#,
                super::InvalidSwitchReason::DuplicateConstantCase {
                    value: "1".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch {
                        case true:
                        case true:
                        }
                    }
                "#,
                super::InvalidSwitchReason::DuplicateConstantCase {
                    value: "true".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case []int{}:
                        }
                    }
                "#,
                super::InvalidSwitchReason::NonComparableCase {
                    type_name: "slice(int)".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        switch 1 {
                        case nil:
                        }
                    }
                "#,
                super::InvalidSwitchReason::CaseTypeMismatch {
                    expected: "int".to_string(),
                    actual: "nil".to_string(),
                },
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Switch { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_expression_switch_cases() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int

                func main() {
                    switch 1 {
                    case 1, 2:
                    }
                    switch 1 {
                    case 1.0:
                    }
                    switch "go" {
                    case "go":
                    }
                    switch {
                    case true, 1 < 2:
                    default:
                    }
                    var c Count
                    switch c {
                    case 1:
                    }
                    v := 1
                    switch v {
                    case v:
                    case v:
                    }
                    var p *int
                    switch p {
                    case nil:
                    }
                    var x any
                    switch x {
                    case nil, 1, "go":
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_type_switch_guards() {
        let cases = vec![
            (
                r#"
                    package main

                    func main(x any) {
                        var v any
                        switch v = x.(type) {
                        default:
                        }
                    }
                "#,
                super::InvalidTypeSwitchGuardReason::InvalidAssignmentToken,
            ),
            (
                r#"
                    package main

                    func main(x any) {
                        switch v, ok := x.(type) {
                        default:
                        }
                    }
                "#,
                super::InvalidTypeSwitchGuardReason::InvalidIdentifierCount,
            ),
            (
                r#"
                    package main

                    func main(x any) {
                        switch _ := x.(type) {
                        default:
                        }
                    }
                "#,
                super::InvalidTypeSwitchGuardReason::BlankIdentifier,
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::TypeSwitchGuard { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_type_switch_guards() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main(x any) {
                    switch x.(type) {
                    default:
                    }

                    switch v := x.(type) {
                    case int:
                        _ = v
                    default:
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_type_switch_cases() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        var x int
                        switch x.(type) {
                        }
                    }
                "#,
                super::InvalidTypeSwitchReason::NonInterfaceGuard {
                    type_name: "int".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        var x any
                        switch x.(type) {
                        case nil:
                        case nil:
                        }
                    }
                "#,
                super::InvalidTypeSwitchReason::DuplicateNil,
            ),
            (
                r#"
                    package main

                    func main() {
                        var x any
                        switch x.(type) {
                        case int:
                        case int:
                        }
                    }
                "#,
                super::InvalidTypeSwitchReason::DuplicateCase {
                    type_name: "int".to_string(),
                },
            ),
            (
                r#"
                    package main

                    type I interface { M() }
                    type T struct {}

                    func main(x I) {
                        switch x.(type) {
                        case T:
                        }
                    }
                "#,
                super::InvalidTypeSwitchReason::CaseDoesNotImplement {
                    case_type: "T".to_string(),
                    interface_type: "I".to_string(),
                },
            ),
            (
                r#"
                    package main

                    type I interface { M() }

                    func main(x I) {
                        switch x.(type) {
                        case string:
                        }
                    }
                "#,
                super::InvalidTypeSwitchReason::CaseDoesNotImplement {
                    case_type: "string".to_string(),
                    interface_type: "I".to_string(),
                },
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::TypeSwitch { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_type_switch_cases() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type I interface { M() }
                type T struct {}
                func (T) M() {}

                func main(x any, y I) {
                    switch x.(type) {
                    case nil:
                    case int, string:
                    case T:
                    }
                    switch y.(type) {
                    case T:
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_type_assertions() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        var x int
                        _ = x.(int)
                    }
                "#,
                "operand must have interface type, got int",
            ),
            (
                r#"
                    package main

                    type I interface { M() }

                    func main(x I) {
                        _ = x.(string)
                    }
                "#,
                "string does not implement I",
            ),
            (
                r#"
                    package main

                    type I interface { M() }
                    type T struct {}

                    func main(x I) {
                        _ = x.(T)
                    }
                "#,
                "T does not implement I",
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Expression {
                    reason: super::InvalidStatementReason::InvalidTypeAssert {
                        reason: reason.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_valid_type_assertions() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type I interface { M() }
                type T struct {}
                func (T) M() {}

                func main(x any, y I) {
                    _ = x.(int)
                    _ = y.(T)
                    _ = y.(interface { M() })
                    _, _ = y.(T)
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    fn invalid_short_var_redeclaration(source: &str) -> Option<super::InvalidStatement> {
        let file = parse_file("test.go", source).unwrap();
        super::invalid_short_var_redeclaration_in_file(&file)
    }

    #[test]
    fn rejects_short_var_decls_without_new_names_in_current_block() {
        for source in [
            r#"
                package main

                func main() {
                    x := 1
                    x := 2
                    _ = x
                }
            "#,
            r#"
                package main

                func f(x int) {
                    x := 1
                    _ = x
                }
            "#,
            r#"
                package main

                func f() (x int) {
                    x := 1
                    return x
                }
            "#,
            r#"
                package main

                func main() {
                    for _ := range []int{1} {
                    }
                }
            "#,
        ] {
            assert_eq!(
                invalid_short_var_redeclaration(source),
                Some(super::InvalidStatement::ShortVarDecl {
                    reason: super::InvalidShortVarDeclReason::NoNewVariables,
                })
            );
        }
    }

    #[test]
    fn accepts_short_var_decls_with_new_names_and_nested_shadowing() {
        assert_eq!(
            invalid_short_var_redeclaration(
                r#"
                    package main

                    func f(x int) {
                        x, y := 1, 2
                        _ = y
                        {
                            x := 3
                            _ = x
                        }
                        if z := x; z > 0 {
                            z := 4
                            _ = z
                        }
                        for i := 0; i < 1; i++ {
                            i := 2
                            _ = i
                        }
                        for i := range []int{1} {
                            _ = i
                            i := 2
                            _ = i
                        }
                    }
                "#,
            ),
            None
        );
    }

    #[test]
    fn accepts_valid_statement_context_calls_receives_and_shadowed_builtins() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func len(s string) {}

                func main() {
                    ch := make(chan int, 1)
                    len("go")
                    (<-ch)
                    <-ch
                    println("ok")
                    panic("boom")
                    clear([]int{1})
                    local := func() {}
                    local()
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_invalid_assignment_left_operands() {
        let cases = vec![
            r#"
                package main

                func main() {
                    1 = 2
                }
            "#,
            r#"
                package main

                func f() int { return 1 }

                func main() {
                    f() = 2
                }
            "#,
            r#"
                package main

                func main() {
                    "go"[0] = 'G'
                }
            "#,
            r#"
                package main

                func main() {
                    len = 1
                }
            "#,
            r#"
                package main

                func main() {
                    1 += 2
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Assignment {
                    reason: super::InvalidAssignmentReason::InvalidLeftOperand,
                })
            );
        }
    }

    #[test]
    fn accepts_valid_assignment_left_operands() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type T struct { N int }

                func main() {
                    x := 1
                    _ = x
                    xs := []int{1}
                    xs[0] = 2
                    arr := [1]int{1}
                    arr[0] = 2
                    m := map[string]int{"x": 1}
                    m["x"] = 2
                    m["x"] += 1
                    p := &T{}
                    p.N = 3
                    _ = 4
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_short_var_declarations_with_non_identifiers() {
        let cases = vec![
            r#"
                package main

                func main() {
                    xs := []int{1}
                    xs[0] := 2
                }
            "#,
            r#"
                package main

                func main() {
                    xs := []int{1}
                    for xs[0] := range xs {
                    }
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::ShortVarDecl {
                    reason: super::InvalidShortVarDeclReason::NonIdentifier,
                })
            );
        }
    }

    #[test]
    fn rejects_invalid_assignment_value_counts() {
        let cases = vec![
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func main() {
                        x := pair()
                        _ = x
                    }
                "#,
                super::InvalidAssignmentReason::CountMismatch { lhs: 1, values: 2 },
            ),
            (
                r#"
                    package main

                    func one() int { return 1 }

                    func main() {
                        x, y := one()
                        _, _ = x, y
                    }
                "#,
                super::InvalidAssignmentReason::CountMismatch { lhs: 2, values: 1 },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func main() {
                        x, y, z := pair()
                        _, _, _ = x, y, z
                    }
                "#,
                super::InvalidAssignmentReason::CountMismatch { lhs: 3, values: 2 },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func main() {
                        x, y, z := pair(), 3
                        _, _, _ = x, y, z
                    }
                "#,
                super::InvalidAssignmentReason::MultiValueInSingleValueContext,
            ),
            (
                r#"
                    package main

                    func main() {
                        xs := []int{1}
                        x, ok := xs[0]
                        _, _ = x, ok
                    }
                "#,
                super::InvalidAssignmentReason::CountMismatch { lhs: 2, values: 1 },
            ),
            (
                r#"
                    package main

                    func main() {
                        _ += 1
                    }
                "#,
                super::InvalidAssignmentReason::CompoundBlankIdentifier,
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Assignment { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_assignment_value_counts() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func pair() (int, int) { return 1, 2 }
                func len() (int, int) { return 3, 4 }

                func main() {
                    x, y := pair()
                    _ = x
                    _ = y

                    lx, ly := len()
                    _ = lx
                    _ = ly

                    m := map[string]int{"go": 1}
                    v, ok := m["go"]
                    _ = v
                    _ = ok

                    ch := make(chan int, 1)
                    r, open := <-ch
                    _ = r
                    _ = open

                    var anyv any = 1
                    asserted, matches := anyv.(int)
                    _ = asserted
                    _ = matches
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_assignment_value_type_mismatches() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x = "go"
                    }
                "#,
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        ok := true
                        ok = 1
                    }
                "#,
                "bool",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        var f float64
                        i = f
                    }
                "#,
                "int",
                "float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int
                        i = 1.5
                    }
                "#,
                "int",
                "float64",
            ),
            (
                r#"
                    package main

                    func pair() (int, string) {
                        return 1, "go"
                    }

                    func main() {
                        x := 1
                        y := 2
                        x, y = pair()
                    }
                "#,
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x, y := "go", 2
                        _, _ = x, y
                    }
                "#,
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func pair() (string, int) {
                        return "go", 2
                    }

                    func main() {
                        x := 1
                        x, y := pair()
                        _, _ = x, y
                    }
                "#,
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x = nil
                    }
                "#,
                "int",
                "nil",
            ),
            (
                r#"
                    package main

                    type S struct{}

                    func main() {
                        var s S
                        s = nil
                    }
                "#,
                "S",
                "nil",
            ),
            (
                r#"
                    package main

                    type Count int

                    func main() {
                        var c Count
                        c = nil
                    }
                "#,
                "int",
                "nil",
            ),
        ];

        for (source, expected, actual) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Assignment {
                    reason: super::InvalidAssignmentReason::TypeMismatch {
                        expected: expected.to_string(),
                        actual: actual.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn rejects_untyped_nil_assignments_without_nilable_target_type() {
        let cases = vec![
            r#"
                package main

                func main() {
                    x := nil
                    _ = x
                }
            "#,
            r#"
                package main

                func main() {
                    _ = nil
                }
            "#,
            r#"
                package main

                func main() {
                    x, y := nil, 1
                    _, _ = x, y
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Assignment {
                    reason: super::InvalidAssignmentReason::UntypedNil,
                })
            );
        }
    }

    #[test]
    fn rejects_invalid_compound_assignment_operands() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        ok := true
                        ok += true
                    }
                "#,
                super::InvalidAssignmentReason::CompoundInvalidOperand {
                    op: "+=".to_string(),
                    side: "left".to_string(),
                    type_name: "bool".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        s := "go"
                        s -= "g"
                    }
                "#,
                super::InvalidAssignmentReason::CompoundInvalidOperand {
                    op: "-=".to_string(),
                    side: "left".to_string(),
                    type_name: "string".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        f := 1.5
                        f %= 1.0
                    }
                "#,
                super::InvalidAssignmentReason::CompoundInvalidOperand {
                    op: "%=".to_string(),
                    side: "left".to_string(),
                    type_name: "float64".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x <<= 1.5
                    }
                "#,
                super::InvalidAssignmentReason::CompoundInvalidOperand {
                    op: "<<=".to_string(),
                    side: "right".to_string(),
                    type_name: "float64".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        x := 1
                        x += "go"
                    }
                "#,
                super::InvalidAssignmentReason::TypeMismatch {
                    expected: "int".to_string(),
                    actual: "string".to_string(),
                },
            ),
        ];

        for (source, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Assignment { reason })
            );
        }
    }

    #[test]
    fn accepts_assignment_values_with_compatible_known_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int

                func pair() (int, string) {
                    return 1, "go"
                }

                func main() {
                    x := 1
                    x = 2
                    x = 2.0
                    f := 1.5
                    f = 1
                    s := "go"
                    s = "rs"
                    var complexValue complex128
                    complexValue = 1
                    var count Count
                    count = 1
                    xs := []int{1}
                    xs = nil
                    p := &x
                    p = nil
                    m := map[string]int{"go": 1}
                    m = nil
                    ch := make(chan int)
                    ch = nil
                    fn := func() {}
                    fn = nil
                    var iface any
                    iface = nil
                    var named I
                    named = nil
                    x += 1
                    x += 1.0
                    f /= 2
                    s += "!"
                    count <<= 1
                    count <<= 1.0
                    count &= 3
                    y := 0
                    z := ""
                    y, z = pair()
                    {
                        x, text := "shadow", 2
                        _, _ = x, text
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_short_redeclaration_type_mismatch_for_parameters() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func f(x int) {
                    x, y := "go", 2
                    _, _ = x, y
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "f" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func_with_type(
                &func.type_,
                func.body.as_ref().expect("body"),
                &env
            ),
            Some(super::InvalidStatement::Assignment {
                reason: super::InvalidAssignmentReason::TypeMismatch {
                    expected: "int".to_string(),
                    actual: "string".to_string(),
                },
            })
        );
    }

    #[test]
    fn accepts_assignment_using_current_function_signature_bindings() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func f(hi uint64) {
                    hi = uint64(1)
                }

                func previous() (hi uint32) {
                    return 0
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "f" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func_with_type(
                &func.type_,
                func.body.as_ref().expect("body"),
                &env
            ),
            None
        );
    }

    #[test]
    fn rejects_invalid_return_value_counts() {
        let cases = vec![
            (
                r#"
                    package main

                    func f() {
                        return 1
                    }
                "#,
                "f",
                super::InvalidReturnReason::CountMismatch {
                    expected: 0,
                    values: 1,
                },
            ),
            (
                r#"
                    package main

                    func f() int {
                        return
                    }
                "#,
                "f",
                super::InvalidReturnReason::CountMismatch {
                    expected: 1,
                    values: 0,
                },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func f() int {
                        return pair()
                    }
                "#,
                "f",
                super::InvalidReturnReason::CountMismatch {
                    expected: 1,
                    values: 2,
                },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func f() (int, int, int) {
                        return pair()
                    }
                "#,
                "f",
                super::InvalidReturnReason::CountMismatch {
                    expected: 3,
                    values: 2,
                },
            ),
            (
                r#"
                    package main

                    func pair() (int, int) { return 1, 2 }

                    func f() (int, int, int) {
                        return pair(), 3
                    }
                "#,
                "f",
                super::InvalidReturnReason::MultiValueInSingleValueContext,
            ),
            (
                r#"
                    package main

                    func outer() {
                        _ = func() int {
                            return
                        }
                    }
                "#,
                "outer",
                super::InvalidReturnReason::CountMismatch {
                    expected: 1,
                    values: 0,
                },
            ),
        ];

        for (source, name, reason) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Return { reason })
            );
        }
    }

    #[test]
    fn accepts_valid_return_value_counts() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type myError struct{}

                func pair() (int, int) { return 1, 2 }

                func noResult() {
                    return
                }

                func explicit() (int, int) {
                    return 1, 2
                }

                func forwarded() (int, int) {
                    return pair()
                }

                func named() (n int, _ myError) {
                    return
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        for name in ["noResult", "explicit", "forwarded", "named"] {
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env),
                None
            );
        }
    }

    #[test]
    fn rejects_return_value_type_mismatches() {
        let cases = vec![
            (
                r#"
                    package main

                    func f() int {
                        return "go"
                    }
                "#,
                "f",
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func f() (int, bool) {
                        return 1, 2
                    }
                "#,
                "f",
                "bool",
                "int",
            ),
            (
                r#"
                    package main

                    func pair() (int, string) {
                        return 1, "go"
                    }

                    func f() (int, int) {
                        return pair()
                    }
                "#,
                "f",
                "int",
                "string",
            ),
            (
                r#"
                    package main

                    func f() int {
                        return nil
                    }
                "#,
                "f",
                "int",
                "nil",
            ),
            (
                r#"
                    package main

                    func f() float64 {
                        var i int
                        return i
                    }
                "#,
                "f",
                "float64",
                "int",
            ),
            (
                r#"
                    package main

                    func f() int {
                        return 1.5
                    }
                "#,
                "f",
                "int",
                "float64",
            ),
        ];

        for (source, name, expected, actual) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Return {
                    reason: super::InvalidReturnReason::TypeMismatch {
                        expected: expected.to_string(),
                        actual: actual.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_return_values_with_compatible_known_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                type Count int

                func pair() (int, string) {
                    return 1, "go"
                }

                func intResult() int {
                    return 1
                }

                func floatResult() float64 {
                    return 1
                }

                func intFromIntegerFloatConstant() int {
                    return 1.0
                }

                func complexResult() complex128 {
                    return 1
                }

                func forwarded() (int, string) {
                    return pair()
                }

                func namedResult() Count {
                    var count Count
                    return count
                }

                func nilLike() []int {
                    return nil
                }

                func nilPointer() *int {
                    return nil
                }

                func nilMap() map[string]int {
                    return nil
                }

                func nilChan() chan int {
                    return nil
                }

                func nilFunc() func() {
                    return nil
                }

                func nilInterface() any {
                    return nil
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        for name in [
            "intResult",
            "floatResult",
            "intFromIntegerFloatConstant",
            "complexResult",
            "forwarded",
            "namedResult",
            "nilLike",
            "nilPointer",
            "nilMap",
            "nilChan",
            "nilFunc",
            "nilInterface",
        ] {
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env),
                None,
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_result_functions_that_can_complete_normally() {
        let cases = vec![
            r#"
                package main

                func f() int {
                }
            "#,
            r#"
                package main

                func f() int {
                    if true {
                        return 1
                    }
                }
            "#,
            r#"
                package main

                func f() int {
                    _ = func() int {
                    }
                    return 1
                }
            "#,
        ];

        for source in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "f" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            let invalid =
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env)
                    .or_else(|| {
                        super::invalid_body_completion_in_func(
                            &func.type_,
                            func.body.as_ref().expect("body"),
                            &env,
                        )
                    });
            assert_eq!(invalid, Some(super::InvalidStatement::MissingReturn));
        }
    }

    #[test]
    fn accepts_result_functions_with_terminating_bodies() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func returned() int {
                    return 1
                }

                func panics() int {
                    panic("stop")
                }

                func loops() int {
                    for {
                    }
                }

                func noResult() {
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        for name in ["returned", "panics", "loops", "noResult"] {
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == name => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            let invalid =
                super::invalid_return_in_func(&func.type_, func.body.as_ref().expect("body"), &env)
                    .or_else(|| {
                        super::invalid_body_completion_in_func(
                            &func.type_,
                            func.body.as_ref().expect("body"),
                            &env,
                        )
                    });
            assert_eq!(invalid, None, "{name}");
        }
    }

    #[test]
    fn rejects_range_clauses_with_too_many_bindings() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int)
                        for i, v := range ch {
                            _, _ = i, v
                        }
                    }
                "#,
                super::RangeKind::Channel,
                1,
                2,
            ),
            (
                r#"
                    package main

                    func main() {
                        for i, v := range 3 {
                            _, _ = i, v
                        }
                    }
                "#,
                super::RangeKind::Integer,
                1,
                2,
            ),
            (
                r#"
                    package main

                    func ints(yield func(int) bool) {}

                    func main() {
                        for i, v := range ints {
                            _, _ = i, v
                        }
                    }
                "#,
                super::RangeKind::Function,
                1,
                2,
            ),
        ];

        for (source, kind, max, got) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Range {
                    reason: super::InvalidRangeReason::BindingCount { kind, max, got },
                })
            );
        }
    }

    #[test]
    fn rejects_range_clauses_over_non_rangeable_operands() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        for range true {
                        }
                    }
                "#,
                "bool",
            ),
            (
                r#"
                    package main

                    func main() {
                        for range 1.5 {
                        }
                    }
                "#,
                "float64",
            ),
            (
                r#"
                    package main

                    func main() {
                        var ch chan<- int
                        for range ch {
                        }
                    }
                "#,
                "send-only channel",
            ),
            (
                r#"
                    package main

                    func bad(yield func(int)) {}

                    func main() {
                        for range bad {
                        }
                    }
                "#,
                "function",
            ),
        ];

        for (source, type_name) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Range {
                    reason: super::InvalidRangeReason::NonRangeable {
                        type_name: type_name.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn rejects_range_assignment_type_mismatches() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        var key string
                        for key = range []int{1} {
                        }
                    }
                "#,
                "string",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var i int8
                        for i = range []int{1} {
                        }
                    }
                "#,
                "int8",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var value int64
                        for _, value = range []uint32{1} {
                        }
                    }
                "#,
                "int64",
                "uint32",
            ),
            (
                r#"
                    package main

                    func main() {
                        var f float64
                        for f = range 10 {
                        }
                    }
                "#,
                "float64",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var small uint8
                        for small = range 256 {
                        }
                    }
                "#,
                "uint8",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var small uint8
                        for small = range -1 {
                        }
                    }
                "#,
                "uint8",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var value string
                        for _, value = range []int{1} {
                        }
                    }
                "#,
                "string",
                "int",
            ),
            (
                r#"
                    package main

                    func main() {
                        var r string
                        for _, r = range "go" {
                        }
                    }
                "#,
                "string",
                "int32",
            ),
            (
                r#"
                    package main

                    func main() {
                        ch := make(chan int)
                        var value string
                        for value = range ch {
                        }
                    }
                "#,
                "string",
                "int",
            ),
            (
                r#"
                    package main

                    func pairs(yield func(string, int) bool) {}

                    func main() {
                        var value string
                        for _, value = range pairs {
                        }
                    }
                "#,
                "string",
                "int",
            ),
        ];

        for (source, expected, actual) in cases {
            let file = parse_file("test.go", source).unwrap();
            let mut env = TypeEnv::new();
            env.scan_file(&file);
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
                crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
                Some(super::InvalidStatement::Range {
                    reason: super::InvalidRangeReason::TypeMismatch {
                        expected: expected.to_string(),
                        actual: actual.to_string(),
                    },
                })
            );
        }
    }

    #[test]
    fn accepts_range_assignments_with_compatible_types() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func pairs(yield func(string, int) bool) {}

                func main() {
                    var i int
                    var n int
                    for i, n = range []int{1} {
                    }

                    var r rune
                    for _, r = range "go" {
                    }

                    var key string
                    var value any
                    for key, value = range map[string]int{"go": 1} {
                    }

                    ch := make(chan int)
                    for n = range ch {
                    }

                    var small uint8
                    for small = range 10 {
                    }

                    for small = range 255 {
                    }

                    var signed int8
                    for signed = range -1 {
                    }

                    for key, n = range pairs {
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn accepts_range_clauses_with_blank_second_binding() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    ch := make(chan int)
                    for v, _ := range ch {
                        _ = v
                    }
                    for i, _ := range 3 {
                        _ = i
                    }
                }
            "#,
        )
        .unwrap();
        let mut env = TypeEnv::new();
        env.scan_file(&file);
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) if func.name.name == "main" => Some(func),
            crate::ast::Decl::FuncDecl(_) | crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_statement_in_func(func.body.as_ref().expect("body"), &env),
            None
        );
    }

    #[test]
    fn rejects_unused_and_duplicate_labels() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                    Done:
                        println("done")
                    }
                "#,
                super::InvalidLabel::Unused {
                    label: "Done".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                    Done:
                        goto Done
                    Done:
                        println("done")
                    }
                "#,
                super::InvalidLabel::Duplicate {
                    label: "Done".to_string(),
                },
            ),
        ];

        for (source, expected) in cases {
            let file = parse_file("test.go", source).unwrap();
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_label_in_func(func.body.as_ref().expect("body")),
                Some(expected)
            );
        }
    }

    #[test]
    fn rejects_unused_and_duplicate_labels_in_function_literals() {
        let cases = vec![
            (
                r#"
                    package main

                    func main() {
                        _ = func() {
                        Done:
                            println("done")
                        }
                    }
                "#,
                super::InvalidLabel::Unused {
                    label: "Done".to_string(),
                },
            ),
            (
                r#"
                    package main

                    func main() {
                        _ = func() {
                        Done:
                            goto Done
                        Done:
                            println("done")
                        }
                    }
                "#,
                super::InvalidLabel::Duplicate {
                    label: "Done".to_string(),
                },
            ),
        ];

        for (source, expected) in cases {
            let file = parse_file("test.go", source).unwrap();
            let Some(func) = file.decls.iter().find_map(|decl| match decl {
                crate::ast::Decl::FuncDecl(func) => Some(func),
                crate::ast::Decl::GenDecl(_) => None,
            }) else {
                panic!("expected function");
            };
            assert_eq!(
                super::invalid_label_in_func(func.body.as_ref().expect("body")),
                Some(expected)
            );
        }
    }

    #[test]
    fn accepts_labels_used_by_break_continue_and_goto() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                Loop:
                    for {
                        continue Loop
                    }
                Break:
                    for {
                        break Break
                    }
                    goto Done
                Done:
                    println("done")
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        assert_eq!(
            super::invalid_label_in_func(func.body.as_ref().expect("body")),
            None
        );
    }

    #[test]
    fn ignores_nested_locals_when_planning_forward_goto_hoists() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    i := 0
                    for i < 10 {
                        sr := i
                        if sr > 0 {
                            goto Done
                        }
                        i++
                    }
                Done:
                    println(i)
                    for _, sr := range []int{1} {
                        println(sr)
                    }
                }
            "#,
        )
        .unwrap();
        let Some(func) = file.decls.iter().find_map(|decl| match decl {
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
        }) else {
            panic!("expected function");
        };
        let Some(plan) = super::goto_state_plan_for_block(func.body.as_ref().expect("body")) else {
            panic!("expected forward goto plan");
        };
        assert_eq!(plan.labels, vec!["Done"]);
        assert_eq!(plan.hoisted_names, vec!["i"]);
    }

    #[test]
    fn lower_select_records_comm_cases() {
        let ir = lower(
            r#"
                package main

                func main() {
                    ch := make(chan int, 1)
                    select {
                    case ch <- 1:
                    case v := <-ch:
                        _ = v
                    default:
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::Select { cases }) = body.stmts.get(1) else {
            panic!("expected select statement");
        };
        assert_eq!(cases.len(), 3);
        assert!(!cases.first().is_some_and(|case| case.is_default));
        assert!(cases.get(2).is_some_and(|case| case.is_default));
    }

    #[test]
    fn classifies_select_completion() {
        let ir = lower(
            r#"
                package main

                func f(ch chan int) {
                    select {}
                    select {
                    case <-ch:
                        return
                    }
                    select {
                    default:
                        return
                    }
                    select {
                    default:
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        assert_eq!(
            super::stmt_completion(&body.stmts[0]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[1]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[2]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[3]),
            Completion::MayComplete
        );
    }

    #[test]
    fn classifies_statement_lists_by_final_non_empty_statement() {
        let ir = lower(
            r#"
                package main

                func f() {
                    return
                    _ = 1
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        assert_eq!(super::block_completion(body), Completion::MayComplete);
        assert_eq!(
            super::stmt_completion(&body.stmts[0]),
            Completion::Terminates
        );
    }

    #[test]
    fn classifies_builtin_panic_as_terminating() {
        let ir = lower(
            r#"
                package main

                func f(x bool) {
                    panic("stop")
                }

                func g() {
                    panic("stop")
                    _ = 1
                }

                func h(x bool) {
                    if x {
                        panic("left")
                    } else {
                        panic("right")
                    }
                }
            "#,
        );
        let Some(Item::Func(f)) = ir.items.first() else {
            panic!("expected first function");
        };
        let Some(f_body) = &f.body else {
            panic!("expected first function body");
        };
        assert_eq!(super::block_completion(f_body), Completion::Terminates);

        let Some(Item::Func(g)) = ir.items.get(1) else {
            panic!("expected second function");
        };
        let Some(g_body) = &g.body else {
            panic!("expected second function body");
        };
        assert_eq!(super::block_completion(g_body), Completion::MayComplete);
        assert_eq!(
            super::stmt_completion(&g_body.stmts[0]),
            Completion::Terminates
        );

        let Some(Item::Func(h)) = ir.items.get(2) else {
            panic!("expected third function");
        };
        let Some(h_body) = &h.body else {
            panic!("expected third function body");
        };
        assert_eq!(super::block_completion(h_body), Completion::Terminates);
    }

    #[test]
    fn classifies_for_completion_and_break_targets() {
        let ir = lower(
            r#"
                package main

                func f(x bool, xs []int) {
                    for {
                    }
                    for {
                        break
                    }
                Outer:
                    for {
                        if x {
                            break Outer
                        }
                    }
                    for {
                        switch {
                        default:
                            break
                        }
                    }
                OuterNested:
                    for {
                        switch {
                        default:
                            break OuterNested
                        }
                    }
                    for range xs {
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        assert_eq!(
            super::stmt_completion(&body.stmts[0]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[1]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[2]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[3]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[4]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[5]),
            Completion::MayComplete
        );
    }

    #[test]
    fn classifies_breaks_for_switch_and_select_completion() {
        let ir = lower(
            r#"
                package main

                func f(x int, ch chan int) {
                    switch x {
                    default:
                        return
                    }
                    switch x {
                    default:
                        break
                    }
                SwitchLabel:
                    switch x {
                    default:
                        for {
                            break SwitchLabel
                        }
                        return
                    }
                    switch x {
                    case 1:
                    Label:
                        fallthrough
                    default:
                        return
                    }
                    select {
                    default:
                        return
                    }
                    select {
                    default:
                        break
                    }
                SelectLabel:
                    select {
                    default:
                        for {
                            break SelectLabel
                        }
                        return
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        assert_eq!(
            super::stmt_completion(&body.stmts[0]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[1]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[2]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[3]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[4]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[5]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[6]),
            Completion::MayComplete
        );
    }

    #[test]
    fn lower_type_switch_records_cases() {
        let ir = lower(
            r#"
                package main

                func f(x any) {
                    switch v := x.(type) {
                    case int:
                        _ = v
                    default:
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        let Some(Stmt::TypeSwitch { assign, cases, .. }) = body.stmts.first() else {
            panic!("expected type switch");
        };
        assert!(matches!(assign.as_ref(), Stmt::Assign(_)));
        assert_eq!(cases.len(), 2);
        assert!(!cases.first().is_some_and(|case| case.is_default));
        assert!(cases.get(1).is_some_and(|case| case.is_default));
    }

    #[test]
    fn classifies_control_flow_completion() {
        let ir = lower(
            r#"
                package main

                func f(x int) {
                    if x > 0 {
                        return
                    } else {
                        return
                    }
                    if x > 0 {
                        return
                    }
                    switch x {
                    case 1:
                        return
                    default:
                        return
                    }
                    switch x {
                    case 1:
                        return
                    }
                    switch x {
                    case 1:
                        fallthrough
                    default:
                        return
                    }
                }
            "#,
        );
        let Some(Item::Func(func)) = ir.items.first() else {
            panic!("expected function item");
        };
        let Some(body) = &func.body else {
            panic!("expected function body");
        };
        assert_eq!(
            super::stmt_completion(&body.stmts[0]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[1]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[2]),
            Completion::Terminates
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[3]),
            Completion::MayComplete
        );
        assert_eq!(
            super::stmt_completion(&body.stmts[4]),
            Completion::Terminates
        );
    }
}
