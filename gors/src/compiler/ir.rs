//! Typed Go IR used as the semantic layer between the parser AST and Rust codegen.

use std::collections::BTreeSet;

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
    Func {
        name: Some(func.name.name.to_string()),
        receiver: func
            .recv
            .as_ref()
            .map_or_else(Vec::new, |receiver| lower_fields(receiver)),
        signature: lower_signature(&func.type_),
        body: func.body.as_ref().map(|body| lower_block(body, env)),
        captures: Vec::new(),
    }
}

fn lower_func_lit(func_lit: &ast::FuncLit<'_>, env: &TypeEnv) -> Func {
    Func {
        name: None,
        receiver: Vec::new(),
        signature: lower_signature(&func_lit.type_),
        body: Some(lower_block(&func_lit.body, env)),
        captures: func_lit_captures(func_lit, env),
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
    Block {
        stmts: block
            .list
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, env))
            .collect(),
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

fn lower_stmt(stmt: &ast::Stmt<'_>, env: &TypeEnv) -> Option<Stmt> {
    match stmt {
        ast::Stmt::AssignStmt(assign) => Some(Stmt::Assign(Assign {
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
        })),
        ast::Stmt::BlockStmt(block) => Some(Stmt::Block(lower_block(block, env))),
        ast::Stmt::BranchStmt(branch) => Some(Stmt::Branch {
            kind: lower_branch_kind(branch.tok),
            label: branch.label.as_ref().map(|label| label.name.to_string()),
        }),
        ast::Stmt::CaseClause(case) => Some(Stmt::Case(lower_case(case, env))),
        ast::Stmt::CommClause(comm) => Some(Stmt::Comm(lower_comm_case(comm, env))),
        ast::Stmt::DeclStmt(decl) => Some(Stmt::Decl(lower_gen_decl(&decl.decl, env))),
        ast::Stmt::DeferStmt(defer) => Some(Stmt::Defer(lower_call(&defer.call, env))),
        ast::Stmt::EmptyStmt(_) => Some(Stmt::Empty),
        ast::Stmt::ExprStmt(expr) => Some(Stmt::Expr(lower_expr(&expr.x, env))),
        ast::Stmt::ForStmt(for_stmt) => Some(Stmt::For {
            init: for_stmt
                .init
                .as_ref()
                .and_then(|init| lower_stmt(init, env).map(Box::new)),
            cond: for_stmt.cond.as_ref().map(|cond| lower_expr(cond, env)),
            post: for_stmt
                .post
                .as_ref()
                .and_then(|post| lower_stmt(post, env).map(Box::new)),
            body: lower_block(&for_stmt.body, env),
        }),
        ast::Stmt::GoStmt(go) => Some(Stmt::Go(lower_call(&go.call, env))),
        ast::Stmt::IfStmt(if_stmt) => Some(Stmt::If {
            init: if_stmt
                .init
                .as_ref()
                .as_ref()
                .and_then(|init| lower_stmt(init, env).map(Box::new)),
            cond: lower_expr(&if_stmt.cond, env),
            body: lower_block(&if_stmt.body, env),
            else_branch: if_stmt
                .else_
                .as_ref()
                .as_ref()
                .and_then(|else_branch| lower_stmt(else_branch, env).map(Box::new)),
        }),
        ast::Stmt::IncDecStmt(inc_dec) => Some(Stmt::IncDec {
            expr: lower_expr(&inc_dec.x, env),
            op: lower_inc_dec_op(inc_dec.tok),
        }),
        ast::Stmt::LabeledStmt(label) => lower_stmt(&label.stmt, env).map(|stmt| Stmt::Label {
            name: label.label.name.to_string(),
            stmt: Box::new(stmt),
        }),
        ast::Stmt::RangeStmt(range) => Some(Stmt::Range {
            key: range.key.as_ref().map(|key| lower_expr(key, env)),
            value: range.value.as_ref().map(|value| lower_expr(value, env)),
            define: matches!(range.tok, Some(token::Token::DEFINE)),
            expr: lower_expr(&range.x, env),
            body: lower_block(&range.body, env),
        }),
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
    Case {
        exprs: case.list.as_ref().map_or_else(Vec::new, |exprs| {
            exprs.iter().map(|expr| lower_expr(expr, env)).collect()
        }),
        body: case
            .body
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, env))
            .collect(),
        is_default: case.list.is_none(),
    }
}

fn lower_comm_case(comm: &ast::CommClause<'_>, env: &TypeEnv) -> CommCase {
    CommCase {
        comm: comm
            .comm
            .as_ref()
            .and_then(|stmt| lower_stmt(stmt, env).map(Box::new)),
        body: comm
            .body
            .iter()
            .filter_map(|stmt| lower_stmt(stmt, env))
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
            if !matches!(ident.name, "_" | "nil" | "true" | "false" | "iota") =>
        {
            Addressability::Addressable
        }
        ast::Expr::IndexExpr(index) => {
            let container = GoType::infer_expr(&index.x, env);
            match env.resolve_alias(&container) {
                GoType::Map(_, _) | GoType::String => Addressability::NotAddressable,
                _ => Addressability::Addressable,
            }
        }
        ast::Expr::ParenExpr(paren) => expr_addressability(&paren.x, env),
        ast::Expr::SelectorExpr(_) | ast::Expr::StarExpr(_) => Addressability::Addressable,
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
    let mut declared = BTreeSet::new();
    collect_signature_bindings(&func_lit.type_, &mut declared);
    collect_declared_names_in_block(&func_lit.body, &mut declared);

    let mut referenced = BTreeSet::new();
    collect_referenced_names_in_block(&func_lit.body, &mut referenced);

    let mut mutated = BTreeSet::new();
    collect_mutated_names_in_block(&func_lit.body, &mut mutated);

    referenced
        .into_iter()
        .filter(|name| !declared.contains(name) && !is_predeclared_name(name))
        .map(|name| {
            let mode = if mutated.contains(&name) {
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

pub fn mutable_func_lit_capture_names_in_block(
    block: &ast::BlockStmt<'_>,
    env: &TypeEnv,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_mutable_func_lit_capture_names_in_block(block, env, &mut names);
    names
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

fn collect_mutated_names_in_block(block: &ast::BlockStmt<'_>, names: &mut BTreeSet<String>) {
    for stmt in &block.list {
        collect_mutated_names_in_stmt(stmt, names);
    }
}

fn collect_mutated_names_in_stmt(stmt: &ast::Stmt<'_>, names: &mut BTreeSet<String>) {
    match stmt {
        ast::Stmt::AssignStmt(assign) if assign.tok != token::Token::DEFINE => {
            names.extend(assign.lhs.iter().filter_map(ident_name));
        }
        ast::Stmt::BlockStmt(block) => collect_mutated_names_in_block(block, names),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_mutated_names_in_stmt(init, names);
            }
            if let Some(post) = &for_stmt.post {
                collect_mutated_names_in_stmt(post, names);
            }
            collect_mutated_names_in_block(&for_stmt.body, names);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                collect_mutated_names_in_stmt(init, names);
            }
            collect_mutated_names_in_block(&if_stmt.body, names);
            if let Some(else_branch) = if_stmt.else_.as_ref().as_ref() {
                collect_mutated_names_in_stmt(else_branch, names);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            if let Some(name) = ident_name(&inc_dec.x) {
                names.insert(name);
            }
        }
        ast::Stmt::RangeStmt(range) => collect_mutated_names_in_block(&range.body, names),
        ast::Stmt::SwitchStmt(switch) => collect_mutated_names_in_block(&switch.body, names),
        ast::Stmt::LabeledStmt(label) => collect_mutated_names_in_stmt(&label.stmt, names),
        _ => {}
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
                    text := "go"
                    for range nums {}
                    for range dict {}
                    for range ch {}
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
            .take(4)
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
        assert_eq!(capture.ty, GoType::Unknown);
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
