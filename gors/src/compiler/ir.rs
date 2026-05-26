//! Typed Go IR used as the semantic layer between the parser AST and Rust codegen.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ast, token};

use super::typeinfer::{GoType, TypeEnv};

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
        GoType::Chan(_) => RangeKind::Channel,
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
                env.set_var(name.name, ty);
            } else {
                env.set_var(name.name, ty);
            }
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

fn record_range_bindings(range: &ast::RangeStmt<'_>, env: &mut TypeEnv) {
    if range.tok != Some(token::Token::DEFINE) {
        return;
    }
    let range_type = env.resolve_alias(&GoType::infer_expr(&range.x, env));
    let (key_type, value_type) = match range_type {
        GoType::String => (GoType::Int, Some(GoType::Int32)),
        GoType::Slice(elem) | GoType::Array(elem) => (GoType::Int, Some(*elem)),
        GoType::Map(key, value) => (*key, Some(*value)),
        GoType::Chan(value) => (*value, None),
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

fn record_range_binding(target: Option<&ast::Expr<'_>>, ty: GoType, env: &mut TypeEnv) {
    if let Some(ast::Expr::Ident(ident)) = target
        && ident.name != "_"
    {
        env.set_var(ident.name, ty);
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
    Defer { reason: InvalidStatementReason },
    Expr { reason: InvalidStatementReason },
    Go { reason: InvalidStatementReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidStatementReason {
    DisallowedBuiltin(String),
    NonCallOrReceive,
    TypeConversion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidLabel {
    Duplicate { label: String },
    Unused { label: String },
}

pub fn invalid_forward_goto_in_block(block: &ast::BlockStmt<'_>) -> Option<InvalidGoto> {
    let mut label_positions = BTreeMap::new();
    for (idx, stmt) in block.list.iter().enumerate() {
        for label in direct_label_names_in_stmt(stmt) {
            label_positions.entry(label).or_insert(idx);
        }
    }
    if label_positions.is_empty() {
        return None;
    }

    for (idx, stmt) in block.list.iter().enumerate() {
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
            for skipped in block.list.iter().take(target_idx).skip(idx + 1) {
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

pub fn invalid_branch_in_func(block: &ast::BlockStmt<'_>) -> Option<InvalidBranch> {
    let mut context = BranchContext::default();
    invalid_branch_in_block(block, &mut context)
}

pub fn invalid_statement_in_func(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    let mut env = env.clone();
    invalid_statement_in_block(block, &mut env)
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
    None
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
    let mut labels = vec![labeled.label.name.to_string()];
    let mut inner = labeled.stmt.as_ref();
    while let ast::Stmt::LabeledStmt(next) = inner {
        labels.push(next.label.name.to_string());
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

    for (case_order, stmt_idx) in case_indices.iter().enumerate() {
        let ast::Stmt::CaseClause(case) = &stmts[*stmt_idx] else {
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
        ast::Expr::FuncLit(_) => None,
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
) -> Option<InvalidStatement> {
    for stmt in &block.list {
        if let Some(invalid) = invalid_statement_in_stmt(stmt, env) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_statement_in_nested_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    let mut block_env = env.clone();
    invalid_statement_in_block(block, &mut block_env)
}

fn invalid_statement_in_stmt(stmt: &ast::Stmt<'_>, env: &mut TypeEnv) -> Option<InvalidStatement> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            record_define_bindings(assign, env);
            None
        }
        ast::Stmt::BlockStmt(block) => invalid_statement_in_nested_block(block, env),
        ast::Stmt::BranchStmt(_) => None,
        ast::Stmt::CaseClause(case) => {
            let mut case_env = env.clone();
            invalid_statement_in_stmt_list(&case.body, &mut case_env)
        }
        ast::Stmt::CommClause(comm) => {
            let mut comm_env = env.clone();
            if let Some(comm) = &comm.comm
                && let Some(invalid) = invalid_statement_in_stmt(comm, &mut comm_env)
            {
                return Some(invalid);
            }
            invalid_statement_in_stmt_list(&comm.body, &mut comm_env)
        }
        ast::Stmt::DeclStmt(decl) => {
            record_decl_bindings(&decl.decl, env);
            None
        }
        ast::Stmt::DeferStmt(defer) => invalid_call_statement(&defer.call, env)
            .map(|reason| InvalidStatement::Defer { reason }),
        ast::Stmt::EmptyStmt(_) => None,
        ast::Stmt::ExprStmt(expr) => invalid_expression_statement(&expr.x, env)
            .map(|reason| InvalidStatement::Expr { reason }),
        ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            if let Some(init) = &for_stmt.init
                && let Some(invalid) = invalid_statement_in_stmt(init, &mut loop_env)
            {
                return Some(invalid);
            }
            if let Some(post) = &for_stmt.post
                && let Some(invalid) = invalid_statement_in_stmt(post, &mut loop_env)
            {
                return Some(invalid);
            }
            invalid_statement_in_nested_block(&for_stmt.body, &loop_env)
        }
        ast::Stmt::GoStmt(go) => {
            invalid_call_statement(&go.call, env).map(|reason| InvalidStatement::Go { reason })
        }
        ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            if let Some(init) = if_stmt.init.as_ref().as_ref()
                && let Some(invalid) = invalid_statement_in_stmt(init, &mut if_env)
            {
                return Some(invalid);
            }
            if let Some(invalid) = invalid_statement_in_nested_block(&if_stmt.body, &if_env) {
                return Some(invalid);
            }
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                let mut else_env = if_env;
                return invalid_statement_in_stmt(else_branch, &mut else_env);
            }
            None
        }
        ast::Stmt::IncDecStmt(_) => None,
        ast::Stmt::LabeledStmt(labeled) => invalid_statement_in_stmt(&labeled.stmt, env),
        ast::Stmt::RangeStmt(range) => {
            let mut range_env = env.clone();
            record_range_bindings(range, &mut range_env);
            invalid_statement_in_nested_block(&range.body, &range_env)
        }
        ast::Stmt::ReturnStmt(_) => None,
        ast::Stmt::SelectStmt(select) => {
            let mut select_env = env.clone();
            for stmt in &select.body.list {
                if let Some(invalid) = invalid_statement_in_stmt(stmt, &mut select_env) {
                    return Some(invalid);
                }
            }
            None
        }
        ast::Stmt::SendStmt(_) => None,
        ast::Stmt::SwitchStmt(switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = &switch.init
                && let Some(invalid) = invalid_statement_in_stmt(init, &mut switch_env)
            {
                return Some(invalid);
            }
            invalid_statement_in_case_block(&switch.body, &switch_env)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = &type_switch.init
                && let Some(invalid) = invalid_statement_in_stmt(init, &mut switch_env)
            {
                return Some(invalid);
            }
            invalid_statement_in_case_block(&type_switch.body, &switch_env)
        }
    }
}

fn invalid_statement_in_stmt_list(
    stmts: &[ast::Stmt<'_>],
    env: &mut TypeEnv,
) -> Option<InvalidStatement> {
    for stmt in stmts {
        if let Some(invalid) = invalid_statement_in_stmt(stmt, env) {
            return Some(invalid);
        }
    }
    None
}

fn invalid_statement_in_case_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatement> {
    for stmt in &block.list {
        let ast::Stmt::CaseClause(case) = stmt else {
            continue;
        };
        let mut case_env = env.clone();
        if let Some(invalid) = invalid_statement_in_stmt_list(&case.body, &mut case_env) {
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

fn invalid_call_statement(
    call: &ast::CallExpr<'_>,
    env: &TypeEnv,
) -> Option<InvalidStatementReason> {
    if let Some(name) = disallowed_builtin_statement_name(call, env) {
        return Some(InvalidStatementReason::DisallowedBuiltin(name));
    }
    call_is_type_conversion(call, env).then_some(InvalidStatementReason::TypeConversion)
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
            labels.push(labeled.label.name.to_string());
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
            labels
                .entry(labeled.label.name.to_string())
                .or_insert_with(|| path.to_vec());
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
        labels.push(label.label.name.to_string());
        current = &label.stmt;
    }
    labels
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
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{Addressability, CaptureMode, Completion, ExprKind, Item, Stmt, lower_file};
    use crate::compiler::typeinfer::{GoType, TypeEnv};
    use crate::parser::parse_file;

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
            crate::ast::Decl::FuncDecl(func) => Some(func),
            crate::ast::Decl::GenDecl(_) => None,
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
