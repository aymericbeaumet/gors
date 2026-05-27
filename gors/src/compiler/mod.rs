//! Go to Rust compiler.
//!
//! This module transforms a Go AST into a Rust `syn` AST. The compilation
//! process involves:
//!
//! 1. Converting Go AST nodes to equivalent Rust AST nodes
//! 2. Applying transformation passes to produce idiomatic Rust code
//!
//! ## Limitations
//!
//! Not all Go constructs can be directly translated to Rust. Currently
//! unsupported features include:
//!
//! - Arbitrary forward goto statements
//! - Some complex type expressions

pub mod ir;
pub mod manifest;
pub(crate) mod passes;
pub mod typeinfer;

use crate::mapping::SourceMapTracker;
use crate::{ast, token};
use proc_macro2::Span;
use quote::ToTokens;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use syn::Token;

// Thread-local storage for source map tracker during compilation
thread_local! {
    static TRACKER: RefCell<SourceMapTracker> = RefCell::new(SourceMapTracker::new());
    static DEFER_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SWITCH_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static SELECT_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static LOOP_BODY_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static GOTO_STATE_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static NAMED_RETURN_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static IMPORT_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static IMPORT_RENAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static IMPORT_PACKAGE_NAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static INTERFACE_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static TYPE_ENV: RefCell<typeinfer::TypeEnv> = RefCell::new(typeinfer::TypeEnv::new());
    static STRING_CONST_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static UNNAMED_ARG_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static BORROW_POINTER_ARG_INDICES: RefCell<BTreeMap<String, std::collections::HashSet<usize>>> = const { RefCell::new(BTreeMap::new()) };
    static BORROW_POINTER_ARG_INDICES_PRESEEDED: RefCell<bool> = const { RefCell::new(false) };
    static BYTE_SEQ_TYPE_PARAMS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static RETURN_TYPES: RefCell<Vec<typeinfer::GoType>> = const { RefCell::new(Vec::new()) };
    static NAMED_RETURN_IDENTS: RefCell<Vec<syn::Ident>> = const { RefCell::new(Vec::new()) };
    static BORROWED_INTERFACE_STRUCTS: RefCell<BTreeMap<String, Vec<EmbeddedInterfaceField>>> = const { RefCell::new(BTreeMap::new()) };
    static MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS: RefCell<bool> = const { RefCell::new(false) };
    static SHARED_CAPTURE_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static GOTO_CONTINUE_LABELS: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static GOTO_STATE_CONTEXTS: RefCell<Vec<GotoStateContext>> = const { RefCell::new(Vec::new()) };
    static BORROWED_POINTER_PARAM_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static RANGE_FUNCTION_COUNTER: RefCell<usize> = const { RefCell::new(0) };
}

#[derive(Clone)]
struct EmbeddedInterfaceField {
    field_ident: syn::Ident,
    trait_path: syn::Path,
}

#[derive(Clone)]
struct ReachableItemsCacheEntry {
    keep: std::collections::HashSet<usize>,
    refs: std::collections::HashMap<String, std::collections::HashSet<String>>,
    names: std::collections::HashSet<String>,
}

static REACHABLE_ITEMS_CACHE: OnceLock<Mutex<BTreeMap<String, ReachableItemsCacheEntry>>> =
    OnceLock::new();

struct MainPackageVarModeGuard {
    previous: bool,
}

struct SharedCaptureNamesGuard {
    previous: std::collections::HashSet<String>,
}

struct GotoContinueLabelsGuard {
    previous: std::collections::HashSet<String>,
}

#[derive(Clone)]
struct GotoStateContext {
    state_ident: syn::Ident,
    loop_label: syn::Lifetime,
    labels: BTreeMap<String, usize>,
}

struct GotoStateContextGuard;

struct BorrowedPointerParamNamesGuard {
    previous: std::collections::HashSet<String>,
}

impl MainPackageVarModeGuard {
    fn set(current: bool) -> Self {
        let previous = MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| {
            let previous = *value.borrow();
            *value.borrow_mut() = current;
            previous
        });
        Self { previous }
    }
}

impl Drop for MainPackageVarModeGuard {
    fn drop(&mut self) {
        MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| {
            *value.borrow_mut() = self.previous;
        });
    }
}

impl SharedCaptureNamesGuard {
    fn extend(names: impl IntoIterator<Item = String>) -> Self {
        let previous = SHARED_CAPTURE_NAMES.with(|shared| {
            let previous = shared.borrow().clone();
            shared.borrow_mut().extend(names);
            previous
        });
        Self { previous }
    }
}

impl Drop for SharedCaptureNamesGuard {
    fn drop(&mut self) {
        SHARED_CAPTURE_NAMES.with(|shared| {
            *shared.borrow_mut() = self.previous.clone();
        });
    }
}

impl GotoContinueLabelsGuard {
    fn extend(names: impl IntoIterator<Item = String>) -> Self {
        let previous = GOTO_CONTINUE_LABELS.with(|labels| {
            let previous = labels.borrow().clone();
            labels.borrow_mut().extend(names);
            previous
        });
        Self { previous }
    }
}

impl Drop for GotoContinueLabelsGuard {
    fn drop(&mut self) {
        GOTO_CONTINUE_LABELS.with(|labels| {
            *labels.borrow_mut() = self.previous.clone();
        });
    }
}

impl GotoStateContextGuard {
    fn push(context: GotoStateContext) -> Self {
        GOTO_STATE_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().push(context);
        });
        Self
    }
}

impl Drop for GotoStateContextGuard {
    fn drop(&mut self) {
        GOTO_STATE_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().pop();
        });
    }
}

impl BorrowedPointerParamNamesGuard {
    fn set(names: std::collections::HashSet<String>) -> Self {
        let previous = BORROWED_POINTER_PARAM_NAMES.with(|borrowed| {
            let previous = borrowed.borrow().clone();
            *borrowed.borrow_mut() = names;
            previous
        });
        Self { previous }
    }
}

impl Drop for BorrowedPointerParamNamesGuard {
    fn drop(&mut self) {
        BORROWED_POINTER_PARAM_NAMES.with(|borrowed| {
            *borrowed.borrow_mut() = self.previous.clone();
        });
    }
}

struct ProfileTimer {
    label: &'static str,
    start: Option<Instant>,
}

impl ProfileTimer {
    fn start(label: &'static str) -> Self {
        let enabled = std::env::var("GORS_PROFILE")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Self {
            label,
            start: enabled.then(Instant::now),
        }
    }
}

impl Drop for ProfileTimer {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        eprintln!(
            "[gors-profile] {}: {:.2}ms",
            self.label,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}

fn is_type_interface(name: &str) -> bool {
    TYPE_ENV.with(|env| env.borrow().is_interface(name))
}

fn go_type_interface_name(go_type: &typeinfer::GoType) -> Option<String> {
    match go_type {
        typeinfer::GoType::Interface(name) => Some(name.clone()),
        typeinfer::GoType::Named(name) if is_type_interface(name) => Some(name.clone()),
        typeinfer::GoType::Named(name) if TYPE_ENV.with(|env| env.borrow().is_interface(name)) => {
            Some(name.clone())
        }
        _ => None,
    }
}

fn go_type_is_interface_like(go_type: &typeinfer::GoType) -> bool {
    go_type.is_interface() || go_type_interface_name(go_type).is_some()
}

fn interface_trait_path_from_name(name: &str) -> syn::Path {
    let mut segments = syn::punctuated::Punctuated::new();
    for part in name.split('.') {
        let ident = syn::Ident::new(&rust_safe_ident_name(part), Span::mixed_site());
        segments.push(syn::PathSegment {
            ident,
            arguments: syn::PathArguments::None,
        });
    }
    syn::Path {
        leading_colon: None,
        segments,
    }
}

fn interface_trait_type_from_expr(expr: &ast::Expr) -> Option<syn::Type> {
    is_interface_expr(expr).then(|| type_from_expr_ref(expr))
}

fn borrowed_interface_value_type_from_expr(expr: &ast::Expr) -> Option<syn::Type> {
    let trait_ty = interface_trait_type_from_expr(expr)?;
    Some(syn::parse_quote! { &mut dyn #trait_ty })
}

fn boxed_interface_value_type_from_expr(expr: &ast::Expr) -> Option<syn::Type> {
    let trait_ty = interface_trait_type_from_expr(expr)?;
    Some(syn::parse_quote! { Box<dyn #trait_ty> })
}

fn local_value_type_from_expr(expr: &ast::Expr) -> syn::Type {
    borrowed_interface_value_type_from_expr(expr).unwrap_or_else(|| type_from_expr_ref(expr))
}

fn static_value_type_from_expr(expr: &ast::Expr) -> syn::Type {
    boxed_interface_value_type_from_expr(expr).unwrap_or_else(|| type_from_expr_ref(expr))
}

fn qualify_interface_param_name(
    package_name: &str,
    go_type: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
) -> Option<String> {
    match env.resolve_alias(go_type) {
        typeinfer::GoType::Interface(name) | typeinfer::GoType::Named(name) => {
            if env.is_interface(&name) {
                Some(name)
            } else if name.contains('.') {
                None
            } else {
                let qualified = format!("{package_name}.{name}");
                env.is_interface(&qualified).then_some(qualified)
            }
        }
        typeinfer::GoType::Pointer(inner) => {
            qualify_interface_param_name(package_name, &inner, env)
        }
        _ => None,
    }
}

fn collect_needed_imported_interface_method_sets<'src>(
    decls: &[ast::Decl<'src>],
) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        for decl in decls {
            collect_decl_needed_imported_interface_method_sets(decl, &env, &mut out);
        }
    });
    out
}

fn collect_decl_needed_imported_interface_method_sets(
    decl: &ast::Decl<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match decl {
        ast::Decl::FuncDecl(func) => {
            if let Some(body) = &func.body {
                collect_block_needed_imported_interface_method_sets(body, env, out);
            }
        }
        ast::Decl::GenDecl(gen_decl) => {
            for spec in &gen_decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    if let Some(values) = &value.values {
                        for expr in values {
                            collect_expr_needed_imported_interface_method_sets(expr, env, out);
                        }
                    }
                }
            }
        }
    }
}

fn collect_block_needed_imported_interface_method_sets(
    block: &ast::BlockStmt<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    for stmt in &block.list {
        collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
    }
}

fn collect_stmt_needed_imported_interface_method_sets(
    stmt: &ast::Stmt<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_expr_needed_imported_interface_method_sets(expr, env, out);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_block_needed_imported_interface_method_sets(block, env, out);
        }
        ast::Stmt::CaseClause(case) => {
            if let Some(list) = &case.list {
                for expr in list {
                    collect_expr_needed_imported_interface_method_sets(expr, env, out);
                }
            }
            for stmt in &case.body {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
            for stmt in &comm.body {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec {
                    if let Some(values) = &value.values {
                        for expr in values {
                            collect_expr_needed_imported_interface_method_sets(expr, env, out);
                        }
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer) => {
            collect_call_needed_imported_interface_method_sets(&defer.call, env, out);
        }
        ast::Stmt::ExprStmt(expr) => {
            collect_expr_needed_imported_interface_method_sets(&expr.x, env, out);
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_stmt_needed_imported_interface_method_sets(init, env, out);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_expr_needed_imported_interface_method_sets(cond, env, out);
            }
            if let Some(post) = &for_stmt.post {
                collect_stmt_needed_imported_interface_method_sets(post, env, out);
            }
            collect_block_needed_imported_interface_method_sets(&for_stmt.body, env, out);
        }
        ast::Stmt::GoStmt(go) => {
            collect_call_needed_imported_interface_method_sets(&go.call, env, out);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_stmt_needed_imported_interface_method_sets(init, env, out);
            }
            collect_expr_needed_imported_interface_method_sets(&if_stmt.cond, env, out);
            collect_block_needed_imported_interface_method_sets(&if_stmt.body, env, out);
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_stmt_needed_imported_interface_method_sets(else_stmt, env, out);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => {
            collect_expr_needed_imported_interface_method_sets(&inc_dec.x, env, out);
        }
        ast::Stmt::LabeledStmt(labeled) => {
            collect_stmt_needed_imported_interface_method_sets(&labeled.stmt, env, out);
        }
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_expr_needed_imported_interface_method_sets(key, env, out);
            }
            if let Some(value) = &range.value {
                collect_expr_needed_imported_interface_method_sets(value, env, out);
            }
            collect_expr_needed_imported_interface_method_sets(&range.x, env, out);
            collect_block_needed_imported_interface_method_sets(&range.body, env, out);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_expr_needed_imported_interface_method_sets(expr, env, out);
            }
        }
        ast::Stmt::SelectStmt(select) => {
            for stmt in &select.body.list {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_expr_needed_imported_interface_method_sets(&send.chan, env, out);
            collect_expr_needed_imported_interface_method_sets(&send.value, env, out);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_stmt_needed_imported_interface_method_sets(init, env, out);
            }
            if let Some(tag) = &switch.tag {
                collect_expr_needed_imported_interface_method_sets(tag, env, out);
            }
            for stmt in &switch.body.list {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_stmt_needed_imported_interface_method_sets(init, env, out);
            }
            collect_stmt_needed_imported_interface_method_sets(&type_switch.assign, env, out);
            for stmt in &type_switch.body.list {
                collect_stmt_needed_imported_interface_method_sets(stmt, env, out);
            }
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_expr_needed_imported_interface_method_sets(
    expr: &ast::Expr<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_expr_needed_imported_interface_method_sets(len, env, out);
            }
            collect_expr_needed_imported_interface_method_sets(&array.elt, env, out);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_expr_needed_imported_interface_method_sets(&binary.x, env, out);
            collect_expr_needed_imported_interface_method_sets(&binary.y, env, out);
        }
        ast::Expr::CallExpr(call) => {
            collect_call_needed_imported_interface_method_sets(call, env, out);
        }
        ast::Expr::ChanType(chan) => {
            collect_expr_needed_imported_interface_method_sets(&chan.value, env, out);
        }
        ast::Expr::CompositeLit(lit) => {
            if let Some(type_) = &lit.type_ {
                collect_expr_needed_imported_interface_method_sets(type_, env, out);
            }
            if let Some(elts) = &lit.elts {
                for elt in elts {
                    collect_expr_needed_imported_interface_method_sets(elt, env, out);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_expr_needed_imported_interface_method_sets(elt, env, out);
            }
        }
        ast::Expr::FuncLit(func) => {
            collect_block_needed_imported_interface_method_sets(&func.body, env, out);
        }
        ast::Expr::FuncType(func) => {
            for field in &func.params.list {
                if let Some(type_) = &field.type_ {
                    collect_expr_needed_imported_interface_method_sets(type_, env, out);
                }
            }
            if let Some(results) = &func.results {
                for field in &results.list {
                    if let Some(type_) = &field.type_ {
                        collect_expr_needed_imported_interface_method_sets(type_, env, out);
                    }
                }
            }
        }
        ast::Expr::IndexExpr(index) => {
            collect_expr_needed_imported_interface_method_sets(&index.x, env, out);
            collect_expr_needed_imported_interface_method_sets(&index.index, env, out);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_expr_needed_imported_interface_method_sets(&index.x, env, out);
            for expr in &index.indices {
                collect_expr_needed_imported_interface_method_sets(expr, env, out);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                for field in &methods.list {
                    if let Some(type_) = &field.type_ {
                        collect_expr_needed_imported_interface_method_sets(type_, env, out);
                    }
                }
            }
        }
        ast::Expr::KeyValueExpr(key_value) => {
            collect_expr_needed_imported_interface_method_sets(&key_value.key, env, out);
            collect_expr_needed_imported_interface_method_sets(&key_value.value, env, out);
        }
        ast::Expr::MapType(map) => {
            collect_expr_needed_imported_interface_method_sets(&map.key, env, out);
            collect_expr_needed_imported_interface_method_sets(&map.value, env, out);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_expr_needed_imported_interface_method_sets(&paren.x, env, out);
        }
        ast::Expr::SelectorExpr(selector) => {
            collect_expr_needed_imported_interface_method_sets(&selector.x, env, out);
        }
        ast::Expr::SliceExpr(slice) => {
            collect_expr_needed_imported_interface_method_sets(&slice.x, env, out);
            if let Some(low) = &slice.low {
                collect_expr_needed_imported_interface_method_sets(low, env, out);
            }
            if let Some(high) = &slice.high {
                collect_expr_needed_imported_interface_method_sets(high, env, out);
            }
            if let Some(max) = &slice.max {
                collect_expr_needed_imported_interface_method_sets(max, env, out);
            }
        }
        ast::Expr::StarExpr(star) => {
            collect_expr_needed_imported_interface_method_sets(&star.x, env, out);
        }
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                for field in &fields.list {
                    if let Some(type_) = &field.type_ {
                        collect_expr_needed_imported_interface_method_sets(type_, env, out);
                    }
                }
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_expr_needed_imported_interface_method_sets(&assert.x, env, out);
            if let Some(type_) = &assert.type_ {
                collect_expr_needed_imported_interface_method_sets(type_, env, out);
            }
        }
        ast::Expr::UnaryExpr(unary) => {
            collect_expr_needed_imported_interface_method_sets(&unary.x, env, out);
        }
        ast::Expr::BasicLit(_) | ast::Expr::Ident(_) => {}
    }
}

fn collect_call_needed_imported_interface_method_sets(
    call: &ast::CallExpr<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    if let ast::Expr::SelectorExpr(selector) = &*call.fun {
        if let ast::Expr::Ident(package) = &*selector.x {
            let function_name = format!("{}.{}", package.name, selector.sel.name);
            for param in env.get_func_params(&function_name) {
                if let Some(interface_name) =
                    qualify_interface_param_name(package.name, &param, env)
                {
                    if let Some(methods) = env.get_interface_methods(&interface_name) {
                        if !methods.is_empty() {
                            out.entry(interface_name).or_insert(methods);
                        }
                    }
                }
            }
        }
    }
    collect_expr_needed_imported_interface_method_sets(&call.fun, env, out);
    if let Some(args) = &call.args {
        for arg in args {
            collect_expr_needed_imported_interface_method_sets(arg, env, out);
        }
    }
}

fn interface_trait_path_from_expr(expr: &ast::Expr) -> Option<syn::Path> {
    match expr {
        ast::Expr::ParenExpr(paren) => interface_trait_path_from_expr(&paren.x),
        ast::Expr::Ident(ident) if is_type_interface(ident.name) => {
            Some(interface_trait_path_from_name(ident.name))
        }
        ast::Expr::Ident(ident) if TYPE_ENV.with(|env| env.borrow().is_interface(ident.name)) => {
            Some(interface_trait_path_from_name(ident.name))
        }
        ast::Expr::SelectorExpr(selector) => selector_type_env_name(selector)
            .filter(|name| TYPE_ENV.with(|env| env.borrow().is_interface(name)))
            .map(|_| {
                let mut segments = syn::punctuated::Punctuated::new();
                if let ast::Expr::Ident(pkg) = &*selector.x {
                    segments.push(syn::PathSegment {
                        ident: syn::Ident::new(&import_rust_name(pkg.name), Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    });
                }
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new(
                        &rust_safe_ident_name(selector.sel.name),
                        Span::mixed_site(),
                    ),
                    arguments: syn::PathArguments::None,
                });
                syn::Path {
                    leading_colon: None,
                    segments,
                }
            }),
        _ => None,
    }
}

fn get_func_returns(name: &str) -> Vec<typeinfer::GoType> {
    TYPE_ENV.with(|env| env.borrow().get_func_returns(name))
}

fn set_type_env(type_env: typeinfer::TypeEnv) {
    STRING_CONST_NAMES.with(|names| {
        *names.borrow_mut() = type_env.string_const_names();
    });
    TYPE_ENV.with(|env| {
        *env.borrow_mut() = type_env;
    });
}

fn set_import_renames(import_renames: BTreeMap<String, String>) {
    IMPORT_RENAMES.with(|renames| {
        *renames.borrow_mut() = import_renames;
    });
}

fn set_import_package_names(import_package_names: BTreeMap<String, String>) {
    IMPORT_PACKAGE_NAMES.with(|names| {
        *names.borrow_mut() = import_package_names;
    });
}

fn import_local_name(import: &ast::ImportSpec<'_>) -> Option<String> {
    let path = import.path.value.trim_matches('"');
    if let Some(name) = &import.name {
        return (!matches!(name.name, "." | "_")).then(|| name.name.to_string());
    }
    IMPORT_PACKAGE_NAMES
        .with(|names| names.borrow().get(path).cloned())
        .or_else(|| crate::resolve::scan_type_env(path).map(|(package_name, _)| package_name))
        .or_else(|| path.rsplit('/').next().map(str::to_string))
}

fn file_import_package_names(file: &ast::File<'_>) -> BTreeMap<String, String> {
    file.imports()
        .into_iter()
        .filter(|import| import.name.is_none())
        .filter_map(|import| {
            let path = import.path.value.trim_matches('"');
            crate::resolve::scan_type_env(path)
                .map(|(package_name, _)| (path.to_string(), package_name))
        })
        .collect()
}

fn import_rust_name(name: &str) -> String {
    let renamed = IMPORT_RENAMES.with(|renames| {
        renames
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    });
    rust_safe_ident_name(&renamed)
}

fn rust_safe_ident_name(name: &str) -> String {
    if name == "_" {
        "_gors_blank".to_string()
    } else if is_rust_keyword(name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "optimize"
            | "yield"
    )
}

fn is_string_const_fn(name: &str) -> bool {
    STRING_CONST_NAMES.with(|names| names.borrow().contains(name))
}

fn is_active_string_const_fn(name: &str) -> bool {
    is_string_const_fn(name) && TYPE_ENV.with(|env| env.borrow().is_const(name))
}

fn selector_string_const_key(selector_expr: &ast::SelectorExpr) -> Option<String> {
    let ast::Expr::Ident(package) = &*selector_expr.x else {
        return None;
    };
    Some(format!("{}.{}", package.name, selector_expr.sel.name))
}

fn is_active_selector_string_const_fn(selector_expr: &ast::SelectorExpr) -> bool {
    selector_string_const_key(selector_expr)
        .as_deref()
        .is_some_and(is_active_string_const_fn)
}

/// Record a mapping if tracking is enabled.
fn record_mapping(pos: &token::Position, name: Option<&str>) {
    let source = if pos.file.is_empty() {
        None
    } else if pos.file.starts_with('/') {
        Some(pos.file.to_string())
    } else {
        Some(format!("{}/{}", pos.directory, pos.file))
    };
    TRACKER.with(|t| {
        t.borrow_mut()
            .record_for_source(source, pos.line as u32, pos.column as u32, name);
    });
}

fn interpret_go_string_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some('r') => result.push('\r'),
            Some('\\') => result.push('\\'),
            Some('"') => result.push('"'),
            Some('\'') => result.push('\''),
            Some('a') => result.push('\x07'),
            Some('b') => result.push('\x08'),
            Some('f') => result.push('\x0C'),
            Some('v') => result.push('\x0B'),
            Some('0') => result.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(val) = u8::from_str_radix(&hex, 16) {
                    result.push(val as char);
                } else {
                    result.push('\\');
                    result.push('x');
                    result.push_str(&hex);
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push('u');
                    result.push_str(&hex);
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push('U');
                    result.push_str(&hex);
                }
            }
            Some(other) => {
                // Octal escapes (e.g., \377)
                if other.is_ascii_digit() {
                    let mut oct = String::new();
                    oct.push(other);
                    for _ in 0..2 {
                        if let Some(&next) = chars.as_str().chars().next().as_ref() {
                            if next.is_ascii_digit() {
                                oct.push(chars.next().unwrap_or('0'));
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(val) = u8::from_str_radix(&oct, 8) {
                        result.push(val as char);
                    } else {
                        result.push('\\');
                        result.push_str(&oct);
                    }
                } else {
                    result.push('\\');
                    result.push(other);
                }
            }
            None => result.push('\\'),
        }
    }
    result
}

/// Convert a Go type constraint expression to Rust trait bounds.
///
/// Maps common Go constraints to appropriate Rust trait bounds:
/// - `any` / `interface{}` → no bounds
/// - `comparable` → `PartialEq`
/// - Union types like `int | float64` → `PartialOrd + Copy + Display`
fn go_constraint_to_rust_bounds(
    constraint: &ast::Expr,
) -> syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]> {
    let mut bounds = syn::punctuated::Punctuated::new();

    match constraint {
        ast::Expr::Ident(ident) => {
            match ident.name {
                "any" => {
                    // No bounds needed
                }
                "comparable" => {
                    bounds.push(syn::parse_quote! { PartialEq });
                    bounds.push(syn::parse_quote! { Clone });
                }
                "bool" | "string" | "int" | "int8" | "int16" | "int32" | "int64" | "uint"
                | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr" | "float32" | "float64" => {
                    bounds.push(syn::parse_quote! { PartialEq });
                    bounds.push(syn::parse_quote! { PartialOrd });
                    bounds.push(syn::parse_quote! { Clone });
                }
                _ => {
                    // Named constraint: use as-is
                    let name =
                        syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
                    bounds.push(syn::parse_quote! { #name });
                }
            }
        }
        ast::Expr::InterfaceType(_) => {
            // interface{} → no bounds (same as `any`)
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(base) = &*selector.x {
                let base = syn::Ident::new(&rust_safe_ident_name(base.name), Span::mixed_site());
                let sel =
                    syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
                bounds.push(syn::parse_quote! { #base::#sel });
            }
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::TILDE => {
            bounds.extend(go_constraint_to_rust_bounds(&unary.x));
        }
        ast::Expr::BinaryExpr(bin) if bin.op == token::Token::OR => {
            if is_string_or_byte_slice_union(constraint) {
                bounds.push(syn::parse_quote! { crate::builtin::ByteSeq });
                bounds.push(syn::parse_quote! { crate::builtin::Len });
                bounds.push(syn::parse_quote! { Clone });
                bounds.push(syn::parse_quote! { PartialEq });
                bounds.push(syn::parse_quote! { PartialOrd });
                return dedupe_type_param_bounds(bounds);
            }
            // Union type like `int | float64` → approximate with common traits
            append_type_param_bounds(&mut bounds, go_constraint_to_rust_bounds(&bin.x));
            append_type_param_bounds(&mut bounds, go_constraint_to_rust_bounds(&bin.y));
            if bounds.is_empty() {
                bounds.push(syn::parse_quote! { PartialEq });
                bounds.push(syn::parse_quote! { PartialOrd });
                bounds.push(syn::parse_quote! { Clone });
            }
            bounds.push(syn::parse_quote! { PartialOrd });
            bounds.push(syn::parse_quote! { Clone });
        }
        _ => {
            // Fallback: no bounds
        }
    }

    dedupe_type_param_bounds(bounds)
}

fn dedupe_type_param_bounds(
    bounds: syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>,
) -> syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = syn::punctuated::Punctuated::new();
    for bound in bounds {
        if seen.insert(bound.to_token_stream().to_string()) {
            deduped.push(bound);
        }
    }
    deduped
}

fn append_type_param_bounds(
    target: &mut syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>,
    source: syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>,
) {
    for bound in source {
        if !target.iter().any(|existing| {
            existing.to_token_stream().to_string() == bound.to_token_stream().to_string()
        }) {
            target.push(bound);
        }
    }
}

#[derive(Default)]
struct TypeParamInfo {
    names: std::collections::HashSet<String>,
    slice_aliases: BTreeMap<String, syn::Type>,
    slice_alias_go_types: BTreeMap<String, typeinfer::GoType>,
    byte_seq_names: std::collections::HashSet<String>,
}

impl TypeParamInfo {
    fn skipped_names(&self) -> std::collections::HashSet<&str> {
        self.slice_aliases.keys().map(String::as_str).collect()
    }
}

fn collect_type_param_info(type_params: Option<&ast::FieldList>) -> TypeParamInfo {
    let Some(type_params) = type_params else {
        return TypeParamInfo::default();
    };

    let mut info = TypeParamInfo::default();
    for field in &type_params.list {
        if let Some(names) = &field.names {
            for name in names {
                info.names.insert(name.name.to_string());
            }
        }
    }
    for field in &type_params.list {
        let Some(elem) = field.type_.as_ref().and_then(slice_alias_element_type) else {
            continue;
        };
        let go_type = field
            .type_
            .as_ref()
            .and_then(slice_alias_element_go_type)
            .map(|elem| typeinfer::GoType::Slice(Box::new(elem)));
        if let Some(names) = &field.names {
            for name in names {
                info.slice_aliases
                    .insert(name.name.to_string(), elem.clone());
                if let Some(go_type) = &go_type {
                    info.slice_alias_go_types
                        .insert(name.name.to_string(), go_type.clone());
                }
            }
        }
    }
    for field in &type_params.list {
        if !field
            .type_
            .as_ref()
            .is_some_and(is_string_or_byte_slice_union)
        {
            continue;
        }
        if let Some(names) = &field.names {
            for name in names {
                info.byte_seq_names.insert(name.name.to_string());
            }
        }
    }
    info
}

fn is_string_or_byte_slice_union(expr: &ast::Expr) -> bool {
    fn collect(expr: &ast::Expr, has_string: &mut bool, has_byte_slice: &mut bool) {
        match expr {
            ast::Expr::BinaryExpr(binary) if binary.op == token::Token::OR => {
                collect(&binary.x, has_string, has_byte_slice);
                collect(&binary.y, has_string, has_byte_slice);
            }
            ast::Expr::Ident(ident) if ident.name == "string" => *has_string = true,
            ast::Expr::ArrayType(array)
                if array.len.is_none()
                    && matches!(&*array.elt, ast::Expr::Ident(ident) if ident.name == "byte" || ident.name == "uint8") =>
            {
                *has_byte_slice = true;
            }
            _ => {}
        }
    }

    let mut has_string = false;
    let mut has_byte_slice = false;
    collect(expr, &mut has_string, &mut has_byte_slice);
    has_string && has_byte_slice
}

fn slice_alias_element_type(expr: &ast::Expr) -> Option<syn::Type> {
    match expr {
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::TILDE => {
            slice_alias_element_type(&unary.x)
        }
        ast::Expr::ArrayType(array) if array.len.is_none() => rust_type_from_type_expr(&array.elt),
        _ => None,
    }
}

fn slice_alias_element_go_type(expr: &ast::Expr) -> Option<typeinfer::GoType> {
    match expr {
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::TILDE => {
            slice_alias_element_go_type(&unary.x)
        }
        ast::Expr::ArrayType(array) if array.len.is_none() => {
            Some(typeinfer::GoType::from_expr(&array.elt))
        }
        _ => None,
    }
}

fn generic_slice_param_element_type(expr: &ast::Expr, info: &TypeParamInfo) -> Option<syn::Type> {
    match expr {
        ast::Expr::Ident(ident) => info.slice_aliases.get(ident.name).cloned(),
        ast::Expr::ArrayType(array)
            if array.len.is_none() && type_expr_mentions_type_param(&array.elt, info) =>
        {
            rust_type_from_type_expr(&array.elt)
        }
        _ => None,
    }
}

fn generic_slice_param_go_type(
    expr: &ast::Expr,
    info: &TypeParamInfo,
) -> Option<typeinfer::GoType> {
    match expr {
        ast::Expr::Ident(ident) => info.slice_alias_go_types.get(ident.name).cloned(),
        ast::Expr::ArrayType(array)
            if array.len.is_none() && type_expr_mentions_type_param(&array.elt, info) =>
        {
            Some(typeinfer::GoType::Slice(Box::new(
                typeinfer::GoType::from_expr(&array.elt),
            )))
        }
        _ => None,
    }
}

fn type_expr_mentions_type_param(expr: &ast::Expr, info: &TypeParamInfo) -> bool {
    match expr {
        ast::Expr::Ident(ident) => info.names.contains(ident.name),
        ast::Expr::ArrayType(array) => type_expr_mentions_type_param(&array.elt, info),
        ast::Expr::StarExpr(star) => type_expr_mentions_type_param(&star.x, info),
        ast::Expr::UnaryExpr(unary) => type_expr_mentions_type_param(&unary.x, info),
        ast::Expr::SelectorExpr(_) => false,
        _ => false,
    }
}

fn rust_type_from_type_expr(expr: &ast::Expr) -> Option<syn::Type> {
    match expr {
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&import_rust_name(ident.name), Span::mixed_site());
            Some(syn::parse_quote! { #ident })
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(base) = &*selector.x {
                let base = syn::Ident::new(&rust_safe_ident_name(base.name), Span::mixed_site());
                let sel =
                    syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
                Some(syn::parse_quote! { #base::#sel })
            } else {
                None
            }
        }
        ast::Expr::ArrayType(array) if array.len.is_none() => {
            let elem = rust_type_from_type_expr(&array.elt)?;
            Some(syn::parse_quote! { Vec<#elem> })
        }
        ast::Expr::StarExpr(star) => {
            let elem = rust_type_from_type_expr(&star.x)?;
            Some(syn::parse_quote! { Box<#elem> })
        }
        _ => None,
    }
}

fn compile_go_type_params(type_params: Option<ast::FieldList>) -> syn::Generics {
    let Some(type_params) = type_params else {
        return syn::Generics::default();
    };

    let info = collect_type_param_info(Some(&type_params));
    let skipped = info.skipped_names();
    let mut params = syn::punctuated::Punctuated::new();
    for field in type_params.list {
        let bounds = field
            .type_
            .as_ref()
            .map(go_constraint_to_rust_bounds)
            .unwrap_or_default();
        if let Some(names) = field.names {
            for name in names {
                if skipped.contains(name.name) {
                    continue;
                }
                let ident: syn::Ident = name.into();
                params.push(syn::GenericParam::Type(syn::TypeParam {
                    attrs: vec![],
                    ident,
                    colon_token: if bounds.is_empty() {
                        None
                    } else {
                        Some(<Token![:]>::default())
                    },
                    bounds: bounds.clone(),
                    eq_token: None,
                    default: None,
                }));
            }
        }
    }

    if params.is_empty() {
        syn::Generics::default()
    } else {
        syn::Generics {
            lt_token: Some(<Token![<]>::default()),
            gt_token: Some(<Token![>]>::default()),
            params,
            where_clause: None,
        }
    }
}

fn generics_for_idents(idents: &[syn::Ident]) -> syn::Generics {
    if idents.is_empty() {
        return syn::Generics::default();
    }
    let mut params = syn::punctuated::Punctuated::new();
    for ident in idents {
        params.push(syn::GenericParam::Type(syn::TypeParam {
            attrs: vec![],
            ident: ident.clone(),
            colon_token: None,
            bounds: syn::punctuated::Punctuated::new(),
            eq_token: None,
            default: None,
        }));
    }
    syn::Generics {
        lt_token: Some(<Token![<]>::default()),
        gt_token: Some(<Token![>]>::default()),
        params,
        where_clause: None,
    }
}

#[derive(Clone, Debug)]
enum ConstValue {
    Bool(bool),
    Complex(f64, f64),
    Float(f64),
    Int(i128),
    Str(String),
    Uint(u128, u32),
}

impl ConstValue {
    fn as_i128(&self) -> Option<i128> {
        match self {
            ConstValue::Int(value) => Some(*value),
            ConstValue::Uint(value, _) => i128::try_from(*value).ok(),
            ConstValue::Float(value) if value.is_finite() && value.fract() == 0.0 => {
                Some(*value as i128)
            }
            ConstValue::Bool(_)
            | ConstValue::Complex(_, _)
            | ConstValue::Float(_)
            | ConstValue::Str(_) => None,
        }
    }

    fn as_u128(&self) -> Option<u128> {
        match self {
            ConstValue::Int(value) => u128::try_from(*value).ok(),
            ConstValue::Uint(value, _) => Some(*value),
            ConstValue::Float(value)
                if *value >= 0.0 && value.is_finite() && value.fract() == 0.0 =>
            {
                Some(*value as u128)
            }
            ConstValue::Bool(_)
            | ConstValue::Complex(_, _)
            | ConstValue::Float(_)
            | ConstValue::Str(_) => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            ConstValue::Float(value) => Some(*value),
            ConstValue::Int(value) => Some(*value as f64),
            ConstValue::Uint(value, _) => Some(*value as f64),
            ConstValue::Bool(_) | ConstValue::Complex(_, _) | ConstValue::Str(_) => None,
        }
    }

    fn as_complex128(&self) -> Option<(f64, f64)> {
        match self {
            ConstValue::Complex(re, im) => Some((*re, *im)),
            ConstValue::Float(value) => Some((*value, 0.0)),
            ConstValue::Int(value) => Some((*value as f64, 0.0)),
            ConstValue::Uint(value, _) => Some((*value as f64, 0.0)),
            ConstValue::Bool(_) | ConstValue::Str(_) => None,
        }
    }

    fn is_complex(&self) -> bool {
        matches!(self, ConstValue::Complex(_, _))
    }

    fn rust_type(&self) -> syn::Type {
        match self {
            ConstValue::Bool(_) => syn::parse_quote! { bool },
            ConstValue::Complex(_, _) => syn::parse_quote! { crate::builtin::Complex128 },
            ConstValue::Float(_) => syn::parse_quote! { f64 },
            ConstValue::Str(_) => syn::parse_quote! { &str },
            ConstValue::Int(value) if *value >= 0 && *value > isize::MAX as i128 => {
                if *value <= u64::MAX as i128 {
                    syn::parse_quote! { u64 }
                } else {
                    syn::parse_quote! { u128 }
                }
            }
            ConstValue::Int(value) if *value < isize::MIN as i128 => syn::parse_quote! { i128 },
            ConstValue::Int(_) => syn::parse_quote! { isize },
            ConstValue::Uint(_, bits) if *bits <= 8 => syn::parse_quote! { u8 },
            ConstValue::Uint(_, bits) if *bits <= 16 => syn::parse_quote! { u16 },
            ConstValue::Uint(_, bits) if *bits <= 32 => syn::parse_quote! { u32 },
            ConstValue::Uint(_, bits) if *bits <= 64 => syn::parse_quote! { u64 },
            ConstValue::Uint(_, _) => syn::parse_quote! { u128 },
        }
    }

    fn to_expr(&self) -> syn::Expr {
        match self {
            ConstValue::Bool(true) => syn::parse_quote! { true },
            ConstValue::Bool(false) => syn::parse_quote! { false },
            ConstValue::Float(value) => {
                if value.is_infinite() && value.is_sign_positive() {
                    syn::parse_quote! { f64::INFINITY }
                } else if value.is_infinite() {
                    syn::parse_quote! { f64::NEG_INFINITY }
                } else if value.is_nan() {
                    syn::parse_quote! { f64::NAN }
                } else {
                    let lit = syn::LitFloat::new(&format!("{value:e}"), Span::mixed_site());
                    syn::parse_quote! { #lit }
                }
            }
            ConstValue::Int(value) => {
                let lit = syn::LitInt::new(&value.to_string(), Span::mixed_site());
                syn::parse_quote! { #lit }
            }
            ConstValue::Complex(re, im) => {
                let re = ConstValue::Float(*re).to_expr();
                let im = ConstValue::Float(*im).to_expr();
                syn::parse_quote! { crate::builtin::complex128(#re, #im) }
            }
            ConstValue::Str(value) => {
                let lit = syn::LitStr::new(value, Span::mixed_site());
                syn::parse_quote! { #lit }
            }
            ConstValue::Uint(value, _) => {
                let lit = syn::LitInt::new(&value.to_string(), Span::mixed_site());
                syn::parse_quote! { #lit }
            }
        }
    }
}

fn const_value_to_expr_for_type(value: &ConstValue, type_name: Option<&str>) -> syn::Expr {
    match (value, type_name) {
        (ConstValue::Complex(re, im), Some("complex64")) => {
            let re = ConstValue::Float(*re).to_expr();
            let im = ConstValue::Float(*im).to_expr();
            syn::parse_quote! { crate::builtin::complex64((#re as f32), (#im as f32)) }
        }
        _ => value.to_expr(),
    }
}

fn const_rust_type_from_inferred(ty: &typeinfer::GoType, value: &ConstValue) -> Option<syn::Type> {
    if matches!(ty, typeinfer::GoType::String) {
        return Some(syn::parse_quote! { &str });
    }
    if matches!(ty, typeinfer::GoType::Int) {
        match value {
            ConstValue::Int(value)
                if *value < isize::MIN as i128 || *value > isize::MAX as i128 =>
            {
                return Some(ConstValue::Int(*value).rust_type());
            }
            ConstValue::Uint(_, _) => return Some(value.rust_type()),
            _ => {}
        }
    }
    rust_type_from_go_type(ty)
}

fn parse_go_int_literal(lit: &str) -> Option<ConstValue> {
    let lit = lit.replace('_', "");
    let (digits, radix) =
        if let Some(hex) = lit.strip_prefix("0x").or_else(|| lit.strip_prefix("0X")) {
            (hex, 16)
        } else if let Some(bin) = lit.strip_prefix("0b").or_else(|| lit.strip_prefix("0B")) {
            (bin, 2)
        } else if let Some(oct) = lit.strip_prefix("0o").or_else(|| lit.strip_prefix("0O")) {
            (oct, 8)
        } else if lit.len() > 1 && lit.starts_with('0') {
            (&lit[1..], 8)
        } else {
            (lit.as_str(), 10)
        };
    let value = u128::from_str_radix(digits, radix).ok()?;
    if let Ok(value) = i128::try_from(value) {
        Some(ConstValue::Int(value))
    } else {
        Some(ConstValue::Uint(value, 128))
    }
}

fn parse_go_float_literal(lit: &str) -> Option<f64> {
    let lit = lit.replace('_', "");
    let lower = lit.to_ascii_lowercase();
    if !lower.starts_with("0x") {
        return lit.parse::<f64>().ok();
    }

    let body = &lower[2..];
    let (mantissa, exponent) = body.split_once('p')?;
    let exponent = exponent.parse::<i32>().ok()?;
    let mut value = 0f64;
    let mut fractional_digits = 0i32;
    let mut after_dot = false;
    for ch in mantissa.chars() {
        if ch == '.' {
            after_dot = true;
            continue;
        }
        let digit = ch.to_digit(16)? as f64;
        value = value.mul_add(16.0, digit);
        if after_dot {
            fractional_digits += 1;
        }
    }
    Some(value * 16f64.powi(-fractional_digits) * 2f64.powi(exponent))
}

fn parse_go_imaginary_literal(lit: &str) -> Option<f64> {
    let body = lit.strip_suffix('i')?;
    let decimal_digits_only = body.chars().all(|ch| ch == '_' || ch.is_ascii_digit());
    if decimal_digits_only {
        return body.replace('_', "").parse::<f64>().ok();
    }
    if body.contains(['.', 'e', 'E', 'p', 'P']) {
        parse_go_float_literal(body)
    } else {
        parse_go_int_literal(body)?.as_f64()
    }
}

fn imaginary_literal_expr_with_expected(
    lit: &ast::BasicLit,
    expected: Option<&typeinfer::GoType>,
) -> Option<syn::Expr> {
    let imag = parse_go_imaginary_literal(lit.value)?;
    let imag = ConstValue::Float(imag).to_expr();
    match expected.map(resolved_go_type) {
        Some(typeinfer::GoType::Complex64) => {
            Some(syn::parse_quote! { crate::builtin::complex64(0.0, (#imag as f32)) })
        }
        _ => Some(syn::parse_quote! { crate::builtin::complex128(0.0, #imag) }),
    }
}

fn imaginary_literal_expr(lit: &ast::BasicLit) -> Option<syn::Expr> {
    imaginary_literal_expr_with_expected(lit, None)
}

fn mask_for_bits(bits: u32) -> u128 {
    if bits >= 128 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    }
}

fn convert_const_value(value: ConstValue, target: &ast::Expr) -> Option<ConstValue> {
    let target_name = extract_type_name(target)?;
    match target_name.as_str() {
        "bool" => match value {
            ConstValue::Bool(value) => Some(ConstValue::Bool(value)),
            _ => None,
        },
        "float32" | "float64" => Some(ConstValue::Float(value.as_f64()?)),
        "complex64" | "complex128" => {
            let (re, im) = value.as_complex128()?;
            Some(ConstValue::Complex(re, im))
        }
        "int" => Some(ConstValue::Int(value.as_i128()? as isize as i128)),
        "int8" => Some(ConstValue::Int(value.as_i128()? as i8 as i128)),
        "int16" => Some(ConstValue::Int(value.as_i128()? as i16 as i128)),
        "int32" => Some(ConstValue::Int(value.as_i128()? as i32 as i128)),
        "rune" => Some(ConstValue::Int(value.as_i128()? as i32 as i128)),
        "int64" => Some(ConstValue::Int(value.as_i128()? as i64 as i128)),
        "uint" | "uintptr" => Some(ConstValue::Uint(
            value.as_u128()? & mask_for_bits(usize::BITS),
            usize::BITS,
        )),
        "uint8" | "byte" => Some(ConstValue::Uint(value.as_u128()? & mask_for_bits(8), 8)),
        "uint16" => Some(ConstValue::Uint(value.as_u128()? & mask_for_bits(16), 16)),
        "uint32" => Some(ConstValue::Uint(value.as_u128()? & mask_for_bits(32), 32)),
        "uint64" => Some(ConstValue::Uint(value.as_u128()? & mask_for_bits(64), 64)),
        "string" => match value {
            ConstValue::Str(value) => Some(ConstValue::Str(value)),
            _ => None,
        },
        _ => None,
    }
}

fn const_binary_expr(lhs: ConstValue, op: token::Token, rhs: ConstValue) -> Option<ConstValue> {
    if lhs.is_complex() || rhs.is_complex() {
        return const_complex_binary_expr(lhs, op, rhs);
    }
    match op {
        token::Token::ADD => match (lhs, rhs) {
            (ConstValue::Str(lhs), ConstValue::Str(rhs)) => Some(ConstValue::Str(lhs + &rhs)),
            (lhs, rhs)
                if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) =>
            {
                Some(ConstValue::Float(lhs.as_f64()? + rhs.as_f64()?))
            }
            (ConstValue::Uint(lhs, bits), rhs) => {
                Some(ConstValue::Uint(lhs + rhs.as_u128()?, bits))
            }
            (lhs, ConstValue::Uint(rhs, bits)) => {
                Some(ConstValue::Uint(lhs.as_u128()? + rhs, bits))
            }
            (lhs, rhs) => Some(ConstValue::Int(lhs.as_i128()?.checked_add(rhs.as_i128()?)?)),
        },
        token::Token::SUB => {
            if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) {
                Some(ConstValue::Float(lhs.as_f64()? - rhs.as_f64()?))
            } else if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_sub(rhs.as_u128()?)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_sub(rhs.as_i128()?)?))
            }
        }
        token::Token::MUL => {
            if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) {
                Some(ConstValue::Float(lhs.as_f64()? * rhs.as_f64()?))
            } else if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_mul(rhs.as_u128()?)?, *bits))
            } else if let ConstValue::Uint(rhs, bits) = &rhs {
                Some(ConstValue::Uint(lhs.as_u128()?.checked_mul(*rhs)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_mul(rhs.as_i128()?)?))
            }
        }
        token::Token::QUO => {
            if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) {
                Some(ConstValue::Float(lhs.as_f64()? / rhs.as_f64()?))
            } else if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_div(rhs.as_u128()?)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_div(rhs.as_i128()?)?))
            }
        }
        token::Token::REM => {
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_rem(rhs.as_u128()?)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_rem(rhs.as_i128()?)?))
            }
        }
        token::Token::SHL => {
            let shift = u32::try_from(rhs.as_u128()?).ok()?;
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_shl(shift)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_shl(shift)?))
            }
        }
        token::Token::SHR => {
            let shift = u32::try_from(rhs.as_u128()?).ok()?;
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(lhs.checked_shr(shift)?, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()?.checked_shr(shift)?))
            }
        }
        token::Token::AND => {
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(*lhs & rhs.as_u128()?, *bits))
            } else if let ConstValue::Uint(rhs, bits) = &rhs {
                Some(ConstValue::Uint(lhs.as_u128()? & *rhs, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()? & rhs.as_i128()?))
            }
        }
        token::Token::AND_NOT => {
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(
                    *lhs & !rhs.as_u128()? & mask_for_bits(*bits),
                    *bits,
                ))
            } else {
                Some(ConstValue::Int(lhs.as_i128()? & !rhs.as_i128()?))
            }
        }
        token::Token::OR => {
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(*lhs | rhs.as_u128()?, *bits))
            } else if let ConstValue::Uint(rhs, bits) = &rhs {
                Some(ConstValue::Uint(lhs.as_u128()? | *rhs, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()? | rhs.as_i128()?))
            }
        }
        token::Token::XOR => {
            if let ConstValue::Uint(lhs, bits) = &lhs {
                Some(ConstValue::Uint(*lhs ^ rhs.as_u128()?, *bits))
            } else if let ConstValue::Uint(rhs, bits) = &rhs {
                Some(ConstValue::Uint(lhs.as_u128()? ^ *rhs, *bits))
            } else {
                Some(ConstValue::Int(lhs.as_i128()? ^ rhs.as_i128()?))
            }
        }
        token::Token::EQL => Some(ConstValue::Bool(match (&lhs, &rhs) {
            (ConstValue::Bool(lhs), ConstValue::Bool(rhs)) => lhs == rhs,
            (ConstValue::Str(lhs), ConstValue::Str(rhs)) => lhs == rhs,
            _ if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) => {
                lhs.as_f64()? == rhs.as_f64()?
            }
            _ => lhs.as_i128()? == rhs.as_i128()?,
        })),
        token::Token::NEQ => const_binary_expr(lhs, token::Token::EQL, rhs).and_then(|value| {
            if let ConstValue::Bool(value) = value {
                Some(ConstValue::Bool(!value))
            } else {
                None
            }
        }),
        token::Token::LSS | token::Token::GTR | token::Token::LEQ | token::Token::GEQ => {
            let ord =
                if matches!(&lhs, ConstValue::Float(_)) || matches!(&rhs, ConstValue::Float(_)) {
                    lhs.as_f64()?.partial_cmp(&rhs.as_f64()?)?
                } else {
                    lhs.as_i128()?.cmp(&rhs.as_i128()?)
                };
            Some(ConstValue::Bool(match op {
                token::Token::LSS => ord.is_lt(),
                token::Token::GTR => ord.is_gt(),
                token::Token::LEQ => !ord.is_gt(),
                token::Token::GEQ => !ord.is_lt(),
                _ => false,
            }))
        }
        token::Token::LAND => {
            let ConstValue::Bool(lhs) = lhs else {
                return None;
            };
            let ConstValue::Bool(rhs) = rhs else {
                return None;
            };
            Some(ConstValue::Bool(lhs && rhs))
        }
        token::Token::LOR => {
            let ConstValue::Bool(lhs) = lhs else {
                return None;
            };
            let ConstValue::Bool(rhs) = rhs else {
                return None;
            };
            Some(ConstValue::Bool(lhs || rhs))
        }
        _ => None,
    }
}

fn const_complex_binary_expr(
    lhs: ConstValue,
    op: token::Token,
    rhs: ConstValue,
) -> Option<ConstValue> {
    let (lhs_re, lhs_im) = lhs.as_complex128()?;
    let (rhs_re, rhs_im) = rhs.as_complex128()?;
    match op {
        token::Token::ADD => Some(ConstValue::Complex(lhs_re + rhs_re, lhs_im + rhs_im)),
        token::Token::SUB => Some(ConstValue::Complex(lhs_re - rhs_re, lhs_im - rhs_im)),
        token::Token::MUL => Some(ConstValue::Complex(
            lhs_re.mul_add(rhs_re, -(lhs_im * rhs_im)),
            lhs_re.mul_add(rhs_im, lhs_im * rhs_re),
        )),
        token::Token::QUO => {
            let denom = rhs_re.mul_add(rhs_re, rhs_im * rhs_im);
            Some(ConstValue::Complex(
                lhs_re.mul_add(rhs_re, lhs_im * rhs_im) / denom,
                lhs_im.mul_add(rhs_re, -(lhs_re * rhs_im)) / denom,
            ))
        }
        token::Token::EQL => Some(ConstValue::Bool(lhs_re == rhs_re && lhs_im == rhs_im)),
        token::Token::NEQ => Some(ConstValue::Bool(lhs_re != rhs_re || lhs_im != rhs_im)),
        _ => None,
    }
}

fn const_eval_expr(
    expr: &ast::Expr,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<ConstValue> {
    match expr {
        ast::Expr::BasicLit(lit) => match lit.kind {
            token::Token::INT => parse_go_int_literal(lit.value),
            token::Token::FLOAT => parse_go_float_literal(lit.value).map(ConstValue::Float),
            token::Token::IMAG => {
                parse_go_imaginary_literal(lit.value).map(|imag| ConstValue::Complex(0.0, imag))
            }
            token::Token::STRING => {
                let raw = lit.value;
                let inner = &raw[1..raw.len() - 1];
                let value = if raw.starts_with('`') {
                    inner.to_string()
                } else {
                    interpret_go_string_escapes(inner)
                };
                Some(ConstValue::Str(value))
            }
            token::Token::CHAR => {
                let raw = lit.value;
                let inner = &raw[1..raw.len() - 1];
                let interpreted = interpret_go_string_escapes(inner);
                let value = interpreted.chars().next()? as u128;
                Some(ConstValue::Int(value as i32 as i128))
            }
            _ => None,
        },
        ast::Expr::Ident(ident) if ident.name == "iota" => Some(ConstValue::Int(iota_value.into())),
        ast::Expr::Ident(ident) if ident.name == "true" => Some(ConstValue::Bool(true)),
        ast::Expr::Ident(ident) if ident.name == "false" => Some(ConstValue::Bool(false)),
        ast::Expr::Ident(ident) => values.get(ident.name).cloned(),
        ast::Expr::BinaryExpr(bin) => {
            let lhs = const_eval_expr(&bin.x, iota_value, values, env)?;
            let rhs = const_eval_expr(&bin.y, iota_value, values, env)?;
            const_binary_expr(lhs, bin.op, rhs)
        }
        ast::Expr::ParenExpr(paren) => const_eval_expr(&paren.x, iota_value, values, env),
        ast::Expr::UnaryExpr(unary) => {
            let value = const_eval_expr(&unary.x, iota_value, values, env)?;
            match unary.op {
                token::Token::SUB => match value {
                    ConstValue::Complex(re, im) => Some(ConstValue::Complex(-re, -im)),
                    ConstValue::Float(value) => Some(ConstValue::Float(-value)),
                    value => Some(ConstValue::Int(value.as_i128()?.checked_neg()?)),
                },
                token::Token::ADD => Some(value),
                token::Token::NOT => {
                    let ConstValue::Bool(value) = value else {
                        return None;
                    };
                    Some(ConstValue::Bool(!value))
                }
                token::Token::XOR => match value {
                    ConstValue::Uint(value, bits) => {
                        Some(ConstValue::Uint(!value & mask_for_bits(bits), bits))
                    }
                    value => Some(ConstValue::Int(!value.as_i128()?)),
                },
                _ => None,
            }
        }
        ast::Expr::CallExpr(call) => {
            if const_eval_builtin_name(call, env).is_some() {
                return const_eval_builtin_call(call, iota_value, values, env);
            }
            let args = call.args.as_ref()?;
            if args.len() != 1 {
                return None;
            }
            let value = const_eval_expr(args.first()?, iota_value, values, env)?;
            convert_const_value(value, &call.fun)
        }
        _ => None,
    }
}

fn const_eval_expr_in_active_env(
    expr: &ast::Expr,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
) -> Option<ConstValue> {
    TYPE_ENV.with(|env| const_eval_expr(expr, iota_value, values, &env.borrow()))
}

fn const_eval_builtin_name<'a>(
    call: &'a ast::CallExpr<'a>,
    env: &typeinfer::TypeEnv,
) -> Option<&'a str> {
    let ast::Expr::Ident(ident) = call.fun.as_ref() else {
        return None;
    };
    if env.get_var(ident.name).is_some()
        || env.has_func(ident.name)
        || env.get_type_kind(ident.name).is_some()
    {
        return None;
    }
    matches!(
        ident.name,
        "cap" | "complex" | "imag" | "len" | "max" | "min" | "real"
    )
    .then_some(ident.name)
}

fn const_eval_builtin_call(
    call: &ast::CallExpr,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<ConstValue> {
    let name = const_eval_builtin_name(call, env)?;
    let args = call.args.as_deref()?;
    match name {
        "len" => const_eval_len_cap(args, true, iota_value, values, env),
        "cap" => const_eval_len_cap(args, false, iota_value, values, env),
        "real" => {
            let [arg] = args else {
                return None;
            };
            let (real, _) = const_eval_expr(arg, iota_value, values, env)?.as_complex128()?;
            Some(ConstValue::Float(real))
        }
        "imag" => {
            let [arg] = args else {
                return None;
            };
            let (_, imag) = const_eval_expr(arg, iota_value, values, env)?.as_complex128()?;
            Some(ConstValue::Float(imag))
        }
        "complex" => {
            let [real, imag] = args else {
                return None;
            };
            Some(ConstValue::Complex(
                const_eval_expr(real, iota_value, values, env)?.as_f64()?,
                const_eval_expr(imag, iota_value, values, env)?.as_f64()?,
            ))
        }
        "max" | "min" => const_eval_min_max(name == "max", args, iota_value, values, env),
        _ => None,
    }
}

fn const_eval_len_cap(
    args: &[ast::Expr<'_>],
    is_len: bool,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<ConstValue> {
    let [arg] = args else {
        return None;
    };
    if is_len && let Some(ConstValue::Str(value)) = const_eval_expr(arg, iota_value, values, env) {
        return Some(ConstValue::Int(value.len() as i128));
    }
    const_eval_array_len(arg, iota_value, values, env).map(ConstValue::Int)
}

fn const_eval_array_len(
    expr: &ast::Expr,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<i128> {
    match expr {
        ast::Expr::ParenExpr(paren) => const_eval_array_len(&paren.x, iota_value, values, env),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::AND => {
            const_eval_array_len(&unary.x, iota_value, values, env)
        }
        ast::Expr::CompositeLit(lit) => {
            let type_expr = lit.type_.as_ref()?;
            const_eval_array_type_len(type_expr, lit.elts.as_deref(), iota_value, values, env)
        }
        ast::Expr::SelectorExpr(selector) => {
            let receiver_type = typeinfer::GoType::infer_expr(&selector.x, env);
            env.get_field_array_len_from_receiver(&receiver_type, selector.sel.name)
        }
        _ => None,
    }
}

fn const_eval_array_type_len(
    type_expr: &ast::Expr,
    elts: Option<&[ast::Expr<'_>]>,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<i128> {
    match type_expr {
        ast::Expr::ParenExpr(paren) => {
            const_eval_array_type_len(&paren.x, elts, iota_value, values, env)
        }
        ast::Expr::ArrayType(array) => {
            let len = array.len.as_ref()?;
            if matches!(len.as_ref(), ast::Expr::Ellipsis(_)) {
                return const_eval_ellipsis_array_len(elts, iota_value, values, env);
            }
            const_eval_expr(len, iota_value, values, env)?.as_i128()
        }
        ast::Expr::StarExpr(star) => {
            const_eval_array_type_len(&star.x, elts, iota_value, values, env)
        }
        _ => None,
    }
}

fn const_eval_ellipsis_array_len(
    elts: Option<&[ast::Expr<'_>]>,
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<i128> {
    let mut next_index = 0i128;
    let mut max_index = None;
    for elt in elts.unwrap_or_default() {
        let index = if let ast::Expr::KeyValueExpr(kv) = elt {
            const_eval_expr(&kv.key, iota_value, values, env)?.as_i128()?
        } else {
            next_index
        };
        max_index = Some(max_index.map_or(index, |max: i128| max.max(index)));
        next_index = index.checked_add(1)?;
    }
    Some(max_index.map_or(0, |index| index + 1))
}

fn const_eval_min_max(
    is_max: bool,
    args: &[ast::Expr<'_>],
    iota_value: i64,
    values: &BTreeMap<String, ConstValue>,
    env: &typeinfer::TypeEnv,
) -> Option<ConstValue> {
    let mut evaluated = args
        .iter()
        .map(|arg| const_eval_expr(arg, iota_value, values, env));
    let first = evaluated.next()??;
    evaluated.try_fold(first, |best, value| {
        let value = value?;
        let use_value = match (&best, &value) {
            (ConstValue::Str(best), ConstValue::Str(value)) => {
                if is_max {
                    value > best
                } else {
                    value < best
                }
            }
            _ if matches!(&best, ConstValue::Float(_))
                || matches!(&value, ConstValue::Float(_)) =>
            {
                let ordering = value.as_f64()?.partial_cmp(&best.as_f64()?)?;
                if is_max {
                    ordering.is_gt()
                } else {
                    ordering.is_lt()
                }
            }
            _ => {
                let ordering = value.as_i128()?.cmp(&best.as_i128()?);
                if is_max {
                    ordering.is_gt()
                } else {
                    ordering.is_lt()
                }
            }
        };
        Some(if use_value { value } else { best })
    })
}

fn const_basic_lit_expr(lit: &ast::BasicLit, target: Option<&syn::Type>) -> Option<syn::Expr> {
    match lit.kind {
        token::Token::INT => {
            let value = parse_go_int_literal(lit.value)?;
            if target.is_some()
                && let Some(unsigned) = value.as_u128()
                && unsigned > isize::MAX as u128
            {
                let lit = syn::LitInt::new(&format!("{unsigned}u128"), Span::mixed_site());
                return Some(syn::parse_quote! { #lit });
            }
            Some(value.to_expr())
        }
        token::Token::FLOAT => {
            let value = parse_go_float_literal(lit.value)
                .map_or_else(|| lit.value.to_string(), |value| format!("{value:e}"));
            let lit = syn::LitFloat::new(&value, Span::mixed_site());
            Some(syn::parse_quote! { #lit })
        }
        token::Token::IMAG => imaginary_literal_expr(lit),
        token::Token::STRING => {
            let raw = lit.value;
            let inner = &raw[1..raw.len() - 1];
            let interpreted = if raw.starts_with('`') {
                inner.to_string()
            } else {
                interpret_go_string_escapes(inner)
            };
            let lit = syn::LitStr::new(&interpreted, Span::mixed_site());
            Some(syn::parse_quote! { #lit })
        }
        token::Token::CHAR => {
            let raw = lit.value;
            let inner = &raw[1..raw.len() - 1];
            let interpreted = interpret_go_string_escapes(inner);
            let value = interpreted.chars().next()? as i32;
            Some(syn::parse_quote! { #value })
        }
        _ => None,
    }
}

fn const_numeric_cast_type_from_rust_type(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let ident = path.path.get_ident()?.to_string();
    matches!(
        ident.as_str(),
        "isize"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "f32"
            | "f64"
    )
    .then(|| ty.clone())
}

fn cast_const_numeric_expr(expr: syn::Expr, target: Option<&syn::Type>) -> syn::Expr {
    if let Some(target) = target {
        syn::parse_quote! { (#expr as #target) }
    } else {
        expr
    }
}

fn const_expr_to_rust_expr(expr: &ast::Expr, target: Option<&syn::Type>) -> Option<syn::Expr> {
    match expr {
        ast::Expr::BasicLit(lit) => Some(cast_const_numeric_expr(
            const_basic_lit_expr(lit, target)?,
            target,
        )),
        ast::Expr::Ident(ident) if ident.name == "true" => Some(syn::parse_quote! { true }),
        ast::Expr::Ident(ident) if ident.name == "false" => Some(syn::parse_quote! { false }),
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&import_rust_name(ident.name), Span::mixed_site());
            Some(cast_const_numeric_expr(
                syn::parse_quote! { #ident },
                target,
            ))
        }
        ast::Expr::BinaryExpr(binary) => {
            let left = const_expr_to_rust_expr(&binary.x, target)?;
            let right = const_expr_to_rust_expr(&binary.y, target)?;
            let op: syn::BinOp = binary.op.into();
            Some(syn::Expr::Binary(syn::ExprBinary {
                attrs: vec![],
                left: Box::new(left),
                op,
                right: Box::new(right),
            }))
        }
        ast::Expr::ParenExpr(paren) => {
            let expr = const_expr_to_rust_expr(&paren.x, target)?;
            Some(syn::Expr::Paren(syn::ExprParen {
                attrs: vec![],
                paren_token: syn::token::Paren::default(),
                expr: Box::new(expr),
            }))
        }
        ast::Expr::UnaryExpr(unary) => {
            let expr = const_expr_to_rust_expr(&unary.x, target)?;
            match unary.op {
                token::Token::ADD => Some(expr),
                token::Token::SUB => Some(syn::Expr::Unary(syn::ExprUnary {
                    attrs: vec![],
                    op: syn::UnOp::Neg(<Token![-]>::default()),
                    expr: Box::new(expr),
                })),
                token::Token::NOT | token::Token::XOR => Some(syn::Expr::Unary(syn::ExprUnary {
                    attrs: vec![],
                    op: syn::UnOp::Not(<Token![!]>::default()),
                    expr: Box::new(expr),
                })),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Compile a top-level const GenDecl into a list of `syn::Item::Const` items.
/// Handles iota, expression inheritance, and typed constants.
fn compile_const_decl(gen_decl: ast::GenDecl) -> Result<Vec<syn::Item>, CompilerError> {
    let mut value_specs: Vec<ast::ValueSpec> = vec![];
    for spec in gen_decl.specs {
        if let ast::Spec::ValueSpec(vs) = spec {
            value_specs.push(vs);
        }
    }

    let mut items = vec![];
    let mut last_valued_idx: Option<usize> = None;
    let mut const_values: BTreeMap<String, ConstValue> = BTreeMap::new();

    for (iota, spec_idx) in (0..value_specs.len()).enumerate() {
        let Some(value_spec) = value_specs.get(spec_idx) else {
            return Err(CompilerError::UnsupportedConstruct(
                "const declaration index out of range".to_string(),
            ));
        };

        let has_own_values = value_spec.values.is_some();
        if has_own_values {
            last_valued_idx = Some(spec_idx);
        }

        let source_idx = if has_own_values {
            Some(spec_idx)
        } else {
            last_valued_idx
        };

        let source_values = source_idx.and_then(|idx| value_specs.get(idx)?.values.as_ref());

        // Determine the type name string (from this spec or inherited)
        let type_name_str: Option<&str> = if let Some(ref te) = value_spec.type_ {
            if let ast::Expr::Ident(id) = te {
                Some(id.name)
            } else {
                None
            }
        } else {
            source_idx.and_then(|idx| {
                value_specs.get(idx)?.type_.as_ref().and_then(|te| {
                    if let ast::Expr::Ident(id) = te {
                        Some(id.name)
                    } else {
                        None
                    }
                })
            })
        };

        for (name_idx, name) in value_spec.names.iter().enumerate() {
            if name.name == "_" {
                continue;
            }

            let vis: syn::Visibility = name.into();
            let ident = syn::Ident::new(&import_rust_name(name.name), Span::mixed_site());

            let value_expr = source_values.and_then(|vals| vals.get(name_idx));
            let evaluated = value_expr
                .and_then(|expr| const_eval_expr_in_active_env(expr, iota as i64, &const_values));

            let rust_type: syn::Type = if let Some(name) = type_name_str {
                const_rust_type_from_type_name(name)
            } else if let Some(value) = &evaluated {
                TYPE_ENV.with(|env| {
                    env.borrow()
                        .get_var(name.name)
                        .and_then(|ty| const_rust_type_from_inferred(&ty, value))
                        .unwrap_or_else(|| value.rust_type())
                })
            } else if let Some(expr) = value_expr {
                TYPE_ENV.with(|env| {
                    let go_type = typeinfer::GoType::infer_expr(expr, &env.borrow());
                    if matches!(go_type, typeinfer::GoType::String) {
                        syn::parse_quote! { &str }
                    } else {
                        rust_type_from_go_type(&go_type)
                            .unwrap_or_else(|| syn::parse_quote! { isize })
                    }
                })
            } else {
                syn::parse_quote! { isize }
            };

            let mut value: syn::Expr = if let Some(expr) = value_expr {
                if let Some(evaluated) = &evaluated {
                    const_value_to_expr_for_type(evaluated, type_name_str)
                } else if let ast::Expr::BasicLit(lit) = expr {
                    match lit.kind {
                        token::Token::STRING => {
                            let raw = lit.value;
                            let inner = &raw[1..raw.len() - 1];
                            let interpreted = if raw.starts_with('`') {
                                inner.to_string()
                            } else {
                                interpret_go_string_escapes(inner)
                            };
                            let s = syn::LitStr::new(&interpreted, Span::mixed_site());
                            syn::parse_quote! { #s }
                        }
                        token::Token::FLOAT => {
                            let value = parse_go_float_literal(lit.value).map_or_else(
                                || lit.value.to_string(),
                                |value| format!("{value:e}"),
                            );
                            let f = syn::LitFloat::new(&value, Span::mixed_site());
                            syn::parse_quote! { #f }
                        }
                        token::Token::IMAG => {
                            let expected = type_name_str.map(typeinfer::GoType::from_name);
                            imaginary_literal_expr_with_expected(lit, expected.as_ref())
                                .unwrap_or_else(|| syn::parse_quote! { Default::default() })
                        }
                        _ => syn::parse_quote! { 0 },
                    }
                } else if let ast::Expr::Ident(id) = expr {
                    match id.name {
                        "true" => syn::parse_quote! { true },
                        "false" => syn::parse_quote! { false },
                        _ => {
                            let id_ident =
                                syn::Ident::new(&import_rust_name(id.name), Span::mixed_site());
                            if is_active_string_const_fn(id.name) {
                                syn::parse_quote! { #id_ident() }
                            } else {
                                syn::parse_quote! { #id_ident }
                            }
                        }
                    }
                } else if is_const_like_expr(expr) {
                    let target = const_numeric_cast_type_from_rust_type(&rust_type);
                    const_expr_to_rust_expr(expr, target.as_ref())
                        .unwrap_or_else(|| syn::parse_quote! { 0 })
                } else {
                    syn::parse_quote! { 0 }
                }
            } else {
                syn::parse_quote! { 0 }
            };
            if let Some(type_name) = type_name_str
                && is_named_numeric_alias(type_name)
            {
                let type_ident = syn::Ident::new(&import_rust_name(type_name), Span::mixed_site());
                value = syn::parse_quote! { #type_ident(#value) };
            }

            // String constants: emit as inline functions returning String
            // since Rust const can't hold String and we need owned values
            let is_str_type = matches!(&rust_type, syn::Type::Reference(r)
                if matches!(&*r.elem, syn::Type::Path(tp) if tp.path.is_ident("str")));
            if is_str_type {
                STRING_CONST_NAMES.with(|names| {
                    names.borrow_mut().insert(ident.to_string());
                });
                items.push(syn::parse_quote! {
                    #[inline]
                    #[allow(non_snake_case)]
                    #vis fn #ident() -> String { #value.to_string() }
                });
            } else {
                items.push(syn::parse_quote! {
                    #vis const #ident: #rust_type = #value;
                });
            }
            if let Some(evaluated) = evaluated {
                const_values.insert(name.name.to_string(), evaluated);
            }
        }
    }
    Ok(items)
}

fn const_rust_type_from_type_name(name: &str) -> syn::Type {
    let go_type = typeinfer::GoType::from_name(name);
    if matches!(go_type, typeinfer::GoType::String) {
        return syn::parse_quote! { &str };
    }
    if let Some(rust_type) = rust_type_from_go_type(&go_type) {
        return rust_type;
    }
    let type_ident = syn::Ident::new(&import_rust_name(name), Span::mixed_site());
    syn::parse_quote! { #type_ident }
}

/// Helper to get the zero value expression for a Go type name.
fn zero_value_for_type(type_name: Option<&str>) -> syn::Expr {
    match type_name {
        Some("string") => syn::parse_quote! { "" },
        Some("int") | Some("int8") | Some("int16") | Some("int32") | Some("int64")
        | Some("uint") | Some("uint8") | Some("uint16") | Some("uint32") | Some("uint64") => {
            syn::parse_quote! { 0 }
        }
        Some("float32") | Some("float64") => syn::parse_quote! { 0.0 },
        Some("bool") => syn::parse_quote! { false },
        Some("error") => syn::parse_quote! { String::new() },
        Some("any") => syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> },
        _ => syn::parse_quote! { Default::default() },
    }
}

/// Helper to get the Go type name from an expression (for zero values).
fn go_type_name_from_expr<'a>(expr: &'a ast::Expr<'a>) -> Option<&'a str> {
    match expr {
        ast::Expr::Ident(ident) => Some(ident.name),
        ast::Expr::StarExpr(_) => None,
        _ => None,
    }
}

fn wrap_named_return_block(
    block: &mut syn::Block,
    named_return_info: &[(syn::Ident, Option<syn::Type>, syn::Expr)],
    named_return_idents: &[syn::Ident],
) {
    let label = next_named_return_label();
    let mut declarations: Vec<syn::Stmt> = named_return_info
        .iter()
        .map(|(ident, rust_type, zero)| named_return_decl_stmt(ident, rust_type, zero))
        .collect();
    let body_stmts = std::mem::take(&mut block.stmts);
    let mut body = syn::Block {
        brace_token: syn::token::Brace::default(),
        stmts: body_stmts,
    };
    rewrite_named_returns_to_break(&mut body, named_return_idents, &label);
    let body_stmts = body.stmts;
    let labeled_body: syn::Stmt = syn::parse_quote! { #label: { #(#body_stmts)* }; };
    declarations.push(labeled_body);
    declarations.push(named_return_return_stmt(named_return_idents));
    block.stmts = declarations;
}

fn rewrite_named_returns_to_break(
    block: &mut syn::Block,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    for stmt in &mut block.stmts {
        rewrite_named_returns_in_stmt(stmt, named_return_idents, label);
    }
}

fn rewrite_named_returns_in_stmt(
    stmt: &mut syn::Stmt,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    if let syn::Stmt::Expr(expr, _semi) = stmt {
        rewrite_named_returns_in_expr(expr, named_return_idents, label);
    }
}

fn rewrite_named_returns_in_expr(
    expr: &mut syn::Expr,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) {
    match expr {
        syn::Expr::Return(ret) => {
            *expr = named_return_break_expr(ret.expr.take(), named_return_idents, label);
        }
        syn::Expr::Block(block) => {
            rewrite_named_returns_to_break(&mut block.block, named_return_idents, label);
        }
        syn::Expr::Closure(_) => {}
        syn::Expr::ForLoop(for_expr) => {
            rewrite_named_returns_to_break(&mut for_expr.body, named_return_idents, label);
        }
        syn::Expr::If(if_expr) => {
            rewrite_named_returns_to_break(&mut if_expr.then_branch, named_return_idents, label);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_named_returns_in_expr(else_expr, named_return_idents, label);
            }
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_named_returns_to_break(&mut loop_expr.body, named_return_idents, label);
        }
        syn::Expr::Match(match_expr) => {
            for arm in &mut match_expr.arms {
                rewrite_named_returns_in_expr(&mut arm.body, named_return_idents, label);
            }
        }
        syn::Expr::TryBlock(try_block) => {
            rewrite_named_returns_to_break(&mut try_block.block, named_return_idents, label);
        }
        syn::Expr::While(while_expr) => {
            rewrite_named_returns_to_break(&mut while_expr.body, named_return_idents, label);
        }
        _ => {}
    }
}

fn named_return_break_expr(
    return_expr: Option<Box<syn::Expr>>,
    named_return_idents: &[syn::Ident],
    label: &syn::Lifetime,
) -> syn::Expr {
    let mut stmts = Vec::new();
    if let Some(return_expr) = return_expr {
        let return_expr = *return_expr;
        match named_return_idents {
            [] => {}
            [ident] => {
                if let Some(stmt) = named_return_assignment_stmt(ident, return_expr) {
                    stmts.push(stmt);
                }
            }
            idents => {
                let temps = next_named_return_temp_idents(idents.len());
                let temp_pats = temps.iter();
                stmts.push(syn::parse_quote! { let (#(#temp_pats),*) = #return_expr; });
                for (ident, temp) in idents.iter().zip(temps) {
                    let value: syn::Expr = syn::parse_quote! { #temp };
                    if let Some(stmt) = named_return_assignment_stmt(ident, value) {
                        stmts.push(stmt);
                    }
                }
            }
        }
    }
    let break_stmt: syn::Stmt = syn::parse_quote! { break #label; };
    stmts.push(break_stmt);
    syn::parse_quote! {{ #(#stmts)* }}
}

fn named_return_assignment_stmt(ident: &syn::Ident, value: syn::Expr) -> Option<syn::Stmt> {
    if expr_is_ident(&value, ident) {
        return None;
    }
    let name = ident.to_string();
    Some(if is_shared_capture_name(&name) {
        syn::parse_quote! { *#ident.lock().unwrap() = #value; }
    } else {
        syn::parse_quote! { #ident = #value; }
    })
}

fn expr_is_ident(expr: &syn::Expr, ident: &syn::Ident) -> bool {
    let syn::Expr::Path(path) = expr else {
        return false;
    };
    path.qself.is_none()
        && path.path.leading_colon.is_none()
        && path.path.segments.len() == 1
        && path
            .path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == *ident)
}

fn named_return_expr(idents: &[syn::Ident], clone_unshared: bool) -> syn::Expr {
    match idents {
        [] => syn::parse_quote! { () },
        [ident] => named_return_ident_expr(ident, clone_unshared),
        idents => {
            let elems = idents
                .iter()
                .map(|ident| named_return_ident_expr(ident, clone_unshared));
            syn::parse_quote! { (#(#elems),*) }
        }
    }
}

fn named_return_ident_expr(ident: &syn::Ident, clone_unshared: bool) -> syn::Expr {
    let name = ident.to_string();
    if let Some(expr) = shared_capture_read_expr(&name) {
        return expr;
    }
    if clone_unshared {
        syn::parse_quote! { (#ident).clone() }
    } else {
        syn::parse_quote! { #ident }
    }
}

fn is_named_return_name(name: &str) -> bool {
    let rust_name = rust_safe_ident_name(name);
    NAMED_RETURN_IDENTS.with(|idents| idents.borrow().iter().any(|ident| *ident == rust_name))
}

fn named_return_decl_stmt(
    ident: &syn::Ident,
    rust_type: &Option<syn::Type>,
    zero: &syn::Expr,
) -> syn::Stmt {
    let name = ident.to_string();
    let init = shared_capture_init_expr(&name, zero.clone());
    if let Some(rust_type) = rust_type {
        let rust_type = shared_capture_type(&name, rust_type.clone());
        syn::parse_quote! { let mut #ident: #rust_type = #init; }
    } else {
        syn::parse_quote! { let mut #ident = #init; }
    }
}

fn named_return_return_stmt(idents: &[syn::Ident]) -> syn::Stmt {
    let expr = named_return_expr(idents, false);
    syn::Stmt::Expr(
        syn::Expr::Return(syn::ExprReturn {
            attrs: vec![],
            return_token: <Token![return]>::default(),
            expr: Some(Box::new(expr)),
        }),
        None,
    )
}

fn goroutine_capture_clones(func_lit: &ast::FuncLit) -> Vec<syn::Stmt> {
    TYPE_ENV.with(|env| {
        ir::func_lit_captures(func_lit, &env.borrow())
            .into_iter()
            .map(|capture| {
                let ident =
                    syn::Ident::new(&rust_safe_ident_name(&capture.name), Span::mixed_site());
                syn::parse_quote! { let #ident = #ident.clone(); }
            })
            .collect()
    })
}

fn move_closure_shared_capture_clones(func_lit: &ast::FuncLit) -> Vec<syn::Stmt> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let mut names: std::collections::BTreeSet<_> = ir::func_lit_captures(func_lit, &env)
            .into_iter()
            .map(|capture| capture.name)
            .collect();
        names.extend(ir::mutable_func_lit_capture_names_in_block(
            &func_lit.body,
            &env,
        ));
        names
            .into_iter()
            .filter(|name| is_shared_capture_name(name))
            .map(|name| {
                let ident = syn::Ident::new(&rust_safe_ident_name(&name), Span::mixed_site());
                syn::parse_quote! { let #ident = #ident.clone(); }
            })
            .collect()
    })
}

fn function_literal_shared_capture_clones(expr: &ast::Expr) -> Vec<syn::Stmt> {
    match expr {
        ast::Expr::FuncLit(func_lit) => move_closure_shared_capture_clones(func_lit),
        ast::Expr::ParenExpr(paren) => function_literal_shared_capture_clones(&paren.x),
        _ => Vec::new(),
    }
}

/// Compile a Go select statement into Rust.
fn compile_select_stmt(select_stmt: ast::SelectStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    let select_label = next_select_label();
    let clauses: Vec<ast::CommClause> = select_stmt
        .body
        .list
        .into_iter()
        .filter_map(|s| {
            if let ast::Stmt::CommClause(cc) = s {
                Some(cc)
            } else {
                None
            }
        })
        .collect();

    let mut cases: Vec<ast::CommClause> = Vec::new();
    let mut default_body: Option<Vec<ast::Stmt>> = None;

    for clause in clauses {
        if clause.comm.is_none() {
            default_body = Some(clause.body);
        } else {
            cases.push(clause);
        }
    }

    // Only default case
    if cases.is_empty() {
        if let Some(body) = default_body {
            let stmts = compile_select_case_body(body, &select_label)?;
            return Ok(vec![select_labeled_block(select_label, stmts)]);
        }
        let stmt: syn::Stmt = syn::parse_quote! {
            loop {
                std::thread::park();
            }
        };
        return Ok(vec![select_labeled_block(select_label, vec![stmt])]);
    }

    // Helper: extract channel expression from a comm statement by consuming it
    fn extract_channel_recv(expr: ast::Expr) -> Option<syn::Expr> {
        if let ast::Expr::UnaryExpr(unary) = expr {
            if unary.op == token::Token::ARROW {
                return Some((*unary.x).into());
            }
        }
        None
    }

    // Single case with optional default
    if cases.len() == 1 {
        let case = cases.remove(0);
        let Some(comm) = case.comm.map(|c| *c) else {
            return Err(CompilerError::UnsupportedConstruct(
                "select case without comm".into(),
            ));
        };

        let case_body_stmts = compile_select_case_body(case.body, &select_label)?;

        if let Some(default) = default_body.take() {
            // Non-blocking: try_recv/try_send with fallback
            let default_stmts = compile_select_case_body(default, &select_label)?;

            match comm {
                ast::Stmt::ExprStmt(expr_stmt) => {
                    // <-ch in expression position
                    if let Some(ch) = extract_channel_recv(expr_stmt.x) {
                        let stmt: syn::Stmt = syn::parse_quote! {
                            {
                                if let Ok(_v) = #ch.try_recv() {
                                    #(#case_body_stmts)*
                                } else {
                                    #(#default_stmts)*
                                }
                            }
                        };
                        return Ok(vec![select_labeled_block(select_label, vec![stmt])]);
                    }
                }
                ast::Stmt::AssignStmt(assign)
                    if assign.rhs.len() == 1 && !assign.lhs.is_empty() =>
                {
                    if let (Some(lhs), Some(rhs_expr)) =
                        (assign.lhs.first(), assign.rhs.into_iter().next())
                    {
                        let lhs_pat = expr_to_pat(lhs);
                        if let Some(ch) = extract_channel_recv(rhs_expr) {
                            let stmt: syn::Stmt = syn::parse_quote! {
                                {
                                    if let Ok(#lhs_pat) = #ch.try_recv() {
                                        #(#case_body_stmts)*
                                    } else {
                                        #(#default_stmts)*
                                    }
                                }
                            };
                            return Ok(vec![select_labeled_block(select_label, vec![stmt])]);
                        }
                    }
                }
                ast::Stmt::SendStmt(send) => {
                    let ch: syn::Expr = send.chan.into();
                    let val: syn::Expr = send.value.into();
                    let stmt: syn::Stmt = syn::parse_quote! {
                        {
                            if #ch.try_send(#val).is_ok() {
                                #(#case_body_stmts)*
                            } else {
                                #(#default_stmts)*
                            }
                        }
                    };
                    return Ok(vec![select_labeled_block(select_label, vec![stmt])]);
                }
                _ => {}
            }
        } else {
            // Blocking single case
            let comm_stmts: Vec<syn::Stmt> = Vec::<syn::Stmt>::try_from(comm)?;
            let mut all_stmts = comm_stmts;
            all_stmts.extend(case_body_stmts);
            return Ok(vec![select_labeled_block(select_label, all_stmts)]);
        }
    }

    // Multiple cases: generate loop with try_recv checks
    let mut arms: Vec<proc_macro2::TokenStream> = Vec::new();
    for case in cases {
        let Some(comm) = case.comm.map(|c| *c) else {
            continue;
        };
        let body_stmts = non_tail_stmt_list(compile_select_case_body(case.body, &select_label)?);

        match comm {
            ast::Stmt::ExprStmt(expr_stmt) => {
                if let Some(ch) = extract_channel_recv(expr_stmt.x) {
                    arms.push(quote::quote! {
                        if let Ok(_v) = #ch.try_recv() {
                            #(#body_stmts)*
                            break;
                        }
                    });
                    continue;
                }
            }
            ast::Stmt::AssignStmt(assign) if assign.rhs.len() == 1 && !assign.lhs.is_empty() => {
                let Some(lhs) = assign.lhs.first() else {
                    continue;
                };
                let pat = expr_to_pat(lhs);
                let Some(rhs_expr) = assign.rhs.into_iter().next() else {
                    continue;
                };
                if let Some(ch) = extract_channel_recv(rhs_expr) {
                    arms.push(quote::quote! {
                        if let Ok(#pat) = #ch.try_recv() {
                            #(#body_stmts)*
                            break;
                        }
                    });
                    continue;
                }
            }
            ast::Stmt::SendStmt(send) => {
                let ch: syn::Expr = send.chan.into();
                let val: syn::Expr = send.value.into();
                arms.push(quote::quote! {
                    if #ch.try_send(#val).is_ok() {
                        #(#body_stmts)*
                        break;
                    }
                });
                continue;
            }
            _ => {}
        }
    }

    let default_arm = if let Some(body) = default_body {
        let stmts = non_tail_stmt_list(compile_select_case_body(body, &select_label)?);
        quote::quote! {
            #(#stmts)*
            break;
        }
    } else {
        quote::quote! {
            std::thread::yield_now();
        }
    };

    let stmt = parse_select_stmt(quote::quote! {
        {
            loop {
                #(#arms)*
                #default_arm
            }
        }
    })?;
    Ok(vec![select_labeled_block(select_label, vec![stmt])])
}

fn select_labeled_block(label: syn::Lifetime, stmts: Vec<syn::Stmt>) -> syn::Stmt {
    syn::Stmt::Expr(
        syn::Expr::Block(syn::ExprBlock {
            attrs: vec![],
            label: Some(syn::Label {
                name: label,
                colon_token: <Token![:]>::default(),
            }),
            block: syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts,
            },
        }),
        Some(<Token![;]>::default()),
    )
}

fn non_tail_stmt_list(mut stmts: Vec<syn::Stmt>) -> Vec<syn::Stmt> {
    for stmt in &mut stmts {
        if let syn::Stmt::Expr(_, semi) = stmt
            && semi.is_none()
        {
            *semi = Some(<Token![;]>::default());
        }
    }
    stmts
}

fn parse_select_stmt(tokens: proc_macro2::TokenStream) -> Result<syn::Stmt, CompilerError> {
    syn::parse2(tokens).map_err(|err| {
        CompilerError::UnsupportedConstruct(format!("failed to lower select statement: {err}"))
    })
}

fn compile_select_case_body(
    body: Vec<ast::Stmt>,
    select_label: &syn::Lifetime,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    compile_breakable_stmt_list(body, select_label)
}

fn compile_breakable_stmt_list(
    body: Vec<ast::Stmt>,
    break_label: &syn::Lifetime,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    if let Some(goto_plan) = ir::goto_state_plan_for_stmt_list(&body) {
        return Ok(vec![compile_goto_state_stmt_list_with(
            body,
            &goto_plan,
            |stmt| compile_breakable_stmt(stmt, break_label),
        )?]);
    }

    let mut stmts = vec![];
    for stmt in body {
        stmts.extend(compile_breakable_stmt(stmt, break_label)?);
    }
    Ok(stmts)
}

fn compile_breakable_stmt(
    stmt: ast::Stmt,
    break_label: &syn::Lifetime,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    match stmt {
        ast::Stmt::BranchStmt(branch)
            if branch.tok == token::Token::BREAK && branch.label.is_none() =>
        {
            Ok(vec![syn::Stmt::Expr(
                syn::Expr::Break(syn::ExprBreak {
                    attrs: vec![],
                    break_token: <Token![break]>::default(),
                    label: Some(break_label.clone()),
                    expr: None,
                }),
                Some(<Token![;]>::default()),
            )])
        }
        ast::Stmt::BlockStmt(block) => {
            let stmts = compile_breakable_stmt_list(block.list, break_label)?;
            Ok(vec![syn::Stmt::Expr(
                syn::Expr::Block(syn::ExprBlock {
                    attrs: vec![],
                    label: None,
                    block: syn::Block {
                        brace_token: syn::token::Brace::default(),
                        stmts,
                    },
                }),
                None,
            )])
        }
        ast::Stmt::IfStmt(if_stmt) => compile_breakable_if_stmt(if_stmt, break_label),
        other => Vec::<syn::Stmt>::try_from(other),
    }
}

fn compile_breakable_if_stmt(
    if_stmt: ast::IfStmt,
    break_label: &syn::Lifetime,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let has_init = if_stmt.init.is_some();
    let init_stmts: Vec<syn::Stmt> = if let Some(init) = *if_stmt.init {
        Vec::<syn::Stmt>::try_from(init)?
    } else {
        vec![]
    };

    let then_stmts = compile_breakable_stmt_list(if_stmt.body.list, break_label)?;
    let else_branch = if let Some(else_) = *if_stmt.else_ {
        Some((
            <Token![else]>::default(),
            Box::new(match else_ {
                ast::Stmt::IfStmt(nested) => {
                    let nested_stmts = compile_breakable_if_stmt(nested, break_label)?;
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts: nested_stmts,
                        },
                    })
                }
                ast::Stmt::BlockStmt(block) => {
                    let stmts = compile_breakable_stmt_list(block.list, break_label)?;
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts,
                        },
                    })
                }
                _ => {
                    return Err(CompilerError::UnsupportedConstruct(
                        "unsupported else branch type".to_string(),
                    ));
                }
            }),
        ))
    } else {
        None
    };

    let if_expr = syn::Expr::If(syn::ExprIf {
        attrs: vec![],
        if_token: <Token![if]>::default(),
        cond: Box::new(if_stmt.cond.into()),
        then_branch: syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: then_stmts,
        },
        else_branch,
    });

    if has_init {
        let mut block_stmts = init_stmts;
        block_stmts.push(syn::Stmt::Expr(if_expr, None));
        Ok(vec![syn::Stmt::Expr(
            syn::Expr::Block(syn::ExprBlock {
                attrs: vec![],
                label: None,
                block: syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: block_stmts,
                },
            }),
            None,
        )])
    } else {
        Ok(vec![syn::Stmt::Expr(if_expr, None)])
    }
}

/// Convert a Go comment group to Rust doc attributes.
fn comment_group_to_attrs(comment_group: &Option<crate::ast::CommentGroup>) -> Vec<syn::Attribute> {
    let Some(group) = comment_group else {
        return vec![];
    };

    group
        .list
        .iter()
        .map(|comment| {
            // Get the comment content (without // or /* */)
            // Keep the leading space as prettyplease will output `///` directly before the content
            let content = comment.content();

            syn::Attribute {
                pound_token: <Token![#]>::default(),
                style: syn::AttrStyle::Outer,
                bracket_token: syn::token::Bracket::default(),
                meta: syn::Meta::NameValue(syn::MetaNameValue {
                    path: syn::Path {
                        leading_colon: None,
                        segments: {
                            let mut segments = syn::punctuated::Punctuated::new();
                            segments.push(syn::PathSegment {
                                ident: syn::Ident::new("doc", Span::mixed_site()),
                                arguments: syn::PathArguments::None,
                            });
                            segments
                        },
                    },
                    eq_token: <Token![=]>::default(),
                    value: syn::Expr::Lit(syn::ExprLit {
                        attrs: vec![],
                        lit: syn::Lit::Str(syn::LitStr::new(content, Span::mixed_site())),
                    }),
                }),
            }
        })
        .collect()
}

/// Error type for compilation failures.
///
/// Represents errors that can occur during the Go to Rust compilation process.
#[derive(Debug, Clone)]
pub enum CompilerError {
    /// A Go construct that cannot be translated to Rust
    UnsupportedConstruct(String),
    /// An invalid assignment statement
    InvalidAssignment(String),
    /// A type conversion error
    TypeMismatch(String),
    /// An invalid function signature
    InvalidFunctionSignature(String),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConstruct(msg) => write!(f, "unsupported construct: {}", msg),
            Self::InvalidAssignment(msg) => write!(f, "invalid assignment: {}", msg),
            Self::TypeMismatch(msg) => write!(f, "type mismatch: {}", msg),
            Self::InvalidFunctionSignature(msg) => write!(f, "invalid function signature: {}", msg),
        }
    }
}

impl std::error::Error for CompilerError {}

fn compile_error_expr(message: impl AsRef<str>) -> syn::Expr {
    let message = syn::LitStr::new(message.as_ref(), Span::mixed_site());
    syn::parse_quote! { compile_error!(#message) }
}

/// Compile a Go AST into a Rust `syn` AST.
///
/// This function takes a parsed Go source file and transforms it into
/// an equivalent Rust AST. The compilation includes applying various
/// transformation passes to produce idiomatic Rust code.
///
/// # Arguments
///
/// * `file` - The Go AST to compile
///
/// # Returns
///
/// Returns `Ok(syn::File)` on success, or `Err(CompilerError)` if the
/// Go code contains constructs that cannot be translated to Rust.
///
/// # Example
///
/// ```
/// use gors::{parser, compiler};
///
/// let go_source = "package main\n\nfunc add(a int, b int) int { return a + b }";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let rust_ast = compiler::compile(go_ast).unwrap();
/// ```
pub fn compile(file: ast::File) -> Result<syn::File, CompilerError> {
    DEFER_COUNTER.with(|c| *c.borrow_mut() = 0);
    SWITCH_COUNTER.with(|c| *c.borrow_mut() = 0);
    SELECT_COUNTER.with(|c| *c.borrow_mut() = 0);
    LOOP_BODY_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|c| *c.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    set_import_renames(BTreeMap::new());
    set_import_package_names(BTreeMap::new());
    // Pre-scan the AST to build a type environment
    let mut type_env = typeinfer::TypeEnv::new();
    type_env.scan_file(&file);
    let import_package_names = file_import_package_names(&file);
    validate_file_with_type_env_and_import_package_names(&file, &type_env, &import_package_names)?;
    validate_unused_imports(&file, &import_package_names)?;
    let _ir = ir::lower_file(&file, &type_env);
    set_import_package_names(import_package_names);
    set_type_env(type_env);
    set_borrow_pointer_arg_indices_for_decls_if_unseeded(&file.decls);
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

pub fn compile_with_type_env(
    file: ast::File,
    type_env: typeinfer::TypeEnv,
) -> Result<syn::File, CompilerError> {
    compile_with_type_env_and_import_renames(file, type_env, BTreeMap::new())
}

pub fn compile_with_type_env_and_import_renames(
    file: ast::File,
    type_env: typeinfer::TypeEnv,
    import_renames: BTreeMap<String, String>,
) -> Result<syn::File, CompilerError> {
    DEFER_COUNTER.with(|c| *c.borrow_mut() = 0);
    SWITCH_COUNTER.with(|c| *c.borrow_mut() = 0);
    SELECT_COUNTER.with(|c| *c.borrow_mut() = 0);
    LOOP_BODY_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|c| *c.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    set_import_renames(import_renames);
    set_import_package_names(BTreeMap::new());
    validate_file_with_type_env(&file, &type_env)?;
    let _ir = ir::lower_file(&file, &type_env);
    set_type_env(type_env);
    set_borrow_pointer_arg_indices_for_decls_if_unseeded(&file.decls);
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

fn validate_file_with_type_env(
    file: &ast::File<'_>,
    type_env: &typeinfer::TypeEnv,
) -> Result<(), CompilerError> {
    validate_file_with_type_env_and_import_package_names(file, type_env, &BTreeMap::new())
}

fn validate_file_with_type_env_and_import_package_names(
    file: &ast::File<'_>,
    type_env: &typeinfer::TypeEnv,
    import_package_names: &BTreeMap<String, String>,
) -> Result<(), CompilerError> {
    if let Some(invalid) = ir::invalid_signature_in_file(file) {
        return Err(invalid_signature_error(invalid));
    }
    if let Some(invalid) = ir::invalid_receiver_type_in_file(file, type_env) {
        return Err(invalid_signature_error(invalid));
    }
    if let Some(invalid) =
        ir::invalid_declaration_in_file_with_import_package_names(file, import_package_names)
    {
        return Err(invalid_declaration_error(invalid));
    }
    if let Some(invalid) = ir::invalid_value_declaration_in_file(file, type_env) {
        return Err(invalid_declaration_error(invalid));
    }
    if let Some(invalid) = ir::invalid_expression_in_file(file, type_env) {
        return Err(invalid_statement_error(invalid));
    }
    if let Some(invalid) = ir::invalid_short_var_redeclaration_in_file(file) {
        return Err(invalid_statement_error(invalid));
    }
    validate_unused_locals(file)?;
    Ok(())
}

fn validate_unused_imports(
    file: &ast::File<'_>,
    import_package_names: &BTreeMap<String, String>,
) -> Result<(), CompilerError> {
    if let Some(invalid) =
        ir::invalid_unused_import_in_file_with_import_package_names(file, import_package_names)
    {
        return Err(invalid_declaration_error(invalid));
    }
    Ok(())
}

fn validate_unused_locals(file: &ast::File<'_>) -> Result<(), CompilerError> {
    if let Some(invalid) = ir::invalid_unused_local_in_file(file) {
        return Err(invalid_declaration_error(invalid));
    }
    Ok(())
}

/// Compile a parsed program (main package + imports) into a single Rust file.
///
/// Imported packages are emitted as `mod` blocks before the main package items.
pub fn compile_program(program: crate::parser::ParsedProgram) -> Result<syn::File, CompilerError> {
    let mut all_items: Vec<syn::Item> = Vec::new();

    for stdlib_path in &program.stdlib_imports {
        if let Some(stdlib_mod) = crate::resolve::resolve(stdlib_path) {
            all_items.push(syn::Item::Mod(stdlib_mod));
        }
    }

    let pkg_names: std::collections::HashSet<String> = program
        .imports
        .iter()
        .map(|p| p.name.clone())
        .chain(
            program
                .stdlib_imports
                .iter()
                .map(|path| crate::resolve::module_name(path)),
        )
        .collect();

    for pkg in program.imports {
        let mut pkg_file = TryInto::<syn::File>::try_into(pkg.ast)?;
        passes::pass_for_imported_package(&mut pkg_file);
        prefix_sibling_paths(&mut pkg_file, &pkg_names);

        let mod_ident = syn::Ident::new(&pkg.name, Span::mixed_site());
        all_items.push(syn::Item::Mod(syn::ItemMod {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            unsafety: None,
            mod_token: <Token![mod]>::default(),
            ident: mod_ident,
            content: Some((syn::token::Brace::default(), pkg_file.items)),
            semi: None,
        }));
    }

    let mut main_file: syn::File = program.main_package.ast.try_into()?;
    passes::pass(&mut main_file);

    all_items.extend(main_file.items);

    Ok(syn::File {
        attrs: vec![],
        items: all_items,
        shebang: None,
    })
}

#[derive(Clone)]
pub struct CompiledModule {
    pub mod_name: String,
    pub import_path: String,
    pub file: syn::File,
    pub filename: String,
    pub content_hash: String,
    pub is_main: bool,
    pub is_stdlib: bool,
}

#[derive(Clone)]
pub struct CompiledProgram {
    pub modules: BTreeMap<String, CompiledModule>,
    pub has_main: bool,
}

pub fn import_path_to_filename(import_path: &str) -> String {
    if import_path.is_empty() || import_path == "main" {
        return "main.rs".to_string();
    }
    format!("{}.rs", import_path.replace('/', "__"))
}

fn import_path_to_mod_name(import_path: &str) -> String {
    import_path.replace('/', "__")
}

fn collect_known_stdlib_imports(file: &ast::File<'_>, stdlib_imports: &mut Vec<String>) {
    for import_spec in file.imports() {
        let import_path = import_spec.path.value.trim_matches('"');
        if crate::resolve::is_known(import_path)
            && !stdlib_imports.contains(&import_path.to_string())
        {
            stdlib_imports.push(import_path.to_string());
        }
    }
}

pub fn compute_content_hash(files: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    let mut sorted: Vec<_> = files.iter().collect();
    sorted.sort_by_key(|(name, _)| name.as_str());
    for (name, content) in sorted {
        hasher.update(name.as_bytes());
        hasher.update(b"\x00");
        hasher.update(content.as_bytes());
        hasher.update(b"\x00");
    }
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn compile_program_multi(
    program: crate::parser::ParsedProgram,
) -> Result<CompiledProgram, CompilerError> {
    compile_program_impl(program, None)
}

/// Like [`compile_program_multi`] but also starts source map tracking for the
/// main package, so that [`build_source_map`] produces valid mappings after
/// code generation.
pub fn compile_program_multi_with_source_map(
    program: crate::parser::ParsedProgram,
    go_file: &str,
    go_source: &str,
) -> Result<CompiledProgram, CompilerError> {
    compile_program_impl(
        program,
        Some(vec![(go_file.to_string(), go_source.to_string())]),
    )
}

/// Like [`compile_program_multi`] but starts source map tracking for every file
/// in the main package.
pub fn compile_program_multi_with_source_maps(
    program: crate::parser::ParsedProgram,
) -> Result<CompiledProgram, CompilerError> {
    let sources = program.main_package.files.clone();
    compile_program_impl(program, Some(sources))
}

fn compile_program_impl(
    program: crate::parser::ParsedProgram,
    source_map_config: Option<Vec<(String, String)>>,
) -> Result<CompiledProgram, CompilerError> {
    DEFER_COUNTER.with(|c| *c.borrow_mut() = 0);
    SWITCH_COUNTER.with(|c| *c.borrow_mut() = 0);
    SELECT_COUNTER.with(|c| *c.borrow_mut() = 0);
    LOOP_BODY_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|c| *c.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    let mut modules = BTreeMap::new();
    let mut stdlib_imports = program.stdlib_imports.clone();
    collect_known_stdlib_imports(&program.main_package.ast, &mut stdlib_imports);
    for pkg in &program.imports {
        collect_known_stdlib_imports(&pkg.ast, &mut stdlib_imports);
    }
    let mut local_type_envs: BTreeMap<String, (String, typeinfer::TypeEnv)> = BTreeMap::new();
    {
        let timer = ProfileTimer::start("compiler.local_type_inference");
        for pkg in &program.imports {
            let mut env = typeinfer::TypeEnv::new();
            env.scan_file(&pkg.ast);
            local_type_envs.insert(pkg.import_path.clone(), (pkg.name.clone(), env));
        }
        drop(timer);
    }
    let mut stdlib_type_envs: BTreeMap<String, (String, typeinfer::TypeEnv)> = BTreeMap::new();

    let builtins_file: syn::File = syn::parse_str(crate::printer::GORS_BUILTINS).map_err(|e| {
        CompilerError::UnsupportedConstruct(format!("failed to parse builtin: {e}"))
    })?;
    modules.insert(
        "builtin".to_string(),
        CompiledModule {
            mod_name: "builtin".to_string(),
            import_path: "builtin".to_string(),
            file: builtins_file,
            filename: "builtin.rs".to_string(),
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        },
    );

    let mut stdlib_type_env_paths = stdlib_imports.clone();
    for stdlib_path in &stdlib_imports {
        for transitive_import in crate::resolve::collect_transitive_imports(stdlib_path) {
            if !stdlib_type_env_paths.contains(&transitive_import) {
                stdlib_type_env_paths.push(transitive_import);
            }
        }
    }
    for stdlib_path in &stdlib_type_env_paths {
        if let Some((package_name, env)) = crate::resolve::scan_type_env(stdlib_path) {
            stdlib_type_envs.insert(stdlib_path.clone(), (package_name, env));
        }
    }

    let import_package_names: BTreeMap<String, String> = local_type_envs
        .iter()
        .map(|(path, (package_name, _))| (path.clone(), package_name.clone()))
        .chain(
            stdlib_type_envs
                .iter()
                .map(|(path, (package_name, _))| (path.clone(), package_name.clone())),
        )
        .collect();

    let stdlib_mod_names: std::collections::HashSet<String> =
        std::iter::once("builtin".to_string())
            .chain(
                crate::resolve::list_packages()
                    .into_iter()
                    .map(|path| crate::resolve::module_name(&path)),
            )
            .chain(
                stdlib_imports
                    .iter()
                    .map(|path| crate::resolve::module_name(path)),
            )
            .collect();
    let local_module_names: BTreeMap<String, String> = program
        .imports
        .iter()
        .map(|pkg| {
            let mod_name = if stdlib_mod_names.contains(&pkg.name) {
                import_path_to_mod_name(&pkg.import_path)
            } else {
                pkg.name.clone()
            };
            (pkg.import_path.clone(), mod_name)
        })
        .collect();
    let stdlib_module_names: BTreeMap<String, String> = stdlib_imports
        .iter()
        .map(|path| (path.clone(), crate::resolve::module_name(path)))
        .collect();
    let pkg_names: std::collections::HashSet<String> = local_module_names
        .values()
        .cloned()
        .chain(stdlib_module_names.values().cloned())
        .collect();

    let local_compile_timer = ProfileTimer::start("compiler.local_compile");
    for pkg in program.imports {
        let content_hash = compute_content_hash(&pkg.files);
        let mut type_env = local_type_envs
            .get(&pkg.import_path)
            .map(|(_, env)| env.clone())
            .unwrap_or_default();
        merge_import_type_envs(&mut type_env, &pkg.ast, &local_type_envs, &stdlib_type_envs);
        let import_rewrites = import_module_rewrites(
            &pkg.ast,
            &local_type_envs,
            &local_module_names,
            &stdlib_type_envs,
            &stdlib_module_names,
        );
        validate_file_with_type_env_and_import_package_names(
            &pkg.ast,
            &type_env,
            &import_package_names,
        )?;
        validate_unused_imports(&pkg.ast, &import_package_names)?;
        let _ir = ir::lower_file(&pkg.ast, &type_env);
        set_import_package_names(import_package_names.clone());
        set_type_env(type_env);
        set_import_renames(import_rewrites.clone());
        let mut pkg_file = TryInto::<syn::File>::try_into(pkg.ast)?;
        rewrite_import_module_paths(&mut pkg_file, &import_rewrites);
        passes::pass_for_imported_package(&mut pkg_file);
        rewrite_import_module_paths(&mut pkg_file, &import_rewrites);
        prefix_sibling_paths(&mut pkg_file, &pkg_names);

        let filename = import_path_to_filename(&pkg.import_path);
        let mod_name = local_module_names
            .get(&pkg.import_path)
            .cloned()
            .unwrap_or_else(|| pkg.name.clone());
        modules.insert(
            pkg.import_path.clone(),
            CompiledModule {
                mod_name,
                import_path: pkg.import_path.clone(),
                file: pkg_file,
                filename,
                content_hash,
                is_main: false,
                is_stdlib: false,
            },
        );
    }

    if let Some(sources) = source_map_config {
        TRACKER.with(|t| {
            t.borrow_mut().start_many(
                sources
                    .into_iter()
                    .map(|(file, source)| (file, Some(source)))
                    .collect(),
                "main.rs",
            );
        });
    }

    let has_main_fn = program.main_package.name == "main"
        && program
            .main_package
            .ast
            .decls
            .iter()
            .any(|d| matches!(d, ast::Decl::FuncDecl(f) if f.name.name == "main"));
    if let Some(invalid) = ir::invalid_main_package_in_file(&program.main_package.ast) {
        return Err(invalid_signature_error(invalid));
    }

    let main_hash = compute_content_hash(&program.main_package.files);
    let mut main_type_env = typeinfer::TypeEnv::new();
    main_type_env.scan_file(&program.main_package.ast);
    merge_import_type_envs(
        &mut main_type_env,
        &program.main_package.ast,
        &local_type_envs,
        &stdlib_type_envs,
    );
    for (package_name, env) in stdlib_type_envs.values() {
        main_type_env.merge_package(package_name, env);
    }
    let main_import_rewrites = import_module_rewrites(
        &program.main_package.ast,
        &local_type_envs,
        &local_module_names,
        &stdlib_type_envs,
        &stdlib_module_names,
    );
    validate_file_with_type_env_and_import_package_names(
        &program.main_package.ast,
        &main_type_env,
        &import_package_names,
    )?;
    validate_unused_imports(&program.main_package.ast, &import_package_names)?;
    let _ir = ir::lower_file(&program.main_package.ast, &main_type_env);
    set_import_package_names(import_package_names);
    set_type_env(main_type_env);
    set_import_renames(main_import_rewrites.clone());
    let mut main_file: syn::File = program.main_package.ast.try_into()?;
    rewrite_import_module_paths(&mut main_file, &main_import_rewrites);
    passes::pass(&mut main_file);
    rewrite_import_module_paths(&mut main_file, &main_import_rewrites);

    modules.insert(
        "__main__".to_string(),
        CompiledModule {
            mod_name: "main".to_string(),
            import_path: String::new(),
            file: main_file,
            filename: "main.rs".to_string(),
            content_hash: main_hash,
            is_main: true,
            is_stdlib: false,
        },
    );
    drop(local_compile_timer);

    let stdlib_timer = ProfileTimer::start("compiler.stdlib_resolution");
    resolve_required_stdlib_modules(&mut modules, &stdlib_imports);
    prune_dependency_stdlib_modules(&mut modules, &stdlib_imports);
    drop(stdlib_timer);

    for module in modules.values_mut() {
        cast_self_in_pointer_comparisons(&mut module.file);
    }

    let dce_timer = ProfileTimer::start("compiler.dce");
    prune_generated_dead_code(&mut modules, has_main_fn);
    inject_post_prune_stdlib_helpers(&mut modules, &stdlib_imports);
    prune_generated_dead_code(&mut modules, has_main_fn);
    borrow_mutated_vec_params(&mut modules);
    restore_vec_newtype_method_receivers(&mut modules);
    borrow_mut_ref_call_args(&mut modules);
    restore_vec_newtype_method_receivers(&mut modules);
    clone_vec_value_call_args(&mut modules);
    drop(dce_timer);

    prefix_final_module_paths(&mut modules);

    Ok(CompiledProgram {
        modules,
        has_main: has_main_fn,
    })
}

fn inject_post_prune_stdlib_helpers(
    modules: &mut BTreeMap<String, CompiledModule>,
    roots: &[String],
) {
    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        match module.mod_name.as_str() {
            "reflect"
                if module
                    .file
                    .items
                    .iter()
                    .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == "Value"))
                => {
                    module.file.items = vec![syn::parse_quote! {
                        #[derive(Clone, Default)]
                        pub struct Value;
                    }];
                    module.content_hash = String::new();
                }
            "os"
                if module
                    .file
                    .items
                    .iter()
                    .any(|item| matches!(item, syn::Item::Static(item_static) if item_static.ident == "Stdout"))
                => {
                    module.file.items.retain(|item| match item {
                        syn::Item::Impl(item_impl) => {
                            named_self_type(&item_impl.self_ty).as_deref() != Some("File")
                        }
                        _ => item_name(item)
                            .as_deref()
                            .is_none_or(|name| !matches!(name, "File" | "Stdout")),
                    });
                    module.file.items.extend([
                        syn::parse_quote! {
                            #[derive(Clone, Copy, Default)]
                            pub struct File;
                        },
                        syn::parse_quote! {
                            #[allow(non_upper_case_globals)]
                            pub static Stdout: std::sync::LazyLock<File> =
                                std::sync::LazyLock::new(|| File);
                        },
                        syn::parse_quote! {
                            impl crate::io::Writer for File {
                                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                                    Some(self)
                                }

                                fn Write(&mut self, b: Vec<u8>) -> (isize, String) {
                                    let mut stdout = std::io::stdout();
                                    match std::io::Write::write_all(&mut stdout, &b) {
                                        Ok(()) => (b.len() as isize, String::new()),
                                        Err(err) => (0, err.to_string()),
                                    }
                                }
                            }
                        },
                    ]);
                    module.content_hash = String::new();
                }
            _ => {}
        }
    }
    let mut preserved = std::collections::HashSet::from(["builtin".to_string()]);
    preserved.extend(roots.iter().map(|root| crate::resolve::module_name(root)));
    prune_unreferenced_stdlib_modules(modules, &preserved);
}

fn borrow_mutated_vec_params(modules: &mut BTreeMap<String, CompiledModule>) {
    let mut targets = collect_mut_ref_vec_targets(modules);

    loop {
        if !targets.is_empty() {
            for module in modules.values_mut() {
                syn::visit_mut::VisitMut::visit_file_mut(
                    &mut BorrowMutatedVecCallArgs {
                        module_name: module.mod_name.clone(),
                        targets: &targets,
                    },
                    &mut module.file,
                );
            }
        }

        let mut changed = false;
        for module in modules.values_mut() {
            let module_name = module.mod_name.clone();
            for item in &mut module.file.items {
                let syn::Item::Fn(item_fn) = item else {
                    continue;
                };
                if return_type_is_vec(&item_fn.sig.output) {
                    continue;
                }
                let params = mutated_vec_param_indices(&item_fn.sig, &item_fn.block);
                if params.is_empty() {
                    continue;
                }
                let key = format!("{}::{}", module_name, item_fn.sig.ident);
                let indices = params.iter().map(|(index, _, _)| *index).collect();
                rewrite_vec_params_as_mut_refs(&mut item_fn.sig, &params);
                reborrow_mutated_vec_params(&mut item_fn.block, &params);
                if targets.insert(key, indices).is_none() {
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }
}

fn collect_mut_ref_vec_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> BTreeMap<String, std::collections::HashSet<usize>> {
    let mut targets = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Fn(item_fn) = item else {
                continue;
            };
            let indices = mut_ref_vec_param_indices(&item_fn.sig);
            if indices.is_empty() {
                continue;
            }
            targets.insert(
                format!("{}::{}", module.mod_name, item_fn.sig.ident),
                indices,
            );
        }
    }
    targets
}

fn mut_ref_vec_param_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            mut_ref_vec_inner(&pat_type.ty).map(|_| index)
        })
        .collect()
}

fn mut_ref_vec_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Reference(reference) = ty else {
        return None;
    };
    reference.mutability.as_ref()?;
    vec_type_inner(&reference.elem)
}

fn return_type_is_vec(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    vec_type_inner(ty).is_some()
}

fn mutated_vec_param_indices(
    sig: &syn::Signature,
    block: &syn::Block,
) -> Vec<(usize, syn::Ident, syn::Type)> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            let syn::Pat::Ident(pat_ident) = &*pat_type.pat else {
                return None;
            };
            let inner = vec_type_inner(&pat_type.ty)?;
            body_mutates_vec_param(block, &pat_ident.ident)
                .then(|| (index, pat_ident.ident.clone(), inner))
        })
        .collect()
}

fn body_mutates_vec_param(block: &syn::Block, ident: &syn::Ident) -> bool {
    struct Finder<'a> {
        ident: &'a syn::Ident,
        found: bool,
    }

    fn lhs_mutates_vec_param(expr: &syn::Expr, ident: &syn::Ident) -> bool {
        match expr {
            syn::Expr::Field(field) => lhs_mutates_vec_param(&field.base, ident),
            syn::Expr::Index(index) => {
                expr_base_is_ident(&index.expr, ident) || lhs_mutates_vec_param(&index.expr, ident)
            }
            syn::Expr::Paren(paren) => lhs_mutates_vec_param(&paren.expr, ident),
            syn::Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .any(|elem| lhs_mutates_vec_param(elem, ident)),
            _ => false,
        }
    }

    fn expr_base_is_ident(expr: &syn::Expr, ident: &syn::Ident) -> bool {
        match expr {
            syn::Expr::Path(path) => path.path.is_ident(ident),
            syn::Expr::Field(field) => expr_base_is_ident(&field.base, ident),
            syn::Expr::Index(index) => expr_base_is_ident(&index.expr, ident),
            syn::Expr::Paren(paren) => expr_base_is_ident(&paren.expr, ident),
            _ => false,
        }
    }

    fn is_assign_binop(op: &syn::BinOp) -> bool {
        matches!(
            op,
            syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_)
        )
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if lhs_mutates_vec_param(&assign.left, self.ident) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_assign(self, assign);
        }

        fn visit_expr_binary(&mut self, binary: &syn::ExprBinary) {
            if is_assign_binop(&binary.op) && lhs_mutates_vec_param(&binary.left, self.ident) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_binary(self, binary);
        }

        fn visit_expr_reference(&mut self, reference: &syn::ExprReference) {
            if reference.mutability.is_some()
                && matches!(&*reference.expr, syn::Expr::Path(path) if path.path.is_ident(self.ident))
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_reference(self, reference);
        }
    }

    let mut finder = Finder {
        ident,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

fn vec_type_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let segment = path.path.segments.first()?;
    if segment.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner.clone())
}

fn rewrite_vec_params_as_mut_refs(
    sig: &mut syn::Signature,
    params: &[(usize, syn::Ident, syn::Type)],
) {
    for (index, _, inner) in params {
        let Some(syn::FnArg::Typed(pat_type)) = sig.inputs.iter_mut().nth(*index) else {
            continue;
        };
        *pat_type.ty = syn::parse_quote! { &mut Vec<#inner> };
    }
}

fn reborrow_mutated_vec_params(block: &mut syn::Block, params: &[(usize, syn::Ident, syn::Type)]) {
    struct Reborrow {
        names: std::collections::HashSet<String>,
    }

    impl syn::visit_mut::VisitMut for Reborrow {
        fn visit_expr_reference_mut(&mut self, reference: &mut syn::ExprReference) {
            syn::visit_mut::visit_expr_reference_mut(self, reference);
            if reference.mutability.is_none() {
                return;
            }
            let syn::Expr::Path(path) = &*reference.expr else {
                return;
            };
            let Some(ident) = path.path.get_ident() else {
                return;
            };
            if !self.names.contains(&ident.to_string()) {
                return;
            }
            *reference.expr = syn::parse_quote! { *#ident };
        }
    }

    let names = params
        .iter()
        .map(|(_, ident, _)| ident.to_string())
        .collect();
    syn::visit_mut::VisitMut::visit_block_mut(&mut Reborrow { names }, block);
}

struct BorrowMutatedVecCallArgs<'a> {
    module_name: String,
    targets: &'a BTreeMap<String, std::collections::HashSet<usize>>,
}

impl syn::visit_mut::VisitMut for BorrowMutatedVecCallArgs<'_> {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);
        let Some(key) = call_target_key(&call.func, &self.module_name) else {
            return;
        };
        let Some(indices) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                borrow_mut_vec_call_arg(arg);
            }
        }
    }
}

fn call_target_key(func: &syn::Expr, current_module: &str) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match segments.as_slice() {
        [name] => Some(format!("{current_module}::{name}")),
        [.., module, name] => Some(format!("{module}::{name}")),
        [] => None,
    }
}

fn borrow_mut_vec_call_arg(arg: &mut syn::Expr) {
    if matches!(arg, syn::Expr::Reference(_)) {
        return;
    }
    if let Some(name) = expr_path_ident(arg) {
        if name == "self" {
            return;
        }
        let ident = syn::Ident::new(&name, Span::mixed_site());
        *arg = syn::parse_quote! { &mut #ident };
        return;
    }
    let inner = arg.clone();
    *arg = syn::parse_quote! { &mut #inner };
}

fn clone_vec_value_call_args(modules: &mut BTreeMap<String, CompiledModule>) {
    let targets = collect_vec_value_param_targets(modules);
    let vec_newtypes = collect_vec_newtypes(modules);
    if targets.is_empty() && vec_newtypes.is_empty() {
        return;
    }

    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut CloneVecValueCallArgs {
                module_name: module.mod_name.clone(),
                targets: &targets,
                vec_newtypes: &vec_newtypes,
            },
            &mut module.file,
        );
    }
}

fn collect_vec_value_param_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> BTreeMap<String, std::collections::HashSet<usize>> {
    let mut targets = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Fn(item_fn) = item else {
                continue;
            };
            let indices = vec_value_param_indices(&item_fn.sig);
            if indices.is_empty() {
                continue;
            }
            targets.insert(
                format!("{}::{}", module.mod_name, item_fn.sig.ident),
                indices,
            );
        }
    }
    targets
}

fn vec_value_param_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    let clone_type_params: std::collections::HashSet<String> = sig
        .generics
        .params
        .iter()
        .filter_map(|param| {
            let syn::GenericParam::Type(type_param) = param else {
                return None;
            };
            type_param
                .bounds
                .iter()
                .any(|bound| {
                    matches!(bound, syn::TypeParamBound::Trait(trait_bound) if trait_bound.path.is_ident("Clone"))
                })
                .then(|| type_param.ident.to_string())
        })
        .collect();
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            cloneable_value_param_type(&pat_type.ty, &clone_type_params).then_some(index)
        })
        .collect()
}

fn cloneable_value_param_type(
    ty: &syn::Type,
    clone_type_params: &std::collections::HashSet<String>,
) -> bool {
    if matches!(ty, syn::Type::Reference(_)) {
        return false;
    }
    if let Some(inner) = vec_type_inner(ty) {
        return !is_box_dyn_any_type(&inner);
    }
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    matches!(segment.ident.to_string().as_str(), "String")
        || clone_type_params.contains(&segment.ident.to_string())
}

fn is_box_dyn_any_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() || type_path.path.segments.len() != 1 {
        return false;
    }
    let Some(segment) = type_path.path.segments.first() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(syn::Type::TraitObject(trait_object))) = args.args.first()
    else {
        return false;
    };
    trait_object
        .bounds
        .iter()
        .any(|bound| matches!(bound, syn::TypeParamBound::Trait(trait_bound) if trait_bound.path.to_token_stream().to_string() == "dyn std :: any :: Any" || trait_bound.path.to_token_stream().to_string() == "std :: any :: Any" || trait_bound.path.to_token_stream().to_string() == "Any"))
}

struct CloneVecValueCallArgs<'a> {
    module_name: String,
    targets: &'a BTreeMap<String, std::collections::HashSet<usize>>,
    vec_newtypes: &'a std::collections::HashSet<String>,
}

impl syn::visit_mut::VisitMut for CloneVecValueCallArgs<'_> {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);

        if let Some(type_key) = from_call_vec_newtype_key(&call.func, &self.module_name)
            && self.vec_newtypes.contains(&type_key)
            && call.args.len() == 1
            && let Some(arg) = call.args.first_mut()
        {
            clone_path_arg(arg);
        }

        let Some(key) = call_target_key(&call.func, &self.module_name) else {
            return;
        };
        let Some(indices) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                clone_path_arg(arg);
            }
        }
    }
}

fn clone_path_arg(arg: &mut syn::Expr) {
    if !matches!(arg, syn::Expr::Path(_)) {
        return;
    }
    let Some(name) = expr_path_ident(arg) else {
        return;
    };
    if name == "self" {
        return;
    }
    let ident = syn::Ident::new(&name, Span::mixed_site());
    *arg = syn::parse_quote! { #ident.clone() };
}

fn expr_path_ident(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    path.path.get_ident().map(ToString::to_string)
}

fn borrow_mut_ref_call_args(modules: &mut BTreeMap<String, CompiledModule>) {
    let targets = collect_mut_ref_targets(modules);
    if targets.is_empty() {
        return;
    }
    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut BorrowMutRefCallArgs {
                module_name: module.mod_name.clone(),
                targets: &targets,
            },
            &mut module.file,
        );
    }
}

fn collect_mut_ref_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> BTreeMap<String, std::collections::HashSet<usize>> {
    let mut targets = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Fn(item_fn) = item else {
                continue;
            };
            let indices = mut_ref_param_indices(&item_fn.sig);
            if indices.is_empty() {
                continue;
            }
            targets.insert(
                format!("{}::{}", module.mod_name, item_fn.sig.ident),
                indices,
            );
        }
    }
    targets
}

fn mut_ref_param_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
                .then_some(index)
        })
        .collect()
}

struct BorrowMutRefCallArgs<'a> {
    module_name: String,
    targets: &'a BTreeMap<String, std::collections::HashSet<usize>>,
}

impl syn::visit_mut::VisitMut for BorrowMutRefCallArgs<'_> {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);
        let Some(key) = call_target_key(&call.func, &self.module_name) else {
            return;
        };
        let Some(indices) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                borrow_mut_vec_call_arg(arg);
            }
        }
    }
}

fn restore_vec_newtype_method_receivers(modules: &mut BTreeMap<String, CompiledModule>) {
    let vec_newtypes = collect_vec_newtypes(modules);
    if vec_newtypes.is_empty() {
        return;
    }
    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut RestoreVecNewtypeMethodReceivers {
                module_name: module.mod_name.clone(),
                vec_newtypes: &vec_newtypes,
                counter: 0,
            },
            &mut module.file,
        );
    }
}

fn collect_vec_newtypes(
    modules: &BTreeMap<String, CompiledModule>,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Struct(item_struct) = item else {
                continue;
            };
            let syn::Fields::Unnamed(fields) = &item_struct.fields else {
                continue;
            };
            let Some(field) = fields.unnamed.first() else {
                continue;
            };
            if vec_type_inner(&field.ty).is_some() {
                out.insert(format!("{}::{}", module.mod_name, item_struct.ident));
            }
        }
    }
    out
}

struct RestoreVecNewtypeMethodReceivers<'a> {
    module_name: String,
    vec_newtypes: &'a std::collections::HashSet<String>,
    counter: usize,
}

impl syn::visit_mut::VisitMut for RestoreVecNewtypeMethodReceivers<'_> {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        syn::visit_mut::visit_block_mut(self, block);
        for stmt in &mut block.stmts {
            if let Some(rewritten) = self.rewrite_stmt(stmt) {
                *stmt = rewritten;
            } else if let Some(rewritten) = self.rewrite_borrowed_newtype_calls(stmt) {
                *stmt = rewritten;
            }
        }
    }
}

impl RestoreVecNewtypeMethodReceivers<'_> {
    fn rewrite_stmt(&mut self, stmt: &syn::Stmt) -> Option<syn::Stmt> {
        let syn::Stmt::Expr(syn::Expr::MethodCall(method_call), semi) = stmt else {
            return None;
        };
        let syn::Expr::Call(receiver_call) = &*method_call.receiver else {
            return None;
        };
        let type_key = from_call_vec_newtype_key(&receiver_call.func, &self.module_name)?;
        if !self.vec_newtypes.contains(&type_key) {
            return None;
        }
        let source_name = receiver_call.args.first().and_then(expr_path_ident)?;
        if receiver_call.args.len() != 1 {
            return None;
        }

        let temp = quote::format_ident!("__gors_vec_newtype_recv_{}", self.counter);
        self.counter += 1;
        let source = syn::Ident::new(&source_name, Span::mixed_site());
        let from_func = receiver_call.func.clone();
        let method = method_call.method.clone();
        let args = method_call.args.iter().cloned().collect::<Vec<_>>();
        let expr: syn::Expr = syn::parse_quote! {{
            let mut #temp = #from_func(std::mem::take(&mut #source));
            #temp.#method(#(#args),*);
            #source = Vec::from(#temp);
        }};
        Some(syn::Stmt::Expr(expr, *semi))
    }

    fn rewrite_borrowed_newtype_calls(&mut self, stmt: &syn::Stmt) -> Option<syn::Stmt> {
        let syn::Stmt::Expr(_, semi) = stmt else {
            return None;
        };
        let mut stmt = stmt.clone();
        let mut hoister = VecNewtypeBorrowHoister {
            module_name: self.module_name.clone(),
            vec_newtypes: self.vec_newtypes,
            counter: &mut self.counter,
            bindings: vec![],
        };
        syn::visit_mut::VisitMut::visit_stmt_mut(&mut hoister, &mut stmt);
        if hoister.bindings.is_empty() {
            return None;
        }
        let prelude = hoister
            .bindings
            .iter()
            .map(|binding| {
                let temp = &binding.temp;
                let source = &binding.source;
                let from_func = &binding.from_func;
                syn::parse_quote! {
                    let mut #temp = #from_func(std::mem::take(&mut #source));
                }
            })
            .collect::<Vec<syn::Stmt>>();
        let epilogue = hoister
            .bindings
            .iter()
            .rev()
            .map(|binding| {
                let temp = &binding.temp;
                let source = &binding.source;
                syn::parse_quote! {
                    #source = Vec::from(#temp);
                }
            })
            .collect::<Vec<syn::Stmt>>();
        let expr: syn::Expr = syn::parse_quote! {{
            #(#prelude)*
            #stmt
            #(#epilogue)*
        }};
        Some(syn::Stmt::Expr(expr, *semi))
    }
}

struct VecNewtypeBorrowBinding {
    temp: syn::Ident,
    source: syn::Ident,
    from_func: syn::Expr,
}

struct VecNewtypeBorrowHoister<'a> {
    module_name: String,
    vec_newtypes: &'a std::collections::HashSet<String>,
    counter: &'a mut usize,
    bindings: Vec<VecNewtypeBorrowBinding>,
}

impl syn::visit_mut::VisitMut for VecNewtypeBorrowHoister<'_> {
    fn visit_expr_reference_mut(&mut self, reference: &mut syn::ExprReference) {
        syn::visit_mut::visit_expr_reference_mut(self, reference);
        if reference.mutability.is_none() {
            return;
        }
        let syn::Expr::Call(call) = &*reference.expr else {
            return;
        };
        let Some(type_key) = from_call_vec_newtype_key(&call.func, &self.module_name) else {
            return;
        };
        if !self.vec_newtypes.contains(&type_key) || call.args.len() != 1 {
            return;
        }
        let Some(source_name) = call.args.first().and_then(expr_path_ident) else {
            return;
        };
        let temp = quote::format_ident!("__gors_vec_newtype_arg_{}", *self.counter);
        *self.counter += 1;
        let source = syn::Ident::new(&source_name, Span::mixed_site());
        self.bindings.push(VecNewtypeBorrowBinding {
            temp: temp.clone(),
            source,
            from_func: (*call.func).clone(),
        });
        *reference.expr = syn::parse_quote! { #temp };
    }
}

fn from_call_vec_newtype_key(func: &syn::Expr, current_module: &str) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match segments.as_slice() {
        [ty, from] if from == "from" => Some(format!("{current_module}::{ty}")),
        [.., module, ty, from] if from == "from" => Some(format!("{module}::{ty}")),
        _ => None,
    }
}

fn prefix_final_module_paths(modules: &mut BTreeMap<String, CompiledModule>) {
    let module_names: std::collections::HashSet<String> = modules
        .values()
        .map(|module| module.mod_name.clone())
        .collect();
    for module in modules
        .values_mut()
        .filter(|module| !module.is_main && module.mod_name != "builtin")
    {
        prefix_sibling_paths(&mut module.file, &module_names);
    }
}

fn prune_generated_dead_code(modules: &mut BTreeMap<String, CompiledModule>, has_main: bool) {
    use std::collections::{HashMap, HashSet};

    loop {
        let before = modules_reachability_fingerprint(modules);
        let module_names: HashSet<String> = modules
            .values()
            .filter(|module| !module.is_main)
            .map(|module| module.mod_name.clone())
            .collect();

        if let Some(main_module) = modules.get_mut("__main__") {
            let roots = if has_main {
                HashSet::from(["main".to_string()])
            } else {
                exported_item_reachability_names(&main_module.file.items)
            };
            prune_items_to_roots(&mut main_module.file.items, &roots, &module_names);
        }

        let mut required: HashMap<String, HashSet<String>> = HashMap::new();
        if let Some(main_module) = modules.get("__main__") {
            merge_required_refs(
                &mut required,
                collect_external_refs(&main_module.file.items, &module_names),
            );
        }

        loop {
            let mut changed = false;
            for module in modules.values().filter(|module| !module.is_main) {
                let Some(roots) = required.get(&module.mod_name) else {
                    continue;
                };
                if roots.is_empty() {
                    continue;
                }
                let expanded_roots;
                let roots = if module.mod_name == "builtin" {
                    expanded_roots = expand_builtin_roots(roots);
                    &expanded_roots
                } else {
                    roots
                };
                let (_, refs, _) = reachable_stdlib_items(&module.file.items, roots, &module_names);
                changed |= merge_required_refs(&mut required, refs);
            }
            if !changed {
                break;
            }
        }

        let removable: Vec<String> = modules
            .iter()
            .filter(|&(_key, module)| {
                !module.is_main
                    && required
                        .get(&module.mod_name)
                        .is_none_or(|roots| roots.is_empty())
            })
            .map(|(key, _module)| key.clone())
            .collect();
        for key in removable {
            modules.remove(&key);
        }

        for module in modules.values_mut().filter(|module| !module.is_main) {
            let Some(roots) = required.get(&module.mod_name) else {
                continue;
            };
            let expanded_roots;
            let roots = if module.mod_name == "builtin" {
                expanded_roots = expand_builtin_roots(roots);
                &expanded_roots
            } else {
                roots
            };
            prune_items_to_roots(&mut module.file.items, roots, &module_names);
            if module.mod_name == "builtin" {
                prune_builtin_channel_helpers(&mut module.file.items, roots);
                prune_builtin_complex_helpers(&mut module.file.items, roots);
                prune_builtin_bitcast_helpers(&mut module.file.items, roots);
                prune_unneeded_builtin_traits(&mut module.file.items, roots);
            } else {
                let mut builtin_roots = required.get("builtin").cloned().unwrap_or_default();
                if let Some(local_builtin_roots) =
                    collect_external_refs(&module.file.items, &module_names).remove("builtin")
                {
                    builtin_roots.extend(local_builtin_roots);
                }
                if !builtin_roots.is_empty() {
                    prune_unneeded_builtin_traits(&mut module.file.items, &builtin_roots);
                }
            }
            prune_display_impls_without_string_method(&mut module.file.items);
            if has_main {
                prune_unused_struct_fields(&mut module.file.items);
            }
            if module.is_stdlib {
                module.content_hash = String::new();
            }
        }

        for module in modules.values_mut() {
            if has_main {
                prune_unused_struct_fields(&mut module.file.items);
            }
            prune_unused_use_items(&mut module.file.items);
        }

        let empty_modules: Vec<String> = modules
            .iter()
            .filter(|&(_key, module)| !module.is_main && module.file.items.is_empty())
            .map(|(key, _module)| key.clone())
            .collect();
        for key in empty_modules {
            modules.remove(&key);
        }

        if modules_reachability_fingerprint(modules) == before {
            break;
        }
    }
}

fn prune_display_impls_without_string_method(items: &mut Vec<syn::Item>) {
    let stringer_types: std::collections::HashSet<String> = items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            if item_impl.trait_.is_some()
                || !item_impl.items.iter().any(|impl_item| {
                    matches!(impl_item, syn::ImplItem::Fn(func) if func.sig.ident == "String")
                })
            {
                return None;
            }
            named_self_type(&item_impl.self_ty)
        })
        .collect();

    items.retain(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        let is_display_impl = item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Display")
        });
        if !is_display_impl {
            return true;
        }
        named_self_type(&item_impl.self_ty)
            .is_none_or(|self_name| stringer_types.contains(&self_name))
    });
}

fn cast_self_in_pointer_comparisons(file: &mut syn::File) {
    use syn::visit_mut::VisitMut;

    fn is_self_expr(expr: &syn::Expr) -> bool {
        matches!(
            expr,
            syn::Expr::Path(path)
                if path.path.leading_colon.is_none()
                    && path.path.segments.len() == 1
                    && path.path.segments.first().is_some_and(|segment| segment.ident == "self")
        )
    }

    fn is_self_field_expr(expr: &syn::Expr) -> bool {
        matches!(
            expr,
            syn::Expr::Field(field)
                if matches!(
                    &*field.base,
                    syn::Expr::Path(path)
                        if path.path.leading_colon.is_none()
                            && path.path.segments.len() == 1
                            && path.path.segments.first().is_some_and(|segment| segment.ident == "self")
                )
        )
    }

    struct Visitor;

    impl VisitMut for Visitor {
        fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
            syn::visit_mut::visit_expr_binary_mut(self, binary);
            if !matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
                return;
            }

            if is_self_expr(&binary.left) && is_self_field_expr(&binary.right) {
                *binary.left = syn::parse_quote! { self as *mut Self };
            } else if is_self_expr(&binary.right) && is_self_field_expr(&binary.left) {
                *binary.right = syn::parse_quote! { self as *mut Self };
            }
        }
    }

    Visitor.visit_file_mut(file);
}

fn prune_items_to_roots(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) {
    let (keep, _, names) = reachable_stdlib_items(items, roots, module_names);
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    *items = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            keep.contains(&idx)
                .then(|| reachable_item_for_names(item, &names, &item_names, &top_level_names))
                .flatten()
        })
        .collect();
}

fn expand_builtin_roots(
    roots: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut expanded = roots.clone();
    let needs_channel_methods = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Chan"
                | "ChanIter"
                | "ChanInner"
                | "make_chan"
                | "close"
                | "send"
                | "recv"
                | "recv_with_ok"
                | "try_send"
                | "try_recv"
                | "Chan::send"
                | "Chan::recv"
                | "Chan::recv_with_ok"
                | "Chan::try_send"
                | "Chan::try_recv"
                | "Chan::len"
                | "Chan::cap"
        )
    });
    if needs_channel_methods {
        for root in [
            "Chan",
            "ChanIter",
            "ChanInner",
            "Chan::new",
            "Chan::send",
            "Chan::recv",
            "Chan::recv_with_ok",
            "Chan::try_send",
            "Chan::try_recv",
            "new",
            "send",
            "recv",
            "recv_with_ok",
            "try_send",
            "try_recv",
        ] {
            expanded.insert(root.to_string());
        }
    }
    expanded
}

fn modules_reachability_fingerprint(modules: &BTreeMap<String, CompiledModule>) -> String {
    let mut out = String::new();
    for (key, module) in modules {
        out.push_str(key);
        out.push('\0');
        out.push_str(&module.mod_name);
        out.push('\0');
        for item in &module.file.items {
            out.push_str(&item.to_token_stream().to_string());
            out.push('\0');
        }
        out.push('\u{1f}');
    }
    out
}

fn exported_item_reachability_names(items: &[syn::Item]) -> std::collections::HashSet<String> {
    let mut roots = std::collections::HashSet::new();
    for item in items {
        if let Some(name) = item_name(item)
            && is_go_exported_name(&name)
        {
            roots.insert(name);
        }
        if let syn::Item::Impl(item_impl) = item
            && let Some(self_name) = named_self_type(&item_impl.self_ty)
        {
            for impl_item in &item_impl.items {
                match impl_item {
                    syn::ImplItem::Const(item) if is_go_exported_name(&item.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.ident.to_string(),
                        ));
                    }
                    syn::ImplItem::Fn(item) if is_go_exported_name(&item.sig.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.sig.ident.to_string(),
                        ));
                    }
                    syn::ImplItem::Type(item) if is_go_exported_name(&item.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.ident.to_string(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if let syn::Item::Trait(item_trait) = item {
            let trait_name = item_trait.ident.to_string();
            for trait_item in &item_trait.items {
                if let syn::TraitItem::Fn(func) = trait_item {
                    let name = func.sig.ident.to_string();
                    roots.insert(name.clone());
                    roots.insert(impl_method_reachability_name(&trait_name, &name));
                }
            }
        }
    }
    roots
}

fn is_go_exported_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn prune_builtin_channel_helpers(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
) {
    if roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Chan"
                | "ChanIter"
                | "ChanInner"
                | "make_chan"
                | "close"
                | "send"
                | "recv"
                | "recv_with_ok"
                | "Chan::send"
                | "Chan::recv"
                | "Chan::recv_with_ok"
                | "Chan::len"
                | "Chan::cap"
        )
    }) {
        return;
    }

    let channel_names = std::collections::HashSet::from([
        "Chan".to_string(),
        "ChanIter".to_string(),
        "ChanInner".to_string(),
        "lock_chan".to_string(),
        "wait_chan".to_string(),
        "make_chan".to_string(),
        "close".to_string(),
    ]);
    items.retain(|item| {
        if item_name(item).is_some_and(|name| channel_names.contains(&name)) {
            return false;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        !named_self_type(&item_impl.self_ty).is_some_and(|name| channel_names.contains(&name))
    });
}

fn prune_builtin_complex_helpers(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
) {
    let needs_complex64 = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Complex64" | "complex64" | "real64" | "imag64" | "to_complex64"
        )
    });
    let needs_complex_conversions = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "to_complex64" | "to_complex128" | "Complex64Value" | "Complex128Value"
        )
    });

    items.retain(|item| {
        if let Some(name) = item_name(item)
            && name == "impl_real_complex_conversions"
        {
            return needs_complex_conversions;
        }
        if let syn::Item::Struct(item_struct) = item
            && item_struct.ident == "Complex64"
        {
            return needs_complex64 || needs_complex_conversions;
        }
        if let syn::Item::Trait(item_trait) = item
            && matches!(
                item_trait.ident.to_string().as_str(),
                "Complex64Value" | "Complex128Value"
            )
        {
            return needs_complex_conversions;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        if named_self_type(&item_impl.self_ty).is_some_and(|name| name == "Complex64") {
            return needs_complex64 || needs_complex_conversions;
        }
        if let Some((_, path, _)) = &item_impl.trait_
            && path.segments.last().is_some_and(|seg| {
                matches!(
                    seg.ident.to_string().as_str(),
                    "Complex64Value" | "Complex128Value"
                )
            })
        {
            return needs_complex_conversions;
        }
        true
    });
}

fn prune_builtin_bitcast_helpers(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
) {
    if roots.contains("bitcast_ref") {
        return;
    }

    items.retain(|item| {
        if let syn::Item::Trait(item_trait) = item
            && item_trait.ident == "BitcastFrom"
        {
            return false;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        item_impl
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_none_or(|seg| seg.ident != "BitcastFrom")
    });
}

fn prune_unneeded_builtin_traits(
    items: &mut Vec<syn::Item>,
    builtin_roots: &std::collections::HashSet<String>,
) {
    items.retain(|item| {
        if let syn::Item::Trait(item_trait) = item
            && let Some(needed_root) = builtin_trait_required_root(&item_trait.ident.to_string())
        {
            return builtin_roots.contains(needed_root)
                || builtin_roots.contains(&item_trait.ident.to_string());
        }

        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        let Some((_, trait_path, _)) = &item_impl.trait_ else {
            return true;
        };
        let Some(trait_name) = trait_path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            return true;
        };
        let needed_root = builtin_trait_required_root(&trait_name);
        let Some(needed_root) = needed_root else {
            return true;
        };
        builtin_roots.contains(needed_root) || builtin_roots.contains(&trait_name)
    });
}

fn builtin_trait_required_root(trait_name: &str) -> Option<&'static str> {
    match trait_name {
        "Append" => Some("append"),
        "Cap" => Some("cap"),
        "Len" => Some("len"),
        "StringValue" => Some("string"),
        _ => None,
    }
}

fn prune_unused_struct_fields(items: &mut Vec<syn::Item>) {
    use syn::visit::Visit;
    use syn::visit_mut::VisitMut;

    let declared_fields = declared_named_fields(items);
    if declared_fields.is_empty() {
        return;
    }

    struct FieldUseCollector {
        used: std::collections::HashSet<String>,
    }

    impl<'ast> Visit<'ast> for FieldUseCollector {
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            if let syn::Member::Named(name) = &field.member {
                self.used.insert(name.to_string());
            }
            syn::visit::visit_expr_field(self, field);
        }

        fn visit_field_pat(&mut self, field: &'ast syn::FieldPat) {
            if let syn::Member::Named(name) = &field.member {
                self.used.insert(name.to_string());
            }
            syn::visit::visit_field_pat(self, field);
        }
    }

    let mut collector = FieldUseCollector {
        used: std::collections::HashSet::new(),
    };
    for item in items.iter() {
        collector.visit_item(item);
    }

    let mut removed = std::collections::HashSet::new();
    for item in items.iter_mut() {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let syn::Fields::Named(fields) = &mut item_struct.fields else {
            continue;
        };
        fields.named = fields
            .named
            .clone()
            .into_iter()
            .filter_map(|field| {
                let keep = field
                    .ident
                    .as_ref()
                    .is_none_or(|ident| collector.used.contains(&ident.to_string()));
                if !keep && let Some(ident) = &field.ident {
                    removed.insert(ident.to_string());
                }
                keep.then_some(field)
            })
            .collect();
    }

    if removed.is_empty() {
        return;
    }

    struct StructLiteralPruner<'a> {
        removed: &'a std::collections::HashSet<String>,
    }

    impl VisitMut for StructLiteralPruner<'_> {
        fn visit_expr_struct_mut(&mut self, expr: &mut syn::ExprStruct) {
            syn::visit_mut::visit_expr_struct_mut(self, expr);
            expr.fields = expr
                .fields
                .clone()
                .into_iter()
                .filter(|field| match &field.member {
                    syn::Member::Named(name) => !self.removed.contains(&name.to_string()),
                    syn::Member::Unnamed(_) => true,
                })
                .collect();
        }
    }

    let mut pruner = StructLiteralPruner { removed: &removed };
    for item in items {
        pruner.visit_item_mut(item);
    }
}

fn declared_named_fields(items: &[syn::Item]) -> std::collections::HashSet<String> {
    let mut fields = std::collections::HashSet::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let syn::Fields::Named(named) = &item_struct.fields else {
            continue;
        };
        for field in &named.named {
            if let Some(ident) = &field.ident {
                fields.insert(ident.to_string());
            }
        }
    }
    fields
}

fn prune_unused_use_items(items: &mut Vec<syn::Item>) {
    use syn::visit::Visit;

    struct UsedIdentCollector {
        used: std::collections::HashSet<String>,
    }

    impl<'ast> Visit<'ast> for UsedIdentCollector {
        fn visit_item_use(&mut self, _item: &'ast syn::ItemUse) {}

        fn visit_path(&mut self, path: &'ast syn::Path) {
            for segment in &path.segments {
                self.used.insert(segment.ident.to_string());
            }
            syn::visit::visit_path(self, path);
        }
    }

    let mut collector = UsedIdentCollector {
        used: std::collections::HashSet::new(),
    };
    for item in items.iter() {
        collector.visit_item(item);
    }

    items.retain_mut(|item| {
        let syn::Item::Use(item_use) = item else {
            return true;
        };
        prune_use_tree(&mut item_use.tree, &collector.used)
    });
}

fn prune_use_tree(tree: &mut syn::UseTree, used: &std::collections::HashSet<String>) -> bool {
    match tree {
        syn::UseTree::Name(name) => used.contains(&name.ident.to_string()),
        syn::UseTree::Rename(rename) => used.contains(&rename.rename.to_string()),
        syn::UseTree::Path(path) => prune_use_tree(&mut path.tree, used),
        syn::UseTree::Group(group) => {
            group.items = group
                .items
                .clone()
                .into_iter()
                .filter_map(|mut tree| prune_use_tree(&mut tree, used).then_some(tree))
                .collect();
            !group.items.is_empty()
        }
        syn::UseTree::Glob(_) => true,
    }
}

pub(crate) fn merge_import_type_envs(
    type_env: &mut typeinfer::TypeEnv,
    file: &ast::File,
    local_type_envs: &BTreeMap<String, (String, typeinfer::TypeEnv)>,
    stdlib_type_envs: &BTreeMap<String, (String, typeinfer::TypeEnv)>,
) {
    for import in file.imports() {
        let import_path = import.path.value.trim_matches('"');
        if let Some((package_name, package_env)) = local_type_envs.get(import_path) {
            type_env.merge_package(package_name, package_env);
            continue;
        }
        if let Some((package_name, package_env)) = stdlib_type_envs.get(import_path) {
            type_env.merge_package(package_name, package_env);
        }
    }
}

fn import_module_rewrites(
    file: &ast::File,
    local_type_envs: &BTreeMap<String, (String, typeinfer::TypeEnv)>,
    local_module_names: &BTreeMap<String, String>,
    stdlib_type_envs: &BTreeMap<String, (String, typeinfer::TypeEnv)>,
    stdlib_module_names: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut rewrites = BTreeMap::new();
    for import in file.imports() {
        let import_path = import.path.value.trim_matches('"');
        let (Some(mod_name), Some((package_name, _))) = (
            local_module_names
                .get(import_path)
                .or_else(|| stdlib_module_names.get(import_path)),
            local_type_envs
                .get(import_path)
                .or_else(|| stdlib_type_envs.get(import_path)),
        ) else {
            continue;
        };
        let local_name = import
            .name
            .as_ref()
            .and_then(|name| match name.name {
                "." | "_" => None,
                other => Some(other.to_string()),
            })
            .unwrap_or_else(|| package_name.clone());
        if local_name != *mod_name {
            rewrites.insert(local_name, mod_name.clone());
        }
    }
    rewrites
}

fn rewrite_import_module_paths(file: &mut syn::File, rewrites: &BTreeMap<String, String>) {
    if rewrites.is_empty() {
        return;
    }

    use syn::visit_mut::VisitMut;

    struct ImportModuleRewriter<'a> {
        rewrites: &'a BTreeMap<String, String>,
    }

    impl VisitMut for ImportModuleRewriter<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.leading_colon.is_some() || path.segments.is_empty() {
                return;
            }
            let Some(segment) = path.segments.iter_mut().next() else {
                return;
            };
            let first = segment.ident.to_string();
            if let Some(replacement) = self.rewrites.get(&first) {
                segment.ident = syn::Ident::new(replacement, Span::mixed_site());
            }
        }
    }

    ImportModuleRewriter { rewrites }.visit_file_mut(file);
}

fn prefix_sibling_paths(file: &mut syn::File, pkg_names: &std::collections::HashSet<String>) {
    use syn::visit_mut::VisitMut;

    struct PrefixSiblings<'a> {
        pkg_names: &'a std::collections::HashSet<String>,
    }

    impl VisitMut for PrefixSiblings<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);

            if path.leading_colon.is_some() {
                return;
            }
            if path.segments.len() >= 2 {
                let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                    return;
                };
                if self.pkg_names.contains(&first) {
                    let crate_seg = syn::PathSegment {
                        ident: syn::Ident::new("crate", Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    };
                    path.segments.insert(0, crate_seg);
                }
            }
        }
    }

    PrefixSiblings { pkg_names }.visit_file_mut(file);
}

fn resolve_required_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    roots: &[String],
) {
    use std::collections::{HashMap, HashSet};

    let mut import_path_by_module: HashMap<String, String> = crate::resolve::list_packages()
        .into_iter()
        .map(|path| (crate::resolve::module_name(&path), path))
        .collect();
    import_path_by_module.remove("builtin");
    for path in roots {
        import_path_by_module
            .entry(crate::resolve::module_name(path))
            .or_insert_with(|| path.clone());
    }
    let mut stdlib_mod_names: HashSet<String> = import_path_by_module.keys().cloned().collect();
    for module in modules.values().filter(|module| module.is_stdlib) {
        stdlib_mod_names.insert(module.mod_name.clone());
    }

    let mut required: HashMap<String, HashSet<String>> = HashMap::new();
    for path in roots {
        required
            .entry(crate::resolve::module_name(path))
            .or_default();
    }
    for module in modules.values().filter(|module| !module.is_stdlib) {
        merge_required_refs(
            &mut required,
            collect_external_refs(&module.file.items, &stdlib_mod_names),
        );
    }

    let mut loaded_roots: HashMap<String, HashSet<String>> = HashMap::new();
    loop {
        let pending: Vec<(String, String)> = required
            .keys()
            .filter(|module_name| {
                required
                    .get(module_name.as_str())
                    .is_some_and(|roots| !roots.is_empty())
            })
            .filter(|module_name| {
                let Some(roots) = required.get(module_name.as_str()) else {
                    return false;
                };
                !loaded_roots
                    .get(module_name.as_str())
                    .is_some_and(|loaded| roots.is_subset(loaded))
            })
            .filter_map(|module_name| {
                import_path_by_module
                    .get(module_name)
                    .map(|path| (module_name.clone(), path.clone()))
            })
            .collect();

        if pending.is_empty() {
            break;
        }

        let mut loaded_any = false;
        for (module_name, import_path) in pending {
            let required_roots = required.get(&module_name).cloned().unwrap_or_default();
            trace_stdlib_resolution(format_args!(
                "[gors] resolve stdlib {import_path} as {module_name} with roots {}",
                format_reachability_roots(required_roots.iter())
            ));
            let items = if let Some(stdlib_mod) =
                crate::resolve::resolve_with_roots(&import_path, &required_roots)
            {
                match stdlib_mod.content {
                    Some((_, items)) => items,
                    None => vec![],
                }
            } else {
                trace_stdlib_resolution(format_args!(
                    "[gors] stdlib {import_path} produced no Rust items"
                ));
                loaded_roots.insert(module_name, required_roots);
                continue;
            };

            for dep in crate::resolve::collect_resolved_imports(&import_path, &required_roots) {
                let dep_module = crate::resolve::module_name(&dep);
                stdlib_mod_names.insert(dep_module.clone());
                import_path_by_module.entry(dep_module).or_insert(dep);
            }

            let filename = format!("{}.rs", crate::resolve::module_name(&import_path));
            let loaded_module_name = module_name.clone();
            modules.insert(
                import_path.clone(),
                CompiledModule {
                    mod_name: module_name,
                    import_path,
                    file: syn::File {
                        attrs: vec![],
                        items,
                        shebang: None,
                    },
                    filename,
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: true,
                },
            );
            loaded_roots.insert(loaded_module_name, required_roots);
            loaded_any = true;
        }

        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if module.mod_name == "builtin" {
                collect_external_refs(&module.file.items, &stdlib_mod_names)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let (_, refs, _) =
                    reachable_stdlib_items(&module.file.items, roots, &stdlib_mod_names);
                refs
            } else {
                continue;
            };
            changed |= merge_required_refs(&mut required, refs);
        }

        if !loaded_any && !changed {
            break;
        }
    }
}

fn trace_stdlib_resolution(args: std::fmt::Arguments<'_>) {
    if std::env::var("GORS_STDLIB_TRACE").is_ok_and(|value| value == "1" || value == "true") {
        eprintln!("{args}");
    }
}

fn format_reachability_roots<'a>(roots: impl IntoIterator<Item = &'a String>) -> String {
    let mut roots: Vec<_> = roots.into_iter().map(String::as_str).collect();
    roots.sort_unstable();
    if roots.is_empty() {
        "<empty>".to_string()
    } else {
        roots.join(",")
    }
}

fn prune_dependency_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    _roots: &[String],
) {
    use std::collections::{HashMap, HashSet};

    let stdlib_mod_names: HashSet<String> = modules
        .values()
        .filter(|module| module.is_stdlib)
        .map(|module| module.mod_name.clone())
        .collect();
    if stdlib_mod_names.is_empty() {
        return;
    }

    let root_mod_names: HashSet<String> = std::iter::once("builtin".to_string()).collect();
    let mut preserved_mod_names: HashSet<String> = root_mod_names.iter().cloned().collect();
    for module in modules.values().filter(|module| !module.is_stdlib) {
        preserved_mod_names
            .extend(collect_external_refs(&module.file.items, &stdlib_mod_names).into_keys());
    }
    trace_stdlib_resolution(format_args!("[gors] preserve stdlib roots: {}", {
        let mut names: Vec<_> = preserved_mod_names.iter().cloned().collect();
        names.sort();
        names.join(",")
    }));
    trace_stdlib_resolution(format_args!("[gors] stdlib modules: {}", {
        let mut names: Vec<_> = stdlib_mod_names.iter().cloned().collect();
        names.sort();
        names.join(",")
    }));

    let mut required: HashMap<String, HashSet<String>> = HashMap::new();
    for module in modules.values().filter(|module| !module.is_stdlib) {
        merge_required_refs(
            &mut required,
            collect_external_refs(&module.file.items, &stdlib_mod_names),
        );
    }
    for (module, roots) in &required {
        trace_stdlib_resolution(format_args!(
            "[gors] prune stdlib {module} with roots {}",
            format_reachability_roots(roots.iter())
        ));
    }

    loop {
        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if root_mod_names.contains(&module.mod_name) {
                collect_external_refs(&module.file.items, &stdlib_mod_names)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let (_, refs, _) =
                    reachable_stdlib_items(&module.file.items, roots, &stdlib_mod_names);
                refs
            } else {
                continue;
            };
            changed |= merge_required_refs(&mut required, refs);
        }
        if !changed {
            break;
        }
    }

    let empty = HashSet::new();
    let removable: Vec<String> = modules
        .iter()
        .filter_map(|(key, module)| {
            if !module.is_stdlib || preserved_mod_names.contains(&module.mod_name) {
                return None;
            }
            if required
                .get(&module.mod_name)
                .is_some_and(|roots| !roots.is_empty())
            {
                None
            } else {
                Some(key.clone())
            }
        })
        .collect();
    for key in removable {
        modules.remove(&key);
    }

    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        if root_mod_names.contains(&module.mod_name) {
            continue;
        }
        let roots = required.get(&module.mod_name).unwrap_or(&empty);
        let (keep, _, names) = reachable_stdlib_items(&module.file.items, roots, &stdlib_mod_names);
        if keep.is_empty() {
            module.file.items.clear();
            module.content_hash = String::new();
            continue;
        }
        let item_names = item_reachability_names(&module.file.items);
        let top_level_names = top_level_item_names(&module.file.items);
        module.file.items = module
            .file
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                keep.contains(&idx)
                    .then(|| reachable_item_for_names(item, &names, &item_names, &top_level_names))
                    .flatten()
            })
            .collect();
        module.content_hash = String::new();
    }
    prune_unreferenced_stdlib_modules(modules, &preserved_mod_names);
}

fn prune_unreferenced_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved_mod_names: &std::collections::HashSet<String>,
) {
    use std::collections::HashSet;

    loop {
        let stdlib_mod_names: HashSet<String> = modules
            .values()
            .filter(|module| module.is_stdlib)
            .map(|module| module.mod_name.clone())
            .collect();
        let mut referenced = HashSet::new();
        for module in modules.values() {
            if module.mod_name == "builtin" {
                continue;
            }
            let refs = collect_external_refs(&module.file.items, &stdlib_mod_names);
            for module_name in refs.into_keys() {
                referenced.insert(module_name);
            }
        }
        referenced.insert("builtin".to_string());

        let removable: Vec<String> = modules
            .iter()
            .filter(|&(_key, module)| {
                module.is_stdlib
                    && !preserved_mod_names.contains(&module.mod_name)
                    && !referenced.contains(&module.mod_name)
            })
            .map(|(key, _module)| key.clone())
            .collect();
        if removable.is_empty() {
            break;
        }
        for key in removable {
            modules.remove(&key);
        }
    }
}

fn merge_required_refs(
    required: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
    refs: std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> bool {
    let mut changed = false;
    for (module, symbols) in refs {
        let entry = required.entry(module).or_default();
        for symbol in symbols {
            changed |= entry.insert(symbol);
        }
    }
    changed
}

fn reachable_stdlib_items(
    items: &[syn::Item],
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> (
    std::collections::HashSet<usize>,
    std::collections::HashMap<String, std::collections::HashSet<String>>,
    std::collections::HashSet<String>,
) {
    let cache_key = reachable_items_cache_key(items, roots, module_names);
    if let Ok(cache) = REACHABLE_ITEMS_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        && let Some(entry) = cache.get(&cache_key)
    {
        return (entry.keep.clone(), entry.refs.clone(), entry.names.clone());
    }

    let mut names = roots.clone();
    let mut keep = std::collections::HashSet::new();
    let mut external_refs = std::collections::HashMap::new();
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    let top_level_types = top_level_item_types(items, module_names);
    let top_level_field_types = top_level_item_field_types(items, module_names);
    let top_level_return_types = top_level_item_return_types(items, module_names);
    let top_level_tuple_return_types = top_level_item_tuple_return_types(items, module_names);

    loop {
        let mut changed = false;
        for (idx, item) in items.iter().enumerate() {
            let Some(mut reachable_item) =
                reachable_item_for_names(item, &names, &item_names, &top_level_names)
            else {
                continue;
            };
            changed |= keep.insert(idx);

            let context = RefCollectionContext {
                module_names,
                item_names: &item_names,
                top_level_names: &top_level_names,
                top_level_types: &top_level_types,
                top_level_field_types: &top_level_field_types,
                top_level_return_types: &top_level_return_types,
                top_level_tuple_return_types: &top_level_tuple_return_types,
            };
            let (local_names, refs) = collect_refs_from_item(&mut reachable_item, &context);
            for name in local_names {
                changed |= names.insert(name);
            }
            changed |= merge_required_refs(&mut external_refs, refs);
        }
        if !changed {
            break;
        }
    }

    let entry = ReachableItemsCacheEntry {
        keep,
        refs: external_refs,
        names,
    };
    if let Ok(mut cache) = REACHABLE_ITEMS_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
    {
        cache.insert(cache_key, entry.clone());
    }
    (entry.keep, entry.refs, entry.names)
}

fn reachable_items_cache_key(
    items: &[syn::Item],
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0reachable-items\0");
    let mut sorted_roots: Vec<_> = roots.iter().map(String::as_str).collect();
    sorted_roots.sort_unstable();
    for root in sorted_roots {
        hasher.update(root.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\x1e");
    let mut sorted_modules: Vec<_> = module_names.iter().map(String::as_str).collect();
    sorted_modules.sort_unstable();
    for module_name in sorted_modules {
        hasher.update(module_name.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(b"\x1e");
    for item in items {
        hasher.update(item.to_token_stream().to_string().as_bytes());
        hasher.update(b"\0");
    }
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn item_reachability_names(items: &[syn::Item]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for item in items {
        if let Some(name) = item_name(item) {
            names.insert(name);
        }
        if let syn::Item::Impl(item_impl) = item {
            let self_name = named_self_type(&item_impl.self_ty);
            for impl_item in &item_impl.items {
                match impl_item {
                    syn::ImplItem::Fn(func) => {
                        let name = func.sig.ident.to_string();
                        names.insert(name.clone());
                        if let Some(self_name) = &self_name {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    syn::ImplItem::Const(konst) => {
                        let name = konst.ident.to_string();
                        names.insert(name.clone());
                        if let Some(self_name) = &self_name {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    syn::ImplItem::Type(ty) => {
                        let name = ty.ident.to_string();
                        names.insert(name.clone());
                        if let Some(self_name) = &self_name {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let syn::Item::Trait(item_trait) = item {
            let trait_name = item_trait.ident.to_string();
            for trait_item in &item_trait.items {
                if let syn::TraitItem::Fn(func) = trait_item {
                    let name = func.sig.ident.to_string();
                    names.insert(name.clone());
                    names.insert(impl_method_reachability_name(&trait_name, &name));
                }
            }
        }
    }
    names
}

fn top_level_item_names(items: &[syn::Item]) -> std::collections::HashSet<String> {
    items.iter().filter_map(item_name).collect()
}

fn reachable_item_for_names(
    item: &syn::Item,
    names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
    top_level_names: &std::collections::HashSet<String>,
) -> Option<syn::Item> {
    if matches!(item, syn::Item::Use(_)) {
        return Some(item.clone());
    }

    if let syn::Item::Trait(item_trait) = item {
        let trait_name = item_trait.ident.to_string();
        if !names.contains(&trait_name) {
            return None;
        }
        if is_ambient_trait_name(&trait_name) {
            return Some(item.clone());
        }
        let mut filtered = item_trait.clone();
        filtered.items.retain(|trait_item| match trait_item {
            syn::TraitItem::Fn(func) => {
                let name = func.sig.ident.to_string();
                is_runtime_interface_hook(&name)
                    || trait_item_name_reachable(&trait_name, &name, names)
            }
            syn::TraitItem::Const(konst) => {
                trait_item_name_reachable(&trait_name, &konst.ident.to_string(), names)
            }
            syn::TraitItem::Type(ty) => {
                trait_item_name_reachable(&trait_name, &ty.ident.to_string(), names)
            }
            syn::TraitItem::Macro(item_macro) => item_macro
                .mac
                .path
                .segments
                .last()
                .is_some_and(|seg| names.contains(&seg.ident.to_string())),
            _ => false,
        });
        return Some(syn::Item::Trait(filtered));
    }

    if let syn::Item::Macro(item_macro) = item {
        let name = item_macro_name(item_macro);
        let token_names = macro_token_item_names(&item_macro.mac.tokens, item_names);
        return (name.as_ref().is_some_and(|name| names.contains(name))
            || token_names.iter().any(|name| names.contains(name)))
        .then(|| item.clone());
    }

    if item_name(item).is_some_and(|name| names.contains(&name)) {
        return Some(item.clone());
    }

    let syn::Item::Impl(item_impl) = item else {
        return None;
    };

    let trait_reachable = item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
        path.segments.last().is_some_and(|seg| {
            let name = seg.ident.to_string();
            names.contains(&name) && !is_ambient_trait_name(&name)
        }) || path_mentions_name(path, names)
    });
    let self_name = named_self_type(&item_impl.self_ty);
    let self_reachable = type_mentions_name(&item_impl.self_ty, names)
        || self_name.as_ref().is_some_and(|name| {
            names
                .iter()
                .any(|root| root.starts_with(&format!("{name}::")))
        });

    if trait_reachable {
        if let Some(self_name) = named_self_type(&item_impl.self_ty)
            && item_names.contains(&self_name)
            && !names.contains(&self_name)
        {
            return None;
        }
        let trait_name = item_impl
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .map(|seg| seg.ident.to_string());
        let mut filtered = item_impl.clone();
        if let Some(trait_name) = trait_name {
            if is_ambient_trait_name(&trait_name) {
                return Some(item.clone());
            }
            filtered.items.retain(|impl_item| match impl_item {
                syn::ImplItem::Fn(func) => {
                    let name = func.sig.ident.to_string();
                    is_runtime_interface_hook(&name)
                        || trait_item_name_reachable(&trait_name, &name, names)
                }
                syn::ImplItem::Const(konst) => {
                    trait_item_name_reachable(&trait_name, &konst.ident.to_string(), names)
                }
                syn::ImplItem::Type(ty) => {
                    trait_item_name_reachable(&trait_name, &ty.ident.to_string(), names)
                }
                syn::ImplItem::Macro(item_macro) => item_macro
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|seg| names.contains(&seg.ident.to_string())),
                _ => false,
            });
        }
        return Some(syn::Item::Impl(filtered));
    }
    if !self_reachable {
        return None;
    }
    if item_impl.trait_.is_some() {
        if let Some((_, path, _)) = &item_impl.trait_
            && let Some(trait_name) = path.segments.last().map(|seg| seg.ident.to_string())
            && top_level_names.contains(&trait_name)
            && !names.contains(&trait_name)
        {
            return None;
        }
        return Some(item.clone());
    }

    let mut filtered = item_impl.clone();
    filtered.items.retain(|impl_item| match impl_item {
        syn::ImplItem::Fn(func) => {
            impl_item_name_reachable(&self_name, &func.sig.ident.to_string(), names)
        }
        syn::ImplItem::Const(konst) => {
            impl_item_name_reachable(&self_name, &konst.ident.to_string(), names)
        }
        syn::ImplItem::Type(ty) => {
            impl_item_name_reachable(&self_name, &ty.ident.to_string(), names)
        }
        syn::ImplItem::Macro(item_macro) => item_macro
            .mac
            .path
            .segments
            .last()
            .is_some_and(|seg| names.contains(&seg.ident.to_string())),
        _ => false,
    });

    (!filtered.items.is_empty()).then(|| syn::Item::Impl(filtered))
}

fn impl_item_name_reachable(
    self_name: &Option<String>,
    item_name: &str,
    names: &std::collections::HashSet<String>,
) -> bool {
    self_name.as_ref().map_or_else(
        || names.contains(item_name),
        |self_name| names.contains(&impl_method_reachability_name(self_name, item_name)),
    )
}

fn trait_item_name_reachable(
    trait_name: &str,
    item_name: &str,
    names: &std::collections::HashSet<String>,
) -> bool {
    names.contains(item_name)
        || names.contains(&impl_method_reachability_name(trait_name, item_name))
}

fn impl_method_reachability_name(self_name: &str, method_name: &str) -> String {
    format!("{self_name}::{method_name}")
}

fn named_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path.path.segments.last().map(|seg| seg.ident.to_string()),
        syn::Type::Reference(reference) => named_self_type(&reference.elem),
        _ => None,
    }
}

fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item) => Some(item.ident.to_string()),
        syn::Item::Enum(item) => Some(item.ident.to_string()),
        syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
        syn::Item::Static(item) => Some(item.ident.to_string()),
        syn::Item::Struct(item) => Some(item.ident.to_string()),
        syn::Item::Trait(item) => Some(item.ident.to_string()),
        syn::Item::Type(item) => Some(item.ident.to_string()),
        syn::Item::Union(item) => Some(item.ident.to_string()),
        syn::Item::Macro(item) => item_macro_name(item),
        _ => None,
    }
}

fn item_macro_name(item: &syn::ItemMacro) -> Option<String> {
    item.ident
        .as_ref()
        .map(std::string::ToString::to_string)
        .or_else(|| {
            item.mac
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
        })
}

fn macro_token_item_names(
    tokens: &proc_macro2::TokenStream,
    item_names: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    fn collect(
        tokens: proc_macro2::TokenStream,
        item_names: &std::collections::HashSet<String>,
        names: &mut std::collections::HashSet<String>,
    ) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if item_names.contains(&name) && is_reachability_name(&name) {
                        names.insert(name);
                    }
                }
                proc_macro2::TokenTree::Group(group) => {
                    collect(group.stream(), item_names, names);
                }
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }

    let mut names = std::collections::HashSet::new();
    collect(tokens.clone(), item_names, &mut names);
    names
}

fn type_mentions_name(ty: &syn::Type, names: &std::collections::HashSet<String>) -> bool {
    match ty {
        syn::Type::Array(array) => type_mentions_name(&array.elem, names),
        syn::Type::Group(group) => type_mentions_name(&group.elem, names),
        syn::Type::Paren(paren) => type_mentions_name(&paren.elem, names),
        syn::Type::Path(path) => path_mentions_name(&path.path, names),
        syn::Type::Reference(reference) => type_mentions_name(&reference.elem, names),
        syn::Type::Ptr(ptr) => type_mentions_name(&ptr.elem, names),
        syn::Type::Slice(slice) => type_mentions_name(&slice.elem, names),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(|ty| type_mentions_name(ty, names)),
        _ => false,
    }
}

fn path_mentions_name(path: &syn::Path, names: &std::collections::HashSet<String>) -> bool {
    path.segments.iter().any(|seg| {
        names.contains(&seg.ident.to_string())
            || match &seg.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    args.args.iter().any(|arg| match arg {
                        syn::GenericArgument::Type(ty) => type_mentions_name(ty, names),
                        syn::GenericArgument::AssocType(assoc) => {
                            type_mentions_name(&assoc.ty, names)
                        }
                        syn::GenericArgument::Constraint(constraint) => {
                            constraint.bounds.iter().any(|bound| match bound {
                                syn::TypeParamBound::Trait(trait_bound) => {
                                    path_mentions_name(&trait_bound.path, names)
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    })
                }
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(|ty| type_mentions_name(ty, names))
                        || matches!(&args.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
                }
                syn::PathArguments::None => false,
            }
    })
}

fn collect_external_refs(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
    let mut external_refs = std::collections::HashMap::new();
    let empty_types = std::collections::HashMap::new();
    let empty_field_types = std::collections::HashMap::new();
    let empty_return_types = std::collections::HashMap::new();
    let empty_tuple_return_types = std::collections::HashMap::new();
    for item in items {
        let mut item_clone = item.clone();
        let empty_item_names = std::collections::HashSet::new();
        let empty_top_level_names = std::collections::HashSet::new();
        let context = RefCollectionContext {
            module_names,
            item_names: &empty_item_names,
            top_level_names: &empty_top_level_names,
            top_level_types: &empty_types,
            top_level_field_types: &empty_field_types,
            top_level_return_types: &empty_return_types,
            top_level_tuple_return_types: &empty_tuple_return_types,
        };
        let (_, refs) = collect_refs_from_item(&mut item_clone, &context);
        merge_required_refs(&mut external_refs, refs);
    }
    external_refs
}

#[derive(Clone)]
struct ReceiverTypeRef {
    module: Option<String>,
    name: String,
}

type ReachabilityNameSet = std::collections::HashSet<String>;
type ReceiverTypeMap = std::collections::HashMap<String, ReceiverTypeRef>;
type ReceiverFieldTypeMap = std::collections::HashMap<String, ReceiverTypeMap>;
type ReceiverTupleTypes = Vec<Option<ReceiverTypeRef>>;
type ReceiverTupleReturnMap = std::collections::HashMap<String, ReceiverTupleTypes>;

struct RefCollectionContext<'a> {
    module_names: &'a ReachabilityNameSet,
    item_names: &'a ReachabilityNameSet,
    top_level_names: &'a ReachabilityNameSet,
    top_level_types: &'a ReceiverTypeMap,
    top_level_field_types: &'a ReceiverFieldTypeMap,
    top_level_return_types: &'a ReceiverTypeMap,
    top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
}

fn top_level_item_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTypeRef> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        match item {
            syn::Item::Const(item_const) => {
                if let Some(ty) = receiver_type_from_type(&item_const.ty, module_names) {
                    types.insert(item_const.ident.to_string(), ty);
                }
            }
            syn::Item::Static(item_static) => {
                if let Some(ty) = receiver_type_from_type(&item_static.ty, module_names) {
                    types.insert(item_static.ident.to_string(), ty);
                }
            }
            _ => {}
        }
    }
    types
}

fn top_level_item_field_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, std::collections::HashMap<String, ReceiverTypeRef>> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let mut fields = std::collections::HashMap::new();
        if let syn::Fields::Named(named_fields) = &item_struct.fields {
            for field in &named_fields.named {
                let Some(field_ident) = &field.ident else {
                    continue;
                };
                if let Some(ty) = receiver_type_from_type(&field.ty, module_names) {
                    fields.insert(field_ident.to_string(), ty);
                }
            }
        }
        if !fields.is_empty() {
            types.insert(item_struct.ident.to_string(), fields);
        }
    }
    types
}

fn top_level_item_return_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTypeRef> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        let syn::Item::Fn(item_fn) = item else {
            continue;
        };
        let syn::ReturnType::Type(_, ty) = &item_fn.sig.output else {
            continue;
        };
        if let Some(return_type) = receiver_type_from_type(ty, module_names) {
            types.insert(item_fn.sig.ident.to_string(), return_type);
        }
    }
    types
}

fn top_level_item_tuple_return_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTupleTypes> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        let syn::Item::Fn(item_fn) = item else {
            continue;
        };
        let syn::ReturnType::Type(_, ty) = &item_fn.sig.output else {
            continue;
        };
        let syn::Type::Tuple(tuple) = ty.as_ref() else {
            continue;
        };
        let tuple_types = tuple
            .elems
            .iter()
            .map(|ty| receiver_type_from_type(ty, module_names))
            .collect::<Vec<_>>();
        if tuple_types.iter().any(Option::is_some) {
            types.insert(item_fn.sig.ident.to_string(), tuple_types);
        }
    }
    types
}

fn receiver_type_from_type(
    ty: &syn::Type,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    match ty {
        syn::Type::Group(group) => receiver_type_from_type(&group.elem, module_names),
        syn::Type::ImplTrait(impl_trait) => {
            impl_trait.bounds.iter().find_map(|bound| match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    receiver_type_from_path(&trait_bound.path, module_names)
                }
                _ => None,
            })
        }
        syn::Type::Paren(paren) => receiver_type_from_type(&paren.elem, module_names),
        syn::Type::Path(path) => receiver_type_from_path(&path.path, module_names),
        syn::Type::Reference(reference) => receiver_type_from_type(&reference.elem, module_names),
        syn::Type::Ptr(ptr) => receiver_type_from_type(&ptr.elem, module_names),
        syn::Type::TraitObject(trait_object) => trait_object.bounds.iter().find_map(|bound| {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                receiver_type_from_path(&trait_bound.path, module_names)
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn receiver_type_from_init_expr(
    expr: &syn::Expr,
    module_names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
    top_level_return_types: &std::collections::HashMap<String, ReceiverTypeRef>,
) -> Option<ReceiverTypeRef> {
    match expr {
        syn::Expr::Call(call) => {
            if is_path_call_expr(&call.func, &["Box", "new"]) {
                return call.args.first().and_then(|arg| {
                    receiver_type_from_init_expr(
                        arg,
                        module_names,
                        item_names,
                        top_level_return_types,
                    )
                });
            }
            if is_path_call_expr(&call.func, &["std", "sync", "Arc", "new"])
                || is_path_call_expr(&call.func, &["std", "sync", "Mutex", "new"])
            {
                return call.args.first().and_then(|arg| {
                    receiver_type_from_init_expr(
                        arg,
                        module_names,
                        item_names,
                        top_level_return_types,
                    )
                });
            }
            if let syn::Expr::Path(path) = &*call.func
                && let Some(first) = path.path.segments.first()
            {
                if let Some(qself) = &path.qself
                    && let Some(receiver_type) = receiver_type_from_type(&qself.ty, module_names)
                {
                    return Some(receiver_type);
                }
                if let Some(receiver_type) =
                    receiver_type_from_associated_call_path(&path.path, module_names, item_names)
                {
                    return Some(receiver_type);
                }
                let name = first.ident.to_string();
                if let Some(return_type) = top_level_return_types.get(&name) {
                    return Some(return_type.clone());
                }
                if item_names.contains(&name) {
                    return Some(ReceiverTypeRef { module: None, name });
                }
            }
            receiver_type_from_init_expr(
                &call.func,
                module_names,
                item_names,
                top_level_return_types,
            )
        }
        syn::Expr::Cast(cast) => receiver_type_from_init_expr(
            &cast.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Group(group) => receiver_type_from_init_expr(
            &group.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Paren(paren) => receiver_type_from_init_expr(
            &paren.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Reference(reference) => receiver_type_from_init_expr(
            &reference.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Struct(expr_struct) => expr_struct
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string())
            .filter(|name| item_names.contains(name))
            .map(|name| ReceiverTypeRef { module: None, name }),
        syn::Expr::MethodCall(method)
            if matches!(
                method.method.to_string().as_str(),
                "clone" | "lock" | "unwrap"
            ) =>
        {
            receiver_type_from_init_expr(
                &method.receiver,
                module_names,
                item_names,
                top_level_return_types,
            )
        }
        syn::Expr::Unary(unary) => receiver_type_from_init_expr(
            &unary.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        _ => None,
    }
}

fn receiver_type_from_associated_call_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let segments = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [krate, module, name, ..] if krate == "crate" && module_names.contains(module) => {
            Some(ReceiverTypeRef {
                module: Some(module.clone()),
                name: name.clone(),
            })
        }
        [module, name, ..] if module_names.contains(module) => Some(ReceiverTypeRef {
            module: Some(module.clone()),
            name: name.clone(),
        }),
        [name, ..] if item_names.contains(name) && segments.len() > 1 => Some(ReceiverTypeRef {
            module: None,
            name: name.clone(),
        }),
        _ => None,
    }
}

fn receiver_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    if let Some(receiver_type) = transparent_receiver_type_from_path(path, module_names) {
        return Some(receiver_type);
    }

    let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
    let first = segments.next();
    let second = segments.next();
    let third = segments.next();
    match (first.as_deref(), second.as_deref(), third.as_deref()) {
        (Some("crate"), Some(module), Some(name)) if module_names.contains(module) => {
            return Some(ReceiverTypeRef {
                module: Some(module.to_string()),
                name: name.to_string(),
            });
        }
        (Some(module), Some(name), _) if module_names.contains(module) => {
            return Some(ReceiverTypeRef {
                module: Some(module.to_string()),
                name: name.to_string(),
            });
        }
        (Some(name), None, None) => {
            return Some(ReceiverTypeRef {
                module: None,
                name: name.to_string(),
            });
        }
        _ => {}
    }

    path.segments.iter().find_map(|seg| match &seg.arguments {
        syn::PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => receiver_type_from_type(ty, module_names),
            syn::GenericArgument::AssocType(assoc) => {
                receiver_type_from_type(&assoc.ty, module_names)
            }
            syn::GenericArgument::Constraint(constraint) => {
                constraint.bounds.iter().find_map(|bound| match bound {
                    syn::TypeParamBound::Trait(trait_bound) => {
                        receiver_type_from_path(&trait_bound.path, module_names)
                    }
                    _ => None,
                })
            }
            _ => None,
        }),
        syn::PathArguments::Parenthesized(args) => args
            .inputs
            .iter()
            .find_map(|ty| receiver_type_from_type(ty, module_names))
            .or_else(|| match &args.output {
                syn::ReturnType::Type(_, ty) => receiver_type_from_type(ty, module_names),
                syn::ReturnType::Default => None,
            }),
        syn::PathArguments::None => None,
    })
}

fn transparent_receiver_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let last = path.segments.last()?;
    if last.ident != "Box" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => receiver_type_from_type(ty, module_names),
        _ => None,
    })
}

fn collect_refs_from_item(
    item: &mut syn::Item,
    context: &RefCollectionContext<'_>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashMap<String, std::collections::HashSet<String>>,
) {
    use syn::visit_mut::VisitMut;

    struct BoundCollector<'a> {
        names: std::collections::HashSet<String>,
        types: std::collections::HashMap<String, ReceiverTypeRef>,
        module_names: &'a ReachabilityNameSet,
        item_names: &'a ReachabilityNameSet,
        top_level_return_types: &'a ReceiverTypeMap,
        top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
    }

    impl BoundCollector<'_> {
        fn bound_receiver_type_from_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
            match expr {
                syn::Expr::Cast(cast) => self.bound_receiver_type_from_expr(&cast.expr),
                syn::Expr::Group(group) => self.bound_receiver_type_from_expr(&group.expr),
                syn::Expr::MethodCall(method)
                    if matches!(
                        method.method.to_string().as_str(),
                        "clone" | "lock" | "unwrap"
                    ) =>
                {
                    receiver_type_from_init_expr(
                        &method.receiver,
                        self.module_names,
                        self.item_names,
                        self.top_level_return_types,
                    )
                    .or_else(|| self.bound_receiver_type_from_expr(&method.receiver))
                }
                syn::Expr::Paren(paren) => self.bound_receiver_type_from_expr(&paren.expr),
                syn::Expr::Path(path)
                    if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
                {
                    let name = path.path.segments.first()?.ident.to_string();
                    self.types.get(&name).cloned()
                }
                syn::Expr::Reference(reference) => {
                    self.bound_receiver_type_from_expr(&reference.expr)
                }
                syn::Expr::Unary(unary) => self.bound_receiver_type_from_expr(&unary.expr),
                _ => None,
            }
        }
    }

    impl VisitMut for BoundCollector<'_> {
        fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
            self.names.insert(pat.ident.to_string());
            syn::visit_mut::visit_pat_ident_mut(self, pat);
        }

        fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
            if let syn::FnArg::Typed(pat_type) = arg
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_fn_arg_mut(self, arg);
        }

        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            if let Some(init) = &local.init
                && let syn::Pat::Tuple(tuple_pat) = &local.pat
                && let Some(tuple_types) = receiver_tuple_types_from_init_expr(
                    &init.expr,
                    self.top_level_tuple_return_types,
                )
            {
                for (pat, receiver_type) in tuple_pat.elems.iter().zip(tuple_types) {
                    if let Some(name) = pat_ident_name(pat)
                        && let Some(receiver_type) = receiver_type
                    {
                        self.types.insert(name, receiver_type);
                    }
                }
            }

            if let syn::Pat::Type(pat_type) = &local.pat
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                self.types.insert(name, ty);
            } else if let Some(init) = &local.init
                && let Some(name) = pat_ident_name(&local.pat)
                && let Some(ty) = receiver_type_from_init_expr(
                    &init.expr,
                    self.module_names,
                    self.item_names,
                    self.top_level_return_types,
                )
                .or_else(|| self.bound_receiver_type_from_expr(&init.expr))
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_local_mut(self, local);
        }
    }

    fn receiver_tuple_types_from_init_expr(
        expr: &syn::Expr,
        top_level_tuple_return_types: &std::collections::HashMap<String, ReceiverTupleTypes>,
    ) -> Option<ReceiverTupleTypes> {
        match expr {
            syn::Expr::Call(call) => {
                if let syn::Expr::Path(path) = &*call.func
                    && let Some(first) = path.path.segments.first()
                    && let Some(types) = top_level_tuple_return_types.get(&first.ident.to_string())
                {
                    return Some(types.clone());
                }
                receiver_tuple_types_from_init_expr(&call.func, top_level_tuple_return_types)
            }
            syn::Expr::Cast(cast) => {
                receiver_tuple_types_from_init_expr(&cast.expr, top_level_tuple_return_types)
            }
            syn::Expr::Group(group) => {
                receiver_tuple_types_from_init_expr(&group.expr, top_level_tuple_return_types)
            }
            syn::Expr::Paren(paren) => {
                receiver_tuple_types_from_init_expr(&paren.expr, top_level_tuple_return_types)
            }
            syn::Expr::Reference(reference) => {
                receiver_tuple_types_from_init_expr(&reference.expr, top_level_tuple_return_types)
            }
            syn::Expr::Unary(unary) => {
                receiver_tuple_types_from_init_expr(&unary.expr, top_level_tuple_return_types)
            }
            _ => None,
        }
    }

    fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
        match pat {
            syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.to_string()),
            syn::Pat::Type(pat_type) => pat_ident_name(&pat_type.pat),
            _ => None,
        }
    }

    fn external_module_from_expr(
        expr: &syn::Expr,
        module_names: &std::collections::HashSet<String>,
    ) -> Option<String> {
        match expr {
            syn::Expr::Call(call) => external_module_from_expr(&call.func, module_names),
            syn::Expr::Cast(cast) => external_module_from_expr(&cast.expr, module_names),
            syn::Expr::Field(field) => external_module_from_expr(&field.base, module_names),
            syn::Expr::Group(group) => external_module_from_expr(&group.expr, module_names),
            syn::Expr::Index(index) => external_module_from_expr(&index.expr, module_names),
            syn::Expr::MethodCall(method) => {
                external_module_from_expr(&method.receiver, module_names)
            }
            syn::Expr::Paren(paren) => external_module_from_expr(&paren.expr, module_names),
            syn::Expr::Path(path) => {
                let mut segments = path.path.segments.iter().map(|seg| seg.ident.to_string());
                let first = segments.next();
                let second = segments.next();
                match (first.as_deref(), second.as_deref()) {
                    (Some(module), None) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    (Some("crate"), Some(module)) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    (Some(module), Some(_)) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    _ => None,
                }
            }
            syn::Expr::Reference(reference) => {
                external_module_from_expr(&reference.expr, module_names)
            }
            syn::Expr::Try(try_expr) => external_module_from_expr(&try_expr.expr, module_names),
            syn::Expr::Unary(unary) => external_module_from_expr(&unary.expr, module_names),
            _ => None,
        }
    }

    fn external_path_symbol_from_expr(
        expr: &syn::Expr,
        module_names: &std::collections::HashSet<String>,
    ) -> Option<(String, String)> {
        match expr {
            syn::Expr::Field(field) => {
                let syn::Member::Named(member) = &field.member else {
                    return None;
                };
                external_module_from_expr(&field.base, module_names)
                    .map(|module| (module, member.to_string()))
            }
            syn::Expr::Path(path) => {
                let mut segments = path.path.segments.iter().map(|seg| seg.ident.to_string());
                match (
                    segments.next().as_deref(),
                    segments.next().as_deref(),
                    segments.next().as_deref(),
                ) {
                    (Some("crate"), Some(module), Some(symbol))
                        if module_names.contains(module) =>
                    {
                        Some((module.to_string(), symbol.to_string()))
                    }
                    (Some(module), Some(symbol), _) if module_names.contains(module) => {
                        Some((module.to_string(), symbol.to_string()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    struct RefCollector<'a> {
        module_names: &'a std::collections::HashSet<String>,
        item_names: &'a std::collections::HashSet<String>,
        top_level_names: &'a std::collections::HashSet<String>,
        top_level_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
        top_level_field_types: &'a std::collections::HashMap<
            String,
            std::collections::HashMap<String, ReceiverTypeRef>,
        >,
        top_level_return_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
        bound_names: std::collections::HashSet<String>,
        bound_types: std::collections::HashMap<String, ReceiverTypeRef>,
        current_self_type: Option<ReceiverTypeRef>,
        local_names: std::collections::HashSet<String>,
        external_refs: std::collections::HashMap<String, std::collections::HashSet<String>>,
    }

    impl RefCollector<'_> {
        fn receiver_type_from_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
            match expr {
                syn::Expr::Group(group) => self.receiver_type_from_expr(&group.expr),
                syn::Expr::Paren(paren) => self.receiver_type_from_expr(&paren.expr),
                syn::Expr::Path(path)
                    if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
                {
                    let name = path.path.segments.first()?.ident.to_string();
                    if name == "self" {
                        return self.current_self_type.clone();
                    }
                    self.bound_types
                        .get(&name)
                        .cloned()
                        .or_else(|| self.top_level_types.get(&name).cloned())
                }
                syn::Expr::Call(call) => receiver_type_from_init_expr(
                    expr,
                    self.module_names,
                    self.item_names,
                    self.top_level_return_types,
                )
                .or_else(|| {
                    if is_path_call_expr(&call.func, &["std", "mem", "take"]) {
                        call.args
                            .first()
                            .and_then(|arg| self.receiver_type_from_expr(arg))
                    } else {
                        None
                    }
                })
                .or_else(|| self.receiver_type_from_expr(&call.func)),
                syn::Expr::MethodCall(method)
                    if matches!(
                        method.method.to_string().as_str(),
                        "clone" | "lock" | "unwrap"
                    ) =>
                {
                    self.receiver_type_from_expr(&method.receiver)
                }
                syn::Expr::Cast(cast) => self.receiver_type_from_expr(&cast.expr),
                syn::Expr::Field(field) => {
                    let base_type = self.receiver_type_from_expr(&field.base)?;
                    let syn::Member::Named(member) = &field.member else {
                        return None;
                    };
                    self.top_level_field_types
                        .get(&base_type.name)
                        .and_then(|fields| fields.get(&member.to_string()))
                        .cloned()
                }
                syn::Expr::Reference(reference) => self.receiver_type_from_expr(&reference.expr),
                syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                    self.receiver_type_from_expr(&unary.expr)
                }
                _ => None,
            }
        }

        fn insert_receiver_method_ref(&mut self, receiver_type: ReceiverTypeRef, method: &str) {
            if let Some(module) = receiver_type.module {
                let entry = self.external_refs.entry(module).or_default();
                entry.insert(receiver_type.name.clone());
                entry.insert(impl_method_reachability_name(&receiver_type.name, method));
            } else {
                if is_reachability_name(&receiver_type.name) {
                    self.local_names.insert(receiver_type.name.clone());
                }
                self.local_names
                    .insert(impl_method_reachability_name(&receiver_type.name, method));
            }
        }
    }

    impl VisitMut for RefCollector<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);

            let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
            let first = segments.next();
            let second = segments.next();
            let third = segments.next();
            let fourth = segments.next();

            match (
                first.as_deref(),
                second.as_deref(),
                third.as_deref(),
                fourth.as_deref(),
            ) {
                (Some("crate"), Some(module), Some(symbol), assoc)
                    if self.module_names.contains(module) =>
                {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(assoc.to_string());
                    }
                }
                (Some(module), Some(symbol), assoc, _) if self.module_names.contains(module) => {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(assoc.to_string());
                    }
                }
                (Some(local), Some(symbol), assoc, _) if self.item_names.contains(local) => {
                    if is_reachability_name(local) {
                        self.local_names.insert(local.to_string());
                    }
                    self.local_names.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        self.local_names.insert(assoc.to_string());
                    }
                }
                _ => {}
            }
        }

        fn visit_expr_path_mut(&mut self, expr_path: &mut syn::ExprPath) {
            syn::visit_mut::visit_expr_path_mut(self, expr_path);
            if expr_path.path.leading_colon.is_some() || expr_path.path.segments.len() != 1 {
                return;
            }
            let Some(name) = expr_path
                .path
                .segments
                .first()
                .map(|seg| seg.ident.to_string())
            else {
                return;
            };
            if self.item_names.contains(&name)
                && !self.bound_names.contains(&name)
                && is_reachability_name(&name)
            {
                self.local_names.insert(name);
            }
        }

        fn visit_type_path_mut(&mut self, type_path: &mut syn::TypePath) {
            syn::visit_mut::visit_type_path_mut(self, type_path);
            let Some(last) = type_path.path.segments.last() else {
                return;
            };
            let name = last.ident.to_string();
            if self.item_names.contains(&name) && is_reachability_name(&name) {
                self.local_names.insert(name);
            }
        }

        fn visit_expr_struct_mut(&mut self, expr_struct: &mut syn::ExprStruct) {
            let Some(last) = expr_struct.path.segments.last() else {
                syn::visit_mut::visit_expr_struct_mut(self, expr_struct);
                return;
            };
            let name = last.ident.to_string();
            if self.item_names.contains(&name) && is_reachability_name(&name) {
                self.local_names.insert(name);
            }
            syn::visit_mut::visit_expr_struct_mut(self, expr_struct);
        }

        fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
            if let Some((_, path, _)) = &item_impl.trait_
                && let Some(last) = path.segments.last()
            {
                let name = last.ident.to_string();
                if self.item_names.contains(&name) && is_reachability_name(&name) {
                    self.local_names.insert(name);
                }
            }
            let previous_self_type = self.current_self_type.clone();
            self.current_self_type = named_self_type(&item_impl.self_ty)
                .map(|name| ReceiverTypeRef { module: None, name });
            syn::visit_mut::visit_item_impl_mut(self, item_impl);
            self.current_self_type = previous_self_type;
        }

        fn visit_type_param_bound_mut(&mut self, bound: &mut syn::TypeParamBound) {
            if let syn::TypeParamBound::Trait(trait_bound) = bound
                && let Some(last) = trait_bound.path.segments.last()
            {
                let name = last.ident.to_string();
                if self.item_names.contains(&name) && is_reachability_name(&name) {
                    self.local_names.insert(name);
                }
            }
            syn::visit_mut::visit_type_param_bound_mut(self, bound);
        }

        fn visit_item_macro_mut(&mut self, item_macro: &mut syn::ItemMacro) {
            if let Some(name) = item_macro_name(item_macro)
                && self.item_names.contains(&name)
                && is_reachability_name(&name)
            {
                self.local_names.insert(name);
            }
            self.local_names.extend(macro_token_item_names(
                &item_macro.mac.tokens,
                self.item_names,
            ));
            syn::visit_mut::visit_item_macro_mut(self, item_macro);
        }

        fn visit_expr_method_call_mut(&mut self, method: &mut syn::ExprMethodCall) {
            let name = method.method.to_string();
            if let Some(receiver_type) = self.receiver_type_from_expr(&method.receiver) {
                self.insert_receiver_method_ref(receiver_type, &name);
            } else if let Some(module) =
                external_module_from_expr(&method.receiver, self.module_names)
            {
                let entry = self.external_refs.entry(module).or_default();
                entry.insert(name);
                if let Some((_, symbol)) =
                    external_path_symbol_from_expr(&method.receiver, self.module_names)
                {
                    entry.insert(symbol);
                }
            } else if !self.top_level_names.contains(&name) {
                self.local_names.insert(name);
            }
            syn::visit_mut::visit_expr_method_call_mut(self, method);
        }

        fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
            if let syn::Expr::Path(path) = &*call.func
                && let Some(receiver_type) = receiver_type_from_associated_call_path(
                    &path.path,
                    self.module_names,
                    self.item_names,
                )
                && let Some(method) = path.path.segments.last()
            {
                self.insert_receiver_method_ref(receiver_type, &method.ident.to_string());
            }
            syn::visit_mut::visit_expr_call_mut(self, call);
        }
    }

    let mut bound_collector = BoundCollector {
        names: std::collections::HashSet::new(),
        types: std::collections::HashMap::new(),
        module_names: context.module_names,
        item_names: context.item_names,
        top_level_return_types: context.top_level_return_types,
        top_level_tuple_return_types: context.top_level_tuple_return_types,
    };
    let mut item_for_bounds = item.clone();
    bound_collector.visit_item_mut(&mut item_for_bounds);

    let mut collector = RefCollector {
        module_names: context.module_names,
        item_names: context.item_names,
        top_level_names: context.top_level_names,
        top_level_types: context.top_level_types,
        top_level_field_types: context.top_level_field_types,
        top_level_return_types: context.top_level_return_types,
        bound_names: bound_collector.names,
        bound_types: bound_collector.types,
        current_self_type: None,
        local_names: std::collections::HashSet::new(),
        external_refs: std::collections::HashMap::new(),
    };
    collector.visit_item_mut(item);
    (collector.local_names, collector.external_refs)
}

fn is_reachability_name(name: &str) -> bool {
    !matches!(
        name,
        "AsMut"
            | "AsRef"
            | "Box"
            | "Clone"
            | "Copy"
            | "Debug"
            | "Default"
            | "Deref"
            | "DerefMut"
            | "Display"
            | "Err"
            | "Error"
            | "From"
            | "Into"
            | "None"
            | "Ok"
            | "Option"
            | "Result"
            | "Self"
            | "Some"
            | "String"
            | "ToString"
            | "Vec"
            | "bool"
            | "char"
            | "clone"
            | "collect"
            | "default"
            | "extend"
            | "false"
            | "is_empty"
            | "iter"
            | "len"
            | "new"
            | "push"
            | "std"
            | "to_string"
            | "true"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
    )
}

fn is_ambient_trait_name(name: &str) -> bool {
    matches!(
        name,
        "AsMut"
            | "AsRef"
            | "Clone"
            | "Copy"
            | "Debug"
            | "Default"
            | "Deref"
            | "DerefMut"
            | "Display"
            | "From"
            | "Append"
            | "BitcastFrom"
            | "ByteSeq"
            | "Cap"
            | "Clear"
            | "Complex64Value"
            | "Complex128Value"
            | "Imag"
            | "Len"
            | "Real"
            | "StringValue"
            | "__GorsReflectKindValue"
            | "comparable"
            | "Into"
            | "ToString"
    )
}

fn is_runtime_interface_hook(name: &str) -> bool {
    name == "__gors_as_any"
}

/// Compile a Go AST into a Rust `syn` AST with source mapping.
///
/// This is like [`compile`], but also enables source map tracking.
/// Call [`get_source_map_tracker`] after compilation to access the tracker,
/// then use it during code generation to build the final source map.
///
/// # Arguments
///
/// * `file` - The Go AST to compile
/// * `go_file` - Path to the Go source file
/// * `go_source` - The Go source code content
///
/// # Returns
///
/// Returns `Ok(syn::File)` on success, or `Err(CompilerError)` if the
/// Go code contains constructs that cannot be translated to Rust.
///
/// # Example
///
/// ```
/// use gors::{parser, compiler};
///
/// let go_source = "package main\n\nfunc main() { x := 42; _ = x }";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let rust_ast = compiler::compile_with_source_map(go_ast, "example.go", go_source).unwrap();
/// ```
pub fn compile_with_source_map(
    file: ast::File,
    go_file: &str,
    go_source: &str,
) -> Result<syn::File, CompilerError> {
    // Start tracking
    TRACKER.with(|t| {
        t.borrow_mut().start(go_file, "output.rs", Some(go_source));
    });

    DEFER_COUNTER.with(|c| *c.borrow_mut() = 0);
    SWITCH_COUNTER.with(|c| *c.borrow_mut() = 0);
    SELECT_COUNTER.with(|c| *c.borrow_mut() = 0);
    LOOP_BODY_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_COUNTER.with(|c| *c.borrow_mut() = 0);
    NAMED_RETURN_COUNTER.with(|c| *c.borrow_mut() = 0);
    GOTO_STATE_CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    set_import_renames(BTreeMap::new());
    set_import_package_names(BTreeMap::new());
    let mut type_env = typeinfer::TypeEnv::new();
    type_env.scan_file(&file);
    let import_package_names = file_import_package_names(&file);
    validate_file_with_type_env_and_import_package_names(&file, &type_env, &import_package_names)?;
    validate_unused_imports(&file, &import_package_names)?;
    let _ir = ir::lower_file(&file, &type_env);
    set_import_package_names(import_package_names);
    set_type_env(type_env);
    set_borrow_pointer_arg_indices_for_decls_if_unseeded(&file.decls);
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

/// Build the source map from the tracker given the generated Rust source.
/// This should be called after code generation.
pub fn build_source_map(rust_source: &str) -> sourcemap::SourceMap {
    TRACKER.with(|t| {
        let tracker = t.borrow();
        tracker.build_source_map(rust_source)
    })
}

/// Clear the source map tracker.
pub fn clear_source_map_tracker() {
    TRACKER.with(|t| {
        t.borrow_mut().clear();
    });
}

fn is_byte_slice_type(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ArrayType(array_type) if array_type.len.is_none() => {
            matches!(&*array_type.elt, ast::Expr::Ident(id) if id.name == "byte" || id.name == "uint8")
        }
        _ => false,
    }
}

fn contains_array_type(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ArrayType(array_type) if array_type.len.is_some() => true,
        ast::Expr::ArrayType(array_type) => contains_array_type(&array_type.elt),
        ast::Expr::StarExpr(star) => contains_array_type(&star.x),
        ast::Expr::MapType(map) => contains_array_type(&map.key) || contains_array_type(&map.value),
        _ => false,
    }
}

fn contains_func_type(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::FuncType(_) => true,
        ast::Expr::ArrayType(array_type) => contains_func_type(&array_type.elt),
        ast::Expr::StarExpr(star) => contains_func_type(&star.x),
        ast::Expr::MapType(map) => contains_func_type(&map.key) || contains_func_type(&map.value),
        _ => false,
    }
}

fn contains_any_type(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(id) if id.name == "any" => true,
        ast::Expr::InterfaceType(_) => true,
        ast::Expr::ArrayType(array_type) => contains_any_type(&array_type.elt),
        ast::Expr::StarExpr(star) => contains_any_type(&star.x),
        ast::Expr::MapType(map) => contains_any_type(&map.key) || contains_any_type(&map.value),
        _ => false,
    }
}

fn array_len_expr(expr: &ast::Expr) -> syn::Expr {
    let values = BTreeMap::new();
    if let Some(value) =
        const_eval_expr_in_active_env(expr, 0, &values).and_then(|value| value.as_u128())
    {
        let lit = syn::LitInt::new(&value.to_string(), Span::mixed_site());
        return syn::parse_quote! { #lit };
    }
    if let Some(expr) = array_len_const_expr(expr) {
        return syn::parse_quote! { ((#expr) as usize) };
    }
    syn::parse_quote! { 0 }
}

fn array_len_const_expr(expr: &ast::Expr) -> Option<syn::Expr> {
    match expr {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            let lit = syn::LitInt::new(lit.value, Span::mixed_site());
            Some(syn::parse_quote! { #lit })
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::CHAR => {
            let value = const_eval_expr_in_active_env(expr, 0, &BTreeMap::new())?.as_u128()?;
            let lit = syn::LitInt::new(&value.to_string(), Span::mixed_site());
            Some(syn::parse_quote! { #lit })
        }
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&import_rust_name(ident.name), Span::mixed_site());
            Some(syn::parse_quote! { #ident })
        }
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(base) = &*selector.x else {
                return None;
            };
            let base = syn::Ident::new(&import_rust_name(base.name), Span::mixed_site());
            let sel = syn::Ident::new(&import_rust_name(selector.sel.name), Span::mixed_site());
            Some(syn::parse_quote! { #base::#sel })
        }
        ast::Expr::ParenExpr(paren) => {
            let inner = array_len_const_expr(&paren.x)?;
            Some(syn::parse_quote! { (#inner) })
        }
        ast::Expr::UnaryExpr(unary) => {
            let inner = array_len_const_expr(&unary.x)?;
            match unary.op {
                token::Token::ADD => Some(inner),
                token::Token::SUB => Some(syn::parse_quote! { -#inner }),
                token::Token::XOR => Some(syn::parse_quote! { !#inner }),
                _ => None,
            }
        }
        ast::Expr::BinaryExpr(binary) => {
            let left = array_len_const_expr(&binary.x)?;
            let right = array_len_const_expr(&binary.y)?;
            let op: syn::BinOp = binary.op.into();
            Some(syn::parse_quote! { (#left #op #right) })
        }
        ast::Expr::CallExpr(call) => {
            let args = call.args.as_deref()?;
            let [arg] = args else {
                return None;
            };
            array_len_const_expr(arg)
        }
        _ => None,
    }
}

fn array_literal_len_expr(len: &ast::Expr, elts: &[ast::Expr]) -> syn::Expr {
    if !matches!(len, ast::Expr::Ellipsis(_)) {
        return array_len_expr(len);
    }

    let mut next_index = 0usize;
    let mut max_len = 0usize;
    for elt in elts {
        let index = if let ast::Expr::KeyValueExpr(kv) = elt {
            const_eval_expr_in_active_env(&kv.key, 0, &BTreeMap::new())
                .and_then(|value| value.as_u128())
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(next_index)
        } else {
            next_index
        };
        next_index = index.saturating_add(1);
        max_len = max_len.max(next_index);
    }

    let lit = syn::LitInt::new(&max_len.to_string(), Span::mixed_site());
    syn::parse_quote! { #lit }
}

fn next_unnamed_arg_ident() -> syn::Ident {
    UNNAMED_ARG_COUNTER.with(|counter| {
        let mut counter = counter.borrow_mut();
        let ident = syn::Ident::new(&format!("__gors_arg_{}", *counter), Span::mixed_site());
        *counter += 1;
        ident
    })
}

fn reset_unnamed_arg_counter() {
    UNNAMED_ARG_COUNTER.with(|counter| *counter.borrow_mut() = 0);
}

fn type_with_generic_args(mut base: syn::Type, args: Vec<syn::Type>) -> syn::Type {
    let syn::Type::Path(type_path) = &mut base else {
        return base;
    };
    let Some(segment) = type_path.path.segments.last_mut() else {
        return base;
    };

    let mut generic_args = syn::punctuated::Punctuated::new();
    for arg in args {
        generic_args.push(syn::GenericArgument::Type(arg));
    }

    segment.arguments = syn::PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments {
        colon2_token: None,
        lt_token: <Token![<]>::default(),
        args: generic_args,
        gt_token: <Token![>]>::default(),
    });
    base
}

fn anonymous_struct_type(struct_type: ast::StructType) -> syn::Type {
    if struct_type
        .fields
        .as_ref()
        .is_none_or(|fields| fields.list.is_empty())
    {
        syn::parse_quote! { () }
    } else {
        syn::parse_quote! { Box<dyn std::any::Any> }
    }
}

fn default_expr_for_type(expr: &ast::Expr) -> syn::Expr {
    match expr {
        ast::Expr::Ident(id) if id.name == "any" => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        ast::Expr::InterfaceType(_) => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        ast::Expr::FuncType(func_type) => default_expr_for_func_type(func_type),
        ast::Expr::ArrayType(array_type) if array_type.len.is_some() => {
            default_expr_for_array_type(array_type)
        }
        _ => syn::parse_quote! { Default::default() },
    }
}

fn default_expr_for_func_type(func_type: &ast::FuncType<'_>) -> syn::Expr {
    let box_ty = shared_func_box_type_from_ast(func_type);
    syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(None::<#box_ty>)) }
}

fn default_expr_for_array_type(array_type: &ast::ArrayType) -> syn::Expr {
    let Some(len) = array_type.len.as_ref() else {
        return syn::parse_quote! { Default::default() };
    };
    let len_expr = array_len_expr(len);
    let elem_default = default_expr_for_type(&array_type.elt);
    syn::parse_quote! { [#elem_default; #len_expr] }
}

fn selector_path_from_ref(selector_expr: &ast::SelectorExpr) -> syn::Path {
    let mut segments = syn::punctuated::Punctuated::new();

    fn push_selector_x(
        expr: &ast::Expr,
        segments: &mut syn::punctuated::Punctuated<syn::PathSegment, Token![::]>,
    ) {
        match expr {
            ast::Expr::Ident(ident) => {
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new(&import_rust_name(ident.name), Span::mixed_site()),
                    arguments: syn::PathArguments::None,
                });
            }
            ast::Expr::SelectorExpr(inner) => {
                push_selector_x(&inner.x, segments);
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new(
                        &rust_safe_ident_name(inner.sel.name),
                        Span::mixed_site(),
                    ),
                    arguments: syn::PathArguments::None,
                });
            }
            _ => {
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new("__expr", Span::mixed_site()),
                    arguments: syn::PathArguments::None,
                });
            }
        }
    }

    push_selector_x(&selector_expr.x, &mut segments);
    segments.push(syn::PathSegment {
        ident: syn::Ident::new(
            &rust_safe_ident_name(selector_expr.sel.name),
            Span::mixed_site(),
        ),
        arguments: syn::PathArguments::None,
    });

    syn::Path {
        leading_colon: None,
        segments,
    }
}

fn type_from_expr_ref(expr: &ast::Expr) -> syn::Type {
    match expr {
        ast::Expr::ParenExpr(paren) => type_from_expr_ref(&paren.x),
        ast::Expr::FuncType(func_type) => shared_func_type_from_ast(func_type),
        ast::Expr::Ident(ident) if ident.name == "any" => {
            syn::parse_quote! { Box<dyn std::any::Any> }
        }
        ast::Expr::Ident(ident) if ident.name == "complex64" => {
            syn::parse_quote! { crate::builtin::Complex64 }
        }
        ast::Expr::Ident(ident) if ident.name == "complex128" => {
            syn::parse_quote! { crate::builtin::Complex128 }
        }
        ast::Expr::Ident(ident) if ident.name == "bool" => syn::parse_quote! { bool },
        ast::Expr::Ident(ident) if ident.name == "byte" || ident.name == "uint8" => {
            syn::parse_quote! { u8 }
        }
        ast::Expr::Ident(ident) if ident.name == "rune" => syn::parse_quote! { i32 },
        ast::Expr::Ident(ident) if ident.name == "string" => syn::parse_quote! { String },
        ast::Expr::Ident(ident) if ident.name == "float32" => syn::parse_quote! { f32 },
        ast::Expr::Ident(ident) if ident.name == "float64" => syn::parse_quote! { f64 },
        ast::Expr::Ident(ident) if ident.name == "int" => syn::parse_quote! { isize },
        ast::Expr::Ident(ident) if ident.name == "int8" => syn::parse_quote! { i8 },
        ast::Expr::Ident(ident) if ident.name == "int16" => syn::parse_quote! { i16 },
        ast::Expr::Ident(ident) if ident.name == "int32" => syn::parse_quote! { i32 },
        ast::Expr::Ident(ident) if ident.name == "int64" => syn::parse_quote! { i64 },
        ast::Expr::Ident(ident) if ident.name == "uint" => syn::parse_quote! { usize },
        ast::Expr::Ident(ident) if ident.name == "uint16" => syn::parse_quote! { u16 },
        ast::Expr::Ident(ident) if ident.name == "uint32" => syn::parse_quote! { u32 },
        ast::Expr::Ident(ident) if ident.name == "uint64" => syn::parse_quote! { u64 },
        ast::Expr::Ident(ident) if ident.name == "uintptr" => syn::parse_quote! { usize },
        ast::Expr::Ident(ident) if ident.name == "error" => syn::parse_quote! { String },
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
            syn::parse_quote! { #ident }
        }
        ast::Expr::StarExpr(star) => {
            let inner = type_from_expr_ref(&star.x);
            syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#inner>> }
        }
        ast::Expr::ArrayType(array_type) => {
            let elem = type_from_expr_ref(&array_type.elt);
            if let Some(len) = &array_type.len {
                let len_expr = array_len_expr(len);
                syn::parse_quote! { [#elem; #len_expr] }
            } else {
                syn::parse_quote! { Vec<#elem> }
            }
        }
        ast::Expr::ChanType(chan_type) => {
            let inner = type_from_expr_ref(&chan_type.value);
            syn::parse_quote! { crate::builtin::Chan<#inner> }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                let inner = type_from_expr_ref(elt);
                syn::parse_quote! { Vec<#inner> }
            } else {
                syn::parse_quote! { Vec<Box<dyn std::any::Any>> }
            }
        }
        ast::Expr::SelectorExpr(selector_expr) => {
            if matches!(&*selector_expr.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
                && selector_expr.sel.name == "Pointer"
            {
                return syn::parse_quote! { usize };
            }
            let path = selector_path_from_ref(selector_expr);
            syn::Type::Path(syn::TypePath { qself: None, path })
        }
        ast::Expr::IndexExpr(index_expr) => {
            let base = type_from_expr_ref(&index_expr.x);
            let arg = type_from_expr_ref(&index_expr.index);
            type_with_generic_args(base, vec![arg])
        }
        ast::Expr::IndexListExpr(index_list_expr) => {
            let base = type_from_expr_ref(&index_list_expr.x);
            let args = index_list_expr
                .indices
                .iter()
                .map(type_from_expr_ref)
                .collect();
            type_with_generic_args(base, args)
        }
        ast::Expr::InterfaceType(_) => {
            syn::parse_quote! { Box<dyn std::any::Any> }
        }
        ast::Expr::MapType(map_type) => {
            let key = type_from_expr_ref(&map_type.key);
            let value = type_from_expr_ref(&map_type.value);
            syn::parse_quote! { std::collections::HashMap<#key, #value> }
        }
        ast::Expr::StructType(struct_type) => {
            if struct_type
                .fields
                .as_ref()
                .is_none_or(|fields| fields.list.is_empty())
            {
                syn::parse_quote! { () }
            } else {
                syn::parse_quote! { Box<dyn std::any::Any> }
            }
        }
        _ => syn::parse_quote! { Box<dyn std::any::Any> },
    }
}

fn func_result_type_from_ast(func_type: &ast::FuncType<'_>) -> syn::Type {
    let Some(results) = &func_type.results else {
        return syn::parse_quote! { () };
    };
    let result_types: Vec<syn::Type> = results
        .list
        .iter()
        .flat_map(|field| {
            let ty = field
                .type_
                .as_ref()
                .map(type_from_expr_ref)
                .unwrap_or_else(|| syn::parse_quote! { () });
            let count = field.names.as_ref().map_or(1, Vec::len);
            std::iter::repeat_n(ty, count)
        })
        .collect();
    match result_types.as_slice() {
        [] => syn::parse_quote! { () },
        [ty] => ty.clone(),
        _ => syn::parse_quote! { (#(#result_types),*) },
    }
}

fn func_param_types_from_ast(func_type: &ast::FuncType<'_>) -> Vec<syn::Type> {
    func_type
        .params
        .list
        .iter()
        .flat_map(|field| {
            let ty = field
                .type_
                .as_ref()
                .map(type_from_expr_ref)
                .unwrap_or_else(|| syn::parse_quote! { () });
            let count = field.names.as_ref().map_or(1, Vec::len);
            std::iter::repeat_n(ty, count)
        })
        .collect()
}

fn shared_func_type_from_ast(func_type: &ast::FuncType<'_>) -> syn::Type {
    let params = func_param_types_from_ast(func_type);
    let result = func_result_type_from_ast(func_type);
    syn::parse_quote! { std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn Fn(#(#params),*) -> #result + Send + Sync>>>> }
}

fn shared_func_box_type_from_ast(func_type: &ast::FuncType<'_>) -> syn::Type {
    let params = func_param_types_from_ast(func_type);
    let result = func_result_type_from_ast(func_type);
    syn::parse_quote! { std::sync::Arc<dyn Fn(#(#params),*) -> #result + Send + Sync> }
}

fn numeric_newtype_impls(ident: &syn::Ident) -> Vec<syn::Item> {
    vec![
        syn::parse_quote! {
            impl std::ops::BitXorAssign for #ident {
                fn bitxor_assign(&mut self, rhs: Self) {
                    self.0 ^= rhs.0;
                }
            }
        },
        syn::parse_quote! {
            impl std::ops::Shl<i32> for #ident {
                type Output = Self;
                fn shl(self, rhs: i32) -> Self {
                    Self(self.0 << rhs)
                }
            }
        },
        syn::parse_quote! {
            impl std::ops::Shr<i32> for #ident {
                type Output = Self;
                fn shr(self, rhs: i32) -> Self {
                    Self(self.0 >> rhs)
                }
            }
        },
    ]
}

fn newtype_into_inner_impl(ident: &syn::Ident, inner: &syn::Type) -> syn::Item {
    syn::parse_quote! {
        impl From<#ident> for #inner {
            fn from(value: #ident) -> #inner {
                value.0
            }
        }
    }
}

fn slice_elem_type_from_expr(expr: &ast::Expr) -> Option<syn::Type> {
    match expr {
        ast::Expr::ArrayType(array) if array.len.is_none() => Some(type_from_expr_ref(&array.elt)),
        _ => None,
    }
}

fn self_referential_pointer_type(expr: &ast::Expr, self_ident: &syn::Ident) -> Option<syn::Type> {
    let ast::Expr::StarExpr(star) = expr else {
        return None;
    };
    let type_name = extract_type_name(&star.x)?;
    (*self_ident == type_name).then(|| syn::parse_quote! { *mut #self_ident })
}

fn type_from_struct_field_expr(expr: &ast::Expr, self_ident: &syn::Ident) -> syn::Type {
    self_referential_pointer_type(expr, self_ident).unwrap_or_else(|| type_from_expr_ref(expr))
}

fn interface_box_impl(ident: &syn::Ident, trait_items: &[syn::TraitItem]) -> Option<syn::Item> {
    let mut impl_methods = Vec::new();
    for trait_item in trait_items {
        let syn::TraitItem::Fn(trait_fn) = trait_item else {
            continue;
        };
        let method = trait_fn.sig.ident.clone();
        let arg_names = trait_fn
            .sig
            .inputs
            .iter()
            .skip(1)
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pat_type) => match &*pat_type.pat {
                    syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
                    _ => None,
                },
                syn::FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();
        let sig = trait_fn.sig.clone();
        let block: syn::Block = if matches!(sig.output, syn::ReturnType::Default) {
            syn::parse_quote!({ (**self).#method(#(#arg_names),*); })
        } else {
            syn::parse_quote!({ (**self).#method(#(#arg_names),*) })
        };
        impl_methods.push(syn::ImplItemFn {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            defaultness: None,
            sig,
            block,
        });
    }
    if impl_methods.is_empty() {
        return None;
    }
    Some(syn::parse_quote! {
        impl<T: #ident + ?Sized> #ident for Box<T> {
            #(#impl_methods)*
        }
    })
}

fn noop_interface_items(ident: &syn::Ident, trait_items: &[syn::TraitItem]) -> Vec<syn::Item> {
    let noop_ident = syn::Ident::new(&format!("__GorsNoop{ident}"), Span::mixed_site());
    let mut impl_methods = Vec::new();
    for trait_item in trait_items {
        let syn::TraitItem::Fn(trait_fn) = trait_item else {
            continue;
        };
        let sig = trait_fn.sig.clone();
        let block = if sig.ident == "__gors_as_any" {
            syn::parse_quote!({ None })
        } else if matches!(sig.output, syn::ReturnType::Default) {
            syn::parse_quote!({})
        } else {
            syn::parse_quote!({ panic!("called no-op interface method") })
        };
        impl_methods.push(syn::ImplItemFn {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            defaultness: None,
            sig,
            block,
        });
    }

    vec![
        syn::parse_quote! {
            #[derive(Clone, Default)]
            pub struct #noop_ident;
        },
        syn::parse_quote! {
            impl #ident for #noop_ident {
                #(#impl_methods)*
            }
        },
    ]
}

fn collect_trait_method_fns(items: &[syn::Item]) -> BTreeMap<String, Vec<syn::TraitItemFn>> {
    let mut traits = BTreeMap::new();
    for item in items {
        let syn::Item::Trait(item_trait) = item else {
            continue;
        };
        let methods = item_trait
            .items
            .iter()
            .filter_map(|trait_item| match trait_item {
                syn::TraitItem::Fn(trait_fn) => Some(trait_fn.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            traits.insert(item_trait.ident.to_string(), methods);
        }
    }
    traits
}

fn embedded_interface_impls(
    items: &[syn::Item],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
) -> Vec<syn::Item> {
    let trait_methods = collect_trait_method_fns(items);
    let embedded_structs = BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().clone());
    let mut out = vec![];

    for (struct_name, fields) in embedded_structs {
        let struct_ident = syn::Ident::new(&struct_name, Span::mixed_site());
        for field in fields {
            let Some(trait_ident) = field
                .trait_path
                .segments
                .last()
                .map(|segment| &segment.ident)
            else {
                continue;
            };
            let Some(required_methods) = trait_methods.get(&trait_ident.to_string()) else {
                continue;
            };
            let mut impl_items = vec![];
            for trait_fn in required_methods {
                let method_name = trait_fn.sig.ident.to_string();
                if let Some(method) = methods.get(&struct_name).and_then(|methods| {
                    methods
                        .iter()
                        .find(|method| method.sig.ident == method_name)
                }) {
                    let mut method = method.clone();
                    method.vis = syn::Visibility::Inherited;
                    if let Some(syn::FnArg::Receiver(receiver)) = method.sig.inputs.first_mut() {
                        receiver.mutability = Some(<Token![mut]>::default());
                        *receiver.ty = syn::parse_quote! { &mut Self };
                    }
                    impl_items.push(syn::ImplItem::Fn(method));
                    continue;
                }

                let mut sig = trait_fn.sig.clone();
                let is_hook = is_runtime_interface_hook(&sig.ident.to_string());
                if let Some(syn::FnArg::Receiver(receiver)) = sig.inputs.first_mut() {
                    if is_hook {
                        receiver.mutability = None;
                        *receiver.ty = syn::parse_quote! { &Self };
                    } else {
                        receiver.mutability = Some(<Token![mut]>::default());
                        *receiver.ty = syn::parse_quote! { &mut Self };
                    }
                }
                let method_ident = sig.ident.clone();
                let field_ident = field.field_ident.clone();
                let arg_idents = sig
                    .inputs
                    .iter()
                    .skip(1)
                    .filter_map(|arg| match arg {
                        syn::FnArg::Typed(pat_type) => match &*pat_type.pat {
                            syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
                            _ => None,
                        },
                        syn::FnArg::Receiver(_) => None,
                    })
                    .collect::<Vec<_>>();
                let block = if matches!(sig.output, syn::ReturnType::Default) {
                    syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*); })
                } else {
                    syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*) })
                };
                impl_items.push(syn::ImplItem::Fn(syn::ImplItemFn {
                    attrs: vec![],
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    sig,
                    block,
                }));
            }
            if impl_items.is_empty() {
                continue;
            }
            let trait_path = field.trait_path.clone();
            out.push(syn::Item::Impl(syn::ItemImpl {
                attrs: vec![],
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics: syn::parse_quote! { <'__gors> },
                trait_: Some((None, trait_path, <Token![for]>::default())),
                self_ty: Box::new(syn::parse_quote! { #struct_ident<'__gors> }),
                brace_token: syn::token::Brace::default(),
                items: impl_items,
            }));
        }
    }

    out
}

fn compile_type_spec(ts: ast::TypeSpec) -> Result<Vec<syn::Item>, CompilerError> {
    let name = ts
        .name
        .ok_or_else(|| CompilerError::UnsupportedConstruct("type spec has no name".to_string()))?;
    let vis: syn::Visibility = (&name).into();
    let ident: syn::Ident = name.into();
    let mut generics = compile_go_type_params(ts.type_params);

    match ts.type_ {
        ast::Expr::StructType(struct_type) => {
            let mut fields = syn::punctuated::Punctuated::new();
            let mut embedded_types: Vec<(syn::Ident, syn::Type, Option<syn::Path>)> = vec![];
            let mut embedded_interface_fields: Vec<EmbeddedInterfaceField> = vec![];
            let mut has_borrowed_interface_field = false;
            let mut default_fields: Vec<(syn::Ident, syn::Expr)> = vec![];
            let mut needs_manual_default = false;
            let mut cannot_derive_clone = false;
            let mut cannot_default = false;
            let mut can_derive_copy = true;
            let mut blank_field_index = 0usize;
            if let Some(field_list) = struct_type.fields {
                for field in field_list.list {
                    let field_type = field.type_.ok_or_else(|| {
                        CompilerError::UnsupportedConstruct("struct field has no type".to_string())
                    })?;
                    let interface_trait_path = interface_trait_path_from_expr(&field_type);
                    has_borrowed_interface_field |= interface_trait_path.is_some();
                    let field_contains_func = contains_func_type(&field_type);
                    let field_needs_manual_default =
                        contains_array_type(&field_type) || field_contains_func;
                    let field_cannot_derive_clone =
                        contains_any_type(&field_type) || interface_trait_path.is_some();
                    let field_cannot_default = interface_trait_path.is_some();
                    let field_can_derive_copy = !field_contains_func
                        && interface_trait_path.is_none()
                        && go_type_is_copy(&typeinfer::GoType::from_expr(&field_type));
                    let field_default = default_expr_for_type(&field_type);
                    can_derive_copy &= field_can_derive_copy;

                    if let Some(names) = field.names {
                        let rust_type: syn::Type = if let Some(trait_path) = &interface_trait_path {
                            syn::parse_quote! { &'__gors mut dyn #trait_path }
                        } else {
                            type_from_struct_field_expr(&field_type, &ident)
                        };
                        for field_name in names {
                            let field_vis: syn::Visibility = (&field_name).into();
                            let field_ident: syn::Ident = if field_name.name == "_" {
                                let ident = syn::Ident::new(
                                    &format!("_gors_blank_{}", blank_field_index),
                                    Span::mixed_site(),
                                );
                                blank_field_index += 1;
                                ident
                            } else {
                                field_name.into()
                            };
                            default_fields.push((field_ident.clone(), field_default.clone()));
                            needs_manual_default |= field_needs_manual_default;
                            cannot_derive_clone |= field_cannot_derive_clone;
                            cannot_default |= field_cannot_default;
                            fields.push(syn::Field {
                                attrs: vec![],
                                vis: field_vis,
                                mutability: syn::FieldMutability::None,
                                ident: Some(field_ident),
                                colon_token: Some(<Token![:]>::default()),
                                ty: rust_type.clone(),
                            });
                        }
                    } else {
                        // Embedded field: type name becomes the field name
                        let embedded_name = extract_type_name(&field_type);
                        let rust_type: syn::Type = if let Some(trait_path) = &interface_trait_path {
                            syn::parse_quote! { &'__gors mut dyn #trait_path }
                        } else {
                            type_from_struct_field_expr(&field_type, &ident)
                        };
                        if let Some(name) = embedded_name {
                            let field_ident =
                                syn::Ident::new(&rust_safe_ident_name(&name), Span::mixed_site());
                            let field_vis: syn::Visibility =
                                if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                                    syn::parse_quote! { pub }
                                } else {
                                    syn::Visibility::Inherited
                                };
                            if let Some(trait_path) = &interface_trait_path {
                                embedded_interface_fields.push(EmbeddedInterfaceField {
                                    field_ident: field_ident.clone(),
                                    trait_path: trait_path.clone(),
                                });
                            }
                            embedded_types.push((
                                field_ident.clone(),
                                rust_type.clone(),
                                interface_trait_path.clone(),
                            ));
                            default_fields.push((field_ident.clone(), field_default.clone()));
                            needs_manual_default |= field_needs_manual_default;
                            cannot_derive_clone |= field_cannot_derive_clone;
                            cannot_default |= field_cannot_default;
                            fields.push(syn::Field {
                                attrs: vec![],
                                vis: field_vis,
                                mutability: syn::FieldMutability::None,
                                ident: Some(field_ident),
                                colon_token: Some(<Token![:]>::default()),
                                ty: rust_type,
                            });
                        }
                    }
                }
            }

            if has_borrowed_interface_field {
                let lifetime = syn::GenericParam::Lifetime(syn::LifetimeParam::new(
                    syn::Lifetime::new("'__gors", Span::mixed_site()),
                ));
                let mut params = syn::punctuated::Punctuated::new();
                params.push(lifetime);
                params.extend(generics.params.clone());
                generics.params = params;
                BORROWED_INTERFACE_STRUCTS.with(|structs| {
                    structs
                        .borrow_mut()
                        .insert(ident.to_string(), embedded_interface_fields.clone());
                });
            }

            if !fields.empty_or_trailing() {
                fields.push_punct(<Token![,]>::default());
            }

            let generics_for_impl = generics.clone();
            let (impl_generics, ty_generics, where_clause) = generics_for_impl.split_for_impl();
            let struct_item = syn::Item::Struct(syn::ItemStruct {
                attrs: if cannot_derive_clone {
                    vec![]
                } else if needs_manual_default {
                    if can_derive_copy {
                        vec![syn::parse_quote! { #[derive(Clone, Copy, PartialEq)] }]
                    } else {
                        vec![syn::parse_quote! { #[derive(Clone)] }]
                    }
                } else if can_derive_copy {
                    vec![syn::parse_quote! { #[derive(Clone, Copy, Default, PartialEq)] }]
                } else {
                    vec![syn::parse_quote! { #[derive(Clone, Default)] }]
                },
                vis,
                struct_token: <Token![struct]>::default(),
                ident: ident.clone(),
                generics,
                fields: syn::Fields::Named(syn::FieldsNamed {
                    brace_token: syn::token::Brace::default(),
                    named: fields,
                }),
                semi_token: None,
            });

            let default_impl = if !cannot_default && (needs_manual_default || cannot_derive_clone) {
                let defaults = default_fields.iter().map(|(field_ident, default_expr)| {
                    quote::quote! { #field_ident: #default_expr }
                });
                Some(syn::parse_quote! {
                    impl #impl_generics Default for #ident #ty_generics #where_clause {
                        fn default() -> Self {
                            Self {
                                #(#defaults),*
                            }
                        }
                    }
                })
            } else {
                None
            };
            if let Some((emb_field, emb_ty, interface_trait_path)) =
                embedded_types.first().filter(|_| embedded_types.len() == 1)
            {
                let deref_impl: syn::Item = if let Some(trait_path) = interface_trait_path {
                    syn::parse_quote! {
                        impl #impl_generics std::ops::Deref for #ident #ty_generics #where_clause {
                            type Target = dyn #trait_path + '__gors;
                            fn deref(&self) -> &(dyn #trait_path + '__gors) {
                                &*self.#emb_field
                            }
                        }
                    }
                } else {
                    syn::parse_quote! {
                        impl #impl_generics std::ops::Deref for #ident #ty_generics #where_clause {
                            type Target = #emb_ty;
                            fn deref(&self) -> &#emb_ty {
                                &self.#emb_field
                            }
                        }
                    }
                };
                let deref_mut_impl: syn::Item = if let Some(trait_path) = interface_trait_path {
                    syn::parse_quote! {
                        impl #impl_generics std::ops::DerefMut for #ident #ty_generics #where_clause {
                            fn deref_mut(&mut self) -> &mut (dyn #trait_path + '__gors) {
                                self.#emb_field
                            }
                        }
                    }
                } else {
                    syn::parse_quote! {
                        impl #impl_generics std::ops::DerefMut for #ident #ty_generics #where_clause {
                            fn deref_mut(&mut self) -> &mut #emb_ty {
                                &mut self.#emb_field
                            }
                        }
                    }
                };
                let mut out = vec![struct_item];
                if let Some(default_impl) = default_impl {
                    out.push(default_impl);
                }
                out.push(deref_impl);
                out.push(deref_mut_impl);
                return Ok(out);
            }

            let mut out = vec![struct_item];
            if let Some(default_impl) = default_impl {
                out.push(default_impl);
            }
            Ok(out)
        }
        ast::Expr::InterfaceType(iface) => {
            INTERFACE_NAMES.with(|names| {
                names.borrow_mut().insert(ident.to_string());
            });
            // Generate trait with method signatures
            let mut trait_items: Vec<syn::TraitItem> = vec![];
            let mut supertraits = syn::punctuated::Punctuated::new();

            if let Some(methods_list) = iface.methods {
                for field in methods_list.list {
                    if let Some(names) = field.names {
                        // Extract parameter types and return type from the FuncType
                        let mut param_types: Vec<(String, syn::Type)> = vec![];
                        let mut output = syn::ReturnType::Default;

                        if let Some(ast::Expr::FuncType(func_type)) = field.type_ {
                            for param in func_type.params.list {
                                let ty: syn::Type = if let Some(type_expr) = param.type_ {
                                    type_expr.into()
                                } else {
                                    syn::parse_quote! { () }
                                };
                                if let Some(param_names) = param.names {
                                    for pname in param_names {
                                        param_types.push((pname.name.to_string(), ty.clone()));
                                    }
                                } else {
                                    param_types.push(("_arg".to_string(), ty));
                                }
                            }
                            if let Ok(ret) = compile_return_type(func_type.results) {
                                output = ret;
                            }
                        }

                        for name in names {
                            let method_ident: syn::Ident = name.into();

                            let mut inputs = syn::punctuated::Punctuated::new();
                            inputs.push(syn::FnArg::Receiver(syn::Receiver {
                                attrs: vec![],
                                reference: Some((<Token![&]>::default(), None)),
                                mutability: Some(<Token![mut]>::default()),
                                self_token: <Token![self]>::default(),
                                colon_token: None,
                                ty: Box::new(syn::parse_quote! { &mut Self }),
                            }));

                            for (pname, ty) in &param_types {
                                let pident = syn::Ident::new(
                                    &rust_safe_ident_name(pname),
                                    Span::mixed_site(),
                                );
                                inputs.push(syn::FnArg::Typed(syn::PatType {
                                    attrs: vec![],
                                    pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                                        attrs: vec![],
                                        by_ref: None,
                                        subpat: None,
                                        mutability: None,
                                        ident: pident,
                                    })),
                                    colon_token: <Token![:]>::default(),
                                    ty: Box::new(ty.clone()),
                                }));
                            }

                            let sig = syn::Signature {
                                constness: None,
                                asyncness: None,
                                unsafety: None,
                                abi: None,
                                fn_token: <Token![fn]>::default(),
                                ident: method_ident,
                                generics: syn::Generics::default(),
                                paren_token: syn::token::Paren::default(),
                                inputs,
                                variadic: None,
                                output: output.clone(),
                            };

                            trait_items.push(syn::TraitItem::Fn(syn::TraitItemFn {
                                attrs: vec![],
                                sig,
                                default: None,
                                semi_token: Some(<Token![;]>::default()),
                            }));
                        }
                    } else if let Some(type_expr) = field.type_ {
                        append_type_param_bounds(
                            &mut supertraits,
                            go_constraint_to_rust_bounds(&type_expr),
                        );
                    }
                }
            }

            let has_supertraits = !supertraits.is_empty();
            let has_go_methods = !trait_items.is_empty();
            if has_go_methods {
                trait_items.insert(
                    0,
                    syn::parse_quote! {
                        fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                    },
                );
            }
            let has_methods = !trait_items.is_empty();
            let box_impl = has_methods.then(|| interface_box_impl(&ident, &trait_items));
            let trait_item = syn::Item::Trait(syn::ItemTrait {
                attrs: vec![],
                vis,
                unsafety: None,
                auto_token: None,
                restriction: None,
                trait_token: <Token![trait]>::default(),
                ident: ident.clone(),
                generics: syn::Generics::default(),
                colon_token: has_supertraits.then(<Token![:]>::default),
                supertraits: supertraits.clone(),
                brace_token: syn::token::Brace::default(),
                items: trait_items.clone(),
            });
            let mut items = vec![trait_item];
            if let Some(Some(box_impl)) = box_impl {
                items.push(box_impl);
            }
            if has_methods {
                items.extend(noop_interface_items(&ident, &trait_items));
            }
            if has_supertraits && !has_methods {
                items.push(syn::parse_quote! {
                    impl<T> #ident for T where T: #supertraits {}
                });
            }
            Ok(items)
        }
        other => {
            let is_byte_slice = is_byte_slice_type(&other);
            let slice_elem_type = slice_elem_type_from_expr(&other);
            let underlying_go_type = typeinfer::GoType::from_expr(&other);
            let is_slice_alias = matches!(underlying_go_type, typeinfer::GoType::Slice(_));
            let is_copy_alias = go_type_is_copy(&underlying_go_type);
            let is_numeric_alias = resolved_go_type(&underlying_go_type).is_numeric();
            let rust_type: syn::Type = other.into();
            let struct_item: syn::Item = if is_copy_alias {
                syn::parse_quote! {
                    #[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
                    #vis struct #ident #generics(pub #rust_type);
                }
            } else {
                syn::parse_quote! {
                    #[derive(Clone, Default)]
                    #vis struct #ident #generics(pub #rust_type);
                }
            };
            let deref_impl: syn::Item = syn::parse_quote! {
                impl std::ops::Deref for #ident {
                    type Target = #rust_type;
                    fn deref(&self) -> &#rust_type { &self.0 }
                }
            };
            let deref_mut_impl: syn::Item = syn::parse_quote! {
                impl std::ops::DerefMut for #ident {
                    fn deref_mut(&mut self) -> &mut #rust_type { &mut self.0 }
                }
            };
            let mut items = vec![struct_item, deref_impl, deref_mut_impl];
            if is_copy_alias {
                items.push(newtype_into_inner_impl(&ident, &rust_type));
            }
            if is_numeric_alias {
                items.extend(numeric_newtype_impls(&ident));
            }
            if is_slice_alias && !is_byte_slice {
                items.push(syn::parse_quote! {
                    impl crate::builtin::Len for #ident {
                        fn len_value(&self) -> usize { self.0.len() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Cap for #ident {
                        fn cap_value(&self) -> usize { self.0.capacity() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl From<#rust_type> for #ident {
                        fn from(value: #rust_type) -> Self { Self(value) }
                    }
                });
                items.push(syn::parse_quote! {
                    impl From<#ident> for #rust_type {
                        fn from(value: #ident) -> Self { value.0 }
                    }
                });
                if let Some(elem_ty) = &slice_elem_type {
                    items.push(syn::parse_quote! {
                        impl AsRef<[#elem_ty]> for #ident {
                            fn as_ref(&self) -> &[#elem_ty] { self.0.as_ref() }
                        }
                    });
                    items.push(syn::parse_quote! {
                        impl AsMut<[#elem_ty]> for #ident {
                            fn as_mut(&mut self) -> &mut [#elem_ty] { self.0.as_mut() }
                        }
                    });
                    items.push(syn::parse_quote! {
                        impl crate::builtin::Append<#elem_ty> for #ident {
                            fn append_value(mut self, elem: #elem_ty) -> Self {
                                self.0.push(elem);
                                self
                            }
                        }
                    });
                }
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<#rust_type> for #ident {
                        fn append_value(mut self, elem: #rust_type) -> Self {
                            self.0.extend(elem);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<#ident> for #rust_type {
                        fn append_value(mut self, elem: #ident) -> Self {
                            self.extend(elem.0);
                            self
                        }
                    }
                });
            }
            if is_byte_slice {
                items.push(syn::parse_quote! {
                    impl crate::builtin::Len for #ident {
                        fn len_value(&self) -> usize { self.0.len() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Cap for #ident {
                        fn cap_value(&self) -> usize { self.0.capacity() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::StringValue for #ident {
                        fn string_value(self) -> String {
                            String::from_utf8(self.0).unwrap_or_default()
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::StringValue for &#ident {
                        fn string_value(self) -> String {
                            String::from_utf8(self.0.clone()).unwrap_or_default()
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl AsRef<[u8]> for #ident {
                        fn as_ref(&self) -> &[u8] { self.0.as_ref() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl AsMut<[u8]> for #ident {
                        fn as_mut(&mut self) -> &mut [u8] { self.0.as_mut() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl From<Vec<u8>> for #ident {
                        fn from(value: Vec<u8>) -> Self { Self(value) }
                    }
                });
                items.push(syn::parse_quote! {
                    impl From<#ident> for Vec<u8> {
                        fn from(value: #ident) -> Self { value.0 }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<u8> for #ident {
                        fn append_value(mut self, elem: u8) -> Self {
                            self.0.push(elem);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<Vec<u8>> for #ident {
                        fn append_value(mut self, elem: Vec<u8>) -> Self {
                            self.0.extend(elem);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<#ident> for Vec<u8> {
                        fn append_value(mut self, elem: #ident) -> Self {
                            self.extend(elem.0);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::Append<String> for #ident {
                        fn append_value(mut self, elem: String) -> Self {
                            self.0.extend(elem.into_bytes());
                            self
                        }
                    }
                });
            }
            Ok(items)
        }
    }
}

fn compile_return_type(results: Option<ast::FieldList>) -> Result<syn::ReturnType, CompilerError> {
    let Some(results) = results else {
        return Ok(syn::ReturnType::Default);
    };
    let result_types: Vec<syn::Type> = results
        .list
        .into_iter()
        .flat_map(|f| {
            let count = f.names.as_ref().map_or(1, |names| names.len());
            f.type_
                .map(return_type_from_expr)
                .map(|ty| std::iter::repeat_n(ty, count).collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    match result_types.len() {
        0 => Ok(syn::ReturnType::Default),
        1 => {
            let Some(ty) = result_types.into_iter().next() else {
                return Err(CompilerError::InvalidFunctionSignature(
                    "missing return type".to_string(),
                ));
            };
            Ok(syn::ReturnType::Type(<Token![->]>::default(), Box::new(ty)))
        }
        _ => {
            let mut elems = syn::punctuated::Punctuated::new();
            for ty in result_types {
                elems.push(ty);
            }
            Ok(syn::ReturnType::Type(
                <Token![->]>::default(),
                Box::new(syn::Type::Tuple(syn::TypeTuple {
                    paren_token: syn::token::Paren::default(),
                    elems,
                })),
            ))
        }
    }
}

fn add_elided_lifetime_to_borrowed_interface_return(
    output: &mut syn::ReturnType,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, Token![,]>,
) {
    let reference_inputs = inputs
        .iter()
        .filter(|input| match input {
            syn::FnArg::Typed(pat_type) => matches!(&*pat_type.ty, syn::Type::Reference(_)),
            syn::FnArg::Receiver(receiver) => receiver.reference.is_some(),
        })
        .count();
    if reference_inputs != 1 {
        return;
    }
    let syn::ReturnType::Type(_, ty) = output else {
        return;
    };
    add_elided_lifetime_to_boxed_trait_object(ty);
}

fn add_elided_lifetime_to_boxed_trait_object(ty: &mut syn::Type) {
    let syn::Type::Path(type_path) = ty else {
        return;
    };
    let Some(segment) = type_path.path.segments.last_mut() else {
        return;
    };
    if segment.ident != "Box" {
        return;
    }
    let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments else {
        return;
    };
    let Some(syn::GenericArgument::Type(syn::Type::TraitObject(trait_object))) =
        args.args.first_mut()
    else {
        return;
    };
    if trait_object
        .bounds
        .iter()
        .any(|bound| matches!(bound, syn::TypeParamBound::Lifetime(_)))
    {
        return;
    }
    trait_object
        .bounds
        .push(syn::TypeParamBound::Lifetime(syn::Lifetime::new(
            "'_",
            Span::mixed_site(),
        )));
}

fn collect_return_go_types(results: Option<&ast::FieldList>) -> Vec<typeinfer::GoType> {
    let Some(results) = results else {
        return Vec::new();
    };
    results
        .list
        .iter()
        .flat_map(|field| {
            let count = field.names.as_ref().map_or(1, |names| names.len());
            field
                .type_
                .as_ref()
                .map(typeinfer::GoType::from_expr)
                .map(|ty| std::iter::repeat_n(ty, count).collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect()
}

fn return_type_from_expr(expr: ast::Expr) -> syn::Type {
    let is_interface = is_interface_expr(&expr);
    let ty: syn::Type = expr.into();
    if is_interface {
        syn::parse_quote! { Box<dyn #ty> }
    } else {
        ty
    }
}

fn is_interface_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => is_interface_expr(&paren.x),
        ast::Expr::Ident(id) => TYPE_ENV.with(|env| env.borrow().is_interface(id.name)),
        ast::Expr::SelectorExpr(sel) => selector_type_env_name(sel)
            .is_some_and(|name| TYPE_ENV.with(|env| env.borrow().is_interface(&name))),
        _ => false,
    }
}

fn selector_type_env_name(sel: &ast::SelectorExpr) -> Option<String> {
    if let ast::Expr::Ident(pkg) = &*sel.x {
        Some(format!("{}.{}", pkg.name, sel.sel.name))
    } else {
        None
    }
}

fn selector_top_level_var_type(sel: &ast::SelectorExpr) -> Option<typeinfer::GoType> {
    let key = selector_type_env_name(sel)?;
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        if env.is_top_level_var(&key) && !env.is_const(&key) {
            env.get_top_level_var(&key)
        } else {
            None
        }
    })
}

fn top_level_var_read_expr(path: syn::Expr, go_type: &typeinfer::GoType) -> syn::Expr {
    if go_type_is_copy(go_type)
        && !matches!(resolved_go_type(go_type), typeinfer::GoType::Pointer(_))
    {
        syn::parse_quote! { *#path }
    } else {
        syn::parse_quote! { (*#path).clone() }
    }
}

fn register_type_spec_in_env(ts: &ast::TypeSpec) {
    let Some(name) = &ts.name else { return };
    match &ts.type_ {
        ast::Expr::StructType(st) => {
            TYPE_ENV.with(|env| {
                let mut env = env.borrow_mut();
                env.set_type_kind(name.name, typeinfer::TypeKind::Struct);
                if let Some(fields) = &st.fields {
                    let mut field_types = vec![];
                    for field in &fields.list {
                        let ty = field
                            .type_
                            .as_ref()
                            .map(typeinfer::GoType::from_expr)
                            .unwrap_or(typeinfer::GoType::Unknown);
                        if let Some(names) = &field.names {
                            for field_name in names {
                                field_types.push((field_name.name.to_string(), ty.clone()));
                            }
                        }
                    }
                    env.set_struct_fields(name.name, field_types);
                }
            });
        }
        ast::Expr::InterfaceType(_) => {
            TYPE_ENV.with(|env| {
                env.borrow_mut()
                    .set_type_kind(name.name, typeinfer::TypeKind::Interface);
            });
        }
        other => {
            let underlying = typeinfer::GoType::from_expr(other);
            TYPE_ENV.with(|env| {
                env.borrow_mut()
                    .set_type_kind(name.name, typeinfer::TypeKind::Alias(underlying));
            });
        }
    }
}

fn extract_receiver_type(expr: &ast::Expr) -> Result<(String, bool), CompilerError> {
    fn receiver_base_name(expr: &ast::Expr) -> Option<String> {
        match expr {
            ast::Expr::Ident(ident) => Some(ident.name.to_string()),
            ast::Expr::IndexExpr(index) => receiver_base_name(&index.x),
            ast::Expr::IndexListExpr(index) => receiver_base_name(&index.x),
            ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
            _ => None,
        }
    }

    match expr {
        ast::Expr::StarExpr(star) => receiver_base_name(&star.x)
            .map(|name| (name, true))
            .ok_or_else(|| {
                CompilerError::UnsupportedConstruct("complex receiver type".to_string())
            }),
        other => receiver_base_name(other)
            .map(|name| (name, false))
            .ok_or_else(|| {
                CompilerError::UnsupportedConstruct(format!(
                    "unsupported receiver type: {:?}",
                    expr
                ))
            }),
    }
}

fn receiver_type_args(expr: &ast::Expr) -> Vec<syn::Ident> {
    match expr {
        ast::Expr::StarExpr(star) => receiver_type_args(&star.x),
        ast::Expr::IndexExpr(index) => {
            if let ast::Expr::Ident(id) = &*index.index {
                vec![syn::Ident::new(
                    &rust_safe_ident_name(id.name),
                    Span::mixed_site(),
                )]
            } else {
                vec![]
            }
        }
        ast::Expr::IndexListExpr(index) => index
            .indices
            .iter()
            .filter_map(|expr| {
                if let ast::Expr::Ident(id) = expr {
                    Some(syn::Ident::new(
                        &rust_safe_ident_name(id.name),
                        Span::mixed_site(),
                    ))
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

fn rewrite_receiver(block: &mut syn::Block, recv_name: &str) {
    use syn::visit_mut::VisitMut;

    struct RewriteReceiver<'a> {
        recv_name: &'a str,
    }

    impl VisitMut for RewriteReceiver<'_> {
        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            // First recurse into children
            syn::visit_mut::visit_expr_mut(self, expr);

            if let syn::Expr::Reference(reference) = expr
                && reference.mutability.is_some()
                && matches!(&*reference.expr, syn::Expr::Path(path) if path.path.is_ident("self"))
            {
                *expr = syn::parse_quote! { self };
                return;
            }

            // Rewrite `recv::Field` paths to `self.Field` field access
            if let syn::Expr::Path(expr_path) = expr {
                if expr_path.path.leading_colon.is_none() && expr_path.path.segments.len() == 2 {
                    let first = expr_path
                        .path
                        .segments
                        .first()
                        .map(|seg| seg.ident.to_string());
                    if first.as_deref() == Some(self.recv_name) {
                        let Some(field_ident) = expr_path
                            .path
                            .segments
                            .iter()
                            .nth(1)
                            .map(|seg| seg.ident.clone())
                        else {
                            return;
                        };
                        *expr = syn::Expr::Field(syn::ExprField {
                            attrs: vec![],
                            base: Box::new(syn::Expr::Path(syn::ExprPath {
                                attrs: vec![],
                                qself: None,
                                path: syn::parse_quote! { self },
                            })),
                            dot_token: <Token![.]>::default(),
                            member: syn::Member::Named(field_ident),
                        });
                        return;
                    }
                }
                // Rewrite standalone receiver name to `self`
                if expr_path.path.leading_colon.is_none() && expr_path.path.segments.len() == 1 {
                    let Some(name) = expr_path
                        .path
                        .segments
                        .first()
                        .map(|seg| seg.ident.to_string())
                    else {
                        return;
                    };
                    if name == self.recv_name {
                        expr_path.path = syn::parse_quote! { self };
                    }
                }
            }
        }
    }

    RewriteReceiver { recv_name }.visit_block_mut(block);
}

fn compile_method(
    func_decl: ast::FuncDecl,
) -> Result<(String, Vec<syn::Ident>, syn::ImplItemFn), CompilerError> {
    reset_unnamed_arg_counter();

    let recv = func_decl
        .recv
        .ok_or_else(|| CompilerError::UnsupportedConstruct("method has no receiver".to_string()))?;

    let recv_field =
        recv.list.into_iter().next().ok_or_else(|| {
            CompilerError::UnsupportedConstruct("empty receiver list".to_string())
        })?;

    let recv_name = recv_field
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| rust_safe_ident_name(n.name))
        .unwrap_or_default();

    let recv_type = recv_field
        .type_
        .ok_or_else(|| CompilerError::UnsupportedConstruct("receiver has no type".to_string()))?;

    let (type_name, is_pointer) = extract_receiver_type(&recv_type)?;
    let is_slice_receiver = TYPE_ENV.with(|env| {
        matches!(
            env.borrow()
                .resolve_alias(&typeinfer::GoType::Named(type_name.clone())),
            typeinfer::GoType::Slice(_)
        )
    });
    let has_borrowed_interface_field =
        BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().contains_key(&type_name));
    let type_args = receiver_type_args(&recv_type);

    let self_arg: syn::FnArg = if is_pointer || is_slice_receiver || has_borrowed_interface_field {
        syn::parse_quote! { &mut self }
    } else {
        syn::parse_quote! { &self }
    };

    if !recv_name.is_empty() {
        let recv_go_type = if is_pointer {
            typeinfer::GoType::Pointer(Box::new(typeinfer::GoType::Named(type_name.clone())))
        } else {
            typeinfer::GoType::Named(type_name.clone())
        };
        TYPE_ENV.with(|env| {
            env.borrow_mut().set_var(&recv_name, recv_go_type);
        });
    }
    TYPE_ENV.with(|env| {
        let mut e = env.borrow_mut();
        for param in &func_decl.type_.params.list {
            let ty = param
                .type_
                .as_ref()
                .map(typeinfer::GoType::from_expr)
                .unwrap_or(typeinfer::GoType::Unknown);
            if let Some(ref names) = param.names {
                for name in names {
                    e.set_var(name.name, ty.clone());
                }
            }
        }
    });

    let mut borrow_pointer_params =
        pointer_params_to_borrow(&func_decl.type_.params, func_decl.body.as_ref());
    if let Some(body) = func_decl.body.as_ref() {
        validate_function_semantics(&func_decl.type_, body)?;
    }
    if is_pointer && !recv_name.is_empty() {
        borrow_pointer_params.insert(recv_name.clone());
    }
    let borrowed_pointer_param_names =
        BorrowedPointerParamNamesGuard::set(borrow_pointer_params.clone());
    let type_param_info = TypeParamInfo::default();
    let mut inputs = syn::punctuated::Punctuated::new();
    inputs.push(self_arg);
    for param in func_decl.type_.params.list {
        for arg in compile_field_to_fn_args_with_type_params(
            param,
            &type_param_info,
            &borrow_pointer_params,
        )? {
            inputs.push(arg);
        }
    }

    let vis: syn::Visibility = (&func_decl.name).into();
    let attrs = comment_group_to_attrs(&func_decl.doc);
    let mut named_return_info: Vec<(syn::Ident, Option<syn::Type>, syn::Expr)> = vec![];
    let mut named_return_idents: Vec<syn::Ident> = vec![];
    if let Some(ref results) = func_decl.type_.results {
        let has_named_returns = results
            .list
            .iter()
            .any(|f| f.names.as_ref().is_some_and(|names| !names.is_empty()));
        if has_named_returns {
            for field in &results.list {
                if let Some(ref names) = field.names {
                    for name in names {
                        let type_name = field.type_.as_ref().and_then(go_type_name_from_expr);
                        let rust_type = field.type_.as_ref().map(type_from_expr_ref);
                        let zero = zero_value_for_type(type_name);
                        let ident =
                            syn::Ident::new(&rust_safe_ident_name(name.name), Span::mixed_site());
                        named_return_info.push((ident.clone(), rust_type, zero));
                        named_return_idents.push(ident);
                    }
                }
            }
        }
    }

    let return_go_types = collect_return_go_types(func_decl.type_.results.as_ref());
    let previous_return_types =
        RETURN_TYPES.with(|types| std::mem::replace(&mut *types.borrow_mut(), return_go_types));
    let previous_named_return_idents = NAMED_RETURN_IDENTS
        .with(|idents| std::mem::replace(&mut *idents.borrow_mut(), named_return_idents.clone()));
    let body_shared_capture_names =
        func_decl
            .body
            .as_ref()
            .map_or_else(std::collections::BTreeSet::new, |body| {
                TYPE_ENV.with(|env| {
                    let env = env.borrow();
                    let mut names = ir::mutable_func_lit_capture_names_in_block(body, &env);
                    names.extend(ir::mutable_range_function_capture_names_in_block(
                        body, &env,
                    ));
                    names.extend(ir::for_clause_per_iteration_capture_names_in_block(
                        body, &env,
                    ));
                    names
                })
            });
    let _body_shared_capture_names = SharedCaptureNamesGuard::extend(body_shared_capture_names);
    let body_has_defer = func_decl.body.as_ref().is_some_and(block_has_defer);
    let block_result = if let Some(body) = func_decl.body {
        body.try_into()
    } else {
        Ok(syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: vec![],
        })
    };
    RETURN_TYPES.with(|types| {
        *types.borrow_mut() = previous_return_types;
    });
    NAMED_RETURN_IDENTS.with(|idents| {
        *idents.borrow_mut() = previous_named_return_idents;
    });
    let mut block = block_result?;
    drop(borrowed_pointer_param_names);

    if !recv_name.is_empty() {
        rewrite_receiver(&mut block, &recv_name);
    }
    if body_has_defer {
        prepend_defer_stack(&mut block);
    }

    // Handle named return values for methods (same logic as top-level functions)
    if !named_return_info.is_empty() {
        wrap_named_return_block(&mut block, &named_return_info, &named_return_idents);
    }

    let mut output = compile_return_type(func_decl.type_.results)?;
    add_elided_lifetime_to_borrowed_interface_return(&mut output, &inputs);

    let sig = syn::Signature {
        constness: None,
        asyncness: None,
        unsafety: None,
        abi: None,
        fn_token: <Token![fn]>::default(),
        ident: func_decl.name.into(),
        generics: syn::Generics::default(),
        paren_token: syn::token::Paren::default(),
        inputs,
        variadic: None,
        output,
    };

    Ok((
        type_name,
        type_args,
        syn::ImplItemFn {
            attrs,
            vis,
            defaultness: None,
            sig,
            block,
        },
    ))
}

fn extract_type_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::StarExpr(star) => extract_type_name(&star.x),
        ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
        ast::Expr::IndexExpr(index) => extract_type_name(&index.x),
        ast::Expr::IndexListExpr(index) => extract_type_name(&index.x),
        _ => None,
    }
}

fn is_general_type_conversion_fun(fun: &ast::Expr) -> bool {
    TYPE_ENV.with(|env| ir::is_general_type_conversion_fun(fun, &env.borrow()))
}

fn special_type_conversion_kind(
    call_expr: &ast::CallExpr,
) -> Option<ir::SpecialTypeConversionKind> {
    TYPE_ENV.with(|env| ir::special_type_conversion(call_expr, &env.borrow()))
}

fn expr_contains_unsafe_pointer_conversion(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::CallExpr(call) => {
            if matches!(
                &*call.fun,
                ast::Expr::SelectorExpr(sel)
                    if matches!(&*sel.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
                        && sel.sel.name == "Pointer"
            ) {
                return true;
            }
            expr_contains_unsafe_pointer_conversion(&call.fun)
                || call
                    .args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(expr_contains_unsafe_pointer_conversion))
        }
        ast::Expr::ParenExpr(paren) => expr_contains_unsafe_pointer_conversion(&paren.x),
        ast::Expr::SelectorExpr(selector) => expr_contains_unsafe_pointer_conversion(&selector.x),
        ast::Expr::UnaryExpr(unary) => expr_contains_unsafe_pointer_conversion(&unary.x),
        _ => false,
    }
}

fn compile_general_type_conversion(call_expr: ast::CallExpr) -> syn::Expr {
    let target_fun = *call_expr.fun;
    let target_fun = match target_fun {
        ast::Expr::ParenExpr(paren) => *paren.x,
        other => other,
    };
    let raw_arg = call_expr
        .args
        .unwrap_or_default()
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            ast::Expr::Ident(ast::Ident {
                name_pos: token::Position::default(),
                name: "__gors_missing_arg",
                obj: None,
            })
        });
    let is_unsafe_pointer_arg = expr_contains_unsafe_pointer_conversion(&raw_arg);
    let arg: syn::Expr = raw_arg.into();

    if matches!(&target_fun, ast::Expr::SelectorExpr(sel) if matches!(&*sel.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe") && sel.sel.name == "Pointer")
    {
        return syn::parse_quote! { 0usize };
    }
    if matches!(&target_fun, ast::Expr::Ident(id) if id.name == "any") {
        return syn::parse_quote! { Box::new(#arg) as Box<dyn std::any::Any> };
    }

    let target_ty = type_from_expr_ref(&target_fun);
    if matches!(target_fun, ast::Expr::StarExpr(_)) && is_unsafe_pointer_arg {
        return syn::parse_quote! { Default::default() };
    }
    if let Some(typeinfer::TypeKind::Alias(inner)) = type_kind_for_type_expr(&target_fun) {
        if inner.is_numeric()
            && let Some(inner_ty) = rust_type_from_go_type(&inner)
        {
            return syn::parse_quote! { #target_ty((#arg) as #inner_ty) };
        }
        if matches!(inner, typeinfer::GoType::Slice(_)) {
            return syn::parse_quote! { #target_ty::from(#arg) };
        }
        return syn::parse_quote! { #target_ty(#arg) };
    }
    if let Some(inner_ty) = arc_mutex_inner_type(&target_ty) {
        return syn::parse_quote! {
            std::sync::Arc::new(std::sync::Mutex::new(<#inner_ty>::default()))
        };
    }
    if let Some(inner_ty) = box_inner_type(&target_ty) {
        return syn::parse_quote! { Box::new(<#inner_ty>::default()) };
    }

    if matches!(target_ty, syn::Type::Tuple(ref tuple) if tuple.elems.is_empty()) {
        syn::parse_quote! { () }
    } else {
        syn::parse_quote! { ((#arg) as #target_ty) }
    }
}

fn compile_type_conversion(call_expr: ast::CallExpr, kind: &str) -> syn::Expr {
    let raw_arg = call_expr
        .args
        .unwrap_or_default()
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            ast::Expr::Ident(ast::Ident {
                name_pos: token::Position::default(),
                name: "__gors_missing_arg",
                obj: None,
            })
        });
    let is_int_arg = matches!(&raw_arg, ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT);
    let arg_go_type =
        typeinfer::GoType::infer_expr(&raw_arg, &TYPE_ENV.with(|e| e.borrow().clone()));
    let is_numeric_var = arg_go_type.is_integer();
    let arg: syn::Expr = raw_arg.into();
    match kind {
        "string" if is_int_arg || is_numeric_var => {
            syn::parse_quote! { char::from_u32(#arg as u32).map(String::from).unwrap_or_default() }
        }
        "string" if is_byte_seq_type_param(&arg_go_type) => {
            syn::parse_quote! { crate::builtin::string_from_byte_seq(&#arg) }
        }
        "string" => {
            syn::parse_quote! { crate::builtin::string(&#arg) }
        }
        "complex64" => syn::parse_quote! { crate::builtin::to_complex64(#arg) },
        "complex128" => syn::parse_quote! { crate::builtin::to_complex128(#arg) },
        "any" => syn::parse_quote! { Box::new(#arg) as Box<dyn std::any::Any> },
        "[]byte" => syn::parse_quote! { (#arg).as_bytes().to_vec() },
        "[]rune" => syn::parse_quote! { (#arg).chars().map(|ch| ch as i32).collect::<Vec<i32>>() },
        _ => compile_error_expr(format!("unsupported type conversion: {kind}")),
    }
}

fn unsafe_intrinsic_name<'ast>(call_expr: &ast::CallExpr<'ast>) -> Option<&'ast str> {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return None;
    };
    if matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe") {
        Some(selector.sel.name)
    } else {
        None
    }
}

fn unsafe_string_byte_source(expr: ast::Expr) -> Option<ast::Expr> {
    match expr {
        ast::Expr::ParenExpr(paren) => unsafe_string_byte_source(*paren.x),
        ast::Expr::CallExpr(call)
            if unsafe_intrinsic_name(&call) == Some("SliceData")
                && call.args.as_ref().is_some_and(|args| args.len() == 1) =>
        {
            call.args.and_then(|mut args| args.pop())
        }
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::AND => match *unary.x {
            ast::Expr::IndexExpr(index) => Some(*index.x),
            _ => None,
        },
        _ => None,
    }
}

fn compile_unsafe_intrinsic_call(call_expr: ast::CallExpr) -> syn::Expr {
    let is_string = unsafe_intrinsic_name(&call_expr) == Some("String");
    let is_slice_data = unsafe_intrinsic_name(&call_expr) == Some("SliceData");
    let args = call_expr.args.unwrap_or_default();
    match (is_string, is_slice_data, args.len()) {
        (true, false, 2) => {
            let mut args = args.into_iter();
            let Some(ptr) = args.next() else {
                return syn::parse_quote! { String::new() };
            };
            let Some(len) = args.next() else {
                return syn::parse_quote! { String::new() };
            };
            let source = unsafe_string_byte_source(ptr)
                .map(syn::Expr::from)
                .unwrap_or_else(|| syn::parse_quote! { Vec::<u8>::new() });
            let len: syn::Expr = len.into();
            syn::parse_quote! {
                String::from_utf8(
                    crate::builtin::byte_slice(&#source, 0usize, (#len) as usize)
                ).unwrap_or_default()
            }
        }
        (false, true, 1) => {
            let Some(source) = args.into_iter().next() else {
                return syn::parse_quote! { Vec::<u8>::new() };
            };
            let source: syn::Expr = source.into();
            syn::parse_quote! { #source }
        }
        _ => {
            if is_string {
                syn::parse_quote! { String::new() }
            } else {
                syn::parse_quote! { Vec::<u8>::new() }
            }
        }
    }
}

fn compile_unsafe_pointer_bitcast(expr: ast::Expr) -> Option<syn::Expr> {
    let ast::Expr::CallExpr(pointer_cast) = expr else {
        return None;
    };
    let target = pointer_type_target(*pointer_cast.fun)?;
    let mut pointer_args = pointer_cast.args?.into_iter();
    let unsafe_pointer_call = pointer_args.next()?;
    if pointer_args.next().is_some() {
        return None;
    }
    let ast::Expr::CallExpr(unsafe_pointer_call) = unsafe_pointer_call else {
        return None;
    };
    let ast::Expr::SelectorExpr(selector) = *unsafe_pointer_call.fun else {
        return None;
    };
    if !matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
        || selector.sel.name != "Pointer"
    {
        return None;
    }
    let mut unsafe_args = unsafe_pointer_call.args?.into_iter();
    let source = unsafe_args.next()?;
    if unsafe_args.next().is_some() {
        return None;
    }
    let ast::Expr::UnaryExpr(source) = source else {
        return None;
    };
    if source.op != token::Token::AND {
        return None;
    }
    let source: syn::Expr = (*source.x).into();
    let target_ty = type_from_expr_ref(&target);
    Some(syn::parse_quote! {
        crate::builtin::bitcast_ref::<_, #target_ty>(&#source)
    })
}

fn pointer_type_target(expr: ast::Expr) -> Option<ast::Expr> {
    match expr {
        ast::Expr::StarExpr(star) => Some(*star.x),
        ast::Expr::ParenExpr(paren) => pointer_type_target(*paren.x),
        _ => None,
    }
}

fn pointer_type_target_ref<'expr, 'ast>(
    expr: &'expr ast::Expr<'ast>,
) -> Option<&'expr ast::Expr<'ast>> {
    match expr {
        ast::Expr::StarExpr(star) => Some(&star.x),
        ast::Expr::ParenExpr(paren) => pointer_type_target_ref(&paren.x),
        _ => None,
    }
}

fn is_unsafe_pointer_bitcast_expr(expr: &ast::Expr) -> bool {
    let ast::Expr::CallExpr(pointer_cast) = expr else {
        return false;
    };
    if pointer_type_target_ref(&pointer_cast.fun).is_none() {
        return false;
    }
    let Some(args) = &pointer_cast.args else {
        return false;
    };
    let [unsafe_pointer_call] = args.as_slice() else {
        return false;
    };
    let ast::Expr::CallExpr(unsafe_pointer_call) = unsafe_pointer_call else {
        return false;
    };
    let ast::Expr::SelectorExpr(selector) = &*unsafe_pointer_call.fun else {
        return false;
    };
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
        && selector.sel.name == "Pointer"
}

fn rust_type_path_from_segments<'a>(
    path_segments: impl IntoIterator<Item = &'a str>,
    safe_idents: bool,
) -> syn::Type {
    let mut segments = syn::punctuated::Punctuated::new();
    for segment in path_segments {
        let ident = if safe_idents {
            syn::Ident::new(&rust_safe_ident_name(segment), Span::mixed_site())
        } else {
            syn::Ident::new(segment, Span::mixed_site())
        };
        segments.push(syn::PathSegment {
            ident,
            arguments: syn::PathArguments::None,
        });
    }
    syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
            leading_colon: None,
            segments,
        },
    })
}

fn rust_type_path_from_rust_segments<'a>(
    path_segments: impl IntoIterator<Item = &'a str>,
) -> syn::Type {
    rust_type_path_from_segments(path_segments, false)
}

fn rust_type_from_go_type(go_type: &typeinfer::GoType) -> Option<syn::Type> {
    match go_type {
        typeinfer::GoType::Bool => Some(rust_type_path_from_rust_segments(["bool"])),
        typeinfer::GoType::Int => Some(rust_type_path_from_rust_segments(["isize"])),
        typeinfer::GoType::Int8 => Some(rust_type_path_from_rust_segments(["i8"])),
        typeinfer::GoType::Int16 => Some(rust_type_path_from_rust_segments(["i16"])),
        typeinfer::GoType::Int32 => Some(rust_type_path_from_rust_segments(["i32"])),
        typeinfer::GoType::Int64 => Some(rust_type_path_from_rust_segments(["i64"])),
        typeinfer::GoType::Uint => Some(rust_type_path_from_rust_segments(["usize"])),
        typeinfer::GoType::Uint8 => Some(rust_type_path_from_rust_segments(["u8"])),
        typeinfer::GoType::Uint16 => Some(rust_type_path_from_rust_segments(["u16"])),
        typeinfer::GoType::Uint32 => Some(rust_type_path_from_rust_segments(["u32"])),
        typeinfer::GoType::Uint64 => Some(rust_type_path_from_rust_segments(["u64"])),
        typeinfer::GoType::Uintptr => Some(rust_type_path_from_rust_segments(["usize"])),
        typeinfer::GoType::Float32 => Some(rust_type_path_from_rust_segments(["f32"])),
        typeinfer::GoType::Float64 => Some(rust_type_path_from_rust_segments(["f64"])),
        typeinfer::GoType::Complex64 => Some(rust_type_path_from_rust_segments([
            "crate",
            "builtin",
            "Complex64",
        ])),
        typeinfer::GoType::Complex128 => Some(rust_type_path_from_rust_segments([
            "crate",
            "builtin",
            "Complex128",
        ])),
        _ => None,
    }
}

fn named_go_type_path(name: &str) -> syn::Type {
    rust_type_path_from_segments(name.split('.'), true)
}

fn rust_type_from_inferred_go_type(go_type: &typeinfer::GoType) -> syn::Type {
    let resolved = resolved_go_type(go_type);
    if let Some(ty) = rust_type_from_go_type(&resolved) {
        return ty;
    }
    match resolved {
        typeinfer::GoType::String => syn::parse_quote! { String },
        typeinfer::GoType::Slice(elem) | typeinfer::GoType::Array(elem) => {
            let elem = rust_type_from_inferred_go_type(&elem);
            syn::parse_quote! { Vec<#elem> }
        }
        typeinfer::GoType::Map(key, value) => {
            let key = rust_type_from_inferred_go_type(&key);
            let value = rust_type_from_inferred_go_type(&value);
            syn::parse_quote! { std::collections::HashMap<#key, #value> }
        }
        typeinfer::GoType::Pointer(inner) => {
            let inner = rust_type_from_inferred_go_type(&inner);
            syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#inner>> }
        }
        typeinfer::GoType::Chan { elem, .. } => {
            let inner = rust_type_from_inferred_go_type(&elem);
            syn::parse_quote! { crate::builtin::Chan<#inner> }
        }
        typeinfer::GoType::Func {
            params, results, ..
        } => shared_func_type_from_go_parts(&params, &results),
        typeinfer::GoType::Named(name) => named_go_type_path(&name),
        typeinfer::GoType::Any | typeinfer::GoType::Interface(_) => {
            syn::parse_quote! { Box<dyn std::any::Any> }
        }
        typeinfer::GoType::Error => syn::parse_quote! { String },
        typeinfer::GoType::Unknown => syn::parse_quote! { Box<dyn std::any::Any> },
        _ => syn::parse_quote! { Box<dyn std::any::Any> },
    }
}

fn shared_func_type_from_go_parts(
    params: &[typeinfer::GoType],
    results: &[typeinfer::GoType],
) -> syn::Type {
    let box_ty = shared_func_box_type_from_go_parts(params, results);
    syn::parse_quote! { std::sync::Arc<std::sync::Mutex<Option<#box_ty>>> }
}

fn shared_func_box_type_from_go_parts(
    params: &[typeinfer::GoType],
    results: &[typeinfer::GoType],
) -> syn::Type {
    let params = params.iter().map(rust_type_from_inferred_go_type);
    let result_types: Vec<syn::Type> = results
        .iter()
        .map(rust_type_from_inferred_go_type)
        .collect();
    let result: syn::Type = match result_types.as_slice() {
        [] => syn::parse_quote! { () },
        [ty] => ty.clone(),
        _ => syn::parse_quote! { (#(#result_types),*) },
    };
    syn::parse_quote! { std::sync::Arc<dyn Fn(#(#params),*) -> #result + Send + Sync> }
}

fn shared_func_type_from_go_type(go_type: &typeinfer::GoType) -> Option<syn::Type> {
    match resolved_go_type(go_type) {
        typeinfer::GoType::Func {
            params, results, ..
        } => Some(shared_func_type_from_go_parts(&params, &results)),
        _ => None,
    }
}

fn shared_func_box_type_from_go_type(go_type: &typeinfer::GoType) -> Option<syn::Type> {
    match resolved_go_type(go_type) {
        typeinfer::GoType::Func {
            params, results, ..
        } => Some(shared_func_box_type_from_go_parts(&params, &results)),
        _ => None,
    }
}

fn is_builtin_call(call_expr: &ast::CallExpr) -> bool {
    builtin_call_kind(call_expr).is_some()
}

fn builtin_call_kind(call_expr: &ast::CallExpr) -> Option<ir::BuiltinCallKind> {
    let ast::Expr::Ident(ident) = call_expr.fun.as_ref() else {
        return None;
    };
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        if env.get_var(ident.name).is_some()
            || env.has_func(ident.name)
            || env.get_type_kind(ident.name).is_some()
        {
            return None;
        }
        ir::builtin_call_kind(call_expr)
    })
}

fn is_variadic_call(call_expr: &ast::CallExpr) -> Option<usize> {
    TYPE_ENV.with(|env| ir::variadic_call_start(call_expr, &env.borrow()))
}

enum VariadicCallTarget {
    Function(syn::Expr),
    Method {
        receiver: syn::Expr,
        method: syn::Ident,
    },
}

impl VariadicCallTarget {
    fn call(self, args: syn::punctuated::Punctuated<syn::Expr, Token![,]>) -> syn::Expr {
        match self {
            Self::Function(fun) => syn::parse_quote! { #fun(#args) },
            Self::Method { receiver, method } => syn::Expr::MethodCall(syn::ExprMethodCall {
                attrs: vec![],
                receiver: Box::new(receiver),
                dot_token: <Token![.]>::default(),
                method,
                turbofish: None,
                paren_token: syn::token::Paren::default(),
                args,
            }),
        }
    }
}

fn selector_base_is_import(selector: &ast::SelectorExpr) -> bool {
    matches!(
        selector.x.as_ref(),
        ast::Expr::Ident(id) if IMPORT_NAMES.with(|names| names.borrow().contains(id.name))
    )
}

fn compile_variadic_call_target(fun: ast::Expr) -> VariadicCallTarget {
    match fun {
        ast::Expr::Ident(ident) => VariadicCallTarget::Function(syn::Expr::Path(ident.into())),
        ast::Expr::SelectorExpr(selector) if selector_base_is_import(&selector) => {
            VariadicCallTarget::Function(syn::Expr::Path(selector.into()))
        }
        ast::Expr::SelectorExpr(selector) => VariadicCallTarget::Method {
            receiver: method_receiver_expr_from_ref(*selector.x),
            method: selector.sel.into(),
        },
        _ => VariadicCallTarget::Function(compile_error_expr("unsupported variadic call target")),
    }
}

fn compile_variadic_call(call_expr: ast::CallExpr, variadic_start: usize) -> syn::Expr {
    let param_types = call_param_types(&call_expr.fun);
    let target = compile_variadic_call_target(*call_expr.fun);

    let variadic_elem = param_types.get(variadic_start).and_then(|ty| match ty {
        typeinfer::GoType::Slice(inner) => Some((**inner).clone()),
        _ => None,
    });
    let variadic_is_any = matches!(
        variadic_elem.as_ref().map(resolved_go_type),
        Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_))
    );
    let raw_args: Vec<ast::Expr> = call_expr.args.unwrap_or_default().into_iter().collect();
    let has_variadic_spread = call_expr.ellipsis.is_some();

    let mut final_args: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> =
        syn::punctuated::Punctuated::new();

    if has_variadic_spread {
        for (i, arg) in raw_args.into_iter().enumerate() {
            let should_clone =
                i >= variadic_start && !variadic_is_any && is_ir_addressable_expr(&arg) && {
                    let actual =
                        TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
                    !go_type_is_copy(&actual)
                };
            let arg = compile_expr_with_expected(arg, param_types.get(i));
            if should_clone {
                final_args.push(syn::parse_quote! { (#arg).clone() });
            } else {
                final_args.push(arg);
            }
        }
        return target.call(final_args);
    }

    for (i, arg) in raw_args.into_iter().enumerate() {
        if i < variadic_start {
            final_args.push(compile_expr_with_expected(arg, param_types.get(i)));
        } else if variadic_is_any {
            let arg = compile_variadic_any_arg(arg, variadic_elem.as_ref());
            final_args
                .push(syn::parse_quote! { Box::new((#arg).clone()) as Box<dyn std::any::Any> });
        } else {
            final_args.push(compile_expr_with_expected(arg, variadic_elem.as_ref()));
        }
    }

    let variadic_args: Vec<&syn::Expr> = final_args.iter().skip(variadic_start).collect();
    let fixed_args: Vec<&syn::Expr> = final_args.iter().take(variadic_start).collect();

    let vec_expr: syn::Expr = if variadic_args.is_empty() && variadic_is_any {
        syn::parse_quote! { Vec::<Box<dyn std::any::Any>>::new() }
    } else if variadic_args.is_empty() {
        syn::parse_quote! { Vec::new() }
    } else {
        syn::parse_quote! { Vec::from([#(#variadic_args),*]) }
    };

    let mut call_args: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> =
        syn::punctuated::Punctuated::new();
    for arg in fixed_args {
        call_args.push(arg.clone());
    }
    call_args.push(vec_expr);

    target.call(call_args)
}

fn compile_variadic_any_arg(
    arg: ast::Expr,
    variadic_elem: Option<&typeinfer::GoType>,
) -> syn::Expr {
    let inferred_type = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
    if let typeinfer::GoType::Named(name) = &inferred_type {
        let returns = get_func_returns(&format!("{name}.String"));
        if matches!(returns.first(), Some(typeinfer::GoType::String)) {
            let expr: syn::Expr = arg.into();
            return syn::parse_quote! { (#expr).String() };
        }
    }
    if matches!(
        resolved_go_type(&inferred_type),
        typeinfer::GoType::Slice(elem) if *elem != typeinfer::GoType::Uint8
    ) {
        let expr = compile_expr_with_expected(arg, None);
        return syn::parse_quote! { crate::builtin::format_slice(&#expr) };
    }

    match &arg {
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::STRING => {
            let expr = compile_expr_with_expected(arg, Some(&typeinfer::GoType::String));
            syn::parse_quote! { #expr }
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            let expr: syn::Expr = arg.into();
            syn::parse_quote! { (#expr as isize) }
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::FLOAT => {
            let expr: syn::Expr = arg.into();
            syn::parse_quote! { (#expr as f64) }
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::CHAR => {
            let expr: syn::Expr = arg.into();
            syn::parse_quote! { (#expr as i32) }
        }
        ast::Expr::Ident(id) if id.name == "true" || id.name == "false" => arg.into(),
        _ if matches!(
            variadic_elem.map(resolved_go_type),
            Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_))
        ) =>
        {
            compile_expr_with_expected(arg, None)
        }
        _ => compile_expr_with_expected(arg, variadic_elem),
    }
}

fn compile_new_builtin(raw_args: Vec<ast::Expr>) -> syn::Expr {
    let Some(arg) = raw_args.into_iter().next() else {
        return compile_error_expr("new requires an argument");
    };
    let kind = TYPE_ENV.with(|env| ir::new_arg_kind(&arg, &env.borrow()));
    if matches!(kind, ir::NewArgKind::Type | ir::NewArgKind::Unknown) {
        let type_arg: syn::Type = arg.into();
        return syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(<#type_arg>::default())) };
    }

    let inferred = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
    let value_type = rust_type_from_inferred_go_type(&inferred);
    let value = compile_expr_with_expected(arg, Some(&inferred));
    syn::parse_quote! {
        {
            let __gors_new_value: #value_type = #value;
            std::sync::Arc::new(std::sync::Mutex::new(__gors_new_value))
        }
    }
}

fn compile_builtin(call_expr: ast::CallExpr) -> syn::Expr {
    let Some(kind) = builtin_call_kind(&call_expr) else {
        return compile_error_expr("builtin call without builtin identifier");
    };
    let name = kind.name();

    let has_variadic_spread = call_expr.ellipsis.is_some();
    let raw_args: Vec<ast::Expr> = call_expr.args.unwrap_or_default().into_iter().collect();

    match kind {
        ir::BuiltinCallKind::Make => {
            let mut it = raw_args.into_iter();
            let Some(type_arg) = it.next() else {
                return compile_error_expr("make requires a type argument");
            };
            let remaining: Vec<syn::Expr> = it.map(syn::Expr::from).collect();
            match type_arg {
                ast::Expr::ArrayType(arr) => {
                    let elem_type: syn::Type = (*arr.elt).into();
                    match remaining.as_slice() {
                        [] => syn::parse_quote! { Vec::<#elem_type>::new() },
                        [size] => {
                            syn::parse_quote! { crate::builtin::make_vec::<#elem_type>((#size) as usize) }
                        }
                        [size, cap_arg, ..] => {
                            syn::parse_quote! { { let mut v = Vec::<#elem_type>::with_capacity((#cap_arg) as usize); v.resize_with((#size) as usize, Default::default); v } }
                        }
                    }
                }
                ast::Expr::MapType(map) => {
                    let key_type: syn::Type = (*map.key).into();
                    let val_type: syn::Type = (*map.value).into();
                    match remaining.as_slice() {
                        [] => {
                            syn::parse_quote! { std::collections::HashMap::<#key_type, #val_type>::new() }
                        }
                        [cap_arg, ..] => {
                            syn::parse_quote! { std::collections::HashMap::<#key_type, #val_type>::with_capacity((#cap_arg) as usize) }
                        }
                    }
                }
                ast::Expr::ChanType(_) => match remaining.as_slice() {
                    [] => syn::parse_quote! { crate::builtin::make_chan(0) },
                    [cap_arg, ..] => syn::parse_quote! { crate::builtin::make_chan(#cap_arg) },
                },
                _ => {
                    syn::parse_quote! { Default::default() }
                }
            }
        }
        ir::BuiltinCallKind::New => compile_new_builtin(raw_args),
        ir::BuiltinCallKind::Append => compile_append_builtin(raw_args, has_variadic_spread),
        ir::BuiltinCallKind::Panic => compile_panic_builtin(raw_args),
        ir::BuiltinCallKind::Copy if raw_args.len() == 2 => {
            let mut raw_args = raw_args.into_iter();
            let Some(dst_raw) = raw_args.next() else {
                return compile_error_expr("copy requires a destination argument");
            };
            let Some(src_raw) = raw_args.next() else {
                return compile_error_expr("copy requires a source argument");
            };
            let src_ty =
                TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&src_raw, &env.borrow()));
            let dst = compile_expr_with_expected(dst_raw, None);
            let src = compile_expr_with_expected(src_raw, None);
            if matches!(resolved_go_type(&src_ty), typeinfer::GoType::String) {
                syn::parse_quote! { crate::builtin::copy_slice(&mut #dst, (#src).as_bytes()) }
            } else {
                syn::parse_quote! { crate::builtin::copy_slice(&mut #dst, &#src) }
            }
        }
        ir::BuiltinCallKind::Delete if raw_args.len() == 2 => {
            let mut raw_args = raw_args.into_iter();
            let Some(map_raw) = raw_args.next() else {
                return compile_error_expr("delete requires a map argument");
            };
            let Some(key_raw) = raw_args.next() else {
                return compile_error_expr("delete requires a key argument");
            };
            let key_ty = TYPE_ENV.with(|env| {
                let env = env.borrow();
                match env.resolve_alias(&typeinfer::GoType::infer_expr(&map_raw, &env)) {
                    typeinfer::GoType::Map(key, _) => Some(*key),
                    _ => None,
                }
            });
            let map = compile_expr_with_expected(map_raw, None);
            let key = compile_expr_with_expected(key_raw, key_ty.as_ref());
            syn::parse_quote! {{
                let __gors_delete_key = #key;
                crate::builtin::delete(&mut #map, &__gors_delete_key)
            }}
        }
        _ => {
            let ordered_expected_type = match kind {
                ir::BuiltinCallKind::Max | ir::BuiltinCallKind::Min => {
                    ordered_builtin_arg_expected_type(&raw_args)
                }
                ir::BuiltinCallKind::Complex => Some(typeinfer::GoType::Float64),
                _ => None,
            };
            let args: Vec<syn::Expr> = raw_args
                .into_iter()
                .map(|arg| compile_expr_with_expected(arg, ordered_expected_type.as_ref()))
                .collect();
            match kind {
                ir::BuiltinCallKind::Len if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::len(&#x) }
                }
                ir::BuiltinCallKind::Cap if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::cap(&#x) }
                }
                ir::BuiltinCallKind::Clear if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::clear(&mut #x) }
                }
                ir::BuiltinCallKind::Close if let [ch] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::close(&#ch) }
                }
                ir::BuiltinCallKind::Max if let [a, b] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::max(#a, #b) }
                }
                ir::BuiltinCallKind::Max if let [a, b, c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::max3(#a, #b, #c) }
                }
                ir::BuiltinCallKind::Min if let [a, b] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::min(#a, #b) }
                }
                ir::BuiltinCallKind::Min if let [a, b, c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::min3(#a, #b, #c) }
                }
                ir::BuiltinCallKind::Complex if let [re, im] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::complex128(#re, #im) }
                }
                ir::BuiltinCallKind::Real if let [c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::real(#c) }
                }
                ir::BuiltinCallKind::Imag if let [c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::imag(#c) }
                }
                ir::BuiltinCallKind::Recover => {
                    syn::parse_quote! { String::new() }
                }
                ir::BuiltinCallKind::Println => compile_builtin_println(args),
                ir::BuiltinCallKind::Print => compile_builtin_print(args),
                _ => compile_error_expr(format!("invalid builtin call: {name}")),
            }
        }
    }
}

fn compile_builtin_println(args: Vec<syn::Expr>) -> syn::Expr {
    if args.is_empty() {
        return syn::parse_quote! { crate::builtin::println_empty() };
    }
    if args.len() == 1 {
        let arg = args
            .into_iter()
            .next()
            .unwrap_or_else(|| syn::parse_quote! { "" });
        return syn::parse_quote! { crate::builtin::println_value(#arg) };
    }

    let len = args.len();
    let mut stmts = Vec::<syn::Stmt>::new();
    for (idx, arg) in args.into_iter().enumerate() {
        if idx + 1 == len {
            stmts.push(syn::parse_quote! { crate::builtin::println_value(#arg); });
        } else {
            stmts.push(syn::parse_quote! { crate::builtin::print_value(#arg); });
            stmts.push(syn::parse_quote! { crate::builtin::print_value(" "); });
        }
    }
    syn::parse_quote! {{ #(#stmts)* }}
}

fn compile_builtin_print(args: Vec<syn::Expr>) -> syn::Expr {
    if args.is_empty() {
        return syn::parse_quote! { crate::builtin::print_empty() };
    }
    if args.len() == 1 {
        let arg = args
            .into_iter()
            .next()
            .unwrap_or_else(|| syn::parse_quote! { "" });
        return syn::parse_quote! { crate::builtin::print_value(#arg) };
    }

    let stmts = args
        .into_iter()
        .map(|arg| syn::parse_quote! { crate::builtin::print_value(#arg); })
        .collect::<Vec<syn::Stmt>>();
    syn::parse_quote! {{ #(#stmts)* }}
}

fn compile_sort_slice_call(call_expr: ast::CallExpr) -> Option<syn::Expr> {
    let ast::Expr::SelectorExpr(selector) = *call_expr.fun else {
        return None;
    };
    if !matches!(*selector.x, ast::Expr::Ident(pkg) if pkg.name == "sort") {
        return None;
    }
    if !matches!(selector.sel.name, "Slice" | "SliceStable" | "SliceIsSorted") {
        return None;
    }

    let mut args = call_expr.args.unwrap_or_default().into_iter();
    let ast::Expr::Ident(slice_ident) = args.next()? else {
        return None;
    };
    let less_arg = args.next()?;
    if args.next().is_some() {
        return None;
    }

    let slice_ident = syn::Ident::new(&rust_safe_ident_name(slice_ident.name), Span::mixed_site());
    let less: syn::Expr = less_arg.into();
    match selector.sel.name {
        "Slice" | "SliceStable" => Some(syn::parse_quote! {{
            let mut __gors_less = #less;
            let __gors_len = #slice_ident.len();
            for __gors_i in 0..__gors_len {
                for __gors_j in (__gors_i + 1)..__gors_len {
                    if __gors_less(__gors_j as isize, __gors_i as isize) {
                        #slice_ident.swap(__gors_i, __gors_j);
                    }
                }
            }
        }}),
        "SliceIsSorted" => Some(syn::parse_quote! {{
            let mut __gors_less = #less;
            let mut __gors_sorted = true;
            let __gors_len = #slice_ident.len();
            let mut __gors_i = 1usize;
            while __gors_i < __gors_len {
                if __gors_less(__gors_i as isize, (__gors_i - 1) as isize) {
                    __gors_sorted = false;
                    break;
                }
                __gors_i += 1;
            }
            __gors_sorted
        }}),
        _ => None,
    }
}

fn is_sort_slice_call(call_expr: &ast::CallExpr) -> bool {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return false;
    };
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "sort")
        && matches!(selector.sel.name, "Slice" | "SliceStable" | "SliceIsSorted")
}

fn compile_append_float_call(call_expr: ast::CallExpr) -> Option<syn::Expr> {
    let ast::Expr::SelectorExpr(selector) = *call_expr.fun else {
        return None;
    };
    if !matches!(*selector.x, ast::Expr::Ident(pkg) if pkg.name == "strconv") {
        return None;
    }
    if selector.sel.name != "AppendFloat" {
        return None;
    }
    let args = call_expr.args.unwrap_or_default();
    if args.len() != 5 {
        return None;
    }
    let mut args = args.into_iter();
    let dst = compile_expr_with_expected(args.next()?, None);
    let value = compile_expr_with_expected(args.next()?, Some(&typeinfer::GoType::Float64));
    let fmt = compile_expr_with_expected(args.next()?, Some(&typeinfer::GoType::Uint8));
    let prec = compile_expr_with_expected(args.next()?, Some(&typeinfer::GoType::Int));
    let bit_size = compile_expr_with_expected(args.next()?, Some(&typeinfer::GoType::Int));
    Some(syn::parse_quote! {
        crate::builtin::append_float(#dst, #value, (#fmt) as u8, #prec, #bit_size)
    })
}

fn is_append_float_call(call_expr: &ast::CallExpr) -> bool {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return false;
    };
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "strconv")
        && selector.sel.name == "AppendFloat"
}

fn ordered_builtin_arg_expected_type(raw_args: &[ast::Expr]) -> Option<typeinfer::GoType> {
    let all_string = TYPE_ENV.with(|env| {
        let env = env.borrow();
        !raw_args.is_empty()
            && raw_args.iter().all(|arg| {
                matches!(
                    env.resolve_alias(&typeinfer::GoType::infer_expr(arg, &env)),
                    typeinfer::GoType::String
                )
            })
    });
    all_string.then_some(typeinfer::GoType::String)
}

fn compile_append_builtin(raw_args: Vec<ast::Expr>, has_variadic_spread: bool) -> syn::Expr {
    let mut args = raw_args.into_iter();
    let Some(slice_arg) = args.next() else {
        return compile_error_expr("append requires a slice argument");
    };

    let slice_go_type = TYPE_ENV.with(|env| {
        let env = env.borrow();
        env.resolve_alias(&typeinfer::GoType::infer_expr(&slice_arg, &env))
    });
    let elem_go_type = match &slice_go_type {
        typeinfer::GoType::Slice(elem) | typeinfer::GoType::Array(elem) => Some((**elem).clone()),
        _ => None,
    };

    let mut out = compile_expr_with_expected(slice_arg, None);
    let remaining: Vec<_> = args.collect();
    if remaining.is_empty() {
        return out;
    }

    if has_variadic_spread {
        if remaining.len() != 1 {
            return compile_error_expr("append spread requires exactly one variadic argument");
        }
        let elem = compile_expr_with_expected(
            remaining.into_iter().next().unwrap_or_else(|| {
                ast::Expr::Ident(ast::Ident {
                    name_pos: token::Position::default(),
                    name: "__gors_missing_append_arg",
                    obj: None,
                })
            }),
            Some(&slice_go_type),
        );
        return syn::parse_quote! { crate::builtin::append(#out, #elem) };
    }

    for elem_arg in remaining {
        let elem = compile_expr_with_expected(elem_arg, elem_go_type.as_ref());
        out = syn::parse_quote! { crate::builtin::append(#out, #elem) };
    }
    out
}

fn compile_panic_builtin(raw_args: Vec<ast::Expr>) -> syn::Expr {
    let Some(arg) = raw_args.into_iter().next() else {
        return syn::parse_quote! { std::panic::panic_any(()) };
    };
    let arg = compile_variadic_any_arg(arg, Some(&typeinfer::GoType::Any));
    syn::parse_quote! { std::panic::panic_any(#arg) }
}

fn compile_struct_field_expr(expr: ast::Expr, expected: &typeinfer::GoType) -> syn::Expr {
    let is_function_field_selector =
        matches!(expr, ast::Expr::SelectorExpr(_)) && function_field_call_params(&expr).is_some();
    if let ast::Expr::FuncLit(func_lit) = expr {
        let compiled = compile_func_lit_with_capture_mode(func_lit, true);
        return shared_func_value_expr(expected, compiled.clone()).unwrap_or(compiled);
    }
    let compiled = compile_expr_with_expected(expr, Some(expected));
    if is_function_field_selector {
        return compiled;
    }
    if let Some(expr) = shared_func_value_expr(expected, compiled.clone()) {
        return expr;
    }
    compiled
}

fn shared_func_value_expr(expected: &typeinfer::GoType, compiled: syn::Expr) -> Option<syn::Expr> {
    let func_ty = shared_func_box_type_from_go_type(expected)?;
    Some(syn::parse_quote! {
        {
            let __gors_func: #func_ty = std::sync::Arc::new(#compiled);
            std::sync::Arc::new(std::sync::Mutex::new(Some(__gors_func)))
        }
    })
}

fn raw_elts_to_field_values(type_name: Option<&str>, elts: Vec<ast::Expr>) -> Vec<syn::FieldValue> {
    let struct_fields = type_name
        .map(|name| TYPE_ENV.with(|env| env.borrow().get_struct_fields(name)))
        .unwrap_or_default();
    let mut positional_index = 0usize;

    elts.into_iter()
        .map(|elt| {
            let elt = if let ast::Expr::KeyValueExpr(kv) = elt {
                let field_name = match &*kv.key {
                    ast::Expr::Ident(ident) => Some(ident.name.to_string()),
                    _ => None,
                };
                if let Some(field_name) = field_name {
                    let expected = struct_fields
                        .iter()
                        .find(|(name, _)| name == &field_name)
                        .map(|(_, ty)| ty.clone());
                    let expr = if let Some(expected) = expected.as_ref() {
                        compile_struct_field_expr(*kv.value, expected)
                    } else {
                        compile_expr_with_expected(*kv.value, None)
                    };
                    return syn::FieldValue {
                        attrs: vec![],
                        member: syn::Member::Named(syn::Ident::new(
                            &rust_safe_ident_name(&field_name),
                            Span::mixed_site(),
                        )),
                        colon_token: Some(<Token![:]>::default()),
                        expr,
                    };
                }
                ast::Expr::KeyValueExpr(kv)
            } else {
                elt
            };

            if let Some((field_name, field_ty)) = struct_fields.get(positional_index) {
                positional_index += 1;
                let field_name = rust_safe_ident_name(field_name);
                return syn::FieldValue {
                    attrs: vec![],
                    member: syn::Member::Named(syn::Ident::new(&field_name, Span::mixed_site())),
                    colon_token: Some(<Token![:]>::default()),
                    expr: compile_struct_field_expr(elt, field_ty),
                };
            }
            let index = positional_index as u32;
            positional_index += 1;
            syn::FieldValue {
                attrs: vec![],
                member: syn::Member::Unnamed(syn::Index {
                    index,
                    span: Span::mixed_site(),
                }),
                colon_token: None,
                expr: elt.into(),
            }
        })
        .collect()
}

fn compile_raw_elts(elts: Vec<ast::Expr>) -> Vec<syn::Expr> {
    elts.into_iter().map(syn::Expr::from).collect()
}

fn has_unnamed_field_values(field_values: &[syn::FieldValue]) -> bool {
    field_values
        .iter()
        .any(|fv| matches!(fv.member, syn::Member::Unnamed(_)))
}

fn compile_array_literal_element(
    elt: ast::Expr,
    elem_type_expr: &ast::Expr,
    elem_go_type: &typeinfer::GoType,
) -> syn::Expr {
    match (elt, elem_type_expr) {
        (ast::Expr::CompositeLit(nested), ast::Expr::ArrayType(nested_array))
            if nested.type_.is_none() =>
        {
            compile_array_literal(nested_array, nested.elts.unwrap_or_default())
        }
        (elt, _) => compile_expr_with_expected(elt, Some(elem_go_type)),
    }
}

fn compile_array_literal(array_type: &ast::ArrayType, raw_elts: Vec<ast::Expr>) -> syn::Expr {
    let elem_go_type = typeinfer::GoType::from_expr(&array_type.elt);
    let fixed_len = array_type
        .len
        .as_ref()
        .map(|array_len| array_literal_len_expr(array_len, &raw_elts));
    let mut indexed_elts: Vec<(proc_macro2::TokenStream, syn::Expr)> = vec![];
    for (default_index, elt) in raw_elts.into_iter().enumerate() {
        if let ast::Expr::KeyValueExpr(kv) = elt {
            let key: syn::Expr = (*kv.key).into();
            let value = compile_array_literal_element(*kv.value, &array_type.elt, &elem_go_type);
            indexed_elts.push((quote::quote! { (#key) as usize }, value));
        } else {
            let index = syn::Index::from(default_index);
            let value = compile_array_literal_element(elt, &array_type.elt, &elem_go_type);
            indexed_elts.push((quote::quote! { #index }, value));
        }
    }
    if array_type.len.is_none() {
        if indexed_elts.is_empty() {
            syn::parse_quote! { Vec::new() }
        } else {
            let elts = indexed_elts.into_iter().map(|(_, elt)| elt);
            syn::parse_quote! { Vec::from([#(#elts),*]) }
        }
    } else if indexed_elts.is_empty() {
        let Some(len) = fixed_len else {
            return syn::parse_quote! { Default::default() };
        };
        syn::parse_quote! {{
            let __gors_array: [_; #len] = std::array::from_fn(|_| Default::default());
            __gors_array
        }}
    } else {
        let Some(len) = fixed_len else {
            return syn::parse_quote! { Default::default() };
        };
        let (indices, elts): (Vec<_>, Vec<_>) = indexed_elts.into_iter().unzip();
        syn::parse_quote! {{
            let mut __gors_array: [_; #len] = std::array::from_fn(|_| Default::default());
            #(
                __gors_array[#indices] = #elts;
            )*
            __gors_array
        }}
    }
}

fn compile_map_literal(map_type: &ast::MapType, raw_elts: Vec<ast::Expr>) -> syn::Expr {
    let key_go_type = typeinfer::GoType::from_expr(&map_type.key);
    let value_go_type = typeinfer::GoType::from_expr(&map_type.value);
    let elts = raw_elts
        .into_iter()
        .filter_map(|elt| {
            let ast::Expr::KeyValueExpr(kv) = elt else {
                return None;
            };
            let key = compile_expr_with_expected(*kv.key, Some(&key_go_type));
            let value = compile_expr_with_expected(*kv.value, Some(&value_go_type));
            Some(syn::parse_quote! { (#key, #value) })
        })
        .collect::<Vec<syn::Expr>>();
    syn::parse_quote! {
        std::collections::HashMap::from([#(#elts),*])
    }
}

fn compile_composite_lit(comp_lit: ast::CompositeLit) -> syn::Expr {
    let raw_elts = comp_lit.elts.unwrap_or_default();

    if let Some(type_expr) = comp_lit.type_ {
        match *type_expr {
            ast::Expr::Ident(ident) => {
                let type_ident: syn::Ident = ident.into();
                let type_name = type_ident.to_string();
                let type_kind =
                    TYPE_ENV.with(|env| env.borrow().get_type_kind(&type_name).cloned());
                if let Some(typeinfer::TypeKind::Alias(alias_type)) = type_kind {
                    if let typeinfer::GoType::Array(elem_type) = alias_type {
                        let elts = raw_elts
                            .into_iter()
                            .map(|elt| compile_expr_with_expected(elt, Some(&elem_type)))
                            .collect::<Vec<_>>();
                        return syn::parse_quote! { #type_ident([#(#elts),*]) };
                    }
                    if let typeinfer::GoType::Slice(elem_type) = alias_type {
                        let elts = raw_elts
                            .into_iter()
                            .map(|elt| compile_expr_with_expected(elt, Some(&elem_type)))
                            .collect::<Vec<_>>();
                        return syn::parse_quote! { #type_ident(Vec::from([#(#elts),*])) };
                    }
                    let elts = compile_raw_elts(raw_elts);
                    if elts.is_empty() || elts.iter().any(|elt| matches!(elt, syn::Expr::Tuple(_)))
                    {
                        return syn::parse_quote! { #type_ident::default() };
                    }
                    return syn::parse_quote! { #type_ident(#(#elts),*) };
                }
                let field_values = raw_elts_to_field_values(Some(&type_name), raw_elts);
                if field_values.is_empty() || has_unnamed_field_values(&field_values) {
                    syn::parse_quote! { #type_ident::default() }
                } else {
                    let all_struct_fields_set = TYPE_ENV.with(|env| {
                        let count = env.borrow().get_struct_fields(&type_name).len();
                        count > 0 && field_values.len() == count
                    });
                    let mut fields = syn::punctuated::Punctuated::new();
                    for fv in field_values {
                        fields.push(fv);
                    }
                    if !fields.trailing_punct() {
                        fields.push_punct(<syn::Token![,]>::default());
                    }
                    syn::Expr::Struct(syn::ExprStruct {
                        attrs: vec![],
                        qself: None,
                        path: syn::parse_quote! { #type_ident },
                        brace_token: syn::token::Brace::default(),
                        fields,
                        dot2_token: (!all_struct_fields_set).then(<syn::Token![..]>::default),
                        rest: (!all_struct_fields_set)
                            .then(|| Box::new(syn::parse_quote! { Default::default() })),
                    })
                }
            }
            ast::Expr::SelectorExpr(sel) => {
                let path: syn::ExprPath = sel.into();
                let type_name = path.path.segments.last().map(|seg| seg.ident.to_string());
                let field_values = raw_elts_to_field_values(type_name.as_deref(), raw_elts);
                if field_values.is_empty() || has_unnamed_field_values(&field_values) {
                    let p = &path.path;
                    syn::parse_quote! { #p::default() }
                } else {
                    let all_struct_fields_set = type_name.as_deref().is_some_and(|type_name| {
                        TYPE_ENV.with(|env| {
                            let count = env.borrow().get_struct_fields(type_name).len();
                            count > 0 && field_values.len() == count
                        })
                    });
                    let mut fields = syn::punctuated::Punctuated::new();
                    for fv in field_values {
                        fields.push(fv);
                    }
                    if !fields.trailing_punct() {
                        fields.push_punct(<syn::Token![,]>::default());
                    }
                    syn::Expr::Struct(syn::ExprStruct {
                        attrs: vec![],
                        qself: None,
                        path: path.path,
                        brace_token: syn::token::Brace::default(),
                        fields,
                        dot2_token: (!all_struct_fields_set).then(<syn::Token![..]>::default),
                        rest: (!all_struct_fields_set)
                            .then(|| Box::new(syn::parse_quote! { Default::default() })),
                    })
                }
            }
            ast::Expr::ArrayType(array_type) => {
                // Slice/array literal: []T{e1, e2, ...} → vec![e1, e2, ...]
                compile_array_literal(&array_type, raw_elts)
            }
            ast::Expr::MapType(map_type) => {
                // Map literal: map[K]V{k1: v1, ...}
                compile_map_literal(&map_type, raw_elts)
            }
            _ => {
                // Fallback: treat as array/vec
                let elts = compile_raw_elts(raw_elts);
                if elts.is_empty() {
                    syn::parse_quote! { Vec::new() }
                } else {
                    syn::parse_quote! { Vec::from([#(#elts),*]) }
                }
            }
        }
    } else {
        // No type — nested composite lit in an array/slice context
        let elts = compile_raw_elts(raw_elts);
        if elts.iter().any(|elt| matches!(elt, syn::Expr::Tuple(_))) {
            return syn::parse_quote! { Default::default() };
        }
        if elts.is_empty() {
            syn::parse_quote! { Vec::new() }
        } else {
            syn::parse_quote! { Vec::from([#(#elts),*]) }
        }
    }
}

fn compile_func_lit(func_lit: ast::FuncLit) -> syn::Expr {
    compile_func_lit_with_capture_mode(func_lit, false)
}

fn compile_func_lit_with_capture_mode(func_lit: ast::FuncLit, move_capture: bool) -> syn::Expr {
    let shared_capture_clones = if move_capture {
        move_closure_shared_capture_clones(&func_lit)
    } else {
        Vec::new()
    };
    let mut params = syn::punctuated::Punctuated::<syn::Pat, Token![,]>::new();
    let mut param_types = Vec::new();

    for field in func_lit.type_.params.list {
        let ty: Option<syn::Type> = field.type_.map(syn::Type::from);
        let names = field.names.unwrap_or_else(|| {
            vec![ast::Ident {
                name_pos: token::Position::default(),
                name: "",
                obj: None,
            }]
        });
        for name in names {
            let ident = if name.name.is_empty() || name.name == "_" {
                next_unnamed_arg_ident()
            } else {
                name.into()
            };
            params.push(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: Some(<Token![mut]>::default()),
                ident,
            }));
            if let Some(ref t) = ty {
                param_types.push(t.clone());
            }
        }
    }

    let return_go_types = collect_return_go_types(func_lit.type_.results.as_ref());
    let ret = compile_return_type(func_lit.type_.results).unwrap_or(syn::ReturnType::Default);

    let previous_return_types =
        RETURN_TYPES.with(|types| std::mem::replace(&mut *types.borrow_mut(), return_go_types));
    let previous_named_return_idents =
        NAMED_RETURN_IDENTS.with(|idents| std::mem::take(&mut *idents.borrow_mut()));
    let completion = TYPE_ENV.with(|env| {
        let env = env.borrow();
        Some(ir::ast_block_completion(&func_lit.body, &env))
    });
    let body_has_defer = block_has_defer(&func_lit.body);
    let block_result = func_lit.body.try_into();
    RETURN_TYPES.with(|types| {
        *types.borrow_mut() = previous_return_types;
    });
    NAMED_RETURN_IDENTS.with(|idents| {
        *idents.borrow_mut() = previous_named_return_idents;
    });
    let mut block: syn::Block = block_result.unwrap_or(syn::Block {
        brace_token: syn::token::Brace::default(),
        stmts: vec![],
    });
    if body_has_defer {
        prepend_defer_stack(&mut block);
    }
    append_missing_return_panic(&mut block, &ret, completion);

    let closure: syn::Expr = if param_types.is_empty() && matches!(ret, syn::ReturnType::Default) {
        if move_capture {
            syn::parse_quote! { move || #block }
        } else {
            syn::parse_quote! { || #block }
        }
    } else if param_types.is_empty() {
        if move_capture {
            syn::parse_quote! { move || #ret #block }
        } else {
            syn::parse_quote! { || #ret #block }
        }
    } else {
        let typed_params: Vec<proc_macro2::TokenStream> = params
            .iter()
            .zip(param_types.iter())
            .map(|(p, t)| quote::quote! { #p: #t })
            .collect();
        if move_capture {
            syn::parse_quote! { move |#(#typed_params),*| #ret #block }
        } else {
            syn::parse_quote! { |#(#typed_params),*| #ret #block }
        }
    };

    if move_capture && !shared_capture_clones.is_empty() {
        syn::parse_quote! {{
            #(#shared_capture_clones)*
            #closure
        }}
    } else {
        closure
    }
}

fn compile_slice_expr(slice_expr: ast::SliceExpr) -> syn::Expr {
    let x_go_type =
        typeinfer::GoType::infer_expr(&slice_expr.x, &TYPE_ENV.with(|e| e.borrow().clone()));
    let is_string_slice = x_go_type.is_string();
    let is_any_slice = is_any_slice_range_type(&x_go_type);
    let x: syn::Expr = (*slice_expr.x).into();
    // Cast range bounds to usize since Rust requires usize for slice indexing
    let low: Option<syn::Expr> = slice_expr.low.map(|l| {
        let e = syn::Expr::from(*l);
        syn::parse_quote! { (#e) as usize }
    });
    let high: Option<syn::Expr> = slice_expr.high.map(|h| {
        let e = syn::Expr::from(*h);
        syn::parse_quote! { (#e) as usize }
    });
    let max: Option<syn::Expr> = slice_expr.max.map(|m| {
        let e = syn::Expr::from(*m);
        syn::parse_quote! { (#e) as usize }
    });

    if is_byte_seq_type_param(&x_go_type) {
        let start: syn::Expr = low
            .as_ref()
            .cloned()
            .unwrap_or_else(|| syn::parse_quote! { 0usize });
        let end: syn::Expr = high.as_ref().cloned().unwrap_or_else(|| {
            syn::parse_quote! { crate::builtin::len(&#x) as usize }
        });
        return syn::parse_quote! { crate::builtin::byte_slice(&#x, #start, #end) };
    }

    if is_any_slice {
        return match (low.as_ref(), high.as_ref()) {
            (None, None) => syn::parse_quote! { #x },
            (Some(lo), None) => {
                syn::parse_quote! { (#x).into_iter().skip(#lo).collect::<Vec<_>>() }
            }
            (None, Some(hi)) => {
                syn::parse_quote! { (#x).into_iter().take(#hi).collect::<Vec<_>>() }
            }
            (Some(lo), Some(hi)) => {
                syn::parse_quote! { (#x).into_iter().skip(#lo).take(#hi - #lo).collect::<Vec<_>>() }
            }
        };
    }

    if let Some(max) = max {
        let start: syn::Expr = low
            .as_ref()
            .cloned()
            .unwrap_or_else(|| syn::parse_quote! { 0usize });
        let end: syn::Expr = high.as_ref().cloned().unwrap_or_else(|| {
            syn::parse_quote! { crate::builtin::len(__gors_source) as usize }
        });
        if is_string_slice {
            return compile_error_expr("full slice expression is not valid for strings");
        }
        return syn::parse_quote! {{
            let __gors_source = &#x;
            let __gors_start = #start;
            let __gors_end = #end;
            let __gors_max = #max;
            let mut __gors_slice = (__gors_source[__gors_start..__gors_end]).to_vec();
            let __gors_cap = __gors_max.saturating_sub(__gors_start);
            if __gors_slice.capacity() < __gors_cap {
                __gors_slice.reserve_exact(__gors_cap - __gors_slice.capacity());
            }
            __gors_slice
        }};
    }

    let slice: syn::Expr = match (low, high) {
        (None, None) => {
            syn::parse_quote! { #x[..] }
        }
        (Some(lo), None) => {
            syn::parse_quote! { #x[#lo..] }
        }
        (None, Some(hi)) => {
            syn::parse_quote! { #x[..#hi] }
        }
        (Some(lo), Some(hi)) => {
            syn::parse_quote! { #x[#lo..#hi] }
        }
    };

    if is_string_slice {
        syn::parse_quote! { (#slice).to_string() }
    } else {
        syn::parse_quote! { (#slice).to_vec() }
    }
}

fn named_byte_slice_type_name(ty: &typeinfer::GoType) -> Option<String> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let typeinfer::GoType::Named(name) = ty else {
            return None;
        };
        match env.resolve_alias(ty) {
            typeinfer::GoType::Slice(elem) if *elem == typeinfer::GoType::Uint8 => {
                Some(name.clone())
            }
            _ => None,
        }
    })
}

fn is_go_byte_slice_type(ty: &typeinfer::GoType) -> bool {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        matches!(env.resolve_alias(ty), typeinfer::GoType::Slice(elem) if *elem == typeinfer::GoType::Uint8)
    })
}

fn infer_assignment_types(
    lhs: &ast::Expr,
    rhs: &ast::Expr,
) -> (typeinfer::GoType, typeinfer::GoType) {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        (
            typeinfer::GoType::infer_expr(lhs, &env),
            typeinfer::GoType::infer_expr(rhs, &env),
        )
    })
}

fn coerce_assignment_expr(
    lhs_ty: &typeinfer::GoType,
    rhs_ty: &typeinfer::GoType,
    mut rhs_expr: syn::Expr,
) -> syn::Expr {
    if let Some(type_name) = named_byte_slice_type_name(lhs_ty) {
        if is_go_byte_slice_type(rhs_ty) && !matches!(rhs_ty, typeinfer::GoType::Named(_)) {
            let ident = syn::Ident::new(&type_name, Span::mixed_site());
            rhs_expr = syn::parse_quote! { #ident(#rhs_expr) };
        }
    }

    rhs_expr = coerce_numeric_expr(lhs_ty, rhs_ty, rhs_expr);

    rhs_expr
}

fn lvalue_expr_from_ref(expr: &ast::Expr) -> Option<syn::Expr> {
    if !is_ir_addressable_expr(expr) {
        return None;
    }
    match expr {
        ast::Expr::Ident(ident) => {
            if let Some(expr) = shared_capture_lvalue_expr(ident.name) {
                return Some(expr);
            }
            let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
            Some(syn::parse_quote! { #ident })
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(pkg) = &*selector.x
                && IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name))
            {
                let module = syn::Ident::new(&import_rust_name(pkg.name), Span::mixed_site());
                let sel =
                    syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
                return Some(syn::parse_quote! { #module::#sel });
            }
            let mut base = lvalue_expr_from_ref(&selector.x)?;
            if is_owning_pointer_cell_expr_ref(&selector.x) {
                base = syn::parse_quote! { #base.lock().unwrap() };
            }
            let sel = syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
            Some(syn::parse_quote! { #base.#sel })
        }
        ast::Expr::IndexExpr(index) => {
            let base = lvalue_expr_from_ref(&index.x)?;
            let index = syn_expr_from_type_expr_like(&index.index)?;
            Some(syn::parse_quote! { #base[(#index) as usize] })
        }
        ast::Expr::ParenExpr(paren) => lvalue_expr_from_ref(&paren.x),
        ast::Expr::StarExpr(star) => {
            if let ast::Expr::Ident(ident) = &*star.x {
                let name = rust_safe_ident_name(ident.name);
                if is_borrowed_pointer_param_name(&name) {
                    let ident = syn::Ident::new(&name, Span::mixed_site());
                    return Some(syn::parse_quote! { *#ident });
                }
            }
            let inner = syn_expr_from_type_expr_like(&star.x)?;
            Some(syn::parse_quote! { *#inner.lock().unwrap() })
        }
        _ => None,
    }
}

fn method_receiver_expr_from_ref(expr: ast::Expr) -> syn::Expr {
    if is_owning_pointer_cell_expr_ref(&expr) {
        let base = lvalue_expr_from_ref(&expr)
            .or_else(|| syn_expr_from_type_expr_like(&expr))
            .unwrap_or_else(|| expr.into());
        return syn::parse_quote! { #base.lock().unwrap() };
    }
    lvalue_expr_from_ref(&expr).unwrap_or_else(|| expr.into())
}

fn is_ir_addressable_expr(expr: &ast::Expr) -> bool {
    TYPE_ENV.with(|env| {
        matches!(
            ir::expr_addressability(expr, &env.borrow()),
            ir::Addressability::Addressable
        )
    })
}

fn take_rhs_lvalue_reads(lhs: &ast::Expr, rhs: &mut syn::Expr) {
    let Some(target) = lvalue_expr_from_ref(lhs) else {
        return;
    };
    let target_key = target.to_token_stream().to_string();

    struct TakeMatchingRead {
        target_key: String,
        replaced: bool,
    }

    impl syn::visit_mut::VisitMut for TakeMatchingRead {
        fn visit_expr_index_mut(&mut self, expr: &mut syn::ExprIndex) {
            if expr.expr.to_token_stream().to_string() == self.target_key {
                return;
            }
            syn::visit_mut::visit_expr_index_mut(self, expr);
        }

        fn visit_expr_reference_mut(&mut self, expr: &mut syn::ExprReference) {
            if expr.expr.to_token_stream().to_string() == self.target_key {
                return;
            }
            syn::visit_mut::visit_expr_reference_mut(self, expr);
        }

        fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
            if !self.replaced && expr.to_token_stream().to_string() == self.target_key {
                let inner = expr.clone();
                *expr = syn::parse_quote! { std::mem::take(&mut #inner) };
                self.replaced = true;
                return;
            }
            syn::visit_mut::visit_expr_mut(self, expr);
        }
    }

    syn::visit_mut::VisitMut::visit_expr_mut(
        &mut TakeMatchingRead {
            target_key,
            replaced: false,
        },
        rhs,
    );
}

fn is_named_numeric_alias(type_name: &str) -> bool {
    if !matches!(
        typeinfer::GoType::from_name(type_name),
        typeinfer::GoType::Named(_)
    ) {
        return false;
    }
    TYPE_ENV.with(|env| {
        env.borrow()
            .resolve_alias(&typeinfer::GoType::Named(type_name.to_string()))
            .is_numeric()
    })
}

fn go_type_is_copy(go_type: &typeinfer::GoType) -> bool {
    match go_type {
        typeinfer::GoType::Bool
        | typeinfer::GoType::Int
        | typeinfer::GoType::Int8
        | typeinfer::GoType::Int16
        | typeinfer::GoType::Int32
        | typeinfer::GoType::Int64
        | typeinfer::GoType::Uint
        | typeinfer::GoType::Uint8
        | typeinfer::GoType::Uint16
        | typeinfer::GoType::Uint32
        | typeinfer::GoType::Uint64
        | typeinfer::GoType::Uintptr
        | typeinfer::GoType::Float32
        | typeinfer::GoType::Float64
        | typeinfer::GoType::Complex64
        | typeinfer::GoType::Complex128
        | typeinfer::GoType::Func { .. }
        | typeinfer::GoType::Pointer(_) => true,
        typeinfer::GoType::Array(elem) => go_type_is_copy(elem),
        typeinfer::GoType::Named(name) => TYPE_ENV.with(|env| {
            let resolved = env.borrow().resolve_alias(go_type);
            if matches!(resolved, typeinfer::GoType::Named(_)) {
                is_named_numeric_alias(name)
            } else {
                go_type_is_copy(&resolved)
            }
        }),
        _ => false,
    }
}

fn is_shared_capture_name(name: &str) -> bool {
    SHARED_CAPTURE_NAMES.with(|shared| shared.borrow().contains(name))
}

fn shared_capture_ident(expr: &ast::Expr) -> Option<syn::Ident> {
    let ast::Expr::Ident(ident) = expr else {
        return None;
    };
    if !is_shared_capture_name(ident.name) {
        return None;
    }
    Some(syn::Ident::new(
        &rust_safe_ident_name(ident.name),
        Span::mixed_site(),
    ))
}

fn is_goto_continue_label(name: &str) -> bool {
    GOTO_CONTINUE_LABELS.with(|labels| labels.borrow().contains(name))
}

fn compile_goto_state_jump(name: &str) -> Option<Vec<syn::Stmt>> {
    let target = rust_safe_ident_name(name);
    GOTO_STATE_CONTEXTS.with(|contexts| {
        contexts.borrow().iter().rev().find_map(|context| {
            context.labels.get(&target).map(|target_index| {
                let state_ident = &context.state_ident;
                let loop_label = &context.loop_label;
                let target_lit =
                    syn::LitInt::new(&format!("{target_index}usize"), Span::mixed_site());
                vec![
                    syn::parse_quote! {
                        #state_ident = #target_lit;
                    },
                    syn::parse_quote! {
                        continue #loop_label;
                    },
                ]
            })
        })
    })
}

fn is_borrowed_pointer_param_name(name: &str) -> bool {
    BORROWED_POINTER_PARAM_NAMES.with(|borrowed| borrowed.borrow().contains(name))
}

fn is_borrowed_pointer_expr_ref(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(ident) => {
            is_borrowed_pointer_param_name(&rust_safe_ident_name(ident.name))
        }
        ast::Expr::ParenExpr(paren) => is_borrowed_pointer_expr_ref(&paren.x),
        _ => false,
    }
}

fn is_owning_pointer_cell_expr_ref(expr: &ast::Expr) -> bool {
    if is_borrowed_pointer_expr_ref(expr) {
        return false;
    }
    TYPE_ENV.with(|env| {
        matches!(
            resolved_go_type(&typeinfer::GoType::infer_expr(expr, &env.borrow())),
            typeinfer::GoType::Pointer(_)
        )
    })
}

fn shared_capture_init_expr(name: &str, init: syn::Expr) -> syn::Expr {
    if is_shared_capture_name(name) {
        syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(#init)) }
    } else {
        init
    }
}

fn shared_capture_type(name: &str, ty: syn::Type) -> syn::Type {
    if is_shared_capture_name(name) {
        syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#ty>> }
    } else {
        ty
    }
}

fn shared_capture_lvalue_expr(name: &str) -> Option<syn::Expr> {
    if !is_shared_capture_name(name) {
        return None;
    }
    let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
    Some(syn::parse_quote! { *#ident.lock().unwrap() })
}

fn shared_capture_read_expr(name: &str) -> Option<syn::Expr> {
    if !is_shared_capture_name(name) {
        return None;
    }
    let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
    let go_type = TYPE_ENV.with(|env| {
        env.borrow()
            .get_var(name)
            .unwrap_or(typeinfer::GoType::Unknown)
    });
    if go_type_is_copy(&go_type) {
        Some(syn::parse_quote! {{
            let __gors_shared_value = *#ident.lock().unwrap();
            __gors_shared_value
        }})
    } else {
        Some(syn::parse_quote! {{
            let __gors_shared_value = #ident.lock().unwrap().clone();
            __gors_shared_value
        }})
    }
}

fn is_byte_seq_type_param(go_type: &typeinfer::GoType) -> bool {
    matches!(go_type, typeinfer::GoType::Named(name) if BYTE_SEQ_TYPE_PARAMS.with(|params| params.borrow().contains(name)))
}

fn resolved_go_type(ty: &typeinfer::GoType) -> typeinfer::GoType {
    TYPE_ENV.with(|env| env.borrow().resolve_alias(ty))
}

fn numeric_cast_type(ty: &typeinfer::GoType) -> Option<syn::Type> {
    let resolved = resolved_go_type(ty);
    if !resolved.is_numeric() && !matches!(resolved, typeinfer::GoType::Uintptr) {
        return None;
    }
    rust_type_from_go_type(&resolved)
}

fn coerce_numeric_expr(
    expected: &typeinfer::GoType,
    actual: &typeinfer::GoType,
    expr: syn::Expr,
) -> syn::Expr {
    let expected_resolved = resolved_go_type(expected);
    let actual_resolved = resolved_go_type(actual);
    if expected_resolved == actual_resolved {
        return expr;
    }
    let Some(target_ty) = numeric_cast_type(&expected_resolved) else {
        return expr;
    };
    if !actual_resolved.is_numeric() && !matches!(actual_resolved, typeinfer::GoType::Uintptr) {
        return expr;
    }
    syn::parse_quote! { (#expr as #target_ty) }
}

fn is_complex_go_type(ty: &typeinfer::GoType) -> bool {
    matches!(
        resolved_go_type(ty),
        typeinfer::GoType::Complex64 | typeinfer::GoType::Complex128
    )
}

fn is_complex_const_conversion_source(ty: &typeinfer::GoType) -> bool {
    let resolved = resolved_go_type(ty);
    resolved.is_numeric()
        || matches!(
            resolved,
            typeinfer::GoType::Uintptr
                | typeinfer::GoType::Complex64
                | typeinfer::GoType::Complex128
                | typeinfer::GoType::Unknown
        )
}

fn coerce_complex_const_expr(
    expected: &typeinfer::GoType,
    actual: &typeinfer::GoType,
    expr: syn::Expr,
) -> Option<syn::Expr> {
    let expected_resolved = resolved_go_type(expected);
    let actual_resolved = resolved_go_type(actual);
    if expected_resolved == actual_resolved {
        return Some(expr);
    }
    if !is_complex_const_conversion_source(actual) {
        return None;
    }
    match expected_resolved {
        typeinfer::GoType::Complex64 => Some(syn::parse_quote! {
            crate::builtin::to_complex64(#expr)
        }),
        typeinfer::GoType::Complex128 => Some(syn::parse_quote! {
            crate::builtin::to_complex128(#expr)
        }),
        _ => None,
    }
}

fn expr_should_clone_for_value_param(
    expr: &ast::Expr,
    expected: &typeinfer::GoType,
    actual: &typeinfer::GoType,
) -> bool {
    if !is_ir_addressable_expr(expr) {
        return false;
    }
    let expected = resolved_go_type(expected);
    let actual = resolved_go_type(actual);
    if matches!(
        expected,
        typeinfer::GoType::Any
            | typeinfer::GoType::Interface(_)
            | typeinfer::GoType::Pointer(_)
            | typeinfer::GoType::Slice(_)
            | typeinfer::GoType::Array(_)
            | typeinfer::GoType::Unknown
    ) || matches!(
        actual,
        typeinfer::GoType::Any
            | typeinfer::GoType::Interface(_)
            | typeinfer::GoType::Pointer(_)
            | typeinfer::GoType::Slice(_)
            | typeinfer::GoType::Array(_)
            | typeinfer::GoType::Unknown
    ) {
        return false;
    }
    if expected != actual {
        return false;
    }
    !go_type_is_copy(&expected) && !go_type_is_copy(&actual)
}

fn binding_init_should_clone(expr: &ast::Expr) -> bool {
    if !is_ir_addressable_expr(expr) {
        return false;
    }
    let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(expr, &env.borrow()));
    !go_type_is_copy(&actual)
}

fn maybe_clone_binding_init(should_clone: bool, init: syn::Expr) -> syn::Expr {
    if should_clone {
        syn::parse_quote! { (#init).clone() }
    } else {
        init
    }
}

fn compile_return_expr_with_expected(
    expr: ast::Expr,
    expected: Option<&typeinfer::GoType>,
) -> syn::Expr {
    if let Some(expected) = expected
        && let Some(interface_name) = go_type_interface_name(expected)
    {
        let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&expr, &env.borrow()));
        let trait_path = interface_trait_path_from_name(&interface_name);
        let compiled: syn::Expr = expr.into();
        if go_type_is_interface_like(&actual) {
            return compiled;
        }
        return if is_box_new_call(&compiled) {
            syn::parse_quote! { #compiled as Box<dyn #trait_path + '_> }
        } else if let Some(inner) = arc_mutex_new_inner_expr(&compiled) {
            syn::parse_quote! { Box::new(#inner) as Box<dyn #trait_path + '_> }
        } else {
            syn::parse_quote! { Box::new(#compiled) as Box<dyn #trait_path + '_> }
        };
    }
    compile_expr_with_expected(expr, expected)
}

fn is_box_new_call(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(path) = &*call.func else {
        return false;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    matches!(segments.as_slice(), [box_name, new_name] if box_name == "Box" && new_name == "new")
}

fn arc_mutex_new_inner_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !is_path_call_expr(&call.func, &["std", "sync", "Arc", "new"]) || call.args.len() != 1 {
        return None;
    }
    let Some(syn::Expr::Call(mutex_call)) = call.args.first() else {
        return None;
    };
    if !is_path_call_expr(&mutex_call.func, &["std", "sync", "Mutex", "new"])
        || mutex_call.args.len() != 1
    {
        return None;
    }
    mutex_call.args.first().cloned()
}

fn compile_expr_with_expected(
    mut expr: ast::Expr,
    expected: Option<&typeinfer::GoType>,
) -> syn::Expr {
    if matches!(&expr, ast::Expr::Ident(id) if id.name == "nil") {
        return match expected {
            Some(ty) if is_go_byte_slice_type(ty) => syn::parse_quote! { Default::default() },
            Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_)) => {
                syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
            }
            _ => syn::parse_quote! { Default::default() },
        };
    }

    if matches!(
        expected.map(resolved_go_type),
        Some(typeinfer::GoType::Func { .. })
    ) {
        if let ast::Expr::Ident(ident) = &expr {
            let is_func_var = TYPE_ENV.with(|env| {
                matches!(
                    env.borrow().get_var(ident.name),
                    Some(typeinfer::GoType::Func { .. })
                )
            });
            if is_func_var {
                let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
                return syn::parse_quote! { #ident.clone() };
            }
        }

        if let ast::Expr::SelectorExpr(selector) = &expr
            && type_method_expression_info(selector).is_some()
        {
            let ast::Expr::SelectorExpr(selector) = expr else {
                return compile_error_expr("invalid method expression");
            };
            let compiled = compile_type_method_expression_value(selector);
            if let Some(expected) = expected
                && let Some(func_value) = shared_func_value_expr(expected, compiled.clone())
            {
                return func_value;
            }
            return compiled;
        }

        match expr {
            ast::Expr::FuncLit(func_lit) => {
                let compiled = compile_func_lit_with_capture_mode(func_lit, true);
                if let Some(expected) = expected
                    && let Some(func_value) = shared_func_value_expr(expected, compiled.clone())
                {
                    return func_value;
                }
                return compiled;
            }
            other => {
                expr = other;
            }
        }

        if let Some(expected) = expected
            && is_function_item_expr(&expr)
        {
            let compiled: syn::Expr = expr.into();
            if let Some(func_value) = shared_func_value_expr(expected, compiled.clone()) {
                return func_value;
            }
            return compiled;
        }
    }

    if matches!(expected.map(resolved_go_type), Some(typeinfer::GoType::Any)) {
        let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&expr, &env.borrow()));
        if matches!(resolved_go_type(&actual), typeinfer::GoType::Any) {
            return expr.into();
        }
        let numeric_const_target = if is_const_like_expr(&expr) {
            numeric_cast_type(&actual)
        } else {
            None
        };
        let expr = if matches!(actual, typeinfer::GoType::Unknown) {
            expr.into()
        } else {
            compile_expr_with_expected(expr, Some(&actual))
        };
        let expr = if let Some(target_ty) = numeric_const_target {
            syn::parse_quote! { (#expr as #target_ty) }
        } else {
            expr
        };
        return syn::parse_quote! { Box::new(#expr) as Box<dyn std::any::Any> };
    }

    if let Some(typeinfer::GoType::Named(name)) = expected {
        if is_type_interface(name) || TYPE_ENV.with(|env| env.borrow().is_interface(name)) {
            let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&expr, &env.borrow()));
            if go_type_is_interface_like(&actual) {
                return expr.into();
            }
            let needs_owned_temp = matches!(expr, ast::Expr::SelectorExpr(_));
            let expr: syn::Expr = expr.into();
            if let Some(inner) = arc_mutex_new_inner_expr(&expr) {
                return syn::parse_quote! { &mut #inner };
            }
            if needs_owned_temp {
                return syn::parse_quote! { &mut (#expr).clone() };
            }
            return syn::parse_quote! { &mut #expr };
        }
    }

    if let ast::Expr::CompositeLit(mut comp_lit) = expr {
        if comp_lit.type_.is_none()
            && let Some(typeinfer::GoType::Named(name)) = expected
            && !name.contains('.')
        {
            let leaked_name: &'static str = Box::leak(name.clone().into_boxed_str());
            comp_lit.type_ = Some(Box::new(ast::Expr::Ident(ast::Ident {
                name_pos: token::Position::default(),
                name: leaked_name,
                obj: None,
            })));
        }
        return compile_composite_lit(comp_lit);
    }

    if matches!(expected, Some(typeinfer::GoType::String)) && is_string_literal(&expr) {
        let expr: syn::Expr = expr.into();
        return syn::parse_quote! { #expr.to_string() };
    }

    if let Some(expected) = expected
        && numeric_cast_type(expected).is_some()
    {
        match expr {
            ast::Expr::BinaryExpr(binary) if is_numeric_value_binary_op(binary.op) => {
                return compile_numeric_binary_expr_with_expected(binary, expected);
            }
            other => {
                expr = other;
            }
        }
    }

    if let Some(expected) = expected {
        let actual = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&expr, &env.borrow()));
        if matches!(actual, typeinfer::GoType::Named(_))
            && matches!(
                resolved_go_type(expected),
                typeinfer::GoType::Slice(elem) if *elem != typeinfer::GoType::Uint8
            )
            && resolved_go_type(&actual) == resolved_go_type(expected)
        {
            let compiled: syn::Expr = expr.into();
            return syn::parse_quote! { (#compiled).to_vec() };
        }
        if expr_should_clone_for_value_param(&expr, expected, &actual) {
            let compiled: syn::Expr = expr.into();
            return syn::parse_quote! { (#compiled).clone() };
        }
        if is_complex_go_type(expected)
            && is_const_like_expr(&expr)
            && is_complex_const_conversion_source(&actual)
        {
            let compiled: syn::Expr = expr.into();
            if let Some(coerced) = coerce_complex_const_expr(expected, &actual, compiled) {
                return coerced;
            }
            return compile_error_expr("unsupported complex constant conversion");
        }
        let numeric_const_like = numeric_cast_type(expected).is_some() && is_const_like_expr(&expr);
        let compiled = if numeric_const_like {
            const_eval_expr_in_active_env(&expr, 0, &BTreeMap::new())
                .map_or_else(|| expr.into(), |value| value.to_expr())
        } else {
            expr.into()
        };
        if numeric_const_like
            && matches!(resolved_go_type(&actual), typeinfer::GoType::Unknown)
            && let Some(target_ty) = numeric_cast_type(expected)
        {
            return syn::parse_quote! { (#compiled as #target_ty) };
        }
        return coerce_numeric_expr(expected, &actual, compiled);
    }

    expr.into()
}

fn is_function_item_expr(expr: &ast::Expr) -> bool {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match expr {
            ast::Expr::Ident(id) => {
                !matches!(env.get_var(id.name), Some(typeinfer::GoType::Func { .. }))
                    && env.has_func(id.name)
            }
            ast::Expr::SelectorExpr(sel) => {
                let ast::Expr::Ident(pkg_or_recv) = &*sel.x else {
                    return false;
                };
                let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                if env.has_func(&package_key) {
                    return true;
                }
                match env.get_var(pkg_or_recv.name) {
                    Some(typeinfer::GoType::Named(name)) => {
                        env.has_func(&format!("{name}.{}", sel.sel.name))
                    }
                    Some(typeinfer::GoType::Pointer(inner)) => match *inner {
                        typeinfer::GoType::Named(name) => {
                            env.has_func(&format!("{name}.{}", sel.sel.name))
                        }
                        _ => false,
                    },
                    _ => false,
                }
            }
            _ => false,
        }
    })
}

fn is_function_literal_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::FuncLit(_) => true,
        ast::Expr::ParenExpr(paren) => is_function_literal_expr(&paren.x),
        _ => false,
    }
}

fn call_param_types(fun: &ast::Expr) -> Vec<typeinfer::GoType> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match fun {
            ast::Expr::Ident(id) => match env.get_var(id.name) {
                Some(typeinfer::GoType::Func { params, .. }) => params,
                _ => env.get_func_params(id.name),
            },
            ast::Expr::SelectorExpr(sel) => {
                if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                    let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                    let package_params = env.get_func_params(&package_key);
                    if !package_params.is_empty() {
                        return package_params;
                    }

                    if let Some(name) = env
                        .get_var(pkg_or_recv.name)
                        .and_then(|ty| receiver_method_type_name_for_call(ty, &env))
                    {
                        return env.get_func_params(&format!("{}.{}", name, sel.sel.name));
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    })
}

fn receiver_method_type_name_for_call(
    ty: typeinfer::GoType,
    env: &typeinfer::TypeEnv,
) -> Option<String> {
    match env.resolve_alias(&ty) {
        typeinfer::GoType::Named(name) => Some(name),
        typeinfer::GoType::Pointer(inner) => receiver_method_type_name_for_call(*inner, env),
        _ => None,
    }
}

struct MethodValueInfo {
    receiver_type: typeinfer::GoType,
    params: Vec<typeinfer::GoType>,
    results: Vec<typeinfer::GoType>,
}

fn typed_ident_pat(ident: syn::Ident, ty: syn::Type) -> syn::Pat {
    syn::Pat::Type(syn::PatType {
        attrs: vec![],
        pat: Box::new(syn::Pat::Ident(syn::PatIdent {
            attrs: vec![],
            by_ref: None,
            mutability: None,
            ident,
            subpat: None,
        })),
        colon_token: <Token![:]>::default(),
        ty: Box::new(ty),
    })
}

fn method_value_info(selector: &ast::SelectorExpr) -> Option<MethodValueInfo> {
    if selector_base_is_import(selector) {
        return None;
    }
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let receiver_type = typeinfer::GoType::infer_expr(&selector.x, &env);
        let receiver_name = receiver_method_type_name_for_call(receiver_type.clone(), &env)?;
        let method_key = format!("{}.{}", receiver_name, selector.sel.name);
        env.has_func(&method_key).then(|| MethodValueInfo {
            receiver_type,
            params: env.get_func_params(&method_key),
            results: env.get_func_returns(&method_key),
        })
    })
}

fn compile_method_value_expr(selector: ast::SelectorExpr) -> syn::Expr {
    let Some(info) = method_value_info(&selector) else {
        return compile_error_expr("invalid method value");
    };
    let method: syn::Ident = selector.sel.into();
    let receiver_is_pointer = matches!(
        resolved_go_type(&info.receiver_type),
        typeinfer::GoType::Pointer(_)
    );
    let receiver: syn::Expr = if receiver_is_pointer {
        let receiver: syn::Expr = (*selector.x).into();
        syn::parse_quote! { (#receiver).clone() }
    } else {
        let should_clone = !go_type_is_copy(&info.receiver_type);
        let receiver = method_receiver_expr_from_ref(*selector.x);
        if should_clone {
            syn::parse_quote! { (#receiver).clone() }
        } else {
            receiver
        }
    };
    let receiver_ident = syn::Ident::new("__gors_method_receiver", Span::mixed_site());
    let param_idents = (0..info.params.len())
        .map(|idx| syn::Ident::new(&format!("__gors_method_arg_{idx}"), Span::mixed_site()))
        .collect::<Vec<_>>();
    let param_types = info.params.iter().map(rust_type_from_inferred_go_type);
    let param_pats = param_idents
        .iter()
        .zip(param_types)
        .map(|(ident, ty)| typed_ident_pat(ident.clone(), ty))
        .collect::<Vec<syn::Pat>>();
    let call_args = param_idents
        .iter()
        .map(|ident| syn::parse_quote! { #ident })
        .collect::<Vec<syn::Expr>>();
    let result_types = info
        .results
        .iter()
        .map(rust_type_from_inferred_go_type)
        .collect::<Vec<_>>();
    let return_type: syn::Type = match result_types.as_slice() {
        [] => syn::parse_quote! { () },
        [ty] => ty.clone(),
        _ => syn::parse_quote! { (#(#result_types),*) },
    };
    let body: syn::Expr = if receiver_is_pointer {
        syn::parse_quote! { #receiver_ident.lock().unwrap().#method(#(#call_args),*) }
    } else {
        syn::parse_quote! { #receiver_ident.#method(#(#call_args),*) }
    };
    syn::parse_quote! {{
        let #receiver_ident = #receiver;
        move |#(#param_pats),*| -> #return_type { #body }
    }}
}

fn function_value_call_params(fun: &ast::Expr) -> Option<Vec<typeinfer::GoType>> {
    function_value_call_info(fun).map(|info| info.params)
}

struct FunctionValueCallInfo {
    params: Vec<typeinfer::GoType>,
    variadic_start: Option<usize>,
}

fn function_value_call_info(fun: &ast::Expr) -> Option<FunctionValueCallInfo> {
    if is_function_item_expr(fun) || is_function_literal_expr(fun) {
        return None;
    }
    if let ast::Expr::SelectorExpr(selector) = fun
        && method_value_info(selector).is_some()
    {
        return None;
    }
    let ty = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(fun, &env.borrow()));
    match resolved_go_type(&ty) {
        typeinfer::GoType::Func {
            params,
            variadic_start,
            ..
        } => Some(FunctionValueCallInfo {
            params,
            variadic_start,
        }),
        _ => None,
    }
}

fn function_field_call_params(fun: &ast::Expr) -> Option<Vec<typeinfer::GoType>> {
    if matches!(fun, ast::Expr::SelectorExpr(_)) {
        function_value_call_params(fun)
    } else {
        None
    }
}

fn compile_function_field_call(call_expr: ast::CallExpr) -> Option<syn::Expr> {
    let info = function_value_call_info(&call_expr.fun)?;
    let func: syn::Expr = (*call_expr.fun).into();
    let args = compile_function_value_call_args(
        call_expr.args.unwrap_or_default(),
        &info.params,
        info.variadic_start,
        call_expr.ellipsis.is_some(),
    );
    Some(syn::parse_quote! {{
        let __gors_func = {
            let __gors_func = crate::builtin::lock_func(&(#func));
            match __gors_func.as_ref() {
                Some(__gors_func) => __gors_func.clone(),
                None => panic!("nil function"),
            }
        };
        (&*__gors_func)(#args)
    }})
}

fn compile_function_value_call_args(
    raw_args: Vec<ast::Expr>,
    params: &[typeinfer::GoType],
    variadic_start: Option<usize>,
    has_variadic_spread: bool,
) -> syn::punctuated::Punctuated<syn::Expr, Token![,]> {
    let Some(variadic_start) = variadic_start else {
        let mut args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
        for (idx, arg) in raw_args.into_iter().enumerate() {
            args.push(compile_function_value_arg_with_expected(
                arg,
                params.get(idx),
            ));
        }
        return args;
    };

    let variadic_elem = params.get(variadic_start).and_then(|ty| match ty {
        typeinfer::GoType::Slice(inner) => Some((**inner).clone()),
        _ => None,
    });
    let variadic_is_any = matches!(
        variadic_elem.as_ref().map(resolved_go_type),
        Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_))
    );

    if has_variadic_spread {
        let mut args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
        for (idx, arg) in raw_args.into_iter().enumerate() {
            let should_clone =
                idx >= variadic_start && !variadic_is_any && is_ir_addressable_expr(&arg) && {
                    let actual =
                        TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
                    !go_type_is_copy(&actual)
                };
            let arg = compile_function_value_arg_with_expected(arg, params.get(idx));
            if should_clone {
                args.push(syn::parse_quote! { (#arg).clone() });
            } else {
                args.push(arg);
            }
        }
        return args;
    }

    let mut compiled_args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
    for (idx, arg) in raw_args.into_iter().enumerate() {
        if idx < variadic_start {
            compiled_args.push(compile_function_value_arg_with_expected(
                arg,
                params.get(idx),
            ));
        } else if variadic_is_any {
            let arg = compile_variadic_any_arg(arg, variadic_elem.as_ref());
            compiled_args
                .push(syn::parse_quote! { Box::new((#arg).clone()) as Box<dyn std::any::Any> });
        } else {
            compiled_args.push(compile_function_value_arg_with_expected(
                arg,
                variadic_elem.as_ref(),
            ));
        }
    }

    let variadic_args: Vec<&syn::Expr> = compiled_args.iter().skip(variadic_start).collect();
    let fixed_args: Vec<&syn::Expr> = compiled_args.iter().take(variadic_start).collect();
    let vec_expr: syn::Expr = if variadic_args.is_empty() && variadic_is_any {
        syn::parse_quote! { Vec::<Box<dyn std::any::Any>>::new() }
    } else if variadic_args.is_empty() {
        syn::parse_quote! { Vec::new() }
    } else {
        syn::parse_quote! { Vec::from([#(#variadic_args),*]) }
    };

    let mut args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
    for arg in fixed_args {
        args.push(arg.clone());
    }
    args.push(vec_expr);
    args
}

fn compile_function_value_arg_with_expected(
    arg: ast::Expr,
    expected: Option<&typeinfer::GoType>,
) -> syn::Expr {
    let should_clone = expected.map(resolved_go_type).is_some_and(|expected| {
        matches!(
            expected,
            typeinfer::GoType::Pointer(_)
                | typeinfer::GoType::Chan { .. }
                | typeinfer::GoType::Func { .. }
        )
    }) && is_ir_addressable_expr(&arg);
    let expr = compile_expr_with_expected(arg, expected);
    if should_clone {
        syn::parse_quote! { (#expr).clone() }
    } else {
        expr
    }
}

fn call_return_types(expr: &ast::Expr) -> Vec<typeinfer::GoType> {
    let ast::Expr::CallExpr(call) = expr else {
        return Vec::new();
    };
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match &*call.fun {
            ast::Expr::Ident(id) => env.get_func_returns(id.name),
            ast::Expr::SelectorExpr(sel) => {
                if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                    let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                    let package_returns = env.get_func_returns(&package_key);
                    if !package_returns.is_empty() {
                        return package_returns;
                    }

                    if let Some(typeinfer::GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                        return env.get_func_returns(&format!("{}.{}", name, sel.sel.name));
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    })
}

fn compile_type_switch_stmt(ts: ast::TypeSwitchStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    // type switch: switch x := val.(type) { case T: ... }
    // Compile to if/else chain with downcast checks
    let (binding_name, assign_expr) = match *ts.assign {
        ast::Stmt::ExprStmt(s) => (None, syn::Expr::from(s.x)),
        ast::Stmt::AssignStmt(s) => {
            let name = s.lhs.first().and_then(|e| {
                if let ast::Expr::Ident(id) = e {
                    if id.name != "_" {
                        Some(id.name.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
            let rhs: syn::Expr = s
                .rhs
                .into_iter()
                .next()
                .map(syn::Expr::from)
                .unwrap_or_else(|| syn::parse_quote! { () });
            (name, rhs)
        }
        _ => (None, syn::parse_quote! { () }),
    };

    let clauses: Vec<ast::CaseClause> = ts
        .body
        .list
        .into_iter()
        .filter_map(|s| {
            if let ast::Stmt::CaseClause(cc) = s {
                Some(cc)
            } else {
                None
            }
        })
        .collect();

    let mut cases: Vec<ast::CaseClause> = Vec::new();
    let mut default_body: Option<Vec<ast::Stmt>> = None;
    for clause in clauses {
        if clause.list.is_none() {
            default_body = Some(clause.body);
        } else {
            cases.push(clause);
        }
    }

    let else_block: Option<syn::Expr> = if let Some(body) = default_body {
        let mut stmts = vec![];
        if let Some(ref name) = binding_name {
            let bind_ident = syn::Ident::new(name, Span::mixed_site());
            let val = &assign_expr;
            stmts.push(syn::parse_quote! {
                let #bind_ident = #val;
            });
        }
        for stmt in body {
            stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }
        Some(syn::Expr::Block(syn::ExprBlock {
            attrs: vec![],
            label: None,
            block: syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts,
            },
        }))
    } else {
        None
    };

    let mut result: Option<syn::Expr> = else_block;
    for case in cases.into_iter().rev() {
        let type_cases: Vec<(syn::Type, bool)> = case
            .list
            .unwrap_or_default()
            .into_iter()
            .map(|expr| {
                let is_interface = is_interface_type_expr(&expr);
                (syn::Type::from(expr), is_interface)
            })
            .collect();

        let cond = if let Some((ty, is_interface)) =
            type_cases.first().filter(|_| type_cases.len() == 1)
        {
            let val = &assign_expr;
            if *is_interface {
                syn::parse_quote! { false }
            } else if is_string_syn_type(ty) {
                syn::parse_quote! {
                    (&*#val as &dyn std::any::Any).is::<String>()
                        || (&*#val as &dyn std::any::Any).is::<&str>()
                }
            } else {
                syn::parse_quote! { (&*#val as &dyn std::any::Any).is::<#ty>() }
            }
        } else {
            type_cases
                .iter()
                .map(|(ty, is_interface)| {
                    let val = &assign_expr;
                    if *is_interface {
                        syn::parse_quote! { false }
                    } else if is_string_syn_type(ty) {
                        syn::parse_quote! {
                            (&*#val as &dyn std::any::Any).is::<String>()
                                || (&*#val as &dyn std::any::Any).is::<&str>()
                        }
                    } else {
                        syn::parse_quote! { (&*#val as &dyn std::any::Any).is::<#ty>() }
                    }
                })
                .reduce(|acc, e| syn::parse_quote! { #acc || #e })
                .unwrap_or_else(|| syn::parse_quote! { true })
        };

        let mut body_stmts = vec![];
        if let Some(ref name) = binding_name {
            if let Some((ty, is_interface)) = type_cases.first().filter(|_| type_cases.len() == 1) {
                let val = &assign_expr;
                let bind_ident = syn::Ident::new(name, Span::mixed_site());
                if *is_interface {
                    body_stmts.push(syn::parse_quote! {
                        let mut #bind_ident = __GorsNoopInterface::default();
                    });
                } else if is_string_syn_type(ty) {
                    body_stmts.push(syn::parse_quote! {
                        let #bind_ident = if let Some(__gors_s) = (&*#val as &dyn std::any::Any).downcast_ref::<String>() {
                            __gors_s.clone()
                        } else if let Some(__gors_s) = (&*#val as &dyn std::any::Any).downcast_ref::<&str>() {
                            __gors_s.to_string()
                        } else {
                            String::new()
                        };
                    });
                } else {
                    body_stmts.push(syn::parse_quote! {
                        let #bind_ident = (&*#val as &dyn std::any::Any)
                            .downcast_ref::<#ty>()
                            .cloned()
                            .unwrap_or_default();
                    });
                }
            }
        }
        for stmt in case.body {
            body_stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }

        result = Some(syn::Expr::If(syn::ExprIf {
            attrs: vec![],
            if_token: <Token![if]>::default(),
            cond: Box::new(cond),
            then_branch: syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: body_stmts,
            },
            else_branch: result.map(|e| (<Token![else]>::default(), Box::new(e))),
        }));
    }

    match result {
        Some(expr) => Ok(vec![syn::Stmt::Expr(expr, None)]),
        None => Ok(vec![]),
    }
}

fn is_string_syn_type(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(type_path)
        if type_path.path.segments.len() == 1
            && type_path.path.segments.first().is_some_and(|seg| seg.ident == "String"))
}

fn is_interface_type_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Ident(id) => id.name == "error" || is_type_interface(id.name),
        ast::Expr::SelectorExpr(selector) => {
            selector.sel.name == "error" || is_type_interface(selector.sel.name)
        }
        _ => false,
    }
}

fn is_string_literal(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::BasicLit(lit) if lit.kind == token::Token::STRING)
}

fn make_for_loop(pat: syn::Pat, iter_expr: syn::Expr, body: syn::Block) -> Vec<syn::Stmt> {
    vec![syn::Stmt::Expr(
        syn::Expr::ForLoop(syn::ExprForLoop {
            attrs: vec![],
            label: None,
            for_token: <Token![for]>::default(),
            pat: Box::new(pat),
            in_token: <Token![in]>::default(),
            expr: Box::new(iter_expr),
            body,
        }),
        None,
    )]
}

fn set_range_binding(expr: Option<&ast::Expr>, ty: typeinfer::GoType) {
    let Some(ast::Expr::Ident(ident)) = expr else {
        return;
    };
    if ident.name == "_" {
        return;
    }
    TYPE_ENV.with(|env| {
        env.borrow_mut().set_var(ident.name, ty);
    });
}

fn set_range_bindings(
    key: Option<&ast::Expr>,
    value: Option<&ast::Expr>,
    inferred_range_type: &typeinfer::GoType,
    is_string: bool,
    is_int: bool,
) {
    if is_string {
        set_range_binding(key, typeinfer::GoType::Int);
        set_range_binding(value, typeinfer::GoType::Int32);
        return;
    }

    if is_int {
        set_range_binding(key, typeinfer::GoType::Int);
        return;
    }

    let resolved = resolved_go_type(inferred_range_type);
    match (key, value, resolved) {
        (Some(key), Some(value), typeinfer::GoType::Slice(elem))
        | (Some(key), Some(value), typeinfer::GoType::Array(elem)) => {
            set_range_binding(Some(key), typeinfer::GoType::Int);
            set_range_binding(Some(value), *elem);
        }
        (Some(key), None, typeinfer::GoType::Slice(_) | typeinfer::GoType::Array(_)) => {
            set_range_binding(Some(key), typeinfer::GoType::Int);
        }
        (Some(key), Some(value), typeinfer::GoType::Map(key_ty, value_ty)) => {
            set_range_binding(Some(key), *key_ty);
            set_range_binding(Some(value), *value_ty);
        }
        (Some(key), None, typeinfer::GoType::Map(key_ty, _)) => {
            set_range_binding(Some(key), *key_ty);
        }
        (Some(key), None, typeinfer::GoType::Chan { elem, .. }) => {
            set_range_binding(Some(key), *elem);
        }
        _ => {}
    }
}

fn range_function_yield_params(range_type: &typeinfer::GoType) -> Option<Vec<typeinfer::GoType>> {
    let typeinfer::GoType::Func {
        params, results, ..
    } = resolved_go_type(range_type)
    else {
        return None;
    };
    if !results.is_empty() || params.len() != 1 {
        return None;
    }
    let yield_type = params.into_iter().next()?;
    let typeinfer::GoType::Func {
        params: yield_params,
        results: yield_results,
        ..
    } = resolved_go_type(&yield_type)
    else {
        return None;
    };
    matches!(yield_results.as_slice(), [typeinfer::GoType::Bool]).then_some(yield_params)
}

fn set_range_function_bindings(
    key: Option<&ast::Expr>,
    value: Option<&ast::Expr>,
    yield_params: &[typeinfer::GoType],
) {
    if let Some(first) = yield_params.first() {
        set_range_binding(key, first.clone());
    }
    if let Some(second) = yield_params.get(1) {
        set_range_binding(value, second.clone());
    }
}

fn compile_range_stmt(range_stmt: ast::RangeStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    let inferred_range_type =
        typeinfer::GoType::infer_expr(&range_stmt.x, &TYPE_ENV.with(|e| e.borrow().clone()));
    let range_kind = TYPE_ENV.with(|env| ir::range_kind(&range_stmt.x, &env.borrow()));
    let is_string = matches!(range_kind, ir::RangeKind::String);
    let is_int = matches!(range_kind, ir::RangeKind::Integer);
    let is_pointer_array = is_pointer_array_range_type(&inferred_range_type);
    let range_function_yield_params = range_function_yield_params(&inferred_range_type);
    let range_function_capture_names = if range_function_yield_params.is_some() {
        TYPE_ENV.with(|env| ir::mutable_range_function_capture_names(&range_stmt, &env.borrow()))
    } else {
        std::collections::BTreeSet::new()
    };
    let env_snapshot = TYPE_ENV.with(|env| env.borrow().clone());
    if let Some(yield_params) = &range_function_yield_params {
        set_range_function_bindings(
            range_stmt.key.as_ref(),
            range_stmt.value.as_ref(),
            yield_params,
        );
    } else {
        set_range_bindings(
            range_stmt.key.as_ref(),
            range_stmt.value.as_ref(),
            &inferred_range_type,
            is_string,
            is_int,
        );
    }
    let body = range_stmt.body.try_into();
    TYPE_ENV.with(|env| {
        *env.borrow_mut() = env_snapshot;
    });
    let body: syn::Block = body?;
    let is_function_item_range =
        range_function_yield_params.is_some() && is_function_item_expr(&range_stmt.x);
    let x: syn::Expr = range_stmt.x.into();
    let range_tok = range_stmt.tok;

    if let Some(yield_params) = range_function_yield_params {
        return compile_range_function_stmt(
            x,
            is_function_item_range,
            range_stmt.key,
            range_stmt.value,
            range_tok,
            yield_params,
            range_function_capture_names,
            body,
        );
    }

    match (range_stmt.key, range_stmt.value) {
        // for i, v := range x
        (Some(key_expr), Some(val_expr)) => {
            let (pat, body) = range_pat_and_body(vec![key_expr, val_expr], range_tok, body)?;
            if is_string {
                // range over string: iterate (byte_index, rune)
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).char_indices().map(|(i, ch)| (i as isize, ch as i32)) },
                    body,
                ))
            } else if is_pointer_array {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! {{
                        let __gors_range_values = (#x).lock().unwrap().iter().cloned().collect::<Vec<_>>();
                        __gors_range_values.into_iter().enumerate().map(|(i, v)| (i as isize, v))
                    }},
                    body,
                ))
            } else if is_any_slice_range_type(&inferred_range_type) {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).into_iter().enumerate().map(|(i, v)| (i as isize, v)) },
                    body,
                ))
            } else if is_indexed_range_type(&inferred_range_type) {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).iter().cloned().enumerate().map(|(i, v)| (i as isize, v)) },
                    body,
                ))
            } else if matches!(range_kind, ir::RangeKind::Map) {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).iter().map(|(k, v)| (k.clone(), v.clone())) },
                    body,
                ))
            } else {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).into_iter().enumerate().map(|(i, v)| (i as isize, v)) },
                    body,
                ))
            }
        }
        // for i := range x  OR  for v := range ch
        (Some(key_expr), None) => {
            let (key_pat, body) = range_pat_and_body(vec![key_expr], range_tok, body)?;
            if is_int {
                Ok(make_for_loop(
                    key_pat,
                    syn::parse_quote! { 0..((#x) as usize) },
                    body,
                ))
            } else {
                // Use into_iter() which works for channels (via IntoIterator) and
                // for slices/vecs (gives values). For index-only iteration over
                // slices, use `for i, _ := range s` instead.
                if is_string {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! { (#x).char_indices().map(|(i, _)| i as isize) },
                        body,
                    ))
                } else if is_pointer_array {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! {
                            0..{
                                let __gors_range_len = crate::builtin::len(&*(#x).lock().unwrap()) as isize;
                                __gors_range_len
                            }
                        },
                        body,
                    ))
                } else if is_indexed_range_type(&inferred_range_type) {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! { 0..(crate::builtin::len(&#x) as isize) },
                        body,
                    ))
                } else if matches!(range_kind, ir::RangeKind::Map) {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! { (#x).keys().cloned() },
                        body,
                    ))
                } else {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! { (#x).into_iter() },
                        body,
                    ))
                }
            }
        }
        // for range x
        (None, None) => {
            let pat: syn::Pat = syn::parse_quote! { _ };
            if is_int {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { 0..((#x) as usize) },
                    body,
                ))
            } else if is_string {
                Ok(make_for_loop(pat, syn::parse_quote! { (#x).chars() }, body))
            } else if is_pointer_array {
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! {
                        0..{
                            let __gors_range_len = crate::builtin::len(&*(#x).lock().unwrap()) as isize;
                            __gors_range_len
                        }
                    },
                    body,
                ))
            } else if matches!(range_kind, ir::RangeKind::Map) {
                Ok(make_for_loop(pat, syn::parse_quote! { (#x).iter() }, body))
            } else {
                Ok(make_for_loop(pat, x, body))
            }
        }
        _ => Err(CompilerError::UnsupportedConstruct(
            "range with value but no key".to_string(),
        )),
    }
}

fn range_pat_and_body(
    targets: Vec<ast::Expr>,
    tok: Option<token::Token>,
    mut body: syn::Block,
) -> Result<(syn::Pat, syn::Block), CompilerError> {
    if tok != Some(token::Token::ASSIGN) {
        let pats: Vec<syn::Pat> = targets.iter().map(expr_to_pat).collect();
        return Ok(match pats.as_slice() {
            [pat] => (pat.clone(), body),
            [key_pat, val_pat] => {
                let pat: syn::Pat = syn::parse_quote! { (#key_pat, #val_pat) };
                (pat, body)
            }
            _ => (syn::parse_quote! { _ }, body),
        });
    }

    let temps: Vec<syn::Ident> = (0..targets.len())
        .map(|idx| syn::Ident::new(&format!("__gors_range_{idx}"), Span::mixed_site()))
        .collect();
    let pat: syn::Pat = match temps.as_slice() {
        [tmp] => syn::parse_quote! { #tmp },
        [key_tmp, val_tmp] => syn::parse_quote! { (#key_tmp, #val_tmp) },
        _ => syn::parse_quote! { _ },
    };

    let assignments = targets
        .into_iter()
        .zip(temps)
        .filter_map(|(target, tmp)| {
            if matches!(&target, ast::Expr::Ident(ident) if ident.name == "_") {
                None
            } else {
                Some(
                    compile_assignment_lhs_checked(target)
                        .map(|lhs| syn::parse_quote! { #lhs = #tmp; }),
                )
            }
        })
        .collect::<Result<Vec<syn::Stmt>, _>>()?;
    body.stmts.splice(0..0, assignments);
    Ok((pat, body))
}

fn range_function_targets<'a>(
    key: Option<ast::Expr<'a>>,
    value: Option<ast::Expr<'a>>,
    yield_arity: usize,
) -> Vec<Option<ast::Expr<'a>>> {
    let mut targets = Vec::with_capacity(yield_arity);
    if yield_arity > 0 {
        targets.push(key);
    }
    if yield_arity > 1 {
        targets.push(value);
    }
    while targets.len() < yield_arity {
        targets.push(None);
    }
    targets
}

fn range_function_param_pat(
    idx: usize,
    target: Option<&ast::Expr>,
    tok: Option<token::Token>,
) -> syn::Pat {
    if tok != Some(token::Token::ASSIGN)
        && let Some(ast::Expr::Ident(ident)) = target
    {
        if ident.name == "_" {
            return syn::parse_quote! { _ };
        }
        let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
        return syn::parse_quote! { mut #ident };
    }
    let ident = quote::format_ident!("__gors_range_arg_{}", idx);
    syn::parse_quote! { mut #ident }
}

fn range_function_assignment_stmts(
    targets: Vec<Option<ast::Expr>>,
    tok: Option<token::Token>,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    if tok != Some(token::Token::ASSIGN) {
        return Ok(Vec::new());
    }
    targets
        .into_iter()
        .enumerate()
        .filter_map(|(idx, target)| {
            let target = target?;
            if matches!(&target, ast::Expr::Ident(ident) if ident.name == "_") {
                return None;
            }
            let right = quote::format_ident!("__gors_range_arg_{}", idx);
            Some(
                compile_assignment_lhs_checked(target)
                    .map(|left| syn::parse_quote! { #left = #right; }),
            )
        })
        .collect()
}

fn range_function_shared_capture_clones(
    names: &std::collections::BTreeSet<String>,
) -> Vec<syn::Stmt> {
    names
        .iter()
        .filter(|name| is_shared_capture_name(name))
        .map(|name| {
            let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
            syn::parse_quote! { let #ident = #ident.clone(); }
        })
        .collect()
}

fn compile_range_function_call_stmt(
    fun_expr: syn::Expr,
    is_function_item: bool,
    yield_value: syn::Expr,
) -> syn::Stmt {
    if is_function_item {
        syn::parse_quote! {
            #fun_expr(#yield_value);
        }
    } else {
        syn::parse_quote! {{
            let __gors_func = {
                let __gors_func = crate::builtin::lock_func(&(#fun_expr));
                match __gors_func.as_ref() {
                    Some(__gors_func) => __gors_func.clone(),
                    None => panic!("nil function"),
                }
            };
            (&*__gors_func)(#yield_value);
        }}
    }
}

#[derive(Clone)]
struct RangeFunctionReturnContext {
    slot_ident: syn::Ident,
    slot_for_yield_ident: syn::Ident,
    return_ty: Option<syn::Type>,
}

fn next_range_function_id() -> usize {
    RANGE_FUNCTION_COUNTER.with(|counter| {
        let mut counter = counter.borrow_mut();
        let id = *counter;
        *counter += 1;
        id
    })
}

fn current_return_ty() -> Option<syn::Type> {
    RETURN_TYPES.with(|types| match types.borrow().as_slice() {
        [] => None,
        [ty] => Some(rust_type_from_inferred_go_type(ty)),
        tys => {
            let elems = tys.iter().map(rust_type_from_inferred_go_type);
            Some(syn::parse_quote! { (#(#elems),*) })
        }
    })
}

fn current_named_return_expr() -> Option<syn::Expr> {
    NAMED_RETURN_IDENTS.with(|idents| {
        let idents = idents.borrow();
        if idents.is_empty() {
            None
        } else {
            Some(named_return_expr(&idents, true))
        }
    })
}

fn range_function_return_context() -> RangeFunctionReturnContext {
    let id = next_range_function_id();
    RangeFunctionReturnContext {
        slot_ident: quote::format_ident!("__gors_range_return_{id}"),
        slot_for_yield_ident: quote::format_ident!("__gors_range_return_for_yield_{id}"),
        return_ty: current_return_ty(),
    }
}

impl RangeFunctionReturnContext {
    fn slot_ty(&self) -> syn::Type {
        self.return_ty
            .clone()
            .unwrap_or_else(|| syn::parse_quote! { () })
    }

    fn slot_stmt(&self) -> syn::Stmt {
        let slot_ident = &self.slot_ident;
        let slot_ty = self.slot_ty();
        syn::parse_quote! {
            let #slot_ident: std::sync::Arc<std::sync::Mutex<Option<#slot_ty>>> =
                std::sync::Arc::new(std::sync::Mutex::new(None));
        }
    }

    fn clone_stmt(&self) -> syn::Stmt {
        let slot_ident = &self.slot_ident;
        let slot_for_yield_ident = &self.slot_for_yield_ident;
        syn::parse_quote! {
            let #slot_for_yield_ident = #slot_ident.clone();
        }
    }

    fn return_value_expr(&self, ret: &syn::ExprReturn) -> syn::Expr {
        if let Some(expr) = &ret.expr {
            return (**expr).clone();
        }
        if self.return_ty.is_some() {
            return current_named_return_expr()
                .unwrap_or_else(|| syn::parse_quote! { Default::default() });
        }
        syn::parse_quote! { () }
    }

    fn signal_return_expr(&self, ret: &syn::ExprReturn) -> syn::Expr {
        let slot_for_yield_ident = &self.slot_for_yield_ident;
        let value = self.return_value_expr(ret);
        syn::parse_quote! {{
            *#slot_for_yield_ident.lock().unwrap() = Some(#value);
            return false;
        }}
    }

    fn after_call_stmt(&self) -> syn::Stmt {
        let slot_ident = &self.slot_ident;
        if self.return_ty.is_some() {
            syn::parse_quote! {
                if let Some(__gors_range_return_value) = #slot_ident.lock().unwrap().take() {
                    return __gors_range_return_value;
                }
            }
        } else {
            syn::parse_quote! {
                if #slot_ident.lock().unwrap().take().is_some() {
                    return;
                }
            }
        }
    }
}

#[derive(Default)]
struct RangeFunctionRewriteResult {
    has_outer_return: bool,
}

impl RangeFunctionRewriteResult {
    fn merge(&mut self, other: Self) {
        self.has_outer_return |= other.has_outer_return;
    }
}

fn rewrite_range_function_control_flow(
    block: &mut syn::Block,
    return_context: &RangeFunctionReturnContext,
) -> RangeFunctionRewriteResult {
    rewrite_range_function_control_flow_block(block, return_context, true)
}

fn rewrite_range_function_control_flow_block(
    block: &mut syn::Block,
    return_context: &RangeFunctionReturnContext,
    rewrite_loop_control: bool,
) -> RangeFunctionRewriteResult {
    let mut result = RangeFunctionRewriteResult::default();
    for stmt in &mut block.stmts {
        result.merge(rewrite_range_function_control_flow_stmt(
            stmt,
            return_context,
            rewrite_loop_control,
        ));
    }
    result
}

fn rewrite_range_function_control_flow_stmt(
    stmt: &mut syn::Stmt,
    return_context: &RangeFunctionReturnContext,
    rewrite_loop_control: bool,
) -> RangeFunctionRewriteResult {
    match stmt {
        syn::Stmt::Expr(syn::Expr::Break(expr), _)
            if rewrite_loop_control && expr.label.is_none() =>
        {
            *stmt = syn::parse_quote! { return false; };
            RangeFunctionRewriteResult::default()
        }
        syn::Stmt::Expr(syn::Expr::Continue(expr), _)
            if rewrite_loop_control && expr.label.is_none() =>
        {
            *stmt = syn::parse_quote! { return true; };
            RangeFunctionRewriteResult::default()
        }
        syn::Stmt::Expr(expr, _) => {
            rewrite_range_function_control_flow_expr(expr, return_context, rewrite_loop_control)
        }
        syn::Stmt::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_range_function_control_flow_expr(
                    &mut init.expr,
                    return_context,
                    rewrite_loop_control,
                )
            } else {
                RangeFunctionRewriteResult::default()
            }
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => RangeFunctionRewriteResult::default(),
    }
}

fn rewrite_range_function_control_flow_expr(
    expr: &mut syn::Expr,
    return_context: &RangeFunctionReturnContext,
    rewrite_loop_control: bool,
) -> RangeFunctionRewriteResult {
    match expr {
        syn::Expr::Return(ret) => {
            *expr = return_context.signal_return_expr(ret);
            RangeFunctionRewriteResult {
                has_outer_return: true,
            }
        }
        syn::Expr::Break(break_expr) if rewrite_loop_control && break_expr.label.is_none() => {
            *expr = syn::parse_quote! { return false };
            RangeFunctionRewriteResult::default()
        }
        syn::Expr::Continue(continue_expr)
            if rewrite_loop_control && continue_expr.label.is_none() =>
        {
            *expr = syn::parse_quote! { return true };
            RangeFunctionRewriteResult::default()
        }
        syn::Expr::If(if_expr) => {
            let mut result = rewrite_range_function_control_flow_block(
                &mut if_expr.then_branch,
                return_context,
                rewrite_loop_control,
            );
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                result.merge(rewrite_range_function_control_flow_expr(
                    else_expr,
                    return_context,
                    rewrite_loop_control,
                ));
            }
            result
        }
        syn::Expr::Block(block) => rewrite_range_function_control_flow_block(
            &mut block.block,
            return_context,
            rewrite_loop_control,
        ),
        syn::Expr::Match(match_expr) => {
            let mut result = RangeFunctionRewriteResult::default();
            for arm in &mut match_expr.arms {
                result.merge(rewrite_range_function_control_flow_expr(
                    &mut arm.body,
                    return_context,
                    rewrite_loop_control,
                ));
            }
            result
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_range_function_control_flow_block(&mut loop_expr.body, return_context, false)
        }
        syn::Expr::While(while_expr) => {
            rewrite_range_function_control_flow_block(&mut while_expr.body, return_context, false)
        }
        syn::Expr::ForLoop(for_expr) => {
            rewrite_range_function_control_flow_block(&mut for_expr.body, return_context, false)
        }
        syn::Expr::Closure(_) => RangeFunctionRewriteResult::default(),
        _ => RangeFunctionRewriteResult::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_range_function_stmt(
    fun: syn::Expr,
    is_function_item: bool,
    key: Option<ast::Expr>,
    value: Option<ast::Expr>,
    tok: Option<token::Token>,
    yield_params: Vec<typeinfer::GoType>,
    shared_capture_names: std::collections::BTreeSet<String>,
    mut body: syn::Block,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    if yield_params.len() > 2 {
        return Err(CompilerError::UnsupportedConstruct(
            "range over function with more than two yield values".to_string(),
        ));
    }

    let targets = range_function_targets(key, value, yield_params.len());
    let param_pats: Vec<syn::Pat> = targets
        .iter()
        .enumerate()
        .map(|(idx, target)| range_function_param_pat(idx, target.as_ref(), tok))
        .collect();
    let assignments = range_function_assignment_stmts(targets, tok)?;
    if !assignments.is_empty() {
        let mut stmts = assignments;
        stmts.extend(body.stmts);
        body.stmts = stmts;
    }
    let return_context = range_function_return_context();
    let rewrite_result = rewrite_range_function_control_flow(&mut body, &return_context);
    let param_types: Vec<syn::Type> = yield_params
        .iter()
        .map(rust_type_from_inferred_go_type)
        .collect();
    let typed_params: Vec<proc_macro2::TokenStream> = param_pats
        .iter()
        .zip(param_types.iter())
        .map(|(pat, ty)| quote::quote! { #pat: #ty })
        .collect();
    let yield_go_type = typeinfer::GoType::Func {
        params: yield_params,
        results: vec![typeinfer::GoType::Bool],
        variadic_start: None,
    };
    let yield_ty = shared_func_box_type_from_go_type(&yield_go_type).ok_or_else(|| {
        CompilerError::InvalidFunctionSignature("invalid range function yield type".to_string())
    })?;
    let return_clone_stmt = rewrite_result
        .has_outer_return
        .then(|| return_context.clone_stmt());
    let shared_capture_clones = range_function_shared_capture_clones(&shared_capture_names);
    let yield_value: syn::Expr = syn::parse_quote! {{
        #return_clone_stmt
        #(#shared_capture_clones)*
        let __gors_func: #yield_ty = std::sync::Arc::new(move |#(#typed_params),*| -> bool {
            #body
            true
        });
        std::sync::Arc::new(std::sync::Mutex::new(Some(__gors_func)))
    }};
    let call_stmt = compile_range_function_call_stmt(fun, is_function_item, yield_value);
    if rewrite_result.has_outer_return {
        Ok(vec![
            return_context.slot_stmt(),
            call_stmt,
            return_context.after_call_stmt(),
        ])
    } else {
        Ok(vec![call_stmt])
    }
}

fn is_indexed_range_type(ty: &typeinfer::GoType) -> bool {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        matches!(
            env.resolve_alias(ty),
            typeinfer::GoType::Slice(_) | typeinfer::GoType::Array(_)
        )
    })
}

fn is_pointer_array_range_type(ty: &typeinfer::GoType) -> bool {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        matches!(
            env.resolve_alias(ty),
            typeinfer::GoType::Pointer(inner) if matches!(inner.as_ref(), typeinfer::GoType::Array(_))
        )
    })
}

fn is_any_slice_range_type(ty: &typeinfer::GoType) -> bool {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        matches!(env.resolve_alias(ty), typeinfer::GoType::Slice(elem) if elem.is_interface())
    })
}

fn expr_to_pat(expr: &ast::Expr) -> syn::Pat {
    match expr {
        ast::Expr::Ident(ident) if ident.name == "_" => syn::parse_quote! { _ },
        ast::Expr::Ident(ident) => {
            let name = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
            syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: Some(<Token![mut]>::default()),
                ident: name,
            })
        }
        _ => syn::parse_quote! { _ },
    }
}

fn infer_static_type_from_init(expr: &ast::Expr) -> Option<syn::Type> {
    match expr {
        ast::Expr::CompositeLit(comp_lit) => comp_lit.type_.as_ref().map(|type_expr| {
            if let ast::Expr::ArrayType(array_type) = &**type_expr {
                type_from_array_lit_ref(array_type, comp_lit.elts.as_deref().unwrap_or(&[]))
            } else {
                type_from_expr_ref(type_expr)
            }
        }),
        ast::Expr::UnaryExpr(unary) if unary.op == token::Token::AND => {
            infer_static_type_from_init(&unary.x)
                .map(|inner| syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#inner>> })
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::STRING => {
            Some(syn::parse_quote! { String })
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::FLOAT => {
            Some(syn::parse_quote! { f64 })
        }
        ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT => {
            Some(syn::parse_quote! { isize })
        }
        ast::Expr::Ident(id) if id.name == "true" || id.name == "false" => {
            Some(syn::parse_quote! { bool })
        }
        ast::Expr::Ident(id) => TYPE_ENV.with(|env| {
            env.borrow()
                .get_var(id.name)
                .filter(|ty| !matches!(ty, typeinfer::GoType::Unknown))
                .map(|ty| rust_type_from_inferred_go_type(&ty))
        }),
        _ => None,
    }
}

fn type_from_array_lit_ref(array_type: &ast::ArrayType, elts: &[ast::Expr]) -> syn::Type {
    let elem = match &*array_type.elt {
        ast::Expr::ArrayType(nested) => type_from_array_lit_ref(nested, &[]),
        other => type_from_expr_ref(other),
    };
    if let Some(len) = &array_type.len {
        let len_expr = array_literal_len_expr(len, elts);
        syn::parse_quote! { [#elem; #len_expr] }
    } else {
        syn::parse_quote! { Vec<#elem> }
    }
}

fn compile_top_level_value_spec(
    vs: ast::ValueSpec,
    tok: token::Token,
) -> Result<Vec<syn::Item>, CompilerError> {
    let mut items = vec![];
    let mut values_iter = vs.values.unwrap_or_default().into_iter();

    for name in vs.names {
        let init_ast = values_iter.next();
        if name.name == "_" {
            continue;
        }

        let vis: syn::Visibility = (&name).into();
        let ident: syn::Ident = name.into();
        let inferred_type = vs
            .type_
            .as_ref()
            .map(static_value_type_from_expr)
            .or_else(|| init_ast.as_ref().and_then(infer_static_type_from_init));
        let init = init_ast.map(syn::Expr::from);

        if tok == token::Token::CONST {
            let mut value = init.unwrap_or_else(|| syn::parse_quote! { 0 });
            let explicit_go_type = vs.type_.as_ref().map(typeinfer::GoType::from_expr);
            let explicit_alias_name = match vs.type_.as_ref() {
                Some(ast::Expr::Ident(id)) => Some(id.name),
                _ => None,
            };
            let ty: syn::Type = if matches!(explicit_go_type, Some(typeinfer::GoType::String)) {
                syn::parse_quote! { &str }
            } else if let Some(type_expr) = vs.type_.as_ref() {
                type_from_expr_ref(type_expr)
            } else if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(_),
                ..
            }) = &value
            {
                syn::parse_quote! { &str }
            } else if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Float(_),
                ..
            }) = &value
            {
                syn::parse_quote! { f64 }
            } else if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Bool(_),
                ..
            }) = &value
            {
                syn::parse_quote! { bool }
            } else {
                syn::parse_quote! { isize }
            };

            if matches!(explicit_go_type, Some(typeinfer::GoType::String))
                && !matches!(
                    &value,
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(_),
                        ..
                    })
                )
            {
                value = syn::parse_quote! { "" };
            } else if let Some(alias_name) = explicit_alias_name {
                let is_alias = TYPE_ENV.with(|env| {
                    matches!(
                        env.borrow().get_type_kind(alias_name),
                        Some(typeinfer::TypeKind::Alias(_))
                    )
                });
                if is_alias {
                    value = syn::parse_quote! { #ty(#value) };
                }
            }

            items.push(syn::parse_quote! {
                #vis const #ident: #ty = #value;
            });
        } else {
            let mut value = init.unwrap_or_else(|| go_zero_value(vs.type_.as_ref()));
            let mut ty: syn::Type =
                inferred_type.unwrap_or_else(|| syn::parse_quote! { Box<dyn std::any::Any> });
            if is_any_type(&ty) {
                ty = syn::parse_quote! { Box<dyn std::any::Any + Send + Sync> };
                if is_box_dyn_any_expr(&value) {
                    value =
                        syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any + Send + Sync> };
                } else if !is_box_dyn_any_cast_expr(&value) {
                    value = syn::parse_quote! {
                        Box::new(#value) as Box<dyn std::any::Any + Send + Sync>
                    };
                }
            }
            items.push(syn::parse_quote! {
                #[allow(non_upper_case_globals)]
                #vis static #ident: std::sync::LazyLock<#ty> = std::sync::LazyLock::new(|| #value);
            });
        }
    }
    Ok(items)
}

impl From<ast::BasicLit<'_>> for syn::ExprLit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        // Record mapping for the literal
        record_mapping(&basic_lit.value_pos, Some(basic_lit.value));

        Self {
            attrs: vec![],
            lit: basic_lit.into(),
        }
    }
}

fn compile_basic_lit_expr(basic_lit: ast::BasicLit) -> syn::Expr {
    if basic_lit.kind == token::Token::IMAG {
        record_mapping(&basic_lit.value_pos, Some(basic_lit.value));
        return imaginary_literal_expr(&basic_lit).unwrap_or_else(|| {
            compile_error_expr(format!(
                "unsupported imaginary literal: {}",
                basic_lit.value
            ))
        });
    }
    syn::Expr::Lit(basic_lit.into())
}

impl From<ast::BasicLit<'_>> for syn::Lit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        use token::Token::*;
        match basic_lit.kind {
            INT => Self::Int(syn::LitInt::new(basic_lit.value, Span::mixed_site())),
            STRING => {
                let raw = basic_lit.value;
                let inner = &raw[1..raw.len() - 1];
                if raw.starts_with('`') {
                    Self::Str(syn::LitStr::new(inner, Span::mixed_site()))
                } else {
                    let interpreted = interpret_go_string_escapes(inner);
                    Self::Str(syn::LitStr::new(&interpreted, Span::mixed_site()))
                }
            }
            CHAR => {
                let raw = basic_lit.value;
                let inner = &raw[1..raw.len() - 1];
                let interpreted = interpret_go_string_escapes(inner);
                let ch = interpreted.chars().next().unwrap_or(' ');
                // Emit as integer since Go's rune is int32, not Rust's char.
                let value = ch as i32;
                let lit_str = format!("{value}");
                Self::Int(syn::LitInt::new(&lit_str, Span::mixed_site()))
            }
            FLOAT => {
                let value = if basic_lit.value.starts_with('.') {
                    format!("0{}", basic_lit.value)
                } else if basic_lit.value.ends_with('.') {
                    format!("{}0", basic_lit.value)
                } else {
                    basic_lit.value.to_string()
                };
                Self::Float(syn::LitFloat::new(&value, Span::mixed_site()))
            }
            _ => {
                // Return a placeholder for unsupported literals
                Self::Str(syn::LitStr::new(
                    &format!("/* unsupported literal: {:?} */", basic_lit.kind),
                    Span::mixed_site(),
                ))
            }
        }
    }
}

impl From<ast::BinaryExpr<'_>> for syn::ExprBinary {
    fn from(binary_expr: ast::BinaryExpr) -> Self {
        // Record mapping for the operator
        let op_str: &'static str = (&binary_expr.op).into();
        record_mapping(&binary_expr.op_pos, Some(op_str));

        if binary_expr.op == token::Token::AND_NOT {
            let x = syn::Expr::from(*binary_expr.x);
            let y = syn::Expr::from(*binary_expr.y);
            let not_y = syn::Expr::Unary(syn::ExprUnary {
                attrs: vec![],
                op: syn::UnOp::Not(<Token![!]>::default()),
                expr: Box::new(y),
            });
            return Self {
                attrs: vec![],
                left: Box::new(x),
                op: syn::BinOp::BitAnd(<Token![&]>::default()),
                right: Box::new(not_y),
            };
        }

        let (x, op, y) = (
            syn::Expr::from(*binary_expr.x),
            syn::BinOp::from(binary_expr.op),
            syn::Expr::from(*binary_expr.y),
        );
        Self {
            attrs: vec![],
            left: Box::new(x),
            op,
            right: Box::new(y),
        }
    }
}

fn is_nil_expr(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::Ident(id) if id.name == "nil")
}

fn import_selector_assignment_expr(expr: &ast::Expr) -> Option<syn::Expr> {
    match expr {
        ast::Expr::SelectorExpr(selector) if matches!(&*selector.x, ast::Expr::Ident(id) if IMPORT_NAMES.with(|names| names.borrow().contains(id.name))) =>
        {
            let ast::Expr::Ident(pkg) = &*selector.x else {
                return None;
            };
            let module = syn::Ident::new(&import_rust_name(pkg.name), Span::mixed_site());
            let sel = syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
            Some(syn::parse_quote! { #module::#sel })
        }
        _ => None,
    }
}

fn compile_assignment_lhs_checked(expr: ast::Expr) -> Result<syn::Expr, CompilerError> {
    if let Some(expr) = import_selector_assignment_expr(&expr) {
        return Ok(expr);
    }
    if !is_ir_addressable_expr(&expr) {
        let shape = expr_shape_key(&expr).unwrap_or_else(|| format!("{expr:?}"));
        return Err(CompilerError::InvalidAssignment(format!(
            "lhs is not addressable: {shape}"
        )));
    }
    Ok(compile_assignment_lhs(expr))
}

fn compile_assignment_lhs(expr: ast::Expr) -> syn::Expr {
    import_selector_assignment_expr(&expr)
        .or_else(|| lvalue_expr_from_ref(&expr))
        .unwrap_or_else(|| expr.into())
}

fn selector_field_go_type(selector: &ast::SelectorExpr) -> Option<typeinfer::GoType> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let base_ty = typeinfer::GoType::infer_expr(&selector.x, &env);
        match env.resolve_alias(&base_ty) {
            typeinfer::GoType::Named(name) => Some(env.get_field_type(&name, selector.sel.name)),
            typeinfer::GoType::Pointer(inner) => match *inner {
                typeinfer::GoType::Named(name) => {
                    Some(env.get_field_type(&name, selector.sel.name))
                }
                _ => None,
            },
            _ => None,
        }
    })
}

fn should_coerce_numeric_binary_side(
    expr: &ast::Expr,
    expr_ty: &typeinfer::GoType,
    other_ty: &typeinfer::GoType,
) -> bool {
    let expr_ty = resolved_go_type(expr_ty);
    let other_ty = resolved_go_type(other_ty);
    if expr_ty == other_ty || numeric_cast_type(&other_ty).is_none() {
        return false;
    }
    if matches!(expr_ty, typeinfer::GoType::Unknown) {
        return is_const_like_expr(expr);
    }
    if !expr_ty.is_numeric() && !matches!(expr_ty, typeinfer::GoType::Uintptr) {
        return false;
    }
    is_const_like_expr(expr)
}

fn is_const_like_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::BasicLit(_) => true,
        ast::Expr::Ident(ident) if matches!(ident.name, "true" | "false" | "iota") => true,
        ast::Expr::Ident(ident) => TYPE_ENV.with(|env| env.borrow().is_const(ident.name)),
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(base) = &*selector.x {
                let key = format!("{}.{}", base.name, selector.sel.name);
                TYPE_ENV.with(|env| env.borrow().is_const(&key))
            } else {
                false
            }
        }
        ast::Expr::ParenExpr(paren) => is_const_like_expr(&paren.x),
        ast::Expr::UnaryExpr(unary) => is_const_like_expr(&unary.x),
        ast::Expr::BinaryExpr(binary) => {
            is_const_like_expr(&binary.x) && is_const_like_expr(&binary.y)
        }
        ast::Expr::CallExpr(_) => {
            const_eval_expr_in_active_env(expr, 0, &BTreeMap::new()).is_some()
        }
        _ => false,
    }
}

fn is_flattenable_binary_op(op: token::Token) -> bool {
    matches!(
        op,
        token::Token::ADD
            | token::Token::MUL
            | token::Token::AND
            | token::Token::OR
            | token::Token::XOR
            | token::Token::LAND
            | token::Token::LOR
    )
}

fn flatten_same_binary_operands(
    binary_expr: ast::BinaryExpr,
) -> Result<(token::Token, Vec<ast::Expr>), ast::BinaryExpr> {
    let op = binary_expr.op;
    if !is_flattenable_binary_op(op) {
        return Err(binary_expr);
    }

    let mut operands = Vec::new();
    let mut stack = vec![ast::Expr::BinaryExpr(binary_expr)];
    while let Some(expr) = stack.pop() {
        match expr {
            ast::Expr::BinaryExpr(binary) if binary.op == op => {
                stack.push(*binary.y);
                stack.push(*binary.x);
            }
            other => operands.push(other),
        }
    }

    if operands.len() <= 16 {
        let mut iter = operands.into_iter();
        let Some(mut expr) = iter.next() else {
            return Ok((op, Vec::new()));
        };
        for right in iter {
            expr = ast::Expr::BinaryExpr(ast::BinaryExpr {
                x: Box::new(expr),
                op_pos: token::Position::default(),
                op,
                y: Box::new(right),
            });
        }
        let ast::Expr::BinaryExpr(binary) = expr else {
            return Ok((op, vec![expr]));
        };
        return Err(binary);
    }

    Ok((op, operands))
}

fn compile_flat_binary_operands(
    op: token::Token,
    operands: Vec<ast::Expr>,
    expected: Option<&typeinfer::GoType>,
) -> syn::Expr {
    let mut iter = operands.into_iter();
    let Some(first) = iter.next() else {
        return syn::parse_quote! { Default::default() };
    };
    let mut expr = match expected {
        Some(expected) => compile_expr_with_expected(first, Some(expected)),
        None => syn::Expr::from(first),
    };
    let op: syn::BinOp = op.into();
    for operand in iter {
        let right = match expected {
            Some(expected) => compile_expr_with_expected(operand, Some(expected)),
            None => syn::Expr::from(operand),
        };
        expr = syn::Expr::Binary(syn::ExprBinary {
            attrs: vec![],
            left: Box::new(expr),
            op,
            right: Box::new(right),
        });
    }
    expr
}

fn compile_binary_side(
    expr: ast::Expr,
    expr_ty: &typeinfer::GoType,
    other_ty: &typeinfer::GoType,
) -> syn::Expr {
    if is_complex_go_type(other_ty)
        && is_const_like_expr(&expr)
        && is_complex_const_conversion_source(expr_ty)
    {
        let compiled: syn::Expr = expr.into();
        return coerce_complex_const_expr(other_ty, expr_ty, compiled)
            .unwrap_or_else(|| compile_error_expr("unsupported complex binary operand"));
    }
    if should_coerce_numeric_binary_side(&expr, expr_ty, other_ty) {
        compile_expr_with_expected(expr, Some(other_ty))
    } else {
        expr.into()
    }
}

enum BinarySide {
    Left,
    Right,
}

fn rust_binary_precedence(op: token::Token) -> u8 {
    match op {
        token::Token::LOR => 1,
        token::Token::LAND => 2,
        token::Token::EQL
        | token::Token::NEQ
        | token::Token::LSS
        | token::Token::LEQ
        | token::Token::GTR
        | token::Token::GEQ => 3,
        token::Token::OR => 4,
        token::Token::XOR => 5,
        token::Token::AND | token::Token::AND_NOT => 6,
        token::Token::SHL | token::Token::SHR => 7,
        token::Token::ADD | token::Token::SUB => 8,
        token::Token::MUL | token::Token::QUO | token::Token::REM => 9,
        _ => 0,
    }
}

fn parenthesize_expr(expr: syn::Expr) -> syn::Expr {
    syn::Expr::Paren(syn::ExprParen {
        attrs: vec![],
        paren_token: syn::token::Paren::default(),
        expr: Box::new(expr),
    })
}

fn parenthesize_binary_child_if_needed(
    compiled: syn::Expr,
    child_op: Option<token::Token>,
    parent_op: token::Token,
    side: BinarySide,
) -> syn::Expr {
    let Some(child_op) = child_op else {
        return compiled;
    };
    let child_precedence = rust_binary_precedence(child_op);
    let parent_precedence = rust_binary_precedence(parent_op);
    let needs_parens = match side {
        BinarySide::Left => child_precedence < parent_precedence,
        BinarySide::Right => child_precedence <= parent_precedence,
    };
    if needs_parens {
        parenthesize_expr(compiled)
    } else {
        compiled
    }
}

fn compile_binary_side_for_parent(
    expr: ast::Expr,
    expr_ty: &typeinfer::GoType,
    other_ty: &typeinfer::GoType,
    parent_op: token::Token,
    side: BinarySide,
) -> syn::Expr {
    let child_op = match &expr {
        ast::Expr::BinaryExpr(binary) => Some(binary.op),
        _ => None,
    };
    let compiled = compile_binary_side(expr, expr_ty, other_ty);
    parenthesize_binary_child_if_needed(compiled, child_op, parent_op, side)
}

fn is_string_concat_binary_expr(binary_expr: &ast::BinaryExpr) -> bool {
    TYPE_ENV.with(|env| ir::is_string_concat_binary_expr(binary_expr, &env.borrow()))
}

fn collect_string_concat_operands<'src>(
    expr: ast::Expr<'src>,
    operands: &mut Vec<ast::Expr<'src>>,
) {
    match expr {
        ast::Expr::BinaryExpr(binary) if binary.op == token::Token::ADD => {
            collect_string_concat_operands(*binary.x, operands);
            collect_string_concat_operands(*binary.y, operands);
        }
        other => operands.push(other),
    }
}

fn compile_string_concat_binary_expr(binary_expr: ast::BinaryExpr) -> syn::Expr {
    let mut operands = Vec::new();
    collect_string_concat_operands(ast::Expr::BinaryExpr(binary_expr), &mut operands);
    let parts: Vec<syn::Expr> = operands
        .into_iter()
        .map(|operand| compile_expr_with_expected(operand, Some(&typeinfer::GoType::String)))
        .collect();
    syn::parse_quote! {{
        let mut __gors_string = std::string::String::new();
        #(
            let __gors_part = #parts;
            __gors_string.push_str(&__gors_part);
        )*
        __gors_string
    }}
}

fn is_self_referential_pointer_selector(expr: &ast::Expr, ty: &typeinfer::GoType) -> bool {
    let ast::Expr::SelectorExpr(selector) = expr else {
        return false;
    };
    let typeinfer::GoType::Pointer(inner) = resolved_go_type(ty) else {
        return false;
    };
    let typeinfer::GoType::Named(inner_name) = *inner else {
        return false;
    };
    let base_type = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&selector.x, &env.borrow()));
    match resolved_go_type(&base_type) {
        typeinfer::GoType::Named(base_name) => base_name == inner_name,
        typeinfer::GoType::Pointer(base_inner) => {
            matches!(*base_inner, typeinfer::GoType::Named(base_name) if base_name == inner_name)
        }
        _ => false,
    }
}

fn compile_binary_expr(binary_expr: ast::BinaryExpr) -> syn::Expr {
    let op = binary_expr.op;
    if let Some(compare) = detect_reflect_kind_compare(&binary_expr) {
        let is_eq = op == token::Token::EQL;
        let value = match compare.side {
            ReflectKindCompareSide::Left => reflect_typeof_kind_arg(*binary_expr.x),
            ReflectKindCompareSide::Right => reflect_typeof_kind_arg(*binary_expr.y),
        };
        let Some(value) = value else {
            return compile_error_expr("unsupported reflect TypeOf Kind expression");
        };
        let Some(kind) = reflect_kind_variant_expr(&compare.kind) else {
            return compile_error_expr("unsupported reflect Kind constant");
        };
        let value: syn::Expr = value.into();
        let check: syn::Expr = syn::parse_quote! {
            crate::builtin::reflect_kind_is(&#value, #kind)
        };
        return if is_eq {
            check
        } else {
            syn::parse_quote! { !(#check) }
        };
    }

    if matches!(op, token::Token::EQL | token::Token::NEQ)
        && (is_nil_expr(&binary_expr.x) || is_nil_expr(&binary_expr.y))
    {
        let left_nil = is_nil_expr(&binary_expr.x);
        let other = if left_nil {
            &binary_expr.y
        } else {
            &binary_expr.x
        };
        let other_ty = typeinfer::GoType::infer_expr(other, &TYPE_ENV.with(|e| e.borrow().clone()));
        let is_eq = op == token::Token::EQL;

        if matches!(other_ty, typeinfer::GoType::Error) {
            let other_expr = if left_nil {
                syn::Expr::from(*binary_expr.y)
            } else {
                syn::Expr::from(*binary_expr.x)
            };
            return if is_eq {
                syn::parse_quote! { (#other_expr).is_empty() }
            } else {
                syn::parse_quote! { !(#other_expr).is_empty() }
            };
        }

        if other_ty.is_interface() {
            return if is_eq {
                syn::parse_quote! { false }
            } else {
                syn::parse_quote! { true }
            };
        }

        if is_go_byte_slice_type(&other_ty) {
            let other_expr = if left_nil {
                syn::Expr::from(*binary_expr.y)
            } else {
                syn::Expr::from(*binary_expr.x)
            };
            return if is_eq {
                syn::parse_quote! { (#other_expr).is_empty() }
            } else {
                syn::parse_quote! { !(#other_expr).is_empty() }
            };
        }

        if other_ty.is_integer() || matches!(other_ty, typeinfer::GoType::Uintptr) {
            let other_expr = if left_nil {
                syn::Expr::from(*binary_expr.y)
            } else {
                syn::Expr::from(*binary_expr.x)
            };
            return if is_eq {
                syn::parse_quote! { #other_expr == 0 }
            } else {
                syn::parse_quote! { #other_expr != 0 }
            };
        }

        if matches!(resolved_go_type(&other_ty), typeinfer::GoType::Pointer(_)) {
            let is_self_referential = is_self_referential_pointer_selector(other, &other_ty);
            let other_expr = if left_nil {
                syn::Expr::from(*binary_expr.y)
            } else {
                syn::Expr::from(*binary_expr.x)
            };
            if is_self_referential {
                return if is_eq {
                    syn::parse_quote! { #other_expr == Default::default() }
                } else {
                    syn::parse_quote! { #other_expr != Default::default() }
                };
            }
            return if is_eq {
                syn::parse_quote! { #other_expr.lock().unwrap().clone() == Default::default() }
            } else {
                syn::parse_quote! { #other_expr.lock().unwrap().clone() != Default::default() }
            };
        }
    }

    if is_string_concat_binary_expr(&binary_expr) {
        return compile_string_concat_binary_expr(binary_expr);
    }

    let env = TYPE_ENV.with(|e| e.borrow().clone());
    let left_ty = typeinfer::GoType::infer_expr(&binary_expr.x, &env);
    let right_ty = typeinfer::GoType::infer_expr(&binary_expr.y, &env);
    let has_complex_side = is_complex_go_type(&left_ty) || is_complex_go_type(&right_ty);
    let binary_expr = if has_complex_side {
        binary_expr
    } else {
        match flatten_same_binary_operands(binary_expr) {
            Ok((op, operands)) => return compile_flat_binary_operands(op, operands, None),
            Err(binary_expr) => binary_expr,
        }
    };
    let original_op = binary_expr.op;
    let op: syn::BinOp = original_op.into();
    let is_shift = matches!(original_op, token::Token::SHL | token::Token::SHR);
    let left = if is_shift {
        (*binary_expr.x).into()
    } else {
        compile_binary_side_for_parent(
            *binary_expr.x,
            &left_ty,
            &right_ty,
            original_op,
            BinarySide::Left,
        )
    };
    let right = compile_binary_side_for_parent(
        *binary_expr.y,
        &right_ty,
        &left_ty,
        original_op,
        BinarySide::Right,
    );
    if original_op == token::Token::AND_NOT {
        let not_right = syn::Expr::Unary(syn::ExprUnary {
            attrs: vec![],
            op: syn::UnOp::Not(<Token![!]>::default()),
            expr: Box::new(right),
        });
        return syn::Expr::Binary(syn::ExprBinary {
            attrs: vec![],
            left: Box::new(left),
            op: syn::BinOp::BitAnd(<Token![&]>::default()),
            right: Box::new(not_right),
        });
    }
    syn::Expr::Binary(syn::ExprBinary {
        attrs: vec![],
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

fn is_numeric_value_binary_op(op: token::Token) -> bool {
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
            | token::Token::SHL
            | token::Token::SHR
    )
}

fn compile_numeric_binary_expr_with_expected(
    binary_expr: ast::BinaryExpr,
    expected: &typeinfer::GoType,
) -> syn::Expr {
    let binary_expr = match flatten_same_binary_operands(binary_expr) {
        Ok((op, operands)) => {
            return compile_flat_binary_operands(op, operands, Some(expected));
        }
        Err(binary_expr) => binary_expr,
    };
    let op = binary_expr.op;
    let left_child_op = match &*binary_expr.x {
        ast::Expr::BinaryExpr(binary) => Some(binary.op),
        _ => None,
    };
    let right_child_op = match &*binary_expr.y {
        ast::Expr::BinaryExpr(binary) => Some(binary.op),
        _ => None,
    };
    let left = parenthesize_binary_child_if_needed(
        compile_expr_with_expected(*binary_expr.x, Some(expected)),
        left_child_op,
        op,
        BinarySide::Left,
    );
    let right = if matches!(op, token::Token::SHL | token::Token::SHR) {
        syn::Expr::from(*binary_expr.y)
    } else {
        compile_expr_with_expected(*binary_expr.y, Some(expected))
    };
    let right = parenthesize_binary_child_if_needed(right, right_child_op, op, BinarySide::Right);
    if op == token::Token::AND_NOT {
        let not_right: syn::Expr = syn::parse_quote! { !#right };
        return syn::parse_quote! { #left & #not_right };
    }
    let op: syn::BinOp = op.into();
    syn::parse_quote! { #left #op #right }
}

struct ReflectKindCompare {
    side: ReflectKindCompareSide,
    kind: String,
}

enum ReflectKindCompareSide {
    Left,
    Right,
}

fn detect_reflect_kind_compare(binary_expr: &ast::BinaryExpr) -> Option<ReflectKindCompare> {
    if !matches!(binary_expr.op, token::Token::EQL | token::Token::NEQ) {
        return None;
    }

    if reflect_typeof_kind_arg_ref(&binary_expr.x) {
        let kind = reflect_kind_const_ref(&binary_expr.y)?;
        reflect_kind_variant_expr(kind)?;
        return Some(ReflectKindCompare {
            side: ReflectKindCompareSide::Left,
            kind: kind.to_string(),
        });
    }

    if reflect_typeof_kind_arg_ref(&binary_expr.y) {
        let kind = reflect_kind_const_ref(&binary_expr.x)?;
        reflect_kind_variant_expr(kind)?;
        return Some(ReflectKindCompare {
            side: ReflectKindCompareSide::Right,
            kind: kind.to_string(),
        });
    }

    None
}

fn reflect_typeof_kind_arg_ref(expr: &ast::Expr) -> bool {
    let ast::Expr::CallExpr(kind_call) = expr else {
        return false;
    };
    if kind_call.args.as_ref().is_some_and(|args| !args.is_empty()) {
        return false;
    }
    let ast::Expr::SelectorExpr(kind_selector) = &*kind_call.fun else {
        return false;
    };
    if kind_selector.sel.name != "Kind" {
        return false;
    }
    let ast::Expr::CallExpr(type_of_call) = &*kind_selector.x else {
        return false;
    };
    let ast::Expr::SelectorExpr(type_of_selector) = &*type_of_call.fun else {
        return false;
    };
    matches!(&*type_of_selector.x, ast::Expr::Ident(pkg) if pkg.name == "reflect")
        && type_of_selector.sel.name == "TypeOf"
        && matches!(type_of_call.args.as_deref(), Some([_]))
}

fn reflect_typeof_kind_arg(expr: ast::Expr) -> Option<ast::Expr> {
    let ast::Expr::CallExpr(kind_call) = expr else {
        return None;
    };
    let ast::Expr::SelectorExpr(kind_selector) = *kind_call.fun else {
        return None;
    };
    let ast::Expr::CallExpr(type_of_call) = *kind_selector.x else {
        return None;
    };
    let mut args = type_of_call.args?;
    if args.len() == 1 {
        Some(args.remove(0))
    } else {
        None
    }
}

fn reflect_kind_const_ref<'ast>(expr: &ast::Expr<'ast>) -> Option<&'ast str> {
    let ast::Expr::SelectorExpr(selector) = expr else {
        return None;
    };
    if !matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "reflect") {
        return None;
    }
    Some(selector.sel.name)
}

fn reflect_kind_variant_expr(name: &str) -> Option<syn::Expr> {
    match name {
        "Invalid" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Invalid }),
        "Bool" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Bool }),
        "Int" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int }),
        "Int8" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int8 }),
        "Int16" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int16 }),
        "Int32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int32 }),
        "Int64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int64 }),
        "Uint" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint }),
        "Uint8" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint8 }),
        "Uint16" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint16 }),
        "Uint32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint32 }),
        "Uint64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint64 }),
        "Uintptr" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uintptr }),
        "Float32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Float32 }),
        "Float64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Float64 }),
        "Complex64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Complex64 }),
        "Complex128" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Complex128 }),
        "Array" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Array }),
        "Chan" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Chan }),
        "Func" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Func }),
        "Interface" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Interface }),
        "Map" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Map }),
        "Pointer" | "Ptr" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Pointer }),
        "Slice" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Slice }),
        "String" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::String }),
        "Struct" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Struct }),
        "UnsafePointer" => {
            Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::UnsafePointer })
        }
        _ => None,
    }
}

fn is_type_arg_expr(expr: &ast::Expr) -> bool {
    matches!(
        expr,
        ast::Expr::Ident(_)
            | ast::Expr::SelectorExpr(_)
            | ast::Expr::StarExpr(_)
            | ast::Expr::ArrayType(_)
            | ast::Expr::MapType(_)
            | ast::Expr::InterfaceType(_)
            | ast::Expr::StructType(_)
            | ast::Expr::FuncType(_)
            | ast::Expr::IndexExpr(_)
            | ast::Expr::IndexListExpr(_)
    )
}

fn is_type_name_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => is_type_name_expr(&paren.x),
        ast::Expr::Ident(ident) => TYPE_ENV.with(|env| {
            let env = env.borrow();
            !IMPORT_NAMES.with(|names| names.borrow().contains(ident.name))
                && !env.is_const(ident.name)
                && env.get_var(ident.name).is_none()
                && env.get_top_level_var(ident.name).is_none()
        }),
        ast::Expr::SelectorExpr(selector) => {
            matches!(&*selector.x, ast::Expr::Ident(pkg) if IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name)))
        }
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::StarExpr(star) => is_type_name_expr(&star.x),
        ast::Expr::IndexExpr(index) => {
            is_type_name_expr(&index.x) && is_type_arg_expr(&index.index)
        }
        ast::Expr::IndexListExpr(index_list) => {
            is_type_name_expr(&index_list.x) && index_list.indices.iter().all(is_type_arg_expr)
        }
        _ => false,
    }
}

fn is_type_method_expression_receiver(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => is_type_method_expression_receiver(&paren.x),
        ast::Expr::StarExpr(star) => is_type_name_expr(&star.x),
        ast::Expr::IndexExpr(index) => {
            is_type_name_expr(&index.x) && is_type_arg_expr(&index.index)
        }
        ast::Expr::IndexListExpr(index_list) => {
            is_type_name_expr(&index_list.x) && index_list.indices.iter().all(is_type_arg_expr)
        }
        ast::Expr::ArrayType(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::FuncType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::Ident(_) | ast::Expr::SelectorExpr(_) => is_type_name_expr(expr),
        _ => false,
    }
}

fn type_method_expression_receiver_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::ParenExpr(paren) => type_method_expression_receiver_name(&paren.x),
        ast::Expr::StarExpr(star) => type_method_expression_receiver_name(&star.x),
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => selector_type_env_name(selector),
        ast::Expr::IndexExpr(index) => type_method_expression_receiver_name(&index.x),
        ast::Expr::IndexListExpr(index) => type_method_expression_receiver_name(&index.x),
        _ => None,
    }
}

fn type_method_expression_receiver_path(expr: &ast::Expr) -> Option<syn::Path> {
    match expr {
        ast::Expr::ParenExpr(paren) => type_method_expression_receiver_path(&paren.x),
        ast::Expr::StarExpr(star) => type_method_expression_receiver_path(&star.x),
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
            Some(syn::parse_quote! { #ident })
        }
        ast::Expr::SelectorExpr(selector) => Some(selector_path_from_ref(selector)),
        ast::Expr::IndexExpr(index) => type_method_expression_receiver_path(&index.x),
        ast::Expr::IndexListExpr(index) => type_method_expression_receiver_path(&index.x),
        _ => None,
    }
}

fn type_method_expression_receiver_is_pointer(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => type_method_expression_receiver_is_pointer(&paren.x),
        ast::Expr::StarExpr(_) => true,
        _ => false,
    }
}

struct TypeMethodExpressionInfo {
    receiver_path: syn::Path,
    receiver_type: typeinfer::GoType,
    pointer_receiver: bool,
    params: Vec<typeinfer::GoType>,
    results: Vec<typeinfer::GoType>,
}

fn type_method_expression_info(selector: &ast::SelectorExpr) -> Option<TypeMethodExpressionInfo> {
    if !is_type_method_expression_receiver(&selector.x) {
        return None;
    }
    let receiver_name = type_method_expression_receiver_name(&selector.x)?;
    let receiver_path = type_method_expression_receiver_path(&selector.x)?;
    let pointer_receiver = type_method_expression_receiver_is_pointer(&selector.x);
    let method_key = format!("{}.{}", receiver_name, selector.sel.name);
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        env.has_func(&method_key).then(|| {
            let receiver_type = typeinfer::GoType::Named(receiver_name);
            let receiver_type = if pointer_receiver {
                typeinfer::GoType::Pointer(Box::new(receiver_type))
            } else {
                receiver_type
            };
            TypeMethodExpressionInfo {
                receiver_path,
                receiver_type,
                pointer_receiver,
                params: env.get_func_params(&method_key),
                results: env.get_func_returns(&method_key),
            }
        })
    })
}

fn compile_type_method_expression_value(selector: ast::SelectorExpr) -> syn::Expr {
    let Some(info) = type_method_expression_info(&selector) else {
        return compile_error_expr("invalid method expression");
    };
    let method_ident: syn::Ident = selector.sel.into();
    let receiver_ident = syn::Ident::new("__gors_method_receiver", Span::mixed_site());
    let method_arg_idents = (0..info.params.len())
        .map(|idx| syn::Ident::new(&format!("__gors_method_arg_{idx}"), Span::mixed_site()))
        .collect::<Vec<_>>();
    let mut param_pats = vec![typed_ident_pat(
        receiver_ident.clone(),
        rust_type_from_inferred_go_type(&info.receiver_type),
    )];
    param_pats.extend(
        method_arg_idents
            .iter()
            .zip(info.params.iter().map(rust_type_from_inferred_go_type))
            .map(|(ident, ty)| typed_ident_pat(ident.clone(), ty)),
    );
    let method_args = method_arg_idents
        .iter()
        .map(|ident| syn::parse_quote! { #ident })
        .collect::<Vec<syn::Expr>>();
    let result_types = info
        .results
        .iter()
        .map(rust_type_from_inferred_go_type)
        .collect::<Vec<_>>();
    let return_type: syn::Type = match result_types.as_slice() {
        [] => syn::parse_quote! { () },
        [ty] => ty.clone(),
        _ => syn::parse_quote! { (#(#result_types),*) },
    };
    let receiver_path = info.receiver_path;
    let body: syn::Expr = if info.pointer_receiver {
        syn::parse_quote! {
            #receiver_path::#method_ident(&mut *#receiver_ident.lock().unwrap(), #(#method_args),*)
        }
    } else {
        syn::parse_quote! { #receiver_path::#method_ident(&#receiver_ident, #(#method_args),*) }
    };
    syn::parse_quote! { move |#(#param_pats),*| -> #return_type { #body } }
}

fn is_type_method_expression_call(call_expr: &ast::CallExpr) -> bool {
    matches!(
        call_expr.fun.as_ref(),
        ast::Expr::SelectorExpr(selector)
            if type_method_expression_info(selector).is_some()
    )
}

fn compile_type_method_expression_receiver_arg(
    receiver: ast::Expr,
    pointer_receiver: bool,
) -> syn::Expr {
    if pointer_receiver {
        let receiver = method_receiver_expr_from_ref(receiver);
        syn::parse_quote! { &mut *#receiver }
    } else {
        let receiver = method_receiver_expr_from_ref(receiver);
        syn::parse_quote! { &#receiver }
    }
}

fn compile_type_method_expression_call(call_expr: ast::CallExpr) -> syn::Expr {
    let ast::Expr::SelectorExpr(selector) = *call_expr.fun else {
        return compile_error_expr("invalid method expression call");
    };
    let Some(receiver_name) = type_method_expression_receiver_name(&selector.x) else {
        return compile_error_expr("invalid method expression receiver");
    };
    let Some(receiver_path) = type_method_expression_receiver_path(&selector.x) else {
        return compile_error_expr("invalid method expression receiver");
    };
    let method_ident: syn::Ident = selector.sel.into();
    let pointer_receiver = type_method_expression_receiver_is_pointer(&selector.x);
    let method_key = format!("{receiver_name}.{}", method_ident);
    let param_types = TYPE_ENV.with(|env| {
        let env = env.borrow();
        (
            env.get_func_params(&method_key),
            env.get_func_variadic_start(&method_key),
        )
    });
    let mut raw_args = call_expr.args.unwrap_or_default().into_iter();
    let Some(receiver_arg) = raw_args.next() else {
        return compile_error_expr("method expression call requires receiver argument");
    };
    let mut args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
    args.push(compile_type_method_expression_receiver_arg(
        receiver_arg,
        pointer_receiver,
    ));
    let (param_types, variadic_start) = param_types;
    let raw_method_args: Vec<ast::Expr> = raw_args.collect();
    if let Some(variadic_start) = variadic_start {
        let variadic_elem = param_types.get(variadic_start).and_then(|ty| match ty {
            typeinfer::GoType::Slice(inner) => Some((**inner).clone()),
            _ => None,
        });
        let variadic_is_any = matches!(
            variadic_elem.as_ref().map(resolved_go_type),
            Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_))
        );
        if call_expr.ellipsis.is_some() {
            for (idx, arg) in raw_method_args.into_iter().enumerate() {
                let should_clone =
                    idx >= variadic_start && !variadic_is_any && is_ir_addressable_expr(&arg) && {
                        let actual =
                            TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
                        !go_type_is_copy(&actual)
                    };
                let arg = compile_expr_with_expected(arg, param_types.get(idx));
                if should_clone {
                    args.push(syn::parse_quote! { (#arg).clone() });
                } else {
                    args.push(arg);
                }
            }
            return syn::parse_quote! { #receiver_path::#method_ident(#args) };
        }

        let mut method_args = syn::punctuated::Punctuated::<syn::Expr, Token![,]>::new();
        for (idx, arg) in raw_method_args.into_iter().enumerate() {
            if idx < variadic_start {
                method_args.push(compile_expr_with_expected(arg, param_types.get(idx)));
            } else if variadic_is_any {
                let arg = compile_variadic_any_arg(arg, variadic_elem.as_ref());
                method_args
                    .push(syn::parse_quote! { Box::new((#arg).clone()) as Box<dyn std::any::Any> });
            } else {
                method_args.push(compile_expr_with_expected(arg, variadic_elem.as_ref()));
            }
        }

        let variadic_args: Vec<&syn::Expr> = method_args.iter().skip(variadic_start).collect();
        let fixed_args: Vec<&syn::Expr> = method_args.iter().take(variadic_start).collect();
        for arg in fixed_args {
            args.push(arg.clone());
        }
        let vec_expr: syn::Expr = if variadic_args.is_empty() && variadic_is_any {
            syn::parse_quote! { Vec::<Box<dyn std::any::Any>>::new() }
        } else if variadic_args.is_empty() {
            syn::parse_quote! { Vec::new() }
        } else {
            syn::parse_quote! { Vec::from([#(#variadic_args),*]) }
        };
        args.push(vec_expr);
        return syn::parse_quote! { #receiver_path::#method_ident(#args) };
    }
    for (idx, arg) in raw_method_args.into_iter().enumerate() {
        args.push(compile_expr_with_expected(arg, param_types.get(idx)));
    }
    syn::parse_quote! { #receiver_path::#method_ident(#args) }
}

fn compile_call_function_expr(fun: ast::Expr) -> syn::Expr {
    match fun {
        ast::Expr::Ident(ident) => {
            let mut segments = syn::punctuated::Punctuated::new();
            segments.push(syn::PathSegment {
                ident: ident.into(),
                arguments: syn::PathArguments::None,
            });

            syn::Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: syn::Path {
                    segments,
                    leading_colon: None,
                },
            })
        }
        ast::Expr::IndexExpr(index_expr) if is_type_arg_expr(&index_expr.index) => {
            compile_call_function_expr(*index_expr.x)
        }
        ast::Expr::IndexListExpr(index_list_expr)
            if index_list_expr.indices.iter().all(is_type_arg_expr) =>
        {
            compile_call_function_expr(*index_list_expr.x)
        }
        other => other.into(),
    }
}

fn synthetic_ident_expr<'a>(name: String) -> ast::Expr<'a> {
    let name: &'static str = Box::leak(name.into_boxed_str());
    ast::Expr::Ident(ast::Ident {
        name_pos: token::Position::default(),
        name,
        obj: None,
    })
}

fn concrete_defer_arg_type(expected: Option<&typeinfer::GoType>) -> Option<syn::Type> {
    let expected = expected?;
    if matches!(
        expected,
        typeinfer::GoType::Any | typeinfer::GoType::Interface(_) | typeinfer::GoType::Unknown
    ) {
        return None;
    }
    Some(rust_type_from_inferred_go_type(expected))
}

fn compile_defer_arg_init(
    arg: ast::Expr,
    expected: Option<&typeinfer::GoType>,
) -> (syn::Expr, Option<syn::Type>) {
    let temp_ty = concrete_defer_arg_type(expected);
    let expected = temp_ty.as_ref().and(expected);
    let should_clone = binding_init_should_clone(&arg);
    let init = compile_expr_with_expected(arg, expected);
    (maybe_clone_binding_init(should_clone, init), temp_ty)
}

fn compile_defer_saved_args<'a>(
    args: Option<Vec<ast::Expr<'a>>>,
    n: usize,
    param_types: &[typeinfer::GoType],
    variadic_start: Option<usize>,
) -> (Vec<syn::Stmt>, Option<Vec<ast::Expr<'a>>>, Vec<syn::Ident>) {
    let Some(args) = args else {
        return (Vec::new(), None, Vec::new());
    };
    let mut prelude = Vec::new();
    let mut saved_args = Vec::with_capacity(args.len());
    let mut saved_arg_idents = Vec::with_capacity(args.len());
    for (idx, arg) in args.into_iter().enumerate() {
        let expected = if variadic_start.is_some_and(|start| idx >= start) {
            None
        } else {
            param_types.get(idx)
        };
        let (init, temp_ty) = compile_defer_arg_init(arg, expected);
        let temp_ident = quote::format_ident!("_defer_{}_arg_{}", n, idx);
        if let Some(temp_ty) = temp_ty {
            prelude.push(syn::parse_quote! {
                let #temp_ident: #temp_ty = #init;
            });
        } else {
            prelude.push(syn::parse_quote! {
                let #temp_ident = #init;
            });
        }
        saved_args.push(synthetic_ident_expr(temp_ident.to_string()));
        saved_arg_idents.push(temp_ident);
    }
    (prelude, Some(saved_args), saved_arg_idents)
}

fn defer_stack_decl_stmt() -> syn::Stmt {
    syn::parse_quote! {
        let mut __gors_defer_stack = {
            struct __GorsDeferStack(Vec<Box<dyn FnOnce()>>);
            impl Drop for __GorsDeferStack {
                fn drop(&mut self) {
                    while let Some(__gors_defer) = self.0.pop() {
                        __gors_defer();
                    }
                }
            }
            __GorsDeferStack(Vec::new())
        };
    }
}

fn push_defer_stack_stmt(setup: Vec<syn::Stmt>, call: syn::Expr) -> syn::Stmt {
    syn::parse_quote! {{
        #(#setup)*
        __gors_defer_stack.0.push(Box::new(move || { #call; }));
    }}
}

fn block_has_defer(block: &ast::BlockStmt) -> bool {
    block.list.iter().any(stmt_has_defer)
}

fn stmt_list_has_defer(stmts: &[ast::Stmt]) -> bool {
    stmts.iter().any(stmt_has_defer)
}

fn stmt_has_defer(stmt: &ast::Stmt) -> bool {
    match stmt {
        ast::Stmt::DeferStmt(_) => true,
        ast::Stmt::BlockStmt(block) => block_has_defer(block),
        ast::Stmt::CaseClause(case_clause) => stmt_list_has_defer(&case_clause.body),
        ast::Stmt::CommClause(comm_clause) => {
            comm_clause.comm.as_deref().is_some_and(stmt_has_defer)
                || stmt_list_has_defer(&comm_clause.body)
        }
        ast::Stmt::ForStmt(for_stmt) => {
            for_stmt.init.as_deref().is_some_and(stmt_has_defer)
                || for_stmt.post.as_deref().is_some_and(stmt_has_defer)
                || block_has_defer(&for_stmt.body)
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if_stmt.init.as_ref().as_ref().is_some_and(stmt_has_defer)
                || block_has_defer(&if_stmt.body)
                || if_stmt.else_.as_ref().as_ref().is_some_and(stmt_has_defer)
        }
        ast::Stmt::LabeledStmt(labeled) => stmt_has_defer(&labeled.stmt),
        ast::Stmt::RangeStmt(range_stmt) => block_has_defer(&range_stmt.body),
        ast::Stmt::SelectStmt(select_stmt) => block_has_defer(&select_stmt.body),
        ast::Stmt::SwitchStmt(switch_stmt) => {
            switch_stmt.init.as_deref().is_some_and(stmt_has_defer)
                || block_has_defer(&switch_stmt.body)
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            type_switch.init.as_deref().is_some_and(stmt_has_defer)
                || stmt_has_defer(&type_switch.assign)
                || block_has_defer(&type_switch.body)
        }
        _ => false,
    }
}

fn compile_defer_stmt(mut call: ast::CallExpr) -> Vec<syn::Stmt> {
    let n = DEFER_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    });
    let fun_ty = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&call.fun, &env.borrow()));
    let param_types = call_param_types(&call.fun);
    let variadic_start = is_variadic_call(&call);

    if !is_function_item_expr(&call.fun)
        && !is_function_literal_expr(&call.fun)
        && let typeinfer::GoType::Func {
            params,
            variadic_start,
            ..
        } = resolved_go_type(&fun_ty)
        && let Some(fun_ty) = shared_func_box_type_from_go_type(&fun_ty)
    {
        let fun_temp_ident = quote::format_ident!("_defer_{}_fun", n);
        let fun_expr: syn::Expr = (*call.fun).into();
        let has_variadic_spread = call.ellipsis.is_some();
        let (mut prelude, saved_args, _) =
            compile_defer_saved_args(call.args.take(), n, &params, variadic_start);
        prelude.insert(
            0,
            syn::parse_quote! {
                let #fun_temp_ident: #fun_ty = {
                    let __gors_func = crate::builtin::lock_func(&(#fun_expr));
                    match __gors_func.as_ref() {
                        Some(__gors_func) => __gors_func.clone(),
                        None => panic!("nil function"),
                    }
                };
            },
        );
        let call_args = compile_function_value_call_args(
            saved_args.unwrap_or_default(),
            &params,
            variadic_start,
            has_variadic_spread,
        );
        let call: syn::Expr = syn::parse_quote! {
            (&*#fun_temp_ident)(#call_args)
        };
        prelude.push(push_defer_stack_stmt(Vec::new(), call));
        return prelude;
    }

    let function_literal_capture_clones = function_literal_shared_capture_clones(&call.fun);
    let (mut prelude, saved_args, _) =
        compile_defer_saved_args(call.args.take(), n, &param_types, variadic_start);
    if let Some(saved_args) = saved_args {
        call.args = Some(saved_args);
    }
    let call: syn::Expr = ast::Expr::CallExpr(call).into();
    prelude.push(push_defer_stack_stmt(function_literal_capture_clones, call));
    prelude
}

fn prepend_defer_stack(block: &mut syn::Block) {
    block.stmts.splice(0..0, [defer_stack_decl_stmt()]);
}

fn invalid_goto_error(invalid: ir::InvalidGoto) -> CompilerError {
    let message = match invalid {
        ir::InvalidGoto::SkipsDeclarations {
            label,
            skipped_names,
        } => format!(
            "invalid goto to {} skips declarations: {}",
            label,
            skipped_names.join(", ")
        ),
        ir::InvalidGoto::EntersBlock { label } => {
            format!("invalid goto to {} enters a nested block", label)
        }
        ir::InvalidGoto::UndefinedLabel { label } => {
            format!("invalid goto to undefined label {}", label)
        }
    };
    CompilerError::UnsupportedConstruct(message)
}

fn invalid_branch_error(invalid: ir::InvalidBranch) -> CompilerError {
    let message = match invalid {
        ir::InvalidBranch::BreakLabel { label } => {
            format!("invalid break to non-enclosing label {}", label)
        }
        ir::InvalidBranch::BreakOutside => {
            "invalid break outside for, switch, or select".to_string()
        }
        ir::InvalidBranch::ContinueLabel { label } => {
            format!("invalid continue to non-enclosing for label {}", label)
        }
        ir::InvalidBranch::ContinueOutside => "invalid continue outside for loop".to_string(),
        ir::InvalidBranch::FallthroughInFinalCase => {
            "invalid fallthrough in final switch case".to_string()
        }
        ir::InvalidBranch::FallthroughInTypeSwitch => {
            "invalid fallthrough in type switch".to_string()
        }
        ir::InvalidBranch::FallthroughNotFinal => {
            "invalid fallthrough before final switch case statement".to_string()
        }
        ir::InvalidBranch::FallthroughOutsideSwitch => {
            "invalid fallthrough outside expression switch case".to_string()
        }
    };
    CompilerError::UnsupportedConstruct(message)
}

fn invalid_statement_error(invalid: ir::InvalidStatement) -> CompilerError {
    let message = match invalid {
        ir::InvalidStatement::Assignment { reason } => {
            format!("invalid assignment: {}", invalid_assignment_reason(reason))
        }
        ir::InvalidStatement::Condition { reason } => {
            let kind = condition_kind_name(reason.kind);
            format!(
                "invalid {kind} condition: condition must be boolean, got {}",
                reason.type_name
            )
        }
        ir::InvalidStatement::Defer { reason } => {
            format!(
                "invalid defer statement: {}",
                invalid_statement_reason(reason)
            )
        }
        ir::InvalidStatement::Declaration { reason } => {
            format!(
                "invalid declaration: {}",
                invalid_declaration_reason(reason)
            )
        }
        ir::InvalidStatement::DuplicateDefault { kind } => {
            format!(
                "invalid {} statement: multiple default clauses",
                default_clause_kind_name(kind)
            )
        }
        ir::InvalidStatement::Expr { reason } => {
            format!(
                "invalid expression statement: {}",
                invalid_statement_reason(reason)
            )
        }
        ir::InvalidStatement::Expression { reason } => {
            format!("invalid expression: {}", invalid_statement_reason(reason))
        }
        ir::InvalidStatement::ForPostShortVarDecl => {
            "invalid for statement: post statement cannot be a short variable declaration"
                .to_string()
        }
        ir::InvalidStatement::Go { reason } => {
            format!("invalid go statement: {}", invalid_statement_reason(reason))
        }
        ir::InvalidStatement::IncDec { reason } => {
            format!(
                "invalid increment/decrement statement: {}",
                invalid_inc_dec_reason(reason)
            )
        }
        ir::InvalidStatement::MissingReturn => {
            "invalid function body: missing terminating statement".to_string()
        }
        ir::InvalidStatement::Range { reason } => match reason {
            ir::InvalidRangeReason::BindingCount { kind, max, got } => format!(
                "invalid range clause: {} range permits at most {} iteration variable(s), got {}",
                range_kind_name(kind),
                max,
                got
            ),
            ir::InvalidRangeReason::NonRangeable { type_name } => {
                format!("invalid range clause: cannot range over {type_name}")
            }
            ir::InvalidRangeReason::TypeMismatch { expected, actual } => {
                format!("invalid range assignment: cannot assign {actual} to {expected}")
            }
        },
        ir::InvalidStatement::Receive { reason } => {
            format!(
                "invalid receive operation: {}",
                invalid_receive_reason(reason)
            )
        }
        ir::InvalidStatement::Return { reason } => {
            format!(
                "invalid return statement: {}",
                invalid_return_reason(reason)
            )
        }
        ir::InvalidStatement::Send { reason } => {
            format!("invalid send statement: {}", invalid_send_reason(reason))
        }
        ir::InvalidStatement::SelectComm { reason } => {
            format!(
                "invalid select communication clause: {}",
                invalid_select_comm_reason(reason)
            )
        }
        ir::InvalidStatement::ShortVarDecl { reason } => {
            format!(
                "invalid short variable declaration: {}",
                invalid_short_var_decl_reason(reason)
            )
        }
        ir::InvalidStatement::Switch { reason } => {
            format!(
                "invalid switch statement: {}",
                invalid_switch_reason(reason)
            )
        }
        ir::InvalidStatement::TypeSwitchGuard { reason } => {
            format!(
                "invalid type switch guard: {}",
                invalid_type_switch_guard_reason(reason)
            )
        }
        ir::InvalidStatement::TypeSwitch { reason } => {
            format!(
                "invalid type switch statement: {}",
                invalid_type_switch_reason(reason)
            )
        }
    };
    CompilerError::UnsupportedConstruct(message)
}

fn condition_kind_name(kind: ir::ConditionKind) -> &'static str {
    match kind {
        ir::ConditionKind::For => "for",
        ir::ConditionKind::If => "if",
    }
}

fn invalid_inc_dec_reason(reason: ir::InvalidIncDecReason) -> String {
    match reason {
        ir::InvalidIncDecReason::InvalidOperand => {
            "operand must be addressable or a map index".to_string()
        }
        ir::InvalidIncDecReason::NonNumericOperand => "operand must have numeric type".to_string(),
    }
}

fn invalid_assignment_reason(reason: ir::InvalidAssignmentReason) -> String {
    match reason {
        ir::InvalidAssignmentReason::CompoundBlankIdentifier => {
            "compound assignment left side must not be blank identifier".to_string()
        }
        ir::InvalidAssignmentReason::CompoundInvalidOperand {
            op,
            side,
            type_name,
        } => format!("{side} operand of {op} has invalid type {type_name}"),
        ir::InvalidAssignmentReason::CompoundNegativeShiftCount { op } => {
            format!("right operand of {op} must be a non-negative constant")
        }
        ir::InvalidAssignmentReason::CompoundOperandCount { lhs, rhs } => format!(
            "compound assignment requires exactly one left and right operand, got {lhs} left and {rhs} right"
        ),
        ir::InvalidAssignmentReason::CountMismatch { lhs, values } => {
            format!("assignment count mismatch: {lhs} left operand(s), {values} value(s)")
        }
        ir::InvalidAssignmentReason::InvalidLeftOperand => {
            "left side is not assignable".to_string()
        }
        ir::InvalidAssignmentReason::MultiValueInSingleValueContext => {
            "multi-valued expression in single-value assignment context".to_string()
        }
        ir::InvalidAssignmentReason::TypeMismatch { expected, actual } => {
            format!("cannot assign {actual} to {expected}")
        }
        ir::InvalidAssignmentReason::UntypedNil => "use of untyped nil in assignment".to_string(),
    }
}

fn default_clause_kind_name(kind: ir::DefaultClauseKind) -> &'static str {
    match kind {
        ir::DefaultClauseKind::Select => "select",
        ir::DefaultClauseKind::Switch => "switch",
        ir::DefaultClauseKind::TypeSwitch => "type switch",
    }
}

fn range_kind_name(kind: ir::RangeKind) -> &'static str {
    match kind {
        ir::RangeKind::String => "string",
        ir::RangeKind::Integer => "integer",
        ir::RangeKind::Indexed => "array or slice",
        ir::RangeKind::Map => "map",
        ir::RangeKind::Channel => "channel",
        ir::RangeKind::Function => "function",
        ir::RangeKind::Other => "unknown",
    }
}

fn invalid_statement_reason(reason: ir::InvalidStatementReason) -> String {
    match reason {
        ir::InvalidStatementReason::BlankIdentifier => "cannot use _ as value or type".to_string(),
        ir::InvalidStatementReason::BuiltinFunctionValue(name) => {
            format!("{name} must be called, not used as a function value")
        }
        ir::InvalidStatementReason::InvalidArrayType { reason } => {
            format!("invalid array type: {reason}")
        }
        ir::InvalidStatementReason::InvalidBinary { op, reason } => {
            format!("invalid binary expression {op}: {reason}")
        }
        ir::InvalidStatementReason::DisallowedBuiltin(name) => {
            format!("{} is not permitted in statement context", name)
        }
        ir::InvalidStatementReason::InvalidBuiltinCall { name, reason } => {
            format!("invalid {name} call: {reason}")
        }
        ir::InvalidStatementReason::InvalidCall { target, reason } => {
            format!("invalid call to {target}: {reason}")
        }
        ir::InvalidStatementReason::InvalidCompositeLiteral { reason } => {
            format!("invalid composite literal: {reason}")
        }
        ir::InvalidStatementReason::InvalidIndex { reason } => {
            format!("invalid index expression: {reason}")
        }
        ir::InvalidStatementReason::InvalidMapType { reason } => {
            format!("invalid map type: {reason}")
        }
        ir::InvalidStatementReason::InvalidSlice { reason } => {
            format!("invalid slice expression: {reason}")
        }
        ir::InvalidStatementReason::InvalidTypeAssert { reason } => {
            format!("invalid type assertion: {reason}")
        }
        ir::InvalidStatementReason::InvalidTypeConversion { target, reason } => {
            format!("invalid type conversion to {target}: {reason}")
        }
        ir::InvalidStatementReason::InvalidUnary { op, reason } => {
            format!("invalid unary expression {op}: {reason}")
        }
        ir::InvalidStatementReason::NonCallOrReceive => {
            "expected a function call, method call, or receive operation".to_string()
        }
        ir::InvalidStatementReason::TypeNameValue(name) => {
            format!("cannot use type {name} as value")
        }
        ir::InvalidStatementReason::TypeConversion => {
            "type conversions are not permitted in statement context".to_string()
        }
    }
}

fn invalid_return_reason(reason: ir::InvalidReturnReason) -> String {
    match reason {
        ir::InvalidReturnReason::CountMismatch { expected, values } => {
            format!("expected {expected} result value(s), got {values}")
        }
        ir::InvalidReturnReason::MultiValueInSingleValueContext => {
            "multi-valued expression in explicit return list".to_string()
        }
        ir::InvalidReturnReason::TypeMismatch { expected, actual } => {
            format!("cannot return {actual} as {expected}")
        }
    }
}

fn invalid_receive_reason(reason: ir::InvalidReceiveReason) -> String {
    match reason {
        ir::InvalidReceiveReason::NonChannel { type_name } => {
            format!("operand must have channel type, got {type_name}")
        }
        ir::InvalidReceiveReason::SendOnlyChannel => {
            "cannot receive from send-only channel".to_string()
        }
    }
}

fn invalid_send_reason(reason: ir::InvalidSendReason) -> String {
    match reason {
        ir::InvalidSendReason::NonChannel { type_name } => {
            format!("channel operand must have channel type, got {type_name}")
        }
        ir::InvalidSendReason::ReceiveOnlyChannel => {
            "cannot send to receive-only channel".to_string()
        }
        ir::InvalidSendReason::ValueTypeMismatch { expected, actual } => {
            format!("cannot send {actual} to channel of {expected}")
        }
    }
}

fn invalid_switch_reason(reason: ir::InvalidSwitchReason) -> String {
    match reason {
        ir::InvalidSwitchReason::CaseMultiValue { values } => {
            format!("case expression must be single-valued, got {values} value(s)")
        }
        ir::InvalidSwitchReason::CaseTypeMismatch { expected, actual } => {
            format!("case expression must be comparable to {expected}, got {actual}")
        }
        ir::InvalidSwitchReason::DuplicateConstantCase { value } => {
            format!("duplicate constant case {value}")
        }
        ir::InvalidSwitchReason::NilTag => "switch expression cannot be nil".to_string(),
        ir::InvalidSwitchReason::NonComparableCase { type_name } => {
            format!("case expression type {type_name} is not comparable")
        }
        ir::InvalidSwitchReason::NonComparableTag { type_name } => {
            format!("switch expression type {type_name} is not comparable")
        }
    }
}

fn invalid_select_comm_reason(reason: ir::InvalidSelectCommReason) -> String {
    match reason {
        ir::InvalidSelectCommReason::InvalidAssignmentToken => {
            "receive assignment must use = or :=".to_string()
        }
        ir::InvalidSelectCommReason::MissingReceiveExpression => {
            "receive assignment must use a receive operation".to_string()
        }
        ir::InvalidSelectCommReason::NonCommunication => {
            "case must be a send statement, receive statement, or default".to_string()
        }
        ir::InvalidSelectCommReason::ShortReceiveDeclarationLhs => {
            "short receive declaration must use identifiers on the left side".to_string()
        }
    }
}

fn invalid_type_switch_guard_reason(reason: ir::InvalidTypeSwitchGuardReason) -> String {
    match reason {
        ir::InvalidTypeSwitchGuardReason::BlankIdentifier => {
            "guard variable must not be blank".to_string()
        }
        ir::InvalidTypeSwitchGuardReason::InvalidAssignmentToken => {
            "guard assignment must use :=".to_string()
        }
        ir::InvalidTypeSwitchGuardReason::InvalidExpression => "guard must be x.(type)".to_string(),
        ir::InvalidTypeSwitchGuardReason::InvalidIdentifierCount => {
            "guard must declare exactly one identifier".to_string()
        }
    }
}

fn invalid_type_switch_reason(reason: ir::InvalidTypeSwitchReason) -> String {
    match reason {
        ir::InvalidTypeSwitchReason::CaseDoesNotImplement {
            case_type,
            interface_type,
        } => format!("{case_type} does not implement {interface_type}"),
        ir::InvalidTypeSwitchReason::DuplicateCase { type_name } => {
            format!("duplicate case type {type_name}")
        }
        ir::InvalidTypeSwitchReason::DuplicateNil => "duplicate nil case".to_string(),
        ir::InvalidTypeSwitchReason::NonInterfaceGuard { type_name } => {
            format!("guard expression must have interface type, got {type_name}")
        }
    }
}

fn invalid_short_var_decl_reason(reason: ir::InvalidShortVarDeclReason) -> String {
    match reason {
        ir::InvalidShortVarDeclReason::DuplicateName(name) => {
            format!("name {name} appears more than once on left side of :=")
        }
        ir::InvalidShortVarDeclReason::NonIdentifier => {
            "left side of := must be identifiers".to_string()
        }
        ir::InvalidShortVarDeclReason::NoNewVariables => {
            "no new variables on left side of :=".to_string()
        }
    }
}

fn invalid_label_error(invalid: ir::InvalidLabel) -> CompilerError {
    let message = match invalid {
        ir::InvalidLabel::Duplicate { label } => format!("invalid duplicate label {}", label),
        ir::InvalidLabel::Unused { label } => format!("invalid unused label {}", label),
    };
    CompilerError::UnsupportedConstruct(message)
}

fn invalid_signature_error(invalid: ir::InvalidSignature) -> CompilerError {
    let message = match invalid {
        ir::InvalidSignature::DuplicateInterfaceMethod { name } => {
            format!("duplicate interface method {name}")
        }
        ir::InvalidSignature::DuplicateName { name } => {
            format!("duplicate parameter/result name {}", name)
        }
        ir::InvalidSignature::DuplicateTypeParameterName { name } => {
            format!("duplicate type parameter name {name}")
        }
        ir::InvalidSignature::InitFunction {
            type_params,
            params,
            results,
        } => format!(
            "init function must not declare type parameters, parameters, or results \
             (got {type_params} type parameter(s), {params} parameter(s), {results} result(s))"
        ),
        ir::InvalidSignature::InvalidTypeParameterDecl => {
            "type parameter declaration must include names and a constraint".to_string()
        }
        ir::InvalidSignature::MainFunction {
            type_params,
            params,
            results,
        } => format!(
            "main function must not declare type parameters, parameters, or results \
             (got {type_params} type parameter(s), {params} parameter(s), {results} result(s))"
        ),
        ir::InvalidSignature::MissingMainFunction => {
            "function main is undeclared in the main package".to_string()
        }
        ir::InvalidSignature::MethodTypeParams { count } => {
            format!("method declaration must not declare type parameters, got {count}")
        }
        ir::InvalidSignature::MixedNamedUnnamed { list } => {
            format!(
                "{} list mixes named and unnamed entries",
                signature_list_name(list)
            )
        }
        ir::InvalidSignature::ReceiverCount { count } => {
            format!("method receiver must declare exactly one parameter, got {count}")
        }
        ir::InvalidSignature::ReceiverType { base, reason } => {
            invalid_receiver_type_reason(base.as_deref(), reason)
        }
        ir::InvalidSignature::ReceiverTypeParameterCount {
            base,
            expected,
            got,
        } => format!(
            "method receiver base type {base} requires {expected} type parameter(s), got {got}"
        ),
        ir::InvalidSignature::ReceiverTypeParameterNotIdentifier => {
            "receiver type parameter must be an identifier".to_string()
        }
        ir::InvalidSignature::ReceiverVariadic => "method receiver cannot be variadic".to_string(),
        ir::InvalidSignature::VariadicNotFinal => {
            "variadic parameter must be the final incoming parameter".to_string()
        }
        ir::InvalidSignature::VariadicResult => "result parameter cannot be variadic".to_string(),
    };
    CompilerError::InvalidFunctionSignature(message)
}

fn invalid_receiver_type_reason(
    base: Option<&str>,
    reason: ir::InvalidReceiverTypeReason,
) -> String {
    match reason {
        ir::InvalidReceiverTypeReason::GenericAlias => {
            let base = base.unwrap_or("receiver");
            format!("method receiver alias {base} cannot be generic")
        }
        ir::InvalidReceiverTypeReason::InstantiatedAlias => {
            let base = base.unwrap_or("receiver");
            format!("method receiver alias {base} cannot denote an instantiated generic type")
        }
        ir::InvalidReceiverTypeReason::Interface => {
            let base = base.unwrap_or("receiver");
            format!("method receiver base type {base} cannot be an interface")
        }
        ir::InvalidReceiverTypeReason::Pointer => {
            let base = base.unwrap_or("receiver");
            format!("method receiver base type {base} cannot be a pointer type")
        }
        ir::InvalidReceiverTypeReason::Undefined => {
            let base = base.unwrap_or("receiver");
            format!("method receiver base type {base} is not defined in this package")
        }
        ir::InvalidReceiverTypeReason::Unnamed => {
            "method receiver type must be a defined type or pointer to one".to_string()
        }
    }
}

fn signature_list_name(list: ir::SignatureList) -> &'static str {
    match list {
        ir::SignatureList::Parameter => "parameter",
        ir::SignatureList::Result => "result",
    }
}

fn invalid_declaration_error(invalid: ir::InvalidDeclaration) -> CompilerError {
    CompilerError::UnsupportedConstruct(invalid_declaration_reason(invalid))
}

fn invalid_declaration_reason(invalid: ir::InvalidDeclaration) -> String {
    match invalid {
        ir::InvalidDeclaration::ConstInvalidInitializer { reason } => {
            format!("invalid const initializer: {reason}")
        }
        ir::InvalidDeclaration::ConstNonConstantInitializer => {
            "const initializer must be a constant expression".to_string()
        }
        ir::InvalidDeclaration::ConstTypeMismatch { expected, actual } => {
            format!("cannot initialize {expected} constant with {actual}")
        }
        ir::InvalidDeclaration::ConstValueCount { names, values } => {
            format!("const declaration has {names} name(s) but {values} value(s)")
        }
        ir::InvalidDeclaration::AliasToOwnTypeParameter { name } => {
            format!("alias declaration cannot alias its own type parameter {name}")
        }
        ir::InvalidDeclaration::DuplicateMethod { base, method } => {
            format!("duplicate method {method} for receiver base type {base}")
        }
        ir::InvalidDeclaration::DuplicateStructField { type_name, field } => {
            if let Some(type_name) = type_name {
                format!("duplicate field {field} in struct {type_name}")
            } else {
                format!("duplicate field {field} in anonymous struct")
            }
        }
        ir::InvalidDeclaration::DuplicateTopLevelName { name } => {
            format!("duplicate top-level declaration {name}")
        }
        ir::InvalidDeclaration::DuplicateDeclarationName { name } => {
            format!("duplicate declaration {name}")
        }
        ir::InvalidDeclaration::DuplicateImportName { name } => {
            format!("duplicate import name {name}")
        }
        ir::InvalidDeclaration::DuplicateLexicalName { name } => {
            format!("duplicate lexical declaration {name}")
        }
        ir::InvalidDeclaration::ImportPackageBlockConflict { name } => {
            format!("import name {name} conflicts with package-block declaration")
        }
        ir::InvalidDeclaration::InvalidInitIdentifier => {
            "init can only be used for init function declarations".to_string()
        }
        ir::InvalidDeclaration::InvalidPackageName { name } => {
            format!("invalid package name {name}")
        }
        ir::InvalidDeclaration::MethodFieldConflict { base, name } => {
            format!("method {name} conflicts with field on struct {base}")
        }
        ir::InvalidDeclaration::MissingConstInitializer => {
            "const declaration missing initializer".to_string()
        }
        ir::InvalidDeclaration::TypeDefinitionFromTypeParameter { name } => {
            format!("type definition cannot use type parameter {name} as the defined type")
        }
        ir::InvalidDeclaration::UnusedImport { path, alias } => {
            if let Some(alias) = alias {
                format!("{path} imported as {alias} and not used")
            } else {
                format!("{path} imported and not used")
            }
        }
        ir::InvalidDeclaration::UnusedVariable { name } => {
            format!("declared and not used: {name}")
        }
        ir::InvalidDeclaration::VarMissingTypeOrInitializer => {
            "var declaration missing type or initializer".to_string()
        }
        ir::InvalidDeclaration::VarMultiValueInSingleValueContext => {
            "multi-valued expression in explicit var initializer list".to_string()
        }
        ir::InvalidDeclaration::VarUntypedNil => {
            "var initializer cannot use untyped nil without an explicit nilable type".to_string()
        }
        ir::InvalidDeclaration::VarTypeMismatch { expected, actual } => {
            format!("cannot initialize {expected} variable with {actual}")
        }
        ir::InvalidDeclaration::VarValueCount { names, values } => {
            format!("var declaration has {names} name(s) but {values} value(s)")
        }
    }
}

fn validate_function_semantics(
    func_type: &ast::FuncType<'_>,
    body: &ast::BlockStmt,
) -> Result<(), CompilerError> {
    if let Some(invalid) = ir::invalid_goto_target_in_func(body) {
        return Err(invalid_goto_error(invalid));
    }
    if let Some(invalid) = ir::invalid_forward_goto_in_func(body) {
        return Err(invalid_goto_error(invalid));
    }
    if let Some(invalid) = ir::invalid_branch_in_func(body) {
        return Err(invalid_branch_error(invalid));
    }
    if let Some(invalid) =
        TYPE_ENV.with(|env| ir::invalid_statement_in_func_with_type(func_type, body, &env.borrow()))
    {
        return Err(invalid_statement_error(invalid));
    }
    if let Some(invalid) =
        TYPE_ENV.with(|env| ir::invalid_return_in_func(func_type, body, &env.borrow()))
    {
        return Err(invalid_statement_error(invalid));
    }
    if let Some(invalid) =
        TYPE_ENV.with(|env| ir::invalid_body_completion_in_func(func_type, body, &env.borrow()))
    {
        return Err(invalid_statement_error(invalid));
    }
    if let Some(invalid) = ir::invalid_label_in_func(body) {
        return Err(invalid_label_error(invalid));
    }
    Ok(())
}

impl TryFrom<ast::BlockStmt<'_>> for syn::Block {
    type Error = CompilerError;

    fn try_from(block_stmt: ast::BlockStmt) -> Result<Self, Self::Error> {
        let shared_capture_names = TYPE_ENV
            .with(|env| ir::mutable_func_lit_capture_names_in_block(&block_stmt, &env.borrow()));
        let range_function_capture_names = TYPE_ENV.with(|env| {
            ir::mutable_range_function_capture_names_in_block(&block_stmt, &env.borrow())
        });
        let for_clause_iteration_capture_names = TYPE_ENV.with(|env| {
            ir::for_clause_per_iteration_capture_names_in_block(&block_stmt, &env.borrow())
        });
        let address_taken_names = ir::address_taken_names_in_block(&block_stmt);
        let _shared_capture_names = SharedCaptureNamesGuard::extend(
            shared_capture_names
                .into_iter()
                .chain(range_function_capture_names)
                .chain(for_clause_iteration_capture_names)
                .chain(address_taken_names),
        );
        if let Some(invalid) = ir::invalid_forward_goto_in_block(&block_stmt) {
            return Err(invalid_goto_error(invalid));
        }
        if let Some(goto_plan) = ir::goto_state_plan_for_block(&block_stmt) {
            return Ok(Self {
                brace_token: syn::token::Brace::default(),
                stmts: vec![compile_goto_state_stmt(block_stmt, &goto_plan)?],
            });
        }
        let mut stmts = vec![];
        for stmt in block_stmt.list {
            stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }
        Ok(Self {
            brace_token: syn::token::Brace::default(),
            stmts,
        })
    }
}

struct GotoStateSegment<'a> {
    labels: Vec<String>,
    stmts: Vec<ast::Stmt<'a>>,
}

fn split_goto_state_segments(list: Vec<ast::Stmt<'_>>) -> Vec<GotoStateSegment<'_>> {
    let mut segments = vec![GotoStateSegment {
        labels: Vec::new(),
        stmts: Vec::new(),
    }];

    for stmt in list {
        let labels = ir::direct_label_names_in_stmt(&stmt)
            .into_iter()
            .map(|label| rust_safe_ident_name(&label))
            .collect::<Vec<_>>();
        if labels.is_empty() {
            if let Some(segment) = segments.last_mut() {
                segment.stmts.push(stmt);
            }
        } else {
            segments.push(GotoStateSegment {
                labels,
                stmts: vec![stmt],
            });
        }
    }

    segments
}

fn goto_state_label_map(segments: &[GotoStateSegment<'_>]) -> BTreeMap<String, usize> {
    let mut labels = BTreeMap::new();
    for (idx, segment) in segments.iter().enumerate() {
        for label in &segment.labels {
            labels.entry(label.clone()).or_insert(idx);
        }
    }
    labels
}

fn goto_state_tail(
    idx: usize,
    len: usize,
    state_ident: &syn::Ident,
    loop_label: &syn::Lifetime,
) -> Vec<syn::Stmt> {
    if idx + 1 < len {
        let next_lit = syn::LitInt::new(&format!("{}usize", idx + 1), Span::mixed_site());
        vec![
            syn::parse_quote! {
                #state_ident = #next_lit;
            },
            syn::parse_quote! {
                continue #loop_label;
            },
        ]
    } else {
        vec![syn::parse_quote! {
            break #loop_label;
        }]
    }
}

fn terminate_goto_state_segment_stmts(stmts: &mut [syn::Stmt]) {
    for stmt in stmts {
        if let syn::Stmt::Expr(_, semi @ None) = stmt {
            *semi = Some(<Token![;]>::default());
        }
    }
}

struct GotoStateHoistBinding {
    name: String,
}

fn goto_state_hoisted_bindings(plan: &ir::GotoStatePlan) -> Vec<GotoStateHoistBinding> {
    plan.hoisted_names
        .iter()
        .map(|name| GotoStateHoistBinding {
            name: rust_safe_ident_name(name),
        })
        .collect()
}

fn goto_state_hoisted_plan_name_set(
    plan: &ir::GotoStatePlan,
) -> std::collections::BTreeSet<String> {
    plan.hoisted_names
        .iter()
        .map(|name| rust_safe_ident_name(name))
        .collect()
}

fn goto_state_hoist_stmts(bindings: &[GotoStateHoistBinding]) -> Vec<syn::Stmt> {
    bindings
        .iter()
        .map(|binding| {
            let ident = syn::Ident::new(&binding.name, Span::mixed_site());
            syn::parse_quote! {
                let mut #ident = Default::default();
            }
        })
        .collect()
}

fn local_pat_ident(pat: &syn::Pat) -> Option<syn::Ident> {
    match pat {
        syn::Pat::Ident(ident) => Some(ident.ident.clone()),
        syn::Pat::Type(pat_type) => local_pat_ident(&pat_type.pat),
        _ => None,
    }
}

fn rewrite_goto_state_hoisted_locals(
    stmts: &mut Vec<syn::Stmt>,
    hoisted_names: &std::collections::BTreeSet<String>,
) {
    let mut rewritten = Vec::with_capacity(stmts.len());
    for stmt in std::mem::take(stmts) {
        let syn::Stmt::Local(local) = stmt else {
            rewritten.push(stmt);
            continue;
        };
        let Some(ident) = local_pat_ident(&local.pat) else {
            rewritten.push(syn::Stmt::Local(local));
            continue;
        };
        if !hoisted_names.contains(&ident.to_string()) {
            rewritten.push(syn::Stmt::Local(local));
            continue;
        }
        let mut local = local;
        let Some(init) = local.init.take() else {
            continue;
        };
        if init.diverge.is_some() {
            local.init = Some(init);
            rewritten.push(syn::Stmt::Local(local));
            continue;
        }
        let expr = init.expr;
        rewritten.push(syn::parse_quote! {
            #ident = #expr;
        });
    }
    *stmts = rewritten;
}

fn compile_goto_state_stmt(
    block_stmt: ast::BlockStmt<'_>,
    plan: &ir::GotoStatePlan,
) -> Result<syn::Stmt, CompilerError> {
    compile_goto_state_stmt_list_with(block_stmt.list, plan, |stmt| {
        Vec::<syn::Stmt>::try_from(stmt)
    })
}

fn compile_goto_state_stmt_list_with<'a, F>(
    list: Vec<ast::Stmt<'a>>,
    plan: &ir::GotoStatePlan,
    mut compile_stmt: F,
) -> Result<syn::Stmt, CompilerError>
where
    F: FnMut(ast::Stmt<'a>) -> Result<Vec<syn::Stmt>, CompilerError>,
{
    let segments = split_goto_state_segments(list);
    let labels = goto_state_label_map(&segments);
    let hoisted_names = goto_state_hoisted_plan_name_set(plan);
    let (state_ident, loop_label) = next_goto_state_names();
    let context = GotoStateContext {
        state_ident: state_ident.clone(),
        loop_label: loop_label.clone(),
        labels,
    };
    let _goto_state = GotoStateContextGuard::push(context);
    let segment_count = segments.len();
    let mut arms: Vec<syn::Arm> = Vec::new();

    for (idx, segment) in segments.into_iter().enumerate() {
        let mut stmts = Vec::new();
        for stmt in segment.stmts {
            stmts.extend(compile_stmt(stmt)?);
        }
        rewrite_goto_state_hoisted_locals(&mut stmts, &hoisted_names);
        terminate_goto_state_segment_stmts(&mut stmts);
        stmts.extend(goto_state_tail(
            idx,
            segment_count,
            &state_ident,
            &loop_label,
        ));
        let idx_lit = syn::LitInt::new(&format!("{idx}usize"), Span::mixed_site());
        arms.push(syn::parse_quote! {
            #idx_lit => {
                #(#stmts)*
            }
        });
    }

    let hoist_bindings = goto_state_hoisted_bindings(plan);
    let hoist_stmts = goto_state_hoist_stmts(&hoist_bindings);

    arms.push(syn::parse_quote! {
        _ => {
            break #loop_label;
        }
    });

    let expr: syn::Expr = syn::parse_quote! {
        {
            #(#hoist_stmts)*
            let mut #state_ident: usize = 0usize;
            #loop_label: loop {
                match #state_ident {
                    #(#arms),*
                }
            }
        }
    };

    Ok(syn::Stmt::Expr(expr, Some(<Token![;]>::default())))
}

impl TryFrom<ast::BlockStmt<'_>> for syn::ExprBlock {
    type Error = CompilerError;

    fn try_from(block_stmt: ast::BlockStmt) -> Result<Self, Self::Error> {
        Ok(Self {
            attrs: vec![],
            label: None,
            block: block_stmt.try_into()?,
        })
    }
}

impl From<ast::CallExpr<'_>> for syn::ExprCall {
    fn from(call_expr: ast::CallExpr) -> Self {
        record_mapping(&call_expr.lparen, None);
        let param_types = call_param_types(&call_expr.fun);
        let borrow_pointer_indices = borrow_pointer_call_arg_indices(&call_expr.fun);

        let func = compile_call_function_expr(*call_expr.fun);

        let mut args = syn::punctuated::Punctuated::new();
        if let Some(cargs) = call_expr.args {
            for (idx, arg) in cargs.into_iter().enumerate() {
                let actual =
                    TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
                let borrow_pointer_by_shape =
                    should_borrow_pointer_arg_by_shape(&arg, param_types.get(idx));
                if borrow_pointer_indices.contains(&idx) || borrow_pointer_by_shape {
                    if let Some(arg) = borrowed_address_of_ident_arg_expr(&arg) {
                        args.push(arg);
                        continue;
                    }
                }
                let mut arg = compile_expr_with_expected(arg, param_types.get(idx));
                if borrow_pointer_indices.contains(&idx) || borrow_pointer_by_shape {
                    borrow_pointer_arg_expr(&mut arg, Some(&actual));
                }
                args.push(arg)
            }
        }

        Self {
            attrs: vec![],
            func: Box::new(func),
            paren_token: syn::token::Paren::default(),
            args,
        }
    }
}

impl From<ast::Expr<'_>> for syn::Expr {
    fn from(expr: ast::Expr) -> Self {
        match expr {
            ast::Expr::BasicLit(basic_lit) => compile_basic_lit_expr(basic_lit),
            ast::Expr::BinaryExpr(binary_expr) => compile_binary_expr(binary_expr),
            ast::Expr::CallExpr(call_expr) => {
                if matches!(
                    unsafe_intrinsic_name(&call_expr),
                    Some("String" | "SliceData")
                ) {
                    return compile_unsafe_intrinsic_call(call_expr);
                }
                if let Some(kind) = special_type_conversion_kind(&call_expr) {
                    return compile_type_conversion(call_expr, kind.name());
                }
                if call_expr.args.as_ref().is_some_and(|args| args.len() == 1)
                    && is_general_type_conversion_fun(&call_expr.fun)
                {
                    return compile_general_type_conversion(call_expr);
                }
                if is_builtin_call(&call_expr) {
                    return compile_builtin(call_expr);
                }
                if is_sort_slice_call(&call_expr) {
                    return compile_sort_slice_call(call_expr)
                        .unwrap_or_else(|| compile_error_expr("invalid sort slice call"));
                }
                if is_append_float_call(&call_expr) {
                    return compile_append_float_call(call_expr)
                        .unwrap_or_else(|| compile_error_expr("invalid strconv AppendFloat call"));
                }
                if is_type_method_expression_call(&call_expr) {
                    return compile_type_method_expression_call(call_expr);
                }
                if let Some(variadic_start) = is_variadic_call(&call_expr) {
                    return compile_variadic_call(call_expr, variadic_start);
                }
                if function_value_call_params(&call_expr.fun).is_some() {
                    return compile_function_field_call(call_expr)
                        .unwrap_or_else(|| compile_error_expr("invalid function field call"));
                }
                let param_types = call_param_types(&call_expr.fun);
                let borrow_pointer_indices = borrow_pointer_call_arg_indices(&call_expr.fun);
                // Detect method call vs package function call
                let is_method_call = matches!(&*call_expr.fun, ast::Expr::SelectorExpr(sel) if {
                    match &*sel.x {
                        ast::Expr::Ident(id) => !IMPORT_NAMES.with(|names| names.borrow().contains(id.name)),
                        _ => true,
                    }
                });
                if is_method_call {
                    if let ast::Expr::SelectorExpr(sel) = *call_expr.fun {
                        let receiver = method_receiver_expr_from_ref(*sel.x);
                        let method: syn::Ident = sel.sel.into();
                        let mut args = syn::punctuated::Punctuated::new();
                        if let Some(cargs) = call_expr.args {
                            for (idx, arg) in cargs.into_iter().enumerate() {
                                let actual = TYPE_ENV
                                    .with(|env| typeinfer::GoType::infer_expr(&arg, &env.borrow()));
                                let borrow_pointer_by_shape =
                                    should_borrow_pointer_arg_by_shape(&arg, param_types.get(idx));
                                if borrow_pointer_indices.contains(&idx) || borrow_pointer_by_shape
                                {
                                    if let Some(arg) = borrowed_address_of_ident_arg_expr(&arg) {
                                        args.push(arg);
                                        continue;
                                    }
                                }
                                let mut arg = compile_expr_with_expected(arg, param_types.get(idx));
                                if borrow_pointer_indices.contains(&idx) || borrow_pointer_by_shape
                                {
                                    borrow_pointer_arg_expr(&mut arg, Some(&actual));
                                }
                                args.push(arg);
                            }
                        }
                        return syn::Expr::MethodCall(syn::ExprMethodCall {
                            attrs: vec![],
                            receiver: Box::new(receiver),
                            dot_token: <Token![.]>::default(),
                            method,
                            turbofish: None,
                            paren_token: syn::token::Paren::default(),
                            args,
                        });
                    }
                }
                Self::Call(call_expr.into())
            }
            ast::Expr::Ident(ident) if ident.name == "nil" => syn::parse_quote! { None },
            ast::Expr::Ident(ident) if ident.name == "true" => syn::parse_quote! { true },
            ast::Expr::Ident(ident) if ident.name == "false" => syn::parse_quote! { false },
            ast::Expr::Ident(ident) if is_active_string_const_fn(ident.name) => {
                let ident: syn::Ident = ident.into();
                syn::parse_quote! { #ident() }
            }
            ast::Expr::Ident(ident) => {
                let ident_name = ident.name;
                if let Some(expr) = shared_capture_read_expr(ident_name) {
                    return expr;
                }
                let is_top_level_var = TYPE_ENV.with(|env| {
                    let env = env.borrow();
                    !MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| *value.borrow())
                        && env.is_top_level_var(ident_name)
                        && !env.is_const(ident_name)
                        && env
                            .get_top_level_var(ident_name)
                            .is_some_and(|top_level_ty| {
                                env.get_var(ident_name)
                                    .is_some_and(|current_ty| current_ty == top_level_ty)
                            })
                });
                let path = Self::Path(ident.into());
                if is_top_level_var {
                    let go_type = TYPE_ENV.with(|env| {
                        env.borrow()
                            .get_var(ident_name)
                            .unwrap_or(typeinfer::GoType::Unknown)
                    });
                    top_level_var_read_expr(path, &go_type)
                } else {
                    path
                }
            }
            ast::Expr::SelectorExpr(selector_expr) => {
                if type_method_expression_info(&selector_expr).is_some() {
                    return compile_type_method_expression_value(selector_expr);
                }
                if method_value_info(&selector_expr).is_some() {
                    return compile_method_value_expr(selector_expr);
                }
                let field_go_type = selector_field_go_type(&selector_expr);
                let is_package = match &*selector_expr.x {
                    ast::Expr::Ident(id) => {
                        IMPORT_NAMES.with(|names| names.borrow().contains(id.name))
                    }
                    _ => false,
                };
                if is_package {
                    let top_level_var_type = selector_top_level_var_type(&selector_expr);
                    if is_active_selector_string_const_fn(&selector_expr) {
                        let path = selector_path_from_ref(&selector_expr);
                        return syn::parse_quote! { #path() };
                    }
                    let path = Self::Path(selector_expr.into());
                    if let Some(go_type) = top_level_var_type {
                        top_level_var_read_expr(path, &go_type)
                    } else {
                        path
                    }
                } else {
                    let base_ast = *selector_expr.x;
                    let base_is_owning_pointer = is_owning_pointer_cell_expr_ref(&base_ast);
                    let mut base: syn::Expr = if base_is_owning_pointer {
                        lvalue_expr_from_ref(&base_ast)
                            .or_else(|| syn_expr_from_type_expr_like(&base_ast))
                            .unwrap_or_else(|| base_ast.into())
                    } else {
                        base_ast.into()
                    };
                    if base_is_owning_pointer {
                        base = syn::parse_quote! { #base.lock().unwrap() };
                    }
                    let field: syn::Ident = selector_expr.sel.into();
                    let field_expr = syn::Expr::Field(syn::ExprField {
                        attrs: vec![],
                        base: Box::new(base),
                        dot_token: <Token![.]>::default(),
                        member: syn::Member::Named(field),
                    });
                    if matches!(
                        field_go_type.as_ref().map(resolved_go_type),
                        Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_))
                    ) {
                        syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
                    } else if field_go_type
                        .as_ref()
                        .is_some_and(|field_ty| !go_type_is_copy(field_ty))
                    {
                        syn::parse_quote! { (#field_expr).clone() }
                    } else {
                        field_expr
                    }
                }
            }
            ast::Expr::ParenExpr(paren_expr) => Self::Paren(syn::ExprParen {
                attrs: vec![],
                paren_token: syn::token::Paren::default(),
                expr: Box::new((*paren_expr.x).into()),
            }),
            ast::Expr::UnaryExpr(unary_expr) => match unary_expr.op {
                token::Token::ADD => {
                    // +x → x (no-op in Go)
                    (*unary_expr.x).into()
                }
                token::Token::AND => {
                    let target = *unary_expr.x;
                    if let ast::Expr::Ident(ident) = &target
                        && is_shared_capture_name(ident.name)
                    {
                        let ident =
                            syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
                        return syn::parse_quote! { #ident.clone() };
                    }
                    let inner: syn::Expr = target.into();
                    syn::parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(#inner)) }
                }
                token::Token::XOR => {
                    // ^x → !x (bitwise NOT in Go)
                    let inner: syn::Expr = (*unary_expr.x).into();
                    Self::Unary(syn::ExprUnary {
                        attrs: vec![],
                        op: syn::UnOp::Not(<Token![!]>::default()),
                        expr: Box::new(inner),
                    })
                }
                token::Token::ARROW => {
                    // <-ch → ch.recv().unwrap_or_default() (channel receive, Go semantics)
                    let receiver: syn::Expr = (*unary_expr.x).into();
                    syn::parse_quote! { #receiver.recv().unwrap_or_default() }
                }
                token::Token::SUB => {
                    let go_type = typeinfer::GoType::infer_expr(
                        &unary_expr.x,
                        &TYPE_ENV.with(|e| e.borrow().clone()),
                    );
                    let inner: syn::Expr = (*unary_expr.x).into();
                    if go_type.is_unsigned_int() {
                        syn::parse_quote! { (#inner).wrapping_neg() }
                    } else {
                        Self::Unary(syn::ExprUnary {
                            attrs: vec![],
                            op: syn::UnOp::Neg(<Token![-]>::default()),
                            expr: Box::new(inner),
                        })
                    }
                }
                _ => Self::Unary(syn::ExprUnary {
                    attrs: vec![],
                    op: match unary_expr.op {
                        token::Token::NOT => syn::UnOp::Not(<Token![!]>::default()),
                        token::Token::MUL => syn::UnOp::Deref(<Token![*]>::default()),
                        _ => {
                            return compile_error_expr(format!(
                                "unsupported unary op: {:?}",
                                unary_expr.op
                            ));
                        }
                    },
                    expr: Box::new((*unary_expr.x).into()),
                }),
            },
            ast::Expr::IndexExpr(index_expr) => {
                let env = TYPE_ENV.with(|e| e.borrow().clone());
                let container_type = typeinfer::GoType::infer_expr(&index_expr.x, &env);
                let base: syn::Expr = (*index_expr.x).into();

                if let typeinfer::GoType::Map(key_ty, _) = env.resolve_alias(&container_type) {
                    let key = compile_expr_with_expected(*index_expr.index, Some(&key_ty));
                    return syn::parse_quote! {{
                        let __gors_map_key = #key;
                        (#base).get(&__gors_map_key).cloned().unwrap_or_default()
                    }};
                }

                let idx: syn::Expr = (*index_expr.index).into();

                if is_byte_seq_type_param(&container_type) {
                    return syn::parse_quote! { crate::builtin::byte_at(&#base, (#idx) as usize) };
                }

                // For string indexing, use .as_bytes()[i] since Go's s[i] returns a byte
                let base = if container_type == typeinfer::GoType::String {
                    syn::parse_quote! { (#base).as_bytes() }
                } else {
                    base
                };

                Self::Index(syn::ExprIndex {
                    attrs: vec![],
                    expr: Box::new(base),
                    bracket_token: syn::token::Bracket::default(),
                    index: Box::new(idx),
                })
            }
            ast::Expr::StarExpr(star_expr) => {
                let inner = *star_expr.x;
                if is_unsafe_pointer_bitcast_expr(&inner) {
                    return compile_unsafe_pointer_bitcast(inner).unwrap_or_else(|| {
                        compile_error_expr("unsupported unsafe pointer bitcast")
                    });
                }
                if let ast::Expr::Ident(ident) = &inner {
                    let name = rust_safe_ident_name(ident.name);
                    if is_borrowed_pointer_param_name(&name) {
                        let ident = syn::Ident::new(&name, Span::mixed_site());
                        return syn::parse_quote! { *#ident };
                    }
                }
                let inner_expr: syn::Expr = inner.into();
                syn::parse_quote! {{
                    let __gors_pointer_value = #inner_expr.lock().unwrap().clone();
                    __gors_pointer_value
                }}
            }
            ast::Expr::CompositeLit(comp_lit) => compile_composite_lit(comp_lit),
            ast::Expr::FuncLit(func_lit) => compile_func_lit(func_lit),
            ast::Expr::SliceExpr(slice_expr) => compile_slice_expr(slice_expr),
            ast::Expr::TypeAssertExpr(ta) => {
                let source_ast = *ta.x;
                let source_is_borrowable = is_ir_addressable_expr(&source_ast);
                let source_type =
                    TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&source_ast, &env.borrow()));
                let x: syn::Expr = source_ast.into();
                if let Some(type_expr) = ta.type_ {
                    if let Some(interface_name) = interface_name_from_type_expr(&type_expr) {
                        type_assert_interface_expr(
                            x,
                            &source_type,
                            source_is_borrowable,
                            &interface_name,
                            false,
                        )
                    } else {
                        let ty: syn::Type = (*type_expr).into();
                        type_assert_concrete_expr(x, &source_type, source_is_borrowable, ty)
                    }
                } else {
                    // type switch x.(type) — handled at statement level
                    x
                }
            }
            ast::Expr::KeyValueExpr(kv) => {
                let key: syn::Expr = (*kv.key).into();
                let value: syn::Expr = (*kv.value).into();
                syn::parse_quote! { (#key, #value) }
            }
            ast::Expr::ArrayType(array_type) => {
                let ty: syn::Type = ast::Expr::ArrayType(array_type).into();
                syn::parse_quote! { <#ty>::default() }
            }
            ast::Expr::MapType(map_type) => {
                let ty: syn::Type = ast::Expr::MapType(map_type).into();
                syn::parse_quote! { <#ty>::default() }
            }
            ast::Expr::ChanType(chan_type) => {
                let ty: syn::Type = ast::Expr::ChanType(chan_type).into();
                syn::parse_quote! { <#ty>::default() }
            }
            ast::Expr::FuncType(_) => {
                syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
            }
            ast::Expr::StructType(struct_type) => {
                let ty = anonymous_struct_type(struct_type);
                syn::parse_quote! { <#ty>::default() }
            }
            ast::Expr::InterfaceType(_) => {
                syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
            }
            ast::Expr::IndexListExpr(index_list_expr) => {
                let base: syn::Expr = (*index_list_expr.x).into();
                base
            }
            _ => compile_error_expr(format!("unsupported expression: {:?}", expr)),
        }
    }
}

fn box_inner_type(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Box" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner.clone())
}

fn arc_mutex_inner_type(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Arc" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(syn::Type::Path(mutex_path)) = args.args.first()? else {
        return None;
    };
    let mutex_segment = mutex_path.path.segments.last()?;
    if mutex_segment.ident != "Mutex" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(mutex_args) = &mutex_segment.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(inner) = mutex_args.args.first()? else {
        return None;
    };
    Some(inner.clone())
}

impl From<ast::Expr<'_>> for syn::Type {
    fn from(expr: ast::Expr) -> Self {
        match expr {
            ast::Expr::Ident(ident) if ident.name == "any" => {
                syn::parse_quote! { Box<dyn std::any::Any> }
            }
            ast::Expr::Ident(ident) if ident.name == "complex64" => {
                syn::parse_quote! { crate::builtin::Complex64 }
            }
            ast::Expr::Ident(ident) if ident.name == "complex128" => {
                syn::parse_quote! { crate::builtin::Complex128 }
            }
            ast::Expr::Ident(ident) => {
                let mut segments = syn::punctuated::Punctuated::new();
                segments.push(syn::PathSegment {
                    ident: ident.into(),
                    arguments: syn::PathArguments::None,
                });
                Self::Path(syn::TypePath {
                    qself: None,
                    path: syn::Path {
                        leading_colon: None,
                        segments,
                    },
                })
            }
            ast::Expr::StarExpr(star_expr) => {
                let inner: syn::Type = (*star_expr.x).into();
                syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#inner>> }
            }
            ast::Expr::ArrayType(array_type) => {
                let elem: syn::Type = (*array_type.elt).into();
                if let Some(len) = array_type.len {
                    let len_expr = array_len_expr(&len);
                    syn::parse_quote! { [#elem; #len_expr] }
                } else {
                    // Slice: []T → Vec<T>
                    syn::parse_quote! { Vec<#elem> }
                }
            }
            ast::Expr::SelectorExpr(selector_expr) => {
                if matches!(&*selector_expr.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
                    && selector_expr.sel.name == "Pointer"
                {
                    return syn::parse_quote! { usize };
                }
                let path: syn::ExprPath = selector_expr.into();
                Self::Path(syn::TypePath {
                    qself: None,
                    path: path.path,
                })
            }
            ast::Expr::InterfaceType(_) => {
                syn::parse_quote! { Box<dyn std::any::Any> }
            }
            ast::Expr::MapType(map_type) => {
                let key: syn::Type = (*map_type.key).into();
                let value: syn::Type = (*map_type.value).into();
                syn::parse_quote! { std::collections::HashMap<#key, #value> }
            }
            ast::Expr::IndexExpr(index_expr) => {
                let base: syn::Type = (*index_expr.x).into();
                let arg: syn::Type = (*index_expr.index).into();
                type_with_generic_args(base, vec![arg])
            }
            ast::Expr::IndexListExpr(index_list_expr) => {
                let base: syn::Type = (*index_list_expr.x).into();
                let args = index_list_expr
                    .indices
                    .into_iter()
                    .map(syn::Type::from)
                    .collect();
                type_with_generic_args(base, args)
            }
            ast::Expr::StructType(struct_type) => anonymous_struct_type(struct_type),
            ast::Expr::FuncType(func_type) => shared_func_type_from_ast(&func_type),
            ast::Expr::ChanType(chan_type) => {
                // chan T → crate::builtin::Chan<T>
                let inner: syn::Type = (*chan_type.value).into();
                syn::parse_quote! { crate::builtin::Chan<#inner> }
            }
            ast::Expr::Ellipsis(ellipsis) => {
                if let Some(elt) = ellipsis.elt {
                    let inner: syn::Type = (*elt).into();
                    syn::parse_quote! { Vec<#inner> }
                } else {
                    syn::parse_quote! { Vec<Box<dyn std::any::Any>> }
                }
            }
            _ => syn::parse_quote! { () },
        }
    }
}

impl TryFrom<ast::File<'_>> for syn::File {
    type Error = CompilerError;

    fn try_from(file: ast::File) -> Result<Self, Self::Error> {
        let is_main_package = file.name.name == "main";
        let _main_package_var_mode = MainPackageVarModeGuard::set(is_main_package);

        // Track import names for selector expr disambiguation
        IMPORT_NAMES.with(|names| {
            let mut set = names.borrow_mut();
            set.clear();
            for import in file.imports() {
                if let Some(pkg_name) = import_local_name(import) {
                    set.insert(pkg_name);
                }
            }
        });
        BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow_mut().clear());

        set_borrow_pointer_arg_indices_for_decls_if_unseeded(&file.decls);
        let needed_imported_interface_methods = if is_main_package {
            collect_needed_imported_interface_method_sets(&file.decls)
        } else {
            BTreeMap::new()
        };

        let mut items = vec![];
        let mut methods: BTreeMap<String, Vec<syn::ImplItemFn>> = BTreeMap::new();
        let mut method_generics: BTreeMap<String, Vec<syn::Ident>> = BTreeMap::new();
        let mut init_bodies: Vec<syn::Block> = vec![];
        let mut package_var_stmts: Vec<syn::Stmt> = vec![];

        // Collect trait names and struct method signatures for interface satisfaction
        let mut trait_methods: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut struct_methods: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut struct_has_string_method: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut struct_has_pointer_string_method: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for decl in file.decls {
            match decl {
                ast::Decl::FuncDecl(func_decl) => {
                    // Detect init() functions: collect their bodies, don't emit as standalone
                    if func_decl.name.name == "init" && func_decl.recv.is_none() {
                        if let Some(body) = func_decl.body {
                            let block: syn::Block = body.try_into()?;
                            init_bodies.push(block);
                        }
                        continue;
                    }

                    if func_decl.recv.is_some() {
                        // Track method names for interface satisfaction
                        let recv_type_name = func_decl
                            .recv
                            .as_ref()
                            .and_then(|r| r.list.first())
                            .and_then(|f| f.type_.as_ref())
                            .and_then(|t| match t {
                                ast::Expr::Ident(id) => Some(id.name.to_string()),
                                ast::Expr::StarExpr(star) => {
                                    if let ast::Expr::Ident(id) = &*star.x {
                                        Some(id.name.to_string())
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            });
                        if let Some(ref type_name) = recv_type_name {
                            let method_name = func_decl.name.name.to_string();
                            if method_name == "String" {
                                // Check if it returns string
                                let returns_string = func_decl
                                    .type_
                                    .results
                                    .as_ref()
                                    .is_some_and(|r| {
                                        r.list.len() == 1
                                            && r.list.first().is_some_and(|f| {
                                                f.type_.as_ref().is_some_and(|t| {
                                                    matches!(t, ast::Expr::Ident(id) if id.name == "string")
                                                })
                                            })
                                    });
                                if returns_string {
                                    struct_has_string_method.insert(type_name.clone());
                                    let is_pointer_receiver = func_decl
                                        .recv
                                        .as_ref()
                                        .and_then(|r| r.list.first())
                                        .and_then(|f| f.type_.as_ref())
                                        .is_some_and(|t| matches!(t, ast::Expr::StarExpr(_)));
                                    if is_pointer_receiver {
                                        struct_has_pointer_string_method.insert(type_name.clone());
                                    }
                                }
                            }
                            struct_methods
                                .entry(type_name.clone())
                                .or_default()
                                .push(method_name);
                        }
                        let (type_name, type_args, method) = compile_method(func_decl)?;
                        method_generics
                            .entry(type_name.clone())
                            .or_insert(type_args);
                        methods.entry(type_name).or_default().push(method);
                    } else {
                        items.push(syn::Item::Fn(func_decl.try_into()?));
                    }
                }
                ast::Decl::GenDecl(gen_decl) => {
                    if gen_decl.tok == token::Token::CONST {
                        items.extend(compile_const_decl(gen_decl)?);
                    } else if gen_decl.tok == token::Token::VAR {
                        for spec in gen_decl.specs {
                            if let ast::Spec::ValueSpec(vs) = spec {
                                if is_main_package {
                                    // Package-level main vars stay local to main's startup path.
                                    let names = vs.names;
                                    let type_expr = vs.type_;
                                    let explicit_go_type =
                                        type_expr.as_ref().map(typeinfer::GoType::from_expr);
                                    let rust_type =
                                        type_expr.as_ref().map(local_value_type_from_expr);
                                    let mut values_iter = vs.values.unwrap_or_default().into_iter();

                                    for name in names {
                                        let ident: syn::Ident = name.into();
                                        let init_ast = values_iter.next();

                                        if let Some(init_ast) = init_ast {
                                            let init = if let Some(go_type) = &explicit_go_type {
                                                compile_expr_with_expected(init_ast, Some(go_type))
                                            } else {
                                                init_ast.into()
                                            };
                                            if let Some(ty) = &rust_type {
                                                package_var_stmts.push(syn::parse_quote! {
                                                    let mut #ident: #ty = #init;
                                                });
                                            } else {
                                                package_var_stmts.push(syn::parse_quote! {
                                                    let mut #ident = #init;
                                                });
                                            }
                                        } else if let (Some(type_expr), Some(ty)) =
                                            (type_expr.as_ref(), rust_type.as_ref())
                                        {
                                            let zero = default_expr_for_type(type_expr);
                                            package_var_stmts.push(syn::parse_quote! {
                                                let mut #ident: #ty = #zero;
                                            });
                                        } else {
                                            package_var_stmts.push(syn::parse_quote! {
                                                let mut #ident = Default::default();
                                            });
                                        }
                                    }
                                } else {
                                    items.extend(compile_top_level_value_spec(
                                        vs,
                                        token::Token::VAR,
                                    )?);
                                }
                            }
                        }
                    } else if gen_decl.tok == token::Token::TYPE {
                        for spec in gen_decl.specs {
                            if let ast::Spec::TypeSpec(ts) = spec {
                                // Track interface methods for satisfaction checking
                                if let ast::Expr::InterfaceType(ref iface) = ts.type_ {
                                    if let Some(ref name_ident) = ts.name {
                                        let trait_name = name_ident.name.to_string();
                                        let mut method_names = vec![];
                                        if let Some(ref methods_list) = iface.methods {
                                            for field in &methods_list.list {
                                                if let Some(ref names) = field.names {
                                                    for n in names {
                                                        method_names.push(n.name.to_string());
                                                    }
                                                }
                                            }
                                        }
                                        trait_methods.insert(trait_name, method_names);
                                    }
                                }
                                items.extend(compile_type_spec(ts)?);
                            }
                        }
                    }
                }
            }
        }

        // If there are init() bodies or package-level vars, prepend them to main()
        if !init_bodies.is_empty() || !package_var_stmts.is_empty() {
            for item in &mut items {
                if let syn::Item::Fn(func) = item {
                    if func.sig.ident == "main" {
                        let mut prepend: Vec<syn::Stmt> = package_var_stmts.clone();
                        for body in &init_bodies {
                            prepend.extend(body.stmts.clone());
                        }
                        let existing = std::mem::take(&mut func.block.stmts);
                        func.block.stmts = prepend;
                        func.block.stmts.extend(existing);
                        break;
                    }
                }
            }
        }

        for (type_name, method_list) in &methods {
            let type_ident = syn::Ident::new(type_name, Span::mixed_site());
            let type_args = method_generics.get(type_name).cloned().unwrap_or_default();
            let has_borrowed_interface_field =
                BORROWED_INTERFACE_STRUCTS.with(|structs| structs.borrow().contains_key(type_name));
            let mut generics = generics_for_idents(&type_args);
            if has_borrowed_interface_field {
                let mut params = syn::punctuated::Punctuated::new();
                params.push(syn::GenericParam::Lifetime(syn::LifetimeParam::new(
                    syn::Lifetime::new("'__gors", Span::mixed_site()),
                )));
                params.extend(generics.params.clone());
                generics.params = params;
            }
            let self_ty: syn::Type = if has_borrowed_interface_field && type_args.is_empty() {
                syn::parse_quote! { #type_ident<'__gors> }
            } else if has_borrowed_interface_field {
                syn::parse_quote! { #type_ident<'__gors, #(#type_args),*> }
            } else if type_args.is_empty() {
                syn::parse_quote! { #type_ident }
            } else {
                syn::parse_quote! { #type_ident<#(#type_args),*> }
            };
            items.push(syn::Item::Impl(syn::ItemImpl {
                attrs: vec![],
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics,
                trait_: None,
                self_ty: Box::new(self_ty),
                brace_token: syn::token::Brace::default(),
                items: method_list.iter().cloned().map(syn::ImplItem::Fn).collect(),
            }));
        }

        if is_main_package {
            for (trait_name, method_names) in needed_imported_interface_methods {
                trait_methods.entry(trait_name).or_insert(method_names);
            }
        }

        // Interface satisfaction: check which structs satisfy which traits
        for (trait_name, required_methods) in &trait_methods {
            if required_methods.is_empty() {
                continue;
            }
            for (struct_name, struct_method_list) in &struct_methods {
                let satisfies = required_methods
                    .iter()
                    .all(|m| struct_method_list.contains(m));
                if satisfies {
                    let trait_path = interface_trait_path_from_name(trait_name);
                    let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());

                    // Get method implementations from the methods map
                    let mut impl_items: Vec<syn::ImplItem> = vec![];
                    let exposes_any = !BORROWED_INTERFACE_STRUCTS
                        .with(|structs| structs.borrow().contains_key(struct_name))
                        && method_generics
                            .get(struct_name)
                            .is_none_or(std::vec::Vec::is_empty);
                    let as_any_method: syn::ImplItem = if exposes_any {
                        syn::parse_quote! {
                            fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                                Some(self)
                            }
                        }
                    } else {
                        syn::parse_quote! {
                            fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                                None
                            }
                        }
                    };
                    impl_items.push(as_any_method);
                    if let Some(method_list) = methods.get(struct_name) {
                        for method in method_list {
                            if required_methods.contains(&method.sig.ident.to_string()) {
                                let mut m = method.clone();
                                m.vis = syn::Visibility::Inherited;
                                if let Some(syn::FnArg::Receiver(receiver)) =
                                    m.sig.inputs.first_mut()
                                {
                                    receiver.mutability = Some(<Token![mut]>::default());
                                    *receiver.ty = syn::parse_quote! { &mut Self };
                                }
                                impl_items.push(syn::ImplItem::Fn(m));
                            }
                        }
                    }

                    items.push(syn::Item::Impl(syn::ItemImpl {
                        attrs: vec![],
                        defaultness: None,
                        unsafety: None,
                        impl_token: <Token![impl]>::default(),
                        generics: if BORROWED_INTERFACE_STRUCTS
                            .with(|structs| structs.borrow().contains_key(struct_name))
                        {
                            syn::parse_quote! { <'__gors> }
                        } else {
                            syn::Generics::default()
                        },
                        trait_: Some((None, trait_path, <Token![for]>::default())),
                        self_ty: Box::new(
                            if BORROWED_INTERFACE_STRUCTS
                                .with(|structs| structs.borrow().contains_key(struct_name))
                            {
                                syn::parse_quote! { #struct_ident<'__gors> }
                            } else {
                                syn::parse_quote! { #struct_ident }
                            },
                        ),
                        brace_token: syn::token::Brace::default(),
                        items: impl_items,
                    }));
                }
            }
        }

        items.extend(embedded_interface_impls(&items, &methods));

        // Stringer pattern: generate `impl Display` for structs with String() string
        for struct_name in &struct_has_string_method {
            if struct_has_pointer_string_method.contains(struct_name) {
                continue;
            }
            if !methods.get(struct_name).is_some_and(|method_list| {
                method_list
                    .iter()
                    .any(|method| method.sig.ident == "String")
            }) {
                continue;
            }
            let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
            items.push(syn::parse_quote! {
                impl std::fmt::Display for #struct_ident {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "{}", self.String())
                    }
                }
            });
        }

        Ok(Self {
            attrs: vec![],
            items,
            shebang: None,
        })
    }
}

fn pointer_param_names(params: &ast::FieldList) -> std::collections::HashSet<String> {
    params
        .list
        .iter()
        .filter(|field| matches!(field.type_, Some(ast::Expr::StarExpr(_))))
        .filter_map(|field| field.names.as_ref())
        .flat_map(|names| names.iter().map(|name| rust_safe_ident_name(name.name)))
        .collect()
}

fn pointer_params_to_borrow(
    params: &ast::FieldList,
    body: Option<&ast::BlockStmt>,
) -> std::collections::HashSet<String> {
    let pointer_names = pointer_param_names(params);
    if pointer_names.is_empty() {
        return pointer_names;
    }

    let mut escaped = std::collections::HashSet::new();
    if let Some(body) = body {
        collect_escaped_pointer_params_block(body, &pointer_names, &mut escaped);
    }

    pointer_names
        .into_iter()
        .filter(|name| !escaped.contains(name))
        .collect()
}

fn borrow_pointer_param_indices(
    params: &ast::FieldList,
    body: Option<&ast::BlockStmt>,
) -> std::collections::HashSet<usize> {
    let borrow_names = pointer_params_to_borrow(params, body);
    let mut indices = std::collections::HashSet::new();
    let mut index = 0;
    for field in &params.list {
        let is_pointer = matches!(field.type_, Some(ast::Expr::StarExpr(_)));
        let count = field.names.as_ref().map_or(1, Vec::len);
        if let Some(names) = &field.names {
            for name in names {
                if is_pointer && borrow_names.contains(&rust_safe_ident_name(name.name)) {
                    indices.insert(index);
                }
                index += 1;
            }
        } else {
            index += count;
        }
    }
    indices
}

fn collect_borrow_pointer_arg_indices(
    decls: &[ast::Decl],
) -> BTreeMap<String, std::collections::HashSet<usize>> {
    let mut map = BTreeMap::new();
    for decl in decls {
        let ast::Decl::FuncDecl(func_decl) = decl else {
            continue;
        };
        let indices =
            borrow_pointer_param_indices(&func_decl.type_.params, func_decl.body.as_ref());
        if indices.is_empty() {
            continue;
        }
        map.insert(func_decl.name.name.to_string(), indices.clone());
        if let Some(recv) = &func_decl.recv
            && let Some(recv_field) = recv.list.first()
            && let Some(recv_type) = &recv_field.type_
            && let Ok((type_name, _)) = extract_receiver_type(recv_type)
        {
            map.insert(format!("{type_name}.{}", func_decl.name.name), indices);
        }
    }
    map
}

fn set_borrow_pointer_arg_indices_for_decls(decls: &[ast::Decl], preseeded: bool) {
    let map = collect_borrow_pointer_arg_indices(decls);
    BORROW_POINTER_ARG_INDICES.with(|indices| {
        *indices.borrow_mut() = map;
    });
    BORROW_POINTER_ARG_INDICES_PRESEEDED.with(|flag| {
        *flag.borrow_mut() = preseeded;
    });
}

fn set_borrow_pointer_arg_indices_for_decls_if_unseeded(decls: &[ast::Decl]) {
    let preseeded = BORROW_POINTER_ARG_INDICES_PRESEEDED.with(|flag| *flag.borrow());
    if !preseeded {
        set_borrow_pointer_arg_indices_for_decls(decls, false);
    }
}

pub(crate) fn set_borrow_pointer_arg_indices_for_files(files: &[&ast::File<'_>]) {
    let mut map = BTreeMap::new();
    for file in files {
        let file_map = collect_borrow_pointer_arg_indices(&file.decls);
        for (name, indices) in &file_map {
            map.insert(format!("{}.{}", file.name.name, name), indices.clone());
        }
        map.extend(file_map);
    }
    BORROW_POINTER_ARG_INDICES.with(|indices| {
        *indices.borrow_mut() = map;
    });
    BORROW_POINTER_ARG_INDICES_PRESEEDED.with(|flag| {
        *flag.borrow_mut() = true;
    });
}

pub(crate) fn clear_borrow_pointer_arg_indices() {
    BORROW_POINTER_ARG_INDICES.with(|indices| indices.borrow_mut().clear());
    BORROW_POINTER_ARG_INDICES_PRESEEDED.with(|flag| {
        *flag.borrow_mut() = false;
    });
}

fn borrow_pointer_call_arg_indices(fun: &ast::Expr) -> std::collections::HashSet<usize> {
    let name = match fun {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(receiver) = &*selector.x {
                TYPE_ENV.with(|env| {
                    let env = env.borrow();
                    let package_key = format!("{}.{}", receiver.name, selector.sel.name);
                    if env.has_func(&package_key) {
                        return Some(package_key);
                    }
                    env.get_var(receiver.name)
                        .and_then(|ty| receiver_method_type_name_for_call(ty, &env))
                        .map(|name| format!("{name}.{}", selector.sel.name))
                        .or_else(|| Some(selector.sel.name.to_string()))
                })
            } else {
                Some(selector.sel.name.to_string())
            }
        }
        _ => None,
    };
    let Some(name) = name else {
        return std::collections::HashSet::new();
    };
    BORROW_POINTER_ARG_INDICES.with(|indices| {
        indices
            .borrow()
            .get(&name)
            .cloned()
            .unwrap_or_else(std::collections::HashSet::new)
    })
}

fn borrow_pointer_arg_expr(expr: &mut syn::Expr, actual: Option<&typeinfer::GoType>) {
    if matches!(expr, syn::Expr::Reference(_)) {
        return;
    }
    if matches!(expr, syn::Expr::Path(path) if path.path.is_ident("self")) {
        return;
    }
    if let syn::Expr::Call(call) = expr
        && is_path_call_expr(&call.func, &["Box", "new"])
        && call.args.len() == 1
        && let Some(inner) = call.args.first()
    {
        *expr = syn::parse_quote! { &mut #inner };
        return;
    }
    if let syn::Expr::Call(call) = expr
        && is_path_call_expr(&call.func, &["std", "sync", "Arc", "new"])
        && call.args.len() == 1
        && let Some(syn::Expr::Call(mutex_call)) = call.args.first()
        && is_path_call_expr(&mutex_call.func, &["std", "sync", "Mutex", "new"])
        && mutex_call.args.len() == 1
        && let Some(inner) = mutex_call.args.first()
    {
        *expr = syn::parse_quote! { &mut #inner };
        return;
    }
    if let Some(lock_target) = pointer_cell_lock_target_expr(expr) {
        *expr = syn::parse_quote! { &mut *(#lock_target).lock().unwrap() };
        return;
    }
    let inner = expr.clone();
    if matches!(
        actual.map(resolved_go_type),
        Some(typeinfer::GoType::Pointer(_))
    ) {
        if is_borrowed_pointer_path_expr(&inner) {
            *expr = syn::parse_quote! { &mut *#inner };
        } else {
            let lock_target = strip_clone_method_call(&inner).unwrap_or(inner);
            *expr = syn::parse_quote! { &mut *(#lock_target).lock().unwrap() };
        }
    } else {
        *expr = syn::parse_quote! { &mut #inner };
    }
}

fn borrowed_address_of_ident_arg_expr(expr: &ast::Expr) -> Option<syn::Expr> {
    let ast::Expr::UnaryExpr(unary) = expr else {
        return None;
    };
    if unary.op != token::Token::AND {
        return None;
    }
    let ast::Expr::Ident(ident) = &*unary.x else {
        return None;
    };
    let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
    if is_shared_capture_name(&ident.to_string()) {
        Some(syn::parse_quote! { &mut *#ident.lock().unwrap() })
    } else {
        Some(syn::parse_quote! { &mut #ident })
    }
}

fn should_borrow_pointer_arg_by_shape(
    expr: &ast::Expr,
    expected: Option<&typeinfer::GoType>,
) -> bool {
    if !matches!(
        expected.map(resolved_go_type),
        Some(typeinfer::GoType::Pointer(_))
    ) {
        return false;
    }
    match expr {
        ast::Expr::ParenExpr(paren) => should_borrow_pointer_arg_by_shape(&paren.x, expected),
        ast::Expr::SelectorExpr(selector) => matches!(
            &*selector.x,
            ast::Expr::Ident(pkg)
                if IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name))
        ),
        _ => false,
    }
}

fn strip_clone_method_call(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::MethodCall(method) = expr else {
        return None;
    };
    if method.method != "clone" || !method.args.is_empty() {
        return None;
    }
    Some((*method.receiver).clone())
}

fn pointer_cell_lock_target_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    if let Some(stripped) = strip_clone_method_call(expr) {
        return pointer_cell_lock_target_expr(&stripped).or(Some(stripped));
    }
    match expr {
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => Some(expr.clone()),
        _ => None,
    }
}

fn is_borrowed_pointer_path_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path
                .path
                .segments
                .first()
                .is_some_and(|segment| is_borrowed_pointer_param_name(&segment.ident.to_string())))
}

fn is_path_call_expr(func: &syn::Expr, segments: &[&str]) -> bool {
    let syn::Expr::Path(path) = func else {
        return false;
    };
    path.path.segments.len() == segments.len()
        && path
            .path
            .segments
            .iter()
            .zip(segments)
            .all(|(segment, expected)| segment.ident == *expected)
}

fn collect_escaped_pointer_params_block(
    block: &ast::BlockStmt,
    pointer_names: &std::collections::HashSet<String>,
    escaped: &mut std::collections::HashSet<String>,
) {
    for stmt in &block.list {
        collect_escaped_pointer_params_stmt(stmt, pointer_names, escaped);
    }
}

fn collect_escaped_pointer_params_stmt(
    stmt: &ast::Stmt,
    pointer_names: &std::collections::HashSet<String>,
    escaped: &mut std::collections::HashSet<String>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in &assign.rhs {
                collect_escaped_pointer_params_value_expr(expr, pointer_names, escaped);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_escaped_pointer_params_block(block, pointer_names, escaped);
        }
        ast::Stmt::CaseClause(case_clause) => {
            for stmt in &case_clause.body {
                collect_escaped_pointer_params_stmt(stmt, pointer_names, escaped);
            }
        }
        ast::Stmt::CommClause(comm_clause) => {
            if let Some(stmt) = &comm_clause.comm {
                collect_escaped_pointer_params_stmt(stmt, pointer_names, escaped);
            }
            for stmt in &comm_clause.body {
                collect_escaped_pointer_params_stmt(stmt, pointer_names, escaped);
            }
        }
        ast::Stmt::DeclStmt(decl_stmt) => {
            for spec in &decl_stmt.decl.specs {
                if let ast::Spec::ValueSpec(value_spec) = spec
                    && let Some(values) = &value_spec.values
                {
                    for expr in values {
                        collect_escaped_pointer_params_value_expr(expr, pointer_names, escaped);
                    }
                }
            }
        }
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_escaped_pointer_params_stmt(init, pointer_names, escaped);
            }
            if let Some(post) = &for_stmt.post {
                collect_escaped_pointer_params_stmt(post, pointer_names, escaped);
            }
            collect_escaped_pointer_params_block(&for_stmt.body, pointer_names, escaped);
        }
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_escaped_pointer_params_stmt(init, pointer_names, escaped);
            }
            collect_escaped_pointer_params_block(&if_stmt.body, pointer_names, escaped);
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_escaped_pointer_params_stmt(else_stmt, pointer_names, escaped);
            }
        }
        ast::Stmt::LabeledStmt(labeled) => {
            collect_escaped_pointer_params_stmt(&labeled.stmt, pointer_names, escaped);
        }
        ast::Stmt::RangeStmt(range_stmt) => {
            collect_escaped_pointer_params_block(&range_stmt.body, pointer_names, escaped);
        }
        ast::Stmt::ReturnStmt(return_stmt) => {
            for expr in &return_stmt.results {
                collect_escaped_pointer_params_value_expr(expr, pointer_names, escaped);
            }
        }
        ast::Stmt::SelectStmt(select_stmt) => {
            collect_escaped_pointer_params_block(&select_stmt.body, pointer_names, escaped);
        }
        ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = &switch_stmt.init {
                collect_escaped_pointer_params_stmt(init, pointer_names, escaped);
            }
            collect_escaped_pointer_params_block(&switch_stmt.body, pointer_names, escaped);
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_escaped_pointer_params_stmt(init, pointer_names, escaped);
            }
            collect_escaped_pointer_params_stmt(&type_switch.assign, pointer_names, escaped);
            collect_escaped_pointer_params_block(&type_switch.body, pointer_names, escaped);
        }
        _ => {}
    }
}

fn collect_escaped_pointer_params_value_expr(
    expr: &ast::Expr,
    pointer_names: &std::collections::HashSet<String>,
    escaped: &mut std::collections::HashSet<String>,
) {
    match expr {
        ast::Expr::Ident(ident) => {
            let name = rust_safe_ident_name(ident.name);
            if pointer_names.contains(&name) {
                escaped.insert(name);
            }
        }
        ast::Expr::CompositeLit(composite) => {
            if let Some(elts) = &composite.elts {
                for elt in elts {
                    collect_escaped_pointer_params_value_expr(elt, pointer_names, escaped);
                }
            }
        }
        ast::Expr::KeyValueExpr(key_value) => {
            collect_escaped_pointer_params_value_expr(&key_value.value, pointer_names, escaped);
        }
        ast::Expr::ParenExpr(paren) => {
            collect_escaped_pointer_params_value_expr(&paren.x, pointer_names, escaped);
        }
        _ => {}
    }
}

fn type_from_param_expr(
    expr: &ast::Expr,
    name: &str,
    borrow_pointer_params: &std::collections::HashSet<String>,
) -> syn::Type {
    match expr {
        ast::Expr::StarExpr(star) => {
            let inner = type_from_expr_ref(&star.x);
            if borrow_pointer_params.contains(&rust_safe_ident_name(name)) {
                syn::parse_quote! { &mut #inner }
            } else {
                syn::parse_quote! { std::sync::Arc<std::sync::Mutex<#inner>> }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                let inner = type_from_expr_ref(elt);
                syn::parse_quote! { Vec<#inner> }
            } else {
                syn::parse_quote! { Vec<Box<dyn std::any::Any>> }
            }
        }
        ast::Expr::FuncType(func_type) => shared_func_type_from_ast(func_type),
        _ if type_expr_resolves_to_slice_alias(expr) => {
            let inner = type_from_expr_ref(expr);
            syn::parse_quote! { &mut #inner }
        }
        _ => type_from_expr_ref(expr),
    }
}

fn type_kind_for_type_expr(expr: &ast::Expr) -> Option<typeinfer::TypeKind> {
    if let ast::Expr::ParenExpr(paren) = expr {
        return type_kind_for_type_expr(&paren.x);
    }
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match expr {
            ast::Expr::Ident(ident) => env.get_type_kind(ident.name).cloned(),
            ast::Expr::SelectorExpr(selector) => {
                if let ast::Expr::Ident(pkg) = &*selector.x {
                    let key = format!("{}.{}", pkg.name, selector.sel.name);
                    env.get_type_kind(&key)
                        .cloned()
                        .or_else(|| env.get_type_kind(selector.sel.name).cloned())
                } else {
                    None
                }
            }
            ast::Expr::IndexExpr(index) => {
                extract_type_name(&index.x).and_then(|name| env.get_type_kind(&name).cloned())
            }
            ast::Expr::IndexListExpr(index) => {
                extract_type_name(&index.x).and_then(|name| env.get_type_kind(&name).cloned())
            }
            _ => None,
        }
    })
}

fn type_expr_resolves_to_slice_alias(expr: &ast::Expr) -> bool {
    matches!(
        type_kind_for_type_expr(expr),
        Some(typeinfer::TypeKind::Alias(typeinfer::GoType::Slice(_)))
    )
}

fn compile_field_to_fn_args_with_type_params(
    field: ast::Field,
    type_param_info: &TypeParamInfo,
    borrow_pointer_params: &std::collections::HashSet<String>,
) -> Result<Vec<syn::FnArg>, CompilerError> {
    let type_expr = field
        .type_
        .ok_or_else(|| CompilerError::InvalidFunctionSignature("field has no type".to_string()))?;
    let go_type = typeinfer::GoType::from_expr(&type_expr);
    let names = field.names.unwrap_or_else(|| {
        vec![ast::Ident {
            name_pos: token::Position::default(),
            name: "",
            obj: None,
        }]
    });

    // Go strings map to String in Rust (owned). Parameters keep String type
    // since Go allows reassigning string parameters within functions.

    let mut args = Vec::new();
    for name in names {
        let name_str = name.name;
        let mut rust_type: syn::Type =
            if let Some(elem) = generic_slice_param_element_type(&type_expr, type_param_info) {
                syn::parse_quote! { &mut Vec<#elem> }
            } else {
                type_from_param_expr(&type_expr, name_str, borrow_pointer_params)
            };

        // Use &mut dyn Trait for interface parameters
        if let typeinfer::GoType::Named(ref name) = go_type {
            if is_type_interface(name) || TYPE_ENV.with(|env| env.borrow().is_interface(name)) {
                rust_type = syn::parse_quote! { &mut dyn #rust_type };
            }
        }

        let ident = if name.name.is_empty() || name.name == "_" {
            next_unnamed_arg_ident()
        } else {
            name.into()
        };
        args.push(syn::FnArg::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: Some(<Token![mut]>::default()),
                ident,
            })),
            colon_token: <Token![:]>::default(),
            ty: Box::new(rust_type.clone()),
        }));
    }
    Ok(args)
}

fn fn_arg_ident(arg: &syn::FnArg) -> Option<syn::Ident> {
    let syn::FnArg::Typed(pat_type) = arg else {
        return None;
    };
    let syn::Pat::Ident(pat_ident) = &*pat_type.pat else {
        return None;
    };
    Some(pat_ident.ident.clone())
}

fn bodyless_function_block(
    output: &syn::ReturnType,
    inputs: &syn::punctuated::Punctuated<syn::FnArg, Token![,]>,
) -> syn::Block {
    if matches!(output, syn::ReturnType::Default) {
        return syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: vec![],
        };
    }

    if let syn::ReturnType::Type(_, ty) = output
        && vec_type_inner(ty).is_some()
        && let Some(len_ident) = inputs.iter().find_map(fn_arg_ident)
    {
        return syn::parse_quote! {{
            let __gors_len = usize::try_from(#len_ident).unwrap_or_default();
            std::iter::repeat_with(Default::default).take(__gors_len).collect()
        }};
    }

    syn::parse_quote! {{
        Default::default()
    }}
}

fn block_ends_with_value(block: &syn::Block) -> bool {
    block.stmts.last().is_some_and(|last| {
        matches!(
            last,
            syn::Stmt::Expr(syn::Expr::Return(_), _) | syn::Stmt::Expr(_, None)
        )
    })
}

fn append_missing_return_panic(
    block: &mut syn::Block,
    output: &syn::ReturnType,
    completion: Option<ir::Completion>,
) {
    if matches!(output, syn::ReturnType::Default) || block_ends_with_value(block) {
        return;
    }

    let message = if completion == Some(ir::Completion::Terminates) {
        "gors: unreachable missing return"
    } else {
        "gors: missing return"
    };
    let panic_expr: syn::Expr = syn::parse_quote! {
        panic!(#message)
    };
    block.stmts.push(syn::Stmt::Expr(panic_expr, None));
}

impl TryFrom<ast::FuncDecl<'_>> for syn::ItemFn {
    type Error = CompilerError;

    fn try_from(func_decl: ast::FuncDecl) -> Result<Self, Self::Error> {
        reset_unnamed_arg_counter();

        // Record mapping for the function keyword with Go name
        if let Some(ref func_pos) = func_decl.type_.func {
            record_mapping(func_pos, Some("func"));
        }

        // Convert doc comments to Rust doc attributes
        let attrs = comment_group_to_attrs(&func_decl.doc);

        let type_param_info = collect_type_param_info(func_decl.type_.type_params.as_ref());

        // Register parameter types in the type environment
        TYPE_ENV.with(|env| {
            let mut e = env.borrow_mut();
            for param in &func_decl.type_.params.list {
                let ty = param
                    .type_
                    .as_ref()
                    .map(|expr| {
                        generic_slice_param_go_type(expr, &type_param_info)
                            .unwrap_or_else(|| typeinfer::GoType::from_expr(expr))
                    })
                    .unwrap_or(typeinfer::GoType::Unknown);
                if let Some(ref names) = param.names {
                    for name in names {
                        e.set_var(name.name, ty.clone());
                    }
                }
            }
        });

        let borrow_pointer_params =
            pointer_params_to_borrow(&func_decl.type_.params, func_decl.body.as_ref());
        if let Some(body) = func_decl.body.as_ref() {
            validate_function_semantics(&func_decl.type_, body)?;
        }
        let borrowed_pointer_param_names =
            BorrowedPointerParamNamesGuard::set(borrow_pointer_params.clone());
        let mut inputs = syn::punctuated::Punctuated::new();
        for param in func_decl.type_.params.list {
            for arg in compile_field_to_fn_args_with_type_params(
                param,
                &type_param_info,
                &borrow_pointer_params,
            )? {
                inputs.push(arg);
            }
        }

        let vis = (&func_decl.name).into();

        // Analyze return values for named returns
        let mut named_return_info: Vec<(syn::Ident, Option<syn::Type>, syn::Expr)> = vec![];
        let mut named_return_idents: Vec<syn::Ident> = vec![];

        if let Some(ref results) = func_decl.type_.results {
            let has_named_returns = results
                .list
                .iter()
                .any(|f| f.names.as_ref().is_some_and(|names| !names.is_empty()));
            if has_named_returns {
                for field in &results.list {
                    if let Some(ref names) = field.names {
                        for name in names {
                            let type_name = field.type_.as_ref().and_then(go_type_name_from_expr);
                            let rust_type = field.type_.as_ref().map(type_from_expr_ref);
                            let zero = zero_value_for_type(type_name);
                            let ident = syn::Ident::new(
                                &rust_safe_ident_name(name.name),
                                Span::mixed_site(),
                            );
                            named_return_info.push((ident.clone(), rust_type, zero));
                            named_return_idents.push(ident);
                        }
                    }
                }
            }
        }

        let return_go_types = collect_return_go_types(func_decl.type_.results.as_ref());
        let mut output = compile_return_type(func_decl.type_.results)?;
        add_elided_lifetime_to_borrowed_interface_return(&mut output, &inputs);

        let previous_byte_seq_type_params = BYTE_SEQ_TYPE_PARAMS.with(|params| {
            std::mem::replace(
                &mut *params.borrow_mut(),
                type_param_info.byte_seq_names.clone(),
            )
        });
        let previous_return_types =
            RETURN_TYPES.with(|types| std::mem::replace(&mut *types.borrow_mut(), return_go_types));
        let previous_named_return_idents = NAMED_RETURN_IDENTS.with(|idents| {
            std::mem::replace(&mut *idents.borrow_mut(), named_return_idents.clone())
        });
        let body_shared_capture_names =
            func_decl
                .body
                .as_ref()
                .map_or_else(std::collections::BTreeSet::new, |body| {
                    TYPE_ENV.with(|env| {
                        let env = env.borrow();
                        let mut names = ir::mutable_func_lit_capture_names_in_block(body, &env);
                        names.extend(ir::mutable_range_function_capture_names_in_block(
                            body, &env,
                        ));
                        names.extend(ir::for_clause_per_iteration_capture_names_in_block(
                            body, &env,
                        ));
                        names
                    })
                });
        let _body_shared_capture_names = SharedCaptureNamesGuard::extend(body_shared_capture_names);
        let body_completion = func_decl.body.as_ref().map(|body| {
            TYPE_ENV.with(|env| {
                let env = env.borrow();
                ir::ast_block_completion(body, &env)
            })
        });
        let body_has_defer = func_decl.body.as_ref().is_some_and(block_has_defer);
        let block_result = if let Some(body) = func_decl.body {
            body.try_into()
        } else {
            Ok(bodyless_function_block(&output, &inputs))
        };
        drop(borrowed_pointer_param_names);
        BYTE_SEQ_TYPE_PARAMS.with(|params| {
            *params.borrow_mut() = previous_byte_seq_type_params;
        });
        RETURN_TYPES.with(|types| {
            *types.borrow_mut() = previous_return_types;
        });
        NAMED_RETURN_IDENTS.with(|idents| {
            *idents.borrow_mut() = previous_named_return_idents;
        });
        let mut block = Box::new(block_result?);
        if body_has_defer {
            prepend_defer_stack(&mut block);
        }

        // For named returns: prepend variable declarations and rewrite bare returns
        if !named_return_info.is_empty() {
            wrap_named_return_block(&mut block, &named_return_info, &named_return_idents);
        } else {
            append_missing_return_panic(&mut block, &output, body_completion);
        }

        // Convert type parameters to Rust generics (Go 1.18+ generics)
        let generics = compile_go_type_params(func_decl.type_.type_params);

        let sig = syn::Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: <Token![fn]>::default(),
            ident: func_decl.name.into(),
            generics,
            paren_token: syn::token::Paren::default(),
            inputs,
            variadic: None,
            output,
        };

        Ok(Self {
            attrs,
            block,
            sig,
            vis,
        })
    }
}

impl From<ast::Ident<'_>> for syn::ExprPath {
    fn from(ident: ast::Ident) -> Self {
        let mut segments = syn::punctuated::Punctuated::new();
        segments.push(syn::PathSegment {
            ident: ident.into(),
            arguments: syn::PathArguments::None,
        });

        Self {
            attrs: vec![],
            path: syn::Path {
                leading_colon: None,
                segments,
            },
            qself: None,
        }
    }
}

impl From<&ast::Ident<'_>> for syn::Visibility {
    fn from(name: &ast::Ident) -> Self {
        if name.name == "main" || matches!(name.name.chars().next(), Some('A'..='Z')) {
            syn::parse_quote! { pub }
        } else {
            Self::Inherited
        }
    }
}

impl From<ast::SelectorExpr<'_>> for syn::ExprPath {
    fn from(selector_expr: ast::SelectorExpr) -> Self {
        let mut segments = syn::punctuated::Punctuated::new();

        match *selector_expr.x {
            ast::Expr::Ident(ident) => {
                let ident_name = import_rust_name(ident.name);
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new(&ident_name, Span::mixed_site()),
                    arguments: syn::PathArguments::None,
                });
            }
            ast::Expr::SelectorExpr(inner_sel) => {
                let inner_path: syn::ExprPath = inner_sel.into();
                for seg in inner_path.path.segments {
                    segments.push(seg);
                }
            }
            _other => {
                return Self {
                    attrs: vec![],
                    path: syn::Path {
                        leading_colon: None,
                        segments: {
                            let mut s = syn::punctuated::Punctuated::new();
                            s.push(syn::PathSegment {
                                ident: syn::Ident::new("__expr", Span::mixed_site()),
                                arguments: syn::PathArguments::None,
                            });
                            s
                        },
                    },
                    qself: None,
                };
            }
        }

        segments.push(syn::PathSegment {
            ident: selector_expr.sel.into(),
            arguments: syn::PathArguments::None,
        });

        Self {
            attrs: vec![],
            path: syn::Path {
                leading_colon: None,
                segments,
            },
            qself: None,
        }
    }
}

impl From<ast::Ident<'_>> for syn::Ident {
    fn from(ident: ast::Ident) -> Self {
        // Record mapping for the identifier
        record_mapping(&ident.name_pos, Some(ident.name));

        Self::new(&rust_safe_ident_name(ident.name), Span::mixed_site())
    }
}

impl TryFrom<ast::Stmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(stmt: ast::Stmt) -> Result<Self, Self::Error> {
        match stmt {
            ast::Stmt::AssignStmt(s) => s.try_into(),
            ast::Stmt::BlockStmt(s) => {
                Ok(vec![syn::Stmt::Expr(syn::Expr::Block(s.try_into()?), None)])
            }
            ast::Stmt::BranchStmt(s) => Ok(s.into()),
            ast::Stmt::DeclStmt(s) => Ok(s.into()),
            ast::Stmt::DeferStmt(s) => {
                if is_catch_panic_defer(&s.call) {
                    return Ok(vec![]);
                }
                Ok(compile_defer_stmt(s.call))
            }
            ast::Stmt::EmptyStmt(_) => Ok(vec![]),
            ast::Stmt::ExprStmt(s) => Ok(vec![syn::Stmt::Expr(
                s.x.into(),
                Some(<Token![;]>::default()),
            )]),
            ast::Stmt::ForStmt(s) => Ok(vec![syn::Stmt::Expr(s.try_into()?, None)]),
            ast::Stmt::GoStmt(go_stmt) => {
                // go f(args...) => std::thread::spawn(move || { f(args...); })
                // go func() { ... }() => std::thread::spawn(move || { ... })
                let call_expr = go_stmt.call;

                if let ast::Expr::FuncLit(func_lit) = *call_expr.fun {
                    // Inline the body directly into the spawn closure
                    let clones = goroutine_capture_clones(&func_lit);
                    let block: syn::Block = func_lit.body.try_into()?;
                    let stmts = &block.stmts;
                    Ok(vec![syn::Stmt::Expr(
                        syn::parse_quote! {
                            {
                                #(#clones)*
                                ::std::thread::spawn(move || { #(#stmts)* })
                            }
                        },
                        Some(<Token![;]>::default()),
                    )])
                } else {
                    // go someFunc(args...)
                    let mut clone_stmts: Vec<syn::Stmt> = Vec::new();
                    if let ast::Expr::Ident(ident) = call_expr.fun.as_ref()
                        && function_value_call_info(&call_expr.fun).is_some()
                    {
                        let name =
                            syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
                        clone_stmts.push(syn::parse_quote! {
                            let #name = #name.clone();
                        });
                    }
                    if let Some(ref args) = call_expr.args {
                        for arg in args.iter() {
                            if let ast::Expr::Ident(ident) = arg {
                                let name = syn::Ident::new(
                                    &rust_safe_ident_name(ident.name),
                                    Span::mixed_site(),
                                );
                                clone_stmts.push(syn::parse_quote! {
                                    let #name = #name.clone();
                                });
                            }
                        }
                    }
                    let call: syn::Expr = ast::Expr::CallExpr(call_expr).into();
                    if clone_stmts.is_empty() {
                        Ok(vec![syn::parse_quote! {
                            ::std::thread::spawn(move || { #call; });
                        }])
                    } else {
                        Ok(vec![syn::Stmt::Expr(
                            syn::parse_quote! {
                                {
                                    #(#clone_stmts)*
                                    ::std::thread::spawn(move || { #call; })
                                }
                            },
                            Some(<Token![;]>::default()),
                        )])
                    }
                }
            }
            ast::Stmt::IfStmt(s) => {
                let has_init = s.init.is_some();
                let init_stmts: Vec<syn::Stmt> = if let Some(init) = *s.init {
                    Vec::<syn::Stmt>::try_from(init)?
                } else {
                    vec![]
                };

                let else_branch = if let Some(else_) = *s.else_ {
                    Some((
                        <Token![else]>::default(),
                        Box::new(match else_ {
                            ast::Stmt::IfStmt(if_stmt) => {
                                let inner_stmts =
                                    Vec::<syn::Stmt>::try_from(ast::Stmt::IfStmt(if_stmt))?;
                                syn::Expr::Block(syn::ExprBlock {
                                    attrs: vec![],
                                    label: None,
                                    block: syn::Block {
                                        brace_token: syn::token::Brace::default(),
                                        stmts: inner_stmts,
                                    },
                                })
                            }
                            ast::Stmt::BlockStmt(block_stmt) => {
                                syn::Expr::Block(block_stmt.try_into()?)
                            }
                            _ => {
                                return Err(CompilerError::UnsupportedConstruct(
                                    "unsupported else branch type".to_string(),
                                ));
                            }
                        }),
                    ))
                } else {
                    None
                };

                let if_expr = syn::Expr::If(syn::ExprIf {
                    attrs: vec![],
                    if_token: <Token![if]>::default(),
                    cond: Box::new(s.cond.into()),
                    then_branch: s.body.try_into()?,
                    else_branch,
                });

                if has_init {
                    let mut block_stmts = init_stmts;
                    block_stmts.push(syn::Stmt::Expr(if_expr, None));
                    Ok(vec![syn::Stmt::Expr(
                        syn::Expr::Block(syn::ExprBlock {
                            attrs: vec![],
                            label: None,
                            block: syn::Block {
                                brace_token: syn::token::Brace::default(),
                                stmts: block_stmts,
                            },
                        }),
                        None,
                    )])
                } else {
                    Ok(vec![syn::Stmt::Expr(if_expr, None)])
                }
            }
            ast::Stmt::IncDecStmt(s) => compile_inc_dec_stmt(s),
            ast::Stmt::LabeledStmt(s) => s.try_into(),
            ast::Stmt::ReturnStmt(s) => {
                Ok(vec![syn::Stmt::Expr(syn::Expr::Return(s.into()), None)])
            }
            ast::Stmt::RangeStmt(s) => compile_range_stmt(s),
            ast::Stmt::SwitchStmt(mut s) => {
                let mut stmts = vec![];
                if let Some(init) = s.init.take() {
                    stmts.extend(Vec::<syn::Stmt>::try_from(*init)?);
                }
                stmts.push(syn::Stmt::Expr(s.try_into()?, Some(<Token![;]>::default())));
                Ok(stmts)
            }
            ast::Stmt::TypeSwitchStmt(s) => compile_type_switch_stmt(s),
            ast::Stmt::SendStmt(send_stmt) => {
                // ch <- value  =>  ch.send(value);
                let chan: syn::Expr = send_stmt.chan.into();
                let value: syn::Expr = send_stmt.value.into();
                Ok(vec![syn::parse_quote! {
                    #chan.send(#value);
                }])
            }
            ast::Stmt::SelectStmt(select_stmt) => compile_select_stmt(select_stmt),
            ast::Stmt::CommClause(_) | ast::Stmt::CaseClause(_) => {
                // These are handled inline by their parent (SwitchStmt/SelectStmt)
                Ok(vec![])
            }
        }
    }
}

fn is_catch_panic_defer(call: &ast::CallExpr) -> bool {
    matches!(
        call.fun.as_ref(),
        ast::Expr::SelectorExpr(selector) if selector.sel.name == "catchPanic"
    )
}

fn compile_inc_dec_stmt(inc_dec_stmt: ast::IncDecStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    if let Some((key_ty, _)) = map_index_types(&inc_dec_stmt.x) {
        return compile_map_index_inc_dec(inc_dec_stmt, key_ty);
    }
    let x = compile_assignment_lhs_checked(inc_dec_stmt.x)?;
    Ok(match inc_dec_stmt.tok {
        token::Token::INC => vec![syn::parse_quote! { #x += 1; }],
        token::Token::DEC => vec![syn::parse_quote! { #x -= 1; }],
        _ => vec![],
    })
}

impl From<ast::BranchStmt<'_>> for Vec<syn::Stmt> {
    fn from(branch_stmt: ast::BranchStmt) -> Self {
        use token::Token::*;
        match branch_stmt.tok {
            BREAK => {
                if let Some(label) = branch_stmt.label {
                    let label_ident: syn::Ident = label.into();
                    let lifetime = syn::Lifetime {
                        apostrophe: Span::call_site(),
                        ident: label_ident,
                    };
                    vec![syn::Stmt::Expr(
                        syn::Expr::Break(syn::ExprBreak {
                            attrs: vec![],
                            break_token: <Token![break]>::default(),
                            label: Some(lifetime),
                            expr: None,
                        }),
                        Some(<Token![;]>::default()),
                    )]
                } else {
                    vec![syn::parse_quote! { break; }]
                }
            }
            CONTINUE => {
                if let Some(label) = branch_stmt.label {
                    let label_ident: syn::Ident = label.into();
                    let lifetime = syn::Lifetime {
                        apostrophe: Span::call_site(),
                        ident: label_ident,
                    };
                    vec![syn::Stmt::Expr(
                        syn::Expr::Continue(syn::ExprContinue {
                            attrs: vec![],
                            continue_token: <Token![continue]>::default(),
                            label: Some(lifetime),
                        }),
                        Some(<Token![;]>::default()),
                    )]
                } else {
                    vec![syn::parse_quote! { continue; }]
                }
            }
            GOTO => {
                if let Some(label) = branch_stmt.label {
                    if let Some(stmts) = compile_goto_state_jump(label.name) {
                        return stmts;
                    }
                    if is_goto_continue_label(label.name) {
                        let label_ident: syn::Ident = label.into();
                        let lifetime = syn::Lifetime {
                            apostrophe: Span::call_site(),
                            ident: label_ident,
                        };
                        return vec![syn::Stmt::Expr(
                            syn::Expr::Continue(syn::ExprContinue {
                                attrs: vec![],
                                continue_token: <Token![continue]>::default(),
                                label: Some(lifetime),
                            }),
                            Some(<Token![;]>::default()),
                        )];
                    }
                }
                vec![]
            }
            // Rust doesn't have fallthrough - switch is match which doesn't fall through
            FALLTHROUGH => vec![],
            _ => vec![],
        }
    }
}

impl TryFrom<ast::LabeledStmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(labeled_stmt: ast::LabeledStmt) -> Result<Self, Self::Error> {
        // Convert to Rust labeled block/loop
        let label_ident: syn::Ident = labeled_stmt.label.into();
        let label_name = label_ident.to_string();
        let stmt = *labeled_stmt.stmt;
        if let ast::Stmt::ForStmt(for_stmt) = stmt {
            return Ok(vec![syn::Stmt::Expr(
                compile_for_stmt(for_stmt, Some(label_ident))?,
                None,
            )]);
        }

        if ir::ast_stmt_has_goto_to_label(&stmt, &label_name) {
            let _goto_continue = GotoContinueLabelsGuard::extend([label_name]);
            let mut inner_stmts: Vec<syn::Stmt> = stmt.try_into()?;
            inner_stmts.push(syn::parse_quote! { break; });
            return Ok(vec![syn::Stmt::Expr(
                syn::Expr::Loop(syn::ExprLoop {
                    attrs: vec![],
                    label: Some(syn::Label {
                        name: syn::Lifetime {
                            apostrophe: Span::call_site(),
                            ident: label_ident,
                        },
                        colon_token: <Token![:]>::default(),
                    }),
                    loop_token: <Token![loop]>::default(),
                    body: syn::Block {
                        brace_token: syn::token::Brace::default(),
                        stmts: inner_stmts,
                    },
                }),
                None,
            )]);
        }

        let inner_stmts: Vec<syn::Stmt> = stmt.try_into()?;

        Ok(vec![syn::Stmt::Expr(
            syn::Expr::Block(syn::ExprBlock {
                attrs: vec![],
                label: Some(syn::Label {
                    name: syn::Lifetime {
                        apostrophe: Span::call_site(),
                        ident: label_ident,
                    },
                    colon_token: <Token![:]>::default(),
                }),
                block: syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: inner_stmts,
                },
            }),
            Some(<Token![;]>::default()),
        )])
    }
}

impl TryFrom<ast::ForStmt<'_>> for syn::Expr {
    type Error = CompilerError;

    fn try_from(for_stmt: ast::ForStmt) -> Result<Self, Self::Error> {
        compile_for_stmt(for_stmt, None)
    }
}

fn compile_for_stmt(
    for_stmt: ast::ForStmt,
    label_ident: Option<syn::Ident>,
) -> Result<syn::Expr, CompilerError> {
    let mut stmts = vec![];
    let per_iteration_capture_names =
        TYPE_ENV.with(|env| ir::for_clause_per_iteration_capture_names(&for_stmt, &env.borrow()));

    if let Some(init) = for_stmt.init {
        stmts.extend(Vec::<syn::Stmt>::try_from(*init)?);
    }

    let mut body: syn::Block = for_stmt.body.try_into()?;
    let loop_label_name = label_ident.as_ref().map(ToString::to_string);

    let per_iteration_stmts = for_clause_per_iteration_capture_stmts(&per_iteration_capture_names);

    if let Some(post) = for_stmt.post {
        let post_stmts = Vec::<syn::Stmt>::try_from(*post)?;

        if has_continue_for_post(&body.stmts, loop_label_name.as_deref(), true) {
            // Go runs the post statement before the next iteration, including
            // `continue label` targeting this loop. Rust `continue` jumps
            // straight to the condition, so route matching continues through a
            // body block and then emit per-iteration rebinding and post
            // statements after that block.
            let body_label = next_loop_body_label();
            rewrite_continue_for_post(
                &mut body.stmts,
                loop_label_name.as_deref(),
                true,
                &body_label,
            );

            let labeled_body = syn::Stmt::Expr(
                syn::Expr::Block(syn::ExprBlock {
                    attrs: vec![],
                    label: Some(syn::Label {
                        name: body_label,
                        colon_token: <Token![:]>::default(),
                    }),
                    block: body,
                }),
                Some(<Token![;]>::default()),
            );

            let mut loop_stmts = vec![labeled_body];
            loop_stmts.extend(per_iteration_stmts);
            loop_stmts.extend(post_stmts);

            body = syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: loop_stmts,
            };
        } else {
            body.stmts.extend(per_iteration_stmts);
            body.stmts.extend(post_stmts);
        }
    } else if !per_iteration_stmts.is_empty() {
        if has_continue_for_post(&body.stmts, loop_label_name.as_deref(), true) {
            let body_label = next_loop_body_label();
            rewrite_continue_for_post(
                &mut body.stmts,
                loop_label_name.as_deref(),
                true,
                &body_label,
            );

            let labeled_body = syn::Stmt::Expr(
                syn::Expr::Block(syn::ExprBlock {
                    attrs: vec![],
                    label: Some(syn::Label {
                        name: body_label,
                        colon_token: <Token![:]>::default(),
                    }),
                    block: body,
                }),
                Some(<Token![;]>::default()),
            );

            let mut loop_stmts = vec![labeled_body];
            loop_stmts.extend(per_iteration_stmts);

            body = syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: loop_stmts,
            };
        } else {
            body.stmts.extend(per_iteration_stmts);
        }
    }

    let loop_label = label_ident.map(|ident| syn::Label {
        name: syn::Lifetime {
            apostrophe: Span::call_site(),
            ident,
        },
        colon_token: <Token![:]>::default(),
    });

    stmts.push(syn::Stmt::Expr(
        if let Some(cond) = for_stmt.cond {
            syn::Expr::While(syn::ExprWhile {
                attrs: vec![],
                label: loop_label,
                cond: Box::new(cond.into()),
                body,
                while_token: <Token![while]>::default(),
            })
        } else {
            syn::Expr::Loop(syn::ExprLoop {
                attrs: vec![],
                label: loop_label,
                body,
                loop_token: <Token![loop]>::default(),
            })
        },
        None,
    ));

    Ok(syn::Expr::Block(syn::ExprBlock {
        attrs: vec![],
        label: None,
        block: syn::Block {
            stmts,
            brace_token: syn::token::Brace::default(),
        },
    }))
}

fn for_clause_per_iteration_capture_stmts(
    names: &std::collections::BTreeSet<String>,
) -> Vec<syn::Stmt> {
    names
        .iter()
        .filter_map(|name| {
            if !is_shared_capture_name(name) {
                return None;
            }
            let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
            let value = shared_capture_read_expr(name)?;
            Some(syn::parse_quote! {
                #ident = std::sync::Arc::new(std::sync::Mutex::new(#value));
            })
        })
        .collect()
}

impl TryFrom<ast::SwitchStmt<'_>> for syn::Expr {
    type Error = CompilerError;

    fn try_from(switch_stmt: ast::SwitchStmt) -> Result<Self, Self::Error> {
        let clauses: Vec<ast::CaseClause> = switch_stmt
            .body
            .list
            .into_iter()
            .filter_map(|s| {
                if let ast::Stmt::CaseClause(cc) = s {
                    Some(cc)
                } else {
                    None
                }
            })
            .collect();

        if clauses.is_empty() {
            return Err(CompilerError::UnsupportedConstruct(
                "empty switch statement".to_string(),
            ));
        }

        let switch_label = next_switch_label();
        if !clauses.iter().any(case_clause_contains_fallthrough) {
            return compile_non_fallthrough_switch(clauses, switch_stmt.tag, switch_label);
        }

        let fallthrough_ident = syn::Ident::new("__gors_switch_fallthrough", Span::mixed_site());
        let selected_ident = syn::Ident::new("__gors_switch_selected", Span::mixed_site());
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            let mut #selected_ident: isize = -1;
        }];

        let tag_ident = syn::Ident::new("__gors_switch_tag", Span::mixed_site());
        let tag_syn: Option<syn::Expr> = if let Some(tag) = switch_stmt.tag {
            let tag_expr: syn::Expr = tag.into();
            stmts.push(syn::parse_quote! { let #tag_ident = #tag_expr; });
            Some(syn::parse_quote! { #tag_ident })
        } else {
            None
        };

        let mut lowered_cases: Vec<(usize, Option<syn::Expr>, Vec<ast::Stmt>)> = vec![];
        let mut default_index: Option<usize> = None;
        for (index, case) in clauses.into_iter().enumerate() {
            let cond = if case.list.is_none() {
                default_index = Some(index);
                None
            } else {
                Some(build_case_condition(case.list, tag_syn.as_ref())?)
            };
            if let Some(cond) = &cond {
                let case_index = syn::LitInt::new(&index.to_string(), Span::mixed_site());
                stmts.push(syn::parse_quote! {
                    if #selected_ident == -1 && (#cond) {
                        #selected_ident = #case_index;
                    };
                });
            }
            lowered_cases.push((index, cond, case.body));
        }

        if let Some(default_index) = default_index {
            let default_index = syn::LitInt::new(&default_index.to_string(), Span::mixed_site());
            stmts.push(syn::parse_quote! {
                if #selected_ident == -1 {
                    #selected_ident = #default_index;
                };
            });
        }
        stmts.push(syn::parse_quote! { let mut #fallthrough_ident = false; });

        for (index, _cond, body) in lowered_cases {
            let case_index = syn::LitInt::new(&index.to_string(), Span::mixed_site());
            let body_stmts = compile_switch_case_body(body, &switch_label, &fallthrough_ident)?;
            stmts.push(syn::parse_quote! {
                if #selected_ident == #case_index || #fallthrough_ident {
                    #(#body_stmts)*
                };
            });
        }

        Ok(syn::Expr::Block(syn::ExprBlock {
            attrs: vec![],
            label: Some(syn::Label {
                name: switch_label,
                colon_token: <Token![:]>::default(),
            }),
            block: syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts,
            },
        }))
    }
}

fn compile_non_fallthrough_switch(
    clauses: Vec<ast::CaseClause>,
    tag: Option<ast::Expr>,
    switch_label: syn::Lifetime,
) -> Result<syn::Expr, CompilerError> {
    let mut prefix_stmts = vec![];
    let tag_ident = syn::Ident::new("__gors_switch_tag", Span::mixed_site());
    let tag_syn: Option<syn::Expr> = if let Some(tag) = tag {
        let tag_expr: syn::Expr = tag.into();
        prefix_stmts.push(syn::parse_quote! { let #tag_ident = #tag_expr; });
        Some(syn::parse_quote! { #tag_ident })
    } else {
        None
    };

    let mut cases = vec![];
    let mut default_body = None;
    for case in clauses {
        if case.list.is_none() {
            default_body = Some(case.body);
        } else {
            cases.push(case);
        }
    }

    let mut result: Option<syn::Expr> = default_body
        .map(|body| switch_case_expr_block(body, &switch_label))
        .transpose()?;
    for case in cases.into_iter().rev() {
        let cond = build_case_condition(case.list, tag_syn.as_ref())?;
        let then_stmts = compile_breakable_stmt_list(case.body, &switch_label)?;
        result = Some(syn::Expr::If(syn::ExprIf {
            attrs: vec![],
            if_token: <Token![if]>::default(),
            cond: Box::new(cond),
            then_branch: syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: then_stmts,
            },
            else_branch: result.map(|expr| (<Token![else]>::default(), Box::new(expr))),
        }));
    }

    if let Some(result) = result {
        prefix_stmts.push(syn::Stmt::Expr(result, None));
    }

    Ok(syn::Expr::Block(syn::ExprBlock {
        attrs: vec![],
        label: Some(syn::Label {
            name: switch_label,
            colon_token: <Token![:]>::default(),
        }),
        block: syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: prefix_stmts,
        },
    }))
}

fn switch_case_expr_block(
    body: Vec<ast::Stmt>,
    switch_label: &syn::Lifetime,
) -> Result<syn::Expr, CompilerError> {
    let stmts = compile_breakable_stmt_list(body, switch_label)?;
    Ok(syn::Expr::Block(syn::ExprBlock {
        attrs: vec![],
        label: None,
        block: syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts,
        },
    }))
}

fn case_clause_contains_fallthrough(case: &ast::CaseClause<'_>) -> bool {
    stmts_contain_fallthrough(&case.body)
}

fn stmts_contain_fallthrough(stmts: &[ast::Stmt<'_>]) -> bool {
    stmts.iter().any(stmt_contains_fallthrough)
}

fn stmt_contains_fallthrough(stmt: &ast::Stmt<'_>) -> bool {
    match stmt {
        ast::Stmt::BranchStmt(branch) => branch.tok == token::Token::FALLTHROUGH,
        ast::Stmt::BlockStmt(block) => stmts_contain_fallthrough(&block.list),
        ast::Stmt::IfStmt(if_stmt) => {
            stmts_contain_fallthrough(&if_stmt.body.list)
                || if_stmt
                    .else_
                    .as_ref()
                    .as_ref()
                    .is_some_and(stmt_contains_fallthrough)
        }
        _ => false,
    }
}

fn compile_switch_case_body(
    body: Vec<ast::Stmt>,
    switch_label: &syn::Lifetime,
    fallthrough_ident: &syn::Ident,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let mut stmts = vec![syn::parse_quote! { #fallthrough_ident = false; }];
    stmts.extend(compile_switch_case_stmt_list(
        body,
        switch_label,
        fallthrough_ident,
    )?);
    Ok(stmts)
}

fn compile_switch_case_stmt_list(
    body: Vec<ast::Stmt>,
    switch_label: &syn::Lifetime,
    fallthrough_ident: &syn::Ident,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let mut stmts = vec![];
    for stmt in body {
        let (compiled, stop) = compile_switch_case_stmt(stmt, switch_label, fallthrough_ident)?;
        stmts.extend(compiled);
        if stop {
            break;
        }
    }
    Ok(stmts)
}

fn compile_switch_case_stmt(
    stmt: ast::Stmt,
    switch_label: &syn::Lifetime,
    fallthrough_ident: &syn::Ident,
) -> Result<(Vec<syn::Stmt>, bool), CompilerError> {
    match stmt {
        ast::Stmt::BranchStmt(branch)
            if branch.tok == token::Token::BREAK && branch.label.is_none() =>
        {
            Ok((
                vec![syn::Stmt::Expr(
                    syn::Expr::Break(syn::ExprBreak {
                        attrs: vec![],
                        break_token: <Token![break]>::default(),
                        label: Some(switch_label.clone()),
                        expr: None,
                    }),
                    Some(<Token![;]>::default()),
                )],
                true,
            ))
        }
        ast::Stmt::BranchStmt(branch) if branch.tok == token::Token::FALLTHROUGH => {
            Ok((vec![syn::parse_quote! { #fallthrough_ident = true; }], true))
        }
        ast::Stmt::BlockStmt(block) => {
            let stmts = compile_switch_case_stmt_list(block.list, switch_label, fallthrough_ident)?;
            Ok((
                vec![syn::Stmt::Expr(
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts,
                        },
                    }),
                    None,
                )],
                false,
            ))
        }
        ast::Stmt::IfStmt(if_stmt) => Ok((
            compile_switch_case_if_stmt(if_stmt, switch_label, fallthrough_ident)?,
            false,
        )),
        other => Ok((Vec::<syn::Stmt>::try_from(other)?, false)),
    }
}

fn compile_switch_case_if_stmt(
    if_stmt: ast::IfStmt,
    switch_label: &syn::Lifetime,
    fallthrough_ident: &syn::Ident,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let has_init = if_stmt.init.is_some();
    let init_stmts: Vec<syn::Stmt> = if let Some(init) = *if_stmt.init {
        Vec::<syn::Stmt>::try_from(init)?
    } else {
        vec![]
    };

    let then_stmts =
        compile_switch_case_stmt_list(if_stmt.body.list, switch_label, fallthrough_ident)?;
    let else_branch = if let Some(else_) = *if_stmt.else_ {
        Some((
            <Token![else]>::default(),
            Box::new(match else_ {
                ast::Stmt::IfStmt(nested) => {
                    let nested_stmts =
                        compile_switch_case_if_stmt(nested, switch_label, fallthrough_ident)?;
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts: nested_stmts,
                        },
                    })
                }
                ast::Stmt::BlockStmt(block) => {
                    let stmts =
                        compile_switch_case_stmt_list(block.list, switch_label, fallthrough_ident)?;
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts,
                        },
                    })
                }
                _ => {
                    return Err(CompilerError::UnsupportedConstruct(
                        "unsupported else branch type".to_string(),
                    ));
                }
            }),
        ))
    } else {
        None
    };

    let if_expr = syn::Expr::If(syn::ExprIf {
        attrs: vec![],
        if_token: <Token![if]>::default(),
        cond: Box::new(if_stmt.cond.into()),
        then_branch: syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: then_stmts,
        },
        else_branch,
    });

    if has_init {
        let mut block_stmts = init_stmts;
        block_stmts.push(syn::Stmt::Expr(if_expr, None));
        Ok(vec![syn::Stmt::Expr(
            syn::Expr::Block(syn::ExprBlock {
                attrs: vec![],
                label: None,
                block: syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: block_stmts,
                },
            }),
            None,
        )])
    } else {
        Ok(vec![syn::Stmt::Expr(if_expr, None)])
    }
}

fn next_switch_label() -> syn::Lifetime {
    let n = SWITCH_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    });
    syn::Lifetime::new(&format!("'__gors_switch_{n}"), Span::mixed_site())
}

fn next_select_label() -> syn::Lifetime {
    let n = SELECT_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    });
    syn::Lifetime::new(&format!("'__gors_select_{n}"), Span::mixed_site())
}

fn next_loop_body_label() -> syn::Lifetime {
    let n = LOOP_BODY_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    });
    syn::Lifetime::new(&format!("'__gors_loop_body_{n}"), Span::mixed_site())
}

fn next_named_return_label() -> syn::Lifetime {
    let n = next_named_return_index();
    syn::Lifetime::new(&format!("'__gors_named_return_{n}"), Span::mixed_site())
}

fn next_named_return_temp_idents(count: usize) -> Vec<syn::Ident> {
    let n = next_named_return_index();
    (0..count)
        .map(|idx| {
            syn::Ident::new(
                &format!("__gors_named_return_{n}_{idx}"),
                Span::mixed_site(),
            )
        })
        .collect()
}

fn next_named_return_index() -> usize {
    NAMED_RETURN_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    })
}

fn next_goto_state_names() -> (syn::Ident, syn::Lifetime) {
    let n = GOTO_STATE_COUNTER.with(|c| {
        let mut val = c.borrow_mut();
        let n = *val;
        *val += 1;
        n
    });
    (
        syn::Ident::new(&format!("__gors_goto_state_{n}"), Span::mixed_site()),
        syn::Lifetime::new(&format!("'__gors_goto_{n}"), Span::mixed_site()),
    )
}

fn build_case_condition(
    list: Option<Vec<ast::Expr>>,
    tag: Option<&syn::Expr>,
) -> Result<syn::Expr, CompilerError> {
    let exprs = list.unwrap_or_default();

    if let Some(tag) = tag {
        let mut conditions: Vec<syn::Expr> = exprs
            .into_iter()
            .map(|e| {
                let tag_expr = tag.clone();
                let val_expr: syn::Expr = e.into();
                binary_expr(tag_expr, syn::BinOp::Eq(<Token![==]>::default()), val_expr)
            })
            .collect();

        Ok(if conditions.len() == 1 {
            conditions.remove(0)
        } else {
            conditions
                .into_iter()
                .reduce(|acc, e| binary_expr(acc, syn::BinOp::Or(<Token![||]>::default()), e))
                .unwrap_or_else(true_expr)
        })
    } else {
        // Tagless switch: `switch { case cond: ... }` → conditions are already booleans
        Ok(if exprs.len() == 1 {
            let Some(expr) = exprs.into_iter().next() else {
                return Err(CompilerError::UnsupportedConstruct(
                    "empty switch case condition".to_string(),
                ));
            };
            expr.into()
        } else {
            exprs
                .into_iter()
                .map(Into::into)
                .reduce(|acc, e| binary_expr(acc, syn::BinOp::Or(<Token![||]>::default()), e))
                .unwrap_or_else(true_expr)
        })
    }
}

fn binary_expr(left: syn::Expr, op: syn::BinOp, right: syn::Expr) -> syn::Expr {
    syn::Expr::Binary(syn::ExprBinary {
        attrs: vec![],
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

fn true_expr() -> syn::Expr {
    syn::Expr::Lit(syn::ExprLit {
        attrs: vec![],
        lit: syn::Lit::Bool(syn::LitBool::new(true, Span::mixed_site())),
    })
}

fn has_continue_for_post(
    stmts: &[syn::Stmt],
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    stmts.iter().any(|stmt| match stmt {
        syn::Stmt::Expr(syn::Expr::Continue(cont), _) => {
            continue_targets_current_loop(cont, loop_label, allow_unlabeled)
        }
        syn::Stmt::Expr(expr, _) => {
            has_continue_for_post_in_expr(expr, loop_label, allow_unlabeled)
        }
        _ => false,
    })
}

fn has_continue_for_post_in_expr(
    expr: &syn::Expr,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    match expr {
        syn::Expr::If(if_expr) => {
            has_continue_for_post(&if_expr.then_branch.stmts, loop_label, allow_unlabeled)
                || if_expr.else_branch.as_ref().is_some_and(|(_, e)| {
                    has_continue_for_post_in_expr(e, loop_label, allow_unlabeled)
                })
        }
        syn::Expr::Block(block) => {
            has_continue_for_post(&block.block.stmts, loop_label, allow_unlabeled)
        }
        syn::Expr::While(while_expr) => has_continue_for_post_in_nested_loop(
            while_expr.label.as_ref(),
            &while_expr.body.stmts,
            loop_label,
        ),
        syn::Expr::Loop(loop_expr) => has_continue_for_post_in_nested_loop(
            loop_expr.label.as_ref(),
            &loop_expr.body.stmts,
            loop_label,
        ),
        syn::Expr::ForLoop(for_loop) => has_continue_for_post_in_nested_loop(
            for_loop.label.as_ref(),
            &for_loop.body.stmts,
            loop_label,
        ),
        _ => false,
    }
}

fn has_continue_for_post_in_nested_loop(
    nested_label: Option<&syn::Label>,
    stmts: &[syn::Stmt],
    loop_label: Option<&str>,
) -> bool {
    let Some(loop_label) = loop_label else {
        return false;
    };
    if nested_label.is_some_and(|label| label.name.ident == loop_label) {
        return false;
    }
    has_continue_for_post(stmts, Some(loop_label), false)
}

fn rewrite_continue_for_post(
    stmts: &mut [syn::Stmt],
    loop_label: Option<&str>,
    allow_unlabeled: bool,
    body_label: &syn::Lifetime,
) {
    for stmt in stmts.iter_mut() {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Continue(cont), semi)
                if continue_targets_current_loop(cont, loop_label, allow_unlabeled) =>
            {
                *stmt = syn::Stmt::Expr(
                    syn::Expr::Break(syn::ExprBreak {
                        attrs: vec![],
                        break_token: <Token![break]>::default(),
                        label: Some(body_label.clone()),
                        expr: None,
                    }),
                    *semi,
                );
            }
            syn::Stmt::Expr(expr, _) => {
                rewrite_continue_for_post_in_expr(expr, loop_label, allow_unlabeled, body_label);
            }
            _ => {}
        }
    }
}

fn rewrite_continue_for_post_in_expr(
    expr: &mut syn::Expr,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
    body_label: &syn::Lifetime,
) {
    match expr {
        syn::Expr::If(if_expr) => {
            rewrite_continue_for_post(
                &mut if_expr.then_branch.stmts,
                loop_label,
                allow_unlabeled,
                body_label,
            );
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_continue_for_post_in_expr(
                    else_expr,
                    loop_label,
                    allow_unlabeled,
                    body_label,
                );
            }
        }
        syn::Expr::Block(block) => {
            rewrite_continue_for_post(
                &mut block.block.stmts,
                loop_label,
                allow_unlabeled,
                body_label,
            );
        }
        syn::Expr::While(while_expr) => {
            rewrite_continue_for_post_in_nested_loop(
                while_expr.label.as_ref(),
                &mut while_expr.body.stmts,
                loop_label,
                body_label,
            );
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_continue_for_post_in_nested_loop(
                loop_expr.label.as_ref(),
                &mut loop_expr.body.stmts,
                loop_label,
                body_label,
            );
        }
        syn::Expr::ForLoop(for_loop) => {
            rewrite_continue_for_post_in_nested_loop(
                for_loop.label.as_ref(),
                &mut for_loop.body.stmts,
                loop_label,
                body_label,
            );
        }
        _ => {}
    }
}

fn rewrite_continue_for_post_in_nested_loop(
    nested_label: Option<&syn::Label>,
    stmts: &mut [syn::Stmt],
    loop_label: Option<&str>,
    body_label: &syn::Lifetime,
) {
    let Some(loop_label) = loop_label else {
        return;
    };
    if nested_label.is_some_and(|label| label.name.ident == loop_label) {
        return;
    }
    rewrite_continue_for_post(stmts, Some(loop_label), false, body_label);
}

fn continue_targets_current_loop(
    cont: &syn::ExprContinue,
    loop_label: Option<&str>,
    allow_unlabeled: bool,
) -> bool {
    if allow_unlabeled && cont.label.is_none() {
        return true;
    }
    loop_label.is_some_and(|label| cont.label.as_ref().is_some_and(|cont| cont.ident == label))
}

fn inferred_function_type_for_name(name: &str) -> Option<typeinfer::GoType> {
    TYPE_ENV.with(|env| {
        env.borrow()
            .get_var(name)
            .filter(|ty| matches!(resolved_go_type(ty), typeinfer::GoType::Func { .. }))
    })
}

impl From<ast::DeclStmt<'_>> for Vec<syn::Stmt> {
    fn from(decl_stmt: ast::DeclStmt) -> Self {
        let gen_decl = decl_stmt.decl;
        let mut stmts = vec![];

        for spec in gen_decl.specs {
            match spec {
                ast::Spec::ValueSpec(value_spec) => {
                    let names = value_spec.names;
                    let type_expr = value_spec.type_;
                    let go_type = type_expr.as_ref().map(typeinfer::GoType::from_expr);
                    let rust_type: Option<syn::Type> =
                        type_expr.as_ref().map(local_value_type_from_expr);
                    let mut values_iter = value_spec.values.unwrap_or_default().into_iter();

                    for name in names {
                        let ident: syn::Ident = name.into();
                        let init_ast = values_iter.next();
                        let binding_go_type = if let Some(ref go_type) = go_type {
                            TYPE_ENV.with(|env| {
                                env.borrow_mut()
                                    .set_var(&ident.to_string(), go_type.clone());
                            });
                            Some(go_type.clone())
                        } else if let Some(ref init_ast) = init_ast {
                            let inferred = TYPE_ENV
                                .with(|env| typeinfer::GoType::infer_expr(init_ast, &env.borrow()));
                            TYPE_ENV.with(|env| {
                                env.borrow_mut()
                                    .set_var(&ident.to_string(), inferred.clone());
                            });
                            Some(inferred)
                        } else {
                            None
                        };
                        let init_expr: Option<syn::Expr> = init_ast.map(|expr| {
                            let should_clone = binding_init_should_clone(&expr);
                            let expected = go_type.as_ref().or_else(|| {
                                binding_go_type.as_ref().filter(|ty| {
                                    matches!(resolved_go_type(ty), typeinfer::GoType::Func { .. })
                                })
                            });
                            if let Some(expected) = expected {
                                let init = compile_expr_with_expected(expr, Some(expected));
                                maybe_clone_binding_init(should_clone, init)
                            } else {
                                let init = expr.into();
                                maybe_clone_binding_init(should_clone, init)
                            }
                        });

                        let init = init_expr.unwrap_or_else(|| {
                            type_expr
                                .as_ref()
                                .map(default_expr_for_type)
                                .unwrap_or_else(|| go_zero_value_from_type(rust_type.as_ref()))
                        });
                        let name = ident.to_string();
                        let init = shared_capture_init_expr(&name, init);
                        if let Some(ref ty) = rust_type {
                            let ty = shared_capture_type(&name, ty.clone());
                            stmts.push(syn::parse_quote! {
                                let mut #ident: #ty = #init;
                            });
                        } else {
                            stmts.push(syn::parse_quote! {
                                let mut #ident = #init;
                            });
                        }
                    }
                }
                ast::Spec::TypeSpec(type_spec) => {
                    register_type_spec_in_env(&type_spec);
                    if let Ok(items) = compile_type_spec(type_spec) {
                        stmts.extend(items.into_iter().map(syn::Stmt::Item));
                    }
                }
                ast::Spec::ImportSpec(_) => {}
            }
        }
        stmts
    }
}

fn go_zero_value(type_expr: Option<&ast::Expr>) -> syn::Expr {
    match type_expr {
        Some(ast::Expr::Ident(ident)) => match ident.name {
            "bool" => syn::parse_quote! { false },
            "string" => syn::parse_quote! { String::new() },
            "float32" | "float64" => syn::parse_quote! { 0.0 },
            "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16"
            | "uint32" | "uint64" | "uintptr" | "byte" | "rune" => syn::parse_quote! { 0 },
            "any" => syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> },
            _ => syn::parse_quote! { Default::default() },
        },
        Some(ast::Expr::InterfaceType(_)) => {
            syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
        }
        Some(ast::Expr::ArrayType(array_type)) => default_expr_for_array_type(array_type),
        Some(ast::Expr::MapType(_)) => syn::parse_quote! { Default::default() },
        Some(ast::Expr::StarExpr(_)) => syn::parse_quote! { Default::default() },
        Some(_) => syn::parse_quote! { Default::default() },
        None => syn::parse_quote! { 0 },
    }
}

fn is_any_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    let first = match type_path.path.segments.first() {
        Some(s) => s,
        None => return false,
    };
    if first.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &first.arguments else {
        return false;
    };
    args.args.iter().any(|a| matches!(a, syn::GenericArgument::Type(syn::Type::TraitObject(to)) if to.bounds.iter().any(|b| {
        if let syn::TypeParamBound::Trait(t) = b {
            t.path.segments.last().is_some_and(|s| s.ident == "Any")
        } else {
            false
        }
    })))
}

fn is_box_dyn_any_expr(expr: &syn::Expr) -> bool {
    expr.to_token_stream().to_string() == "Box :: new (()) as Box < dyn std :: any :: Any >"
}

fn is_box_dyn_any_cast_expr(expr: &syn::Expr) -> bool {
    let syn::Expr::Cast(cast) = expr else {
        return false;
    };
    is_any_type(&cast.ty)
}

fn go_zero_value_from_type(ty: Option<&syn::Type>) -> syn::Expr {
    if let Some(ty) = ty {
        if is_any_type(ty) {
            return syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> };
        }
    }
    if let Some(syn::Type::Array(type_array)) = ty {
        let len = &type_array.len;
        return syn::parse_quote! { [Default::default(); #len] };
    }
    if let Some(syn::Type::Path(type_path)) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            let name = seg.ident.to_string();
            match name.as_str() {
                "bool" => return syn::parse_quote! { false },
                "string" | "String" => return syn::parse_quote! { String::new() },
                "float32" | "float64" | "f32" | "f64" => return syn::parse_quote! { 0.0 },
                "isize" | "i8" | "i16" | "i32" | "i64" | "usize" | "u8" | "u16" | "u32" | "u64" => {
                    return syn::parse_quote! { 0 };
                }
                "Vec" => return syn::parse_quote! { Vec::new() },
                "HashMap" => return syn::parse_quote! { std::collections::HashMap::new() },
                _ => {}
            }
        }
    }
    if ty.is_some() {
        syn::parse_quote! { Default::default() }
    } else {
        syn::parse_quote! { 0 }
    }
}

enum CommaOkKind {
    MapIndex,
    ChanRecv,
    TypeAssert,
}

fn detect_comma_ok(rhs: &ast::Expr) -> Option<CommaOkKind> {
    match rhs {
        ast::Expr::IndexExpr(_) => Some(CommaOkKind::MapIndex),
        ast::Expr::UnaryExpr(u) if u.op == token::Token::ARROW => Some(CommaOkKind::ChanRecv),
        ast::Expr::TypeAssertExpr(ta) if ta.type_.is_some() => Some(CommaOkKind::TypeAssert),
        _ => None,
    }
}

fn type_assert_any_option_expr(source: syn::Expr, source_type: &typeinfer::GoType) -> syn::Expr {
    if go_type_is_interface_like(source_type)
        && !matches!(resolved_go_type(source_type), typeinfer::GoType::Any)
    {
        syn::parse_quote! { (#source).__gors_as_any() }
    } else {
        syn::parse_quote! { Some((#source).as_ref() as &dyn std::any::Any) }
    }
}

fn type_assert_with_any_option(
    source: syn::Expr,
    source_type: &typeinfer::GoType,
    source_is_borrowable: bool,
    body: syn::Expr,
) -> syn::Expr {
    if go_type_is_interface_like(source_type)
        && !matches!(resolved_go_type(source_type), typeinfer::GoType::Any)
        || source_is_borrowable
    {
        let any_option = type_assert_any_option_expr(source, source_type);
        syn::parse_quote! {{
            let __gors_any_option = #any_option;
            #body
        }}
    } else {
        syn::parse_quote! {{
            let __gors_any_source = #source;
            let __gors_any_option = Some(__gors_any_source.as_ref() as &dyn std::any::Any);
            #body
        }}
    }
}

fn type_assert_concrete_expr(
    source: syn::Expr,
    source_type: &typeinfer::GoType,
    source_is_borrowable: bool,
    asserted_type: syn::Type,
) -> syn::Expr {
    let body = syn::parse_quote! {
        match __gors_any_option.and_then(|__gors_any| __gors_any.downcast_ref::<#asserted_type>()) {
            Some(__v) => __v.clone(),
            None => panic!("type assertion failed"),
        }
    };
    type_assert_with_any_option(source, source_type, source_is_borrowable, body)
}

fn comma_ok_type_assert_concrete_expr(
    source: syn::Expr,
    source_type: &typeinfer::GoType,
    source_is_borrowable: bool,
    asserted_type: syn::Type,
) -> syn::Expr {
    let body = syn::parse_quote! {
        match __gors_any_option.and_then(|__gors_any| __gors_any.downcast_ref::<#asserted_type>()) {
            Some(__v) => (__v.clone(), true),
            None => (Default::default(), false),
        }
    };
    type_assert_with_any_option(source, source_type, source_is_borrowable, body)
}

fn interface_assertion_implementors(interface_name: &str) -> Vec<syn::Type> {
    TYPE_ENV.with(|env| {
        env.borrow()
            .interface_implementors(interface_name)
            .into_iter()
            .map(|name| named_go_type_path(&name))
            .collect()
    })
}

fn interface_assertion_fallback(
    trait_path: &syn::Path,
    interface_name: &str,
    comma_ok: bool,
) -> syn::Expr {
    let noop_ty = noop_interface_type_from_name(interface_name);
    let value: syn::Expr =
        syn::parse_quote! { Box::new(#noop_ty::default()) as Box<dyn #trait_path> };
    if comma_ok {
        syn::parse_quote! { (#value, false) }
    } else {
        value
    }
}

fn noop_interface_type_from_name(name: &str) -> syn::Type {
    let mut parts = name.split('.').collect::<Vec<_>>();
    let Some(last) = parts.pop() else {
        return syn::parse_quote! { __GorsNoopInterface };
    };
    let noop_name = format!("__GorsNoop{}", rust_safe_ident_name(last));
    parts.push(Box::leak(noop_name.into_boxed_str()));
    rust_type_path_from_segments(parts, true)
}

fn type_assert_interface_expr(
    source: syn::Expr,
    source_type: &typeinfer::GoType,
    source_is_borrowable: bool,
    interface_name: &str,
    comma_ok: bool,
) -> syn::Expr {
    if interface_name == "error" {
        return if comma_ok {
            syn::parse_quote! { (String::new(), false) }
        } else {
            syn::parse_quote! { String::new() }
        };
    }
    let trait_path = interface_trait_path_from_name(interface_name);
    let implementors = interface_assertion_implementors(interface_name);
    let fallback = interface_assertion_fallback(&trait_path, interface_name, comma_ok);
    if implementors.is_empty() {
        return fallback;
    }
    let mut result = fallback.clone();
    for implementor in implementors.iter().rev() {
        result = if comma_ok {
            syn::parse_quote! {
                if let Some(__gors_value) = __gors_any.downcast_ref::<#implementor>() {
                    (Box::new(__gors_value.clone()) as Box<dyn #trait_path>, true)
                } else {
                    #result
                }
            }
        } else {
            syn::parse_quote! {
                if let Some(__gors_value) = __gors_any.downcast_ref::<#implementor>() {
                    Box::new(__gors_value.clone()) as Box<dyn #trait_path>
                } else {
                    #result
                }
            }
        };
    }
    let body = syn::parse_quote! {
            if let Some(__gors_any) = __gors_any_option {
                #result
            } else {
                #fallback
            }
    };
    type_assert_with_any_option(source, source_type, source_is_borrowable, body)
}

fn compile_comma_ok(
    lhs: Vec<ast::Expr>,
    rhs: ast::Expr,
    kind: CommaOkKind,
    is_define: bool,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let lhs_idents: Vec<Option<syn::Ident>> = if is_define {
        lhs.iter()
            .map(|e| match e {
                ast::Expr::Ident(id) if id.name == "_" => Ok(None),
                ast::Expr::Ident(id) => Ok(Some(syn::Ident::new(
                    &rust_safe_ident_name(id.name),
                    Span::mixed_site(),
                ))),
                _ => Err(CompilerError::InvalidAssignment(
                    "expected identifier in comma-ok lhs".to_string(),
                )),
            })
            .collect::<Result<_, _>>()?
    } else {
        vec![None, None]
    };

    let val_pat: syn::Pat = match lhs_idents.first().unwrap_or(&None) {
        None => syn::parse_quote! { _ },
        Some(id) => syn::parse_quote! { mut #id },
    };
    let ok_pat: syn::Pat = match lhs_idents.get(1).unwrap_or(&None) {
        None => syn::parse_quote! { _ },
        Some(id) => syn::parse_quote! { mut #id },
    };

    let rhs_expr: syn::Expr = match kind {
        CommaOkKind::MapIndex => {
            if let ast::Expr::IndexExpr(ie) = rhs {
                let key_ty = TYPE_ENV.with(|env| {
                    let env = env.borrow();
                    match env.resolve_alias(&typeinfer::GoType::infer_expr(&ie.x, &env)) {
                        typeinfer::GoType::Map(key, _) => Some(*key),
                        _ => None,
                    }
                });
                let map_e: syn::Expr = (*ie.x).into();
                let key_e = compile_expr_with_expected(*ie.index, key_ty.as_ref());
                syn::parse_quote! {
                    {
                        let __gors_map_key = #key_e;
                        match (#map_e).get(&__gors_map_key) {
                            Some(__v) => (__v.clone(), true),
                            None => (Default::default(), false),
                        }
                    }
                }
            } else {
                return Err(CompilerError::InvalidAssignment(
                    "comma-ok map assignment without map index rhs".to_string(),
                ));
            }
        }
        CommaOkKind::ChanRecv => {
            if let ast::Expr::UnaryExpr(u) = rhs {
                let ch_e: syn::Expr = (*u.x).into();
                syn::parse_quote! { (#ch_e).recv_with_ok() }
            } else {
                return Err(CompilerError::InvalidAssignment(
                    "comma-ok channel assignment without receive rhs".to_string(),
                ));
            }
        }
        CommaOkKind::TypeAssert => {
            if let ast::Expr::TypeAssertExpr(ta) = rhs {
                let source_ast = *ta.x;
                let source_is_borrowable = is_ir_addressable_expr(&source_ast);
                let source_type =
                    TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&source_ast, &env.borrow()));
                let x_e: syn::Expr = source_ast.into();
                let Some(type_expr) = ta.type_ else {
                    return Err(CompilerError::InvalidAssignment(
                        "comma-ok type assertion without asserted type".to_string(),
                    ));
                };
                if let Some(interface_name) = interface_name_from_type_expr(&type_expr) {
                    type_assert_interface_expr(
                        x_e,
                        &source_type,
                        source_is_borrowable,
                        &interface_name,
                        true,
                    )
                } else {
                    let ty: syn::Type = (*type_expr).into();
                    comma_ok_type_assert_concrete_expr(x_e, &source_type, source_is_borrowable, ty)
                }
            } else {
                return Err(CompilerError::InvalidAssignment(
                    "comma-ok type assertion assignment without assertion rhs".to_string(),
                ));
            }
        }
    };

    if is_define {
        Ok(vec![syn::parse_quote! {
            let (#val_pat, #ok_pat) = #rhs_expr;
        }])
    } else {
        let lhs_exprs = lhs
            .into_iter()
            .map(comma_ok_lhs_expr)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(comma_ok_assignment_stmts(lhs_exprs, rhs_expr))
    }
}

fn comma_ok_lhs_expr(expr: ast::Expr) -> Result<Option<syn::Expr>, CompilerError> {
    if matches!(&expr, ast::Expr::Ident(id) if id.name == "_") {
        Ok(None)
    } else {
        compile_assignment_lhs_checked(expr).map(Some)
    }
}

fn comma_ok_assignment_stmts(lhs: Vec<Option<syn::Expr>>, rhs_expr: syn::Expr) -> Vec<syn::Stmt> {
    let val_tmp = syn::Ident::new("__gors_comma_ok_value", Span::mixed_site());
    let ok_tmp = syn::Ident::new("__gors_comma_ok_ok", Span::mixed_site());
    let mut stmts = vec![syn::parse_quote! {
        let (mut #val_tmp, mut #ok_tmp) = #rhs_expr;
    }];

    for (index, lhs_expr) in lhs.into_iter().enumerate() {
        let Some(lhs_expr) = lhs_expr else { continue };
        let src = if index == 0 {
            syn::Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: syn::Path::from(val_tmp.clone()),
            })
        } else {
            syn::Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: syn::Path::from(ok_tmp.clone()),
            })
        };
        stmts.push(syn::parse_quote! {
            #lhs_expr = #src;
        });
    }

    stmts
}

fn interface_name_from_type_expr(type_expr: &ast::Expr) -> Option<String> {
    match type_expr {
        ast::Expr::Ident(id) if id.name == "error" => Some(id.name.to_string()),
        ast::Expr::Ident(id) if is_type_interface(id.name) => Some(id.name.to_string()),
        ast::Expr::Ident(id) if TYPE_ENV.with(|env| env.borrow().is_interface(id.name)) => {
            Some(id.name.to_string())
        }
        _ => None,
    }
}

fn compile_index_swap_assignment(assign_stmt: &ast::AssignStmt) -> Option<Vec<syn::Stmt>> {
    if assign_stmt.lhs.len() != 2 || assign_stmt.rhs.len() != 2 {
        return None;
    }
    let (lhs_base_a, lhs_index_a) = index_expr_parts(assign_stmt.lhs.first()?)?;
    let (lhs_base_b, lhs_index_b) = index_expr_parts(assign_stmt.lhs.get(1)?)?;
    let (rhs_base_a, rhs_index_a) = index_expr_parts(assign_stmt.rhs.first()?)?;
    let (rhs_base_b, rhs_index_b) = index_expr_parts(assign_stmt.rhs.get(1)?)?;

    let lhs_base_key = expr_shape_key(lhs_base_a)?;
    if lhs_base_key != expr_shape_key(lhs_base_b)?
        || lhs_base_key != expr_shape_key(rhs_base_a)?
        || lhs_base_key != expr_shape_key(rhs_base_b)?
        || expr_shape_key(lhs_index_a)? != expr_shape_key(rhs_index_b)?
        || expr_shape_key(lhs_index_b)? != expr_shape_key(rhs_index_a)?
    {
        return None;
    }

    let base = syn_expr_from_type_expr_like(lhs_base_a)?;
    let left_index = syn_expr_from_type_expr_like(lhs_index_a)?;
    let right_index = syn_expr_from_type_expr_like(lhs_index_b)?;
    Some(vec![syn::parse_quote! {
        #base.swap((#left_index) as usize, (#right_index) as usize);
    }])
}

fn index_expr_parts<'a, 'src>(
    expr: &'a ast::Expr<'src>,
) -> Option<(&'a ast::Expr<'src>, &'a ast::Expr<'src>)> {
    let ast::Expr::IndexExpr(index) = expr else {
        return None;
    };
    Some((&index.x, &index.index))
}

fn map_index_types(expr: &ast::Expr) -> Option<(typeinfer::GoType, typeinfer::GoType)> {
    let ast::Expr::IndexExpr(index) = expr else {
        return None;
    };
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match env.resolve_alias(&typeinfer::GoType::infer_expr(&index.x, &env)) {
            typeinfer::GoType::Map(key, value) => Some((*key, *value)),
            _ => None,
        }
    })
}

fn compile_map_index_assignment(
    lhs: ast::Expr,
    rhs: ast::Expr,
    key_ty: typeinfer::GoType,
    value_ty: typeinfer::GoType,
) -> Result<syn::Stmt, CompilerError> {
    let ast::Expr::IndexExpr(index) = lhs else {
        return Err(CompilerError::InvalidAssignment(
            "map assignment without map index lhs".to_string(),
        ));
    };
    let rhs_ty = TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&rhs, &env.borrow()));
    let base = lvalue_expr_from_ref(&index.x).ok_or_else(|| {
        CompilerError::InvalidAssignment("map assignment lhs is not addressable".to_string())
    })?;
    let key = compile_expr_with_expected(*index.index, Some(&key_ty));
    let right_raw = compile_expr_with_expected(rhs, Some(&value_ty));
    let right = coerce_assignment_expr(&value_ty, &rhs_ty, right_raw);
    Ok(syn::parse_quote! { #base.insert(#key, #right); })
}

fn binary_expr_stmt(left: syn::Expr, op: syn::BinOp, right: syn::Expr) -> syn::Stmt {
    syn::Stmt::Expr(
        syn::Expr::Binary(syn::ExprBinary {
            attrs: vec![],
            left: Box::new(left),
            op,
            right: Box::new(right),
        }),
        Some(<Token![;]>::default()),
    )
}

fn assign_expr_stmt(left: syn::Expr, right: syn::Expr) -> syn::Stmt {
    syn::Stmt::Expr(
        syn::Expr::Assign(syn::ExprAssign {
            attrs: vec![],
            left: Box::new(left),
            eq_token: <Token![=]>::default(),
            right: Box::new(right),
        }),
        Some(<Token![;]>::default()),
    )
}

fn compile_map_index_inc_dec(
    inc_dec_stmt: ast::IncDecStmt,
    key_ty: typeinfer::GoType,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    let ast::Expr::IndexExpr(index) = inc_dec_stmt.x else {
        return Err(CompilerError::InvalidAssignment(
            "map increment/decrement without map index lhs".to_string(),
        ));
    };
    let base = lvalue_expr_from_ref(&index.x).ok_or_else(|| {
        CompilerError::InvalidAssignment(
            "map increment/decrement lhs is not addressable".to_string(),
        )
    })?;
    let key = compile_expr_with_expected(*index.index, Some(&key_ty));
    Ok(match inc_dec_stmt.tok {
        token::Token::INC => vec![syn::parse_quote! { *#base.entry(#key).or_default() += 1; }],
        token::Token::DEC => vec![syn::parse_quote! { *#base.entry(#key).or_default() -= 1; }],
        _ => vec![],
    })
}

fn expr_shape_key(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(ident) => Some(format!("id:{}", ident.name)),
        ast::Expr::BasicLit(lit) => Some(format!("lit:{:?}:{}", lit.kind, lit.value)),
        ast::Expr::SelectorExpr(selector) => Some(format!(
            "sel:{}:{}",
            expr_shape_key(&selector.x)?,
            selector.sel.name
        )),
        ast::Expr::BinaryExpr(binary) => Some(format!(
            "bin:{:?}:{}:{}",
            binary.op,
            expr_shape_key(&binary.x)?,
            expr_shape_key(&binary.y)?
        )),
        ast::Expr::ParenExpr(paren) => expr_shape_key(&paren.x),
        ast::Expr::UnaryExpr(unary) => Some(format!(
            "unary:{:?}:{}",
            unary.op,
            expr_shape_key(&unary.x)?
        )),
        ast::Expr::IndexExpr(index) => Some(format!(
            "index:{}:{}",
            expr_shape_key(&index.x)?,
            expr_shape_key(&index.index)?
        )),
        _ => None,
    }
}

fn syn_expr_from_type_expr_like(expr: &ast::Expr) -> Option<syn::Expr> {
    match expr {
        ast::Expr::Ident(ident) => {
            let ident = syn::Ident::new(&rust_safe_ident_name(ident.name), Span::mixed_site());
            Some(syn::parse_quote! { #ident })
        }
        ast::Expr::SelectorExpr(selector) => {
            if let ast::Expr::Ident(base) = &*selector.x {
                let base = syn::Ident::new(&rust_safe_ident_name(base.name), Span::mixed_site());
                let sel =
                    syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
                Some(syn::parse_quote! { #base::#sel })
            } else {
                let base = syn_expr_from_type_expr_like(&selector.x)?;
                let sel =
                    syn::Ident::new(&rust_safe_ident_name(selector.sel.name), Span::mixed_site());
                Some(syn::parse_quote! { #base.#sel })
            }
        }
        ast::Expr::BinaryExpr(binary) => {
            let left = syn_expr_from_type_expr_like(&binary.x)?;
            let right = syn_expr_from_type_expr_like(&binary.y)?;
            let op: syn::BinOp = binary.op.into();
            Some(syn::parse_quote! { #left #op #right })
        }
        ast::Expr::ParenExpr(paren) => {
            let inner = syn_expr_from_type_expr_like(&paren.x)?;
            Some(syn::parse_quote! { (#inner) })
        }
        ast::Expr::BasicLit(lit) => {
            let lit: syn::Expr = ast::Expr::BasicLit(ast::BasicLit {
                value_pos: lit.value_pos,
                value_end: lit.value_end,
                kind: lit.kind,
                value: lit.value,
            })
            .into();
            Some(lit)
        }
        _ => None,
    }
}

impl TryFrom<ast::AssignStmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(assign_stmt: ast::AssignStmt) -> Result<Self, Self::Error> {
        if assign_stmt.tok == token::Token::DEFINE && assign_stmt.lhs.len() == assign_stmt.rhs.len()
        {
            TYPE_ENV.with(|env| {
                let inferred = {
                    let borrowed = env.borrow();
                    assign_stmt
                        .rhs
                        .iter()
                        .map(|rhs| typeinfer::GoType::infer_expr(rhs, &borrowed))
                        .collect::<Vec<_>>()
                };
                let mut borrowed = env.borrow_mut();
                for (lhs, ty) in assign_stmt.lhs.iter().zip(inferred) {
                    if let ast::Expr::Ident(ident) = lhs {
                        if ident.name != "_" {
                            borrowed.set_var(ident.name, ty);
                        }
                    }
                }
            });
        }
        if assign_stmt.tok == token::Token::DEFINE
            && assign_stmt.lhs.len() > 1
            && assign_stmt.rhs.len() == 1
        {
            let returns = call_return_types(
                assign_stmt
                    .rhs
                    .first()
                    .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?,
            );
            if !returns.is_empty() {
                TYPE_ENV.with(|env| {
                    let mut env = env.borrow_mut();
                    for (lhs, ty) in assign_stmt.lhs.iter().zip(returns) {
                        if let ast::Expr::Ident(ident) = lhs
                            && ident.name != "_"
                        {
                            env.set_var(ident.name, ty);
                        }
                    }
                });
            }
        }

        // Comma-ok patterns: v, ok := m[k] / v, ok := <-ch / v, ok := x.(T)
        if assign_stmt.lhs.len() == 2 && assign_stmt.rhs.len() == 1 {
            if let Some(kind) = assign_stmt.rhs.first().and_then(detect_comma_ok) {
                let is_define = assign_stmt.tok == token::Token::DEFINE;
                let rhs = assign_stmt
                    .rhs
                    .into_iter()
                    .next()
                    .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?;
                return compile_comma_ok(assign_stmt.lhs, rhs, kind, is_define);
            }
        }

        // Multi-value return: x, y := f() or x, y = f()
        if assign_stmt.lhs.len() > 1 && assign_stmt.rhs.len() == 1 {
            let rhs_expr: syn::Expr = assign_stmt
                .rhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?
                .into();

            if assign_stmt.tok == token::Token::DEFINE {
                if assign_stmt.lhs.iter().any(
                    |expr| {
                        matches!(expr, ast::Expr::Ident(ident) if is_named_return_name(ident.name))
                    },
                ) {
                    let temps = next_named_return_temp_idents(assign_stmt.lhs.len());
                    let temp_pats = temps.iter();
                    let mut out: Vec<syn::Stmt> = vec![syn::parse_quote! {
                        let (#(#temp_pats),*) = #rhs_expr;
                    }];
                    for (lhs, temp) in assign_stmt.lhs.into_iter().zip(temps) {
                        let ast::Expr::Ident(ident) = lhs else {
                            return Err(CompilerError::InvalidAssignment(
                                "expected identifier on lhs of :=".to_string(),
                            ));
                        };
                        if ident.name == "_" {
                            continue;
                        }
                        let name = ident.name;
                        let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
                        let value: syn::Expr = syn::parse_quote! { #temp };
                        if is_named_return_name(name) {
                            if let Some(stmt) = named_return_assignment_stmt(&ident, value) {
                                out.push(stmt);
                            }
                        } else {
                            let init = shared_capture_init_expr(name, value);
                            out.push(syn::parse_quote! { let mut #ident = #init; });
                        }
                    }
                    return Ok(out);
                }

                let mut elems = syn::punctuated::Punctuated::new();
                for expr in assign_stmt.lhs {
                    if let ast::Expr::Ident(ident) = expr {
                        if ident.name == "_" {
                            elems.push(syn::Pat::Wild(syn::PatWild {
                                attrs: vec![],
                                underscore_token: <Token![_]>::default(),
                            }));
                        } else {
                            elems.push(syn::Pat::Ident(syn::PatIdent {
                                attrs: vec![],
                                ident: ident.into(),
                                by_ref: None,
                                subpat: None,
                                mutability: Some(<Token![mut]>::default()),
                            }));
                        }
                    } else {
                        return Err(CompilerError::InvalidAssignment(
                            "expected identifier on lhs of :=".to_string(),
                        ));
                    }
                }
                let pat = syn::Pat::Tuple(syn::PatTuple {
                    attrs: vec![],
                    paren_token: syn::token::Paren::default(),
                    elems,
                });
                return Ok(vec![syn::Stmt::Local(syn::Local {
                    attrs: vec![],
                    pat,
                    init: Some(syn::LocalInit {
                        eq_token: <Token![=]>::default(),
                        expr: Box::new(rhs_expr),
                        diverge: None,
                    }),
                    let_token: <Token![let]>::default(),
                    semi_token: <Token![;]>::default(),
                })]);
            } else {
                // x, y = f()
                let mut tmp_pats = syn::punctuated::Punctuated::new();
                let mut assignments = vec![];
                for (idx, lhs) in assign_stmt.lhs.into_iter().enumerate() {
                    let tmp = quote::format_ident!("__gors_multi_{}", idx);
                    tmp_pats.push(syn::Pat::Ident(syn::PatIdent {
                        attrs: vec![],
                        ident: tmp.clone(),
                        by_ref: None,
                        subpat: None,
                        mutability: Some(<Token![mut]>::default()),
                    }));
                    if !matches!(&lhs, ast::Expr::Ident(ident) if ident.name == "_") {
                        let left = compile_assignment_lhs_checked(lhs)?;
                        let right: syn::Expr = syn::parse_quote! { #tmp };
                        assignments.push(assign_expr_stmt(left, right));
                    }
                }
                let pat = syn::Pat::Tuple(syn::PatTuple {
                    attrs: vec![],
                    paren_token: syn::token::Paren::default(),
                    elems: tmp_pats,
                });
                let mut out: Vec<syn::Stmt> = vec![syn::parse_quote! {
                    let #pat = #rhs_expr;
                }];
                out.extend(assignments);
                return Ok(out);
            }
        }

        if assign_stmt.lhs.len() != assign_stmt.rhs.len() {
            return Err(CompilerError::InvalidAssignment(
                "different numbers of lhs/rhs in assignment".to_string(),
            ));
        }

        if assign_stmt.lhs.is_empty() {
            return Err(CompilerError::InvalidAssignment("empty lhs".to_string()));
        }

        // a := 1
        // b, c := 2, 3
        if assign_stmt.tok == token::Token::DEFINE {
            let define_expected_types: Vec<Option<typeinfer::GoType>> = assign_stmt
                .lhs
                .iter()
                .map(|expr| match expr {
                    ast::Expr::Ident(ident) if ident.name != "_" => {
                        inferred_function_type_for_name(ident.name)
                    }
                    _ => None,
                })
                .collect();

            if assign_stmt.lhs.iter().any(
                |expr| matches!(expr, ast::Expr::Ident(ident) if is_named_return_name(ident.name)),
            ) {
                let temps = next_named_return_temp_idents(assign_stmt.lhs.len());
                let mut out = vec![];
                for (idx, ((lhs, rhs), temp)) in assign_stmt
                    .lhs
                    .into_iter()
                    .zip(assign_stmt.rhs)
                    .zip(temps)
                    .enumerate()
                {
                    let ast::Expr::Ident(ident) = lhs else {
                        return Err(CompilerError::InvalidAssignment(
                            "expected identifier on lhs of :=".to_string(),
                        ));
                    };
                    let should_clone = binding_init_should_clone(&rhs);
                    let init = if let Some(expected) =
                        define_expected_types.get(idx).and_then(|ty| ty.as_ref())
                    {
                        compile_expr_with_expected(rhs, Some(expected))
                    } else {
                        rhs.into()
                    };
                    let init = maybe_clone_binding_init(should_clone, init);
                    out.push(syn::parse_quote! { let #temp = #init; });
                    if ident.name == "_" {
                        continue;
                    }
                    let name = ident.name;
                    let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
                    let value: syn::Expr = syn::parse_quote! { #temp };
                    if is_named_return_name(name) {
                        if let Some(stmt) = named_return_assignment_stmt(&ident, value) {
                            out.push(stmt);
                        }
                    } else {
                        let init = shared_capture_init_expr(name, value);
                        out.push(syn::parse_quote! { let mut #ident = #init; });
                    }
                }
                return Ok(out);
            }

            if assign_stmt
                .lhs
                .iter()
                .any(|expr| matches!(expr, ast::Expr::Ident(ident) if is_shared_capture_name(ident.name)))
            {
                let mut out = vec![];
                for (lhs, rhs) in assign_stmt.lhs.into_iter().zip(assign_stmt.rhs) {
                    let ast::Expr::Ident(ident) = lhs else {
                        return Err(CompilerError::InvalidAssignment(
                            "expected identifier on lhs of :=".to_string(),
                        ));
                    };
                    let should_clone = binding_init_should_clone(&rhs);
                    let expected = inferred_function_type_for_name(ident.name);
                    let init = if let Some(expected) = expected.as_ref() {
                        compile_expr_with_expected(rhs, Some(expected))
                    } else {
                        rhs.into()
                    };
                    let init = maybe_clone_binding_init(should_clone, init);
                    if ident.name == "_" {
                        out.push(syn::parse_quote! { let _ = #init; });
                    } else {
                        let name = ident.name;
                        let ident = syn::Ident::new(&rust_safe_ident_name(name), Span::mixed_site());
                        let init = shared_capture_init_expr(name, init);
                        out.push(syn::parse_quote! { let mut #ident = #init; });
                    }
                }
                return Ok(out);
            }

            let pat = match assign_stmt.lhs.len() {
                1 => {
                    let first_lhs =
                        assign_stmt.lhs.into_iter().next().ok_or_else(|| {
                            CompilerError::InvalidAssignment("empty lhs".to_string())
                        })?;
                    if let ast::Expr::Ident(ident) = first_lhs {
                        if ident.name == "_" {
                            syn::Pat::Wild(syn::PatWild {
                                attrs: vec![],
                                underscore_token: <Token![_]>::default(),
                            })
                        } else {
                            syn::Pat::Ident(syn::PatIdent {
                                attrs: vec![],
                                ident: ident.into(),
                                by_ref: None,
                                subpat: None,
                                mutability: Some(<Token![mut]>::default()),
                            })
                        }
                    } else {
                        return Err(CompilerError::InvalidAssignment(
                            "expected identifier on lhs of :=".to_string(),
                        ));
                    }
                }
                _ => {
                    let mut elems = syn::punctuated::Punctuated::new();
                    for expr in assign_stmt.lhs {
                        if let ast::Expr::Ident(ident) = expr {
                            if ident.name == "_" {
                                elems.push(syn::Pat::Wild(syn::PatWild {
                                    attrs: vec![],
                                    underscore_token: <Token![_]>::default(),
                                }))
                            } else {
                                elems.push(syn::Pat::Ident(syn::PatIdent {
                                    attrs: vec![],
                                    ident: ident.into(),
                                    by_ref: None,
                                    subpat: None,
                                    mutability: Some(<Token![mut]>::default()),
                                }))
                            }
                        } else {
                            return Err(CompilerError::InvalidAssignment(
                                "expected identifier on lhs of :=".to_string(),
                            ));
                        }
                    }
                    syn::Pat::Tuple(syn::PatTuple {
                        attrs: vec![],
                        paren_token: syn::token::Paren {
                            ..Default::default()
                        },
                        elems,
                    })
                }
            };

            let init = match assign_stmt.rhs.len() {
                1 => {
                    let first_rhs =
                        assign_stmt.rhs.into_iter().next().ok_or_else(|| {
                            CompilerError::InvalidAssignment("empty rhs".to_string())
                        })?;
                    let should_clone = binding_init_should_clone(&first_rhs);
                    let init = if let Some(expected) =
                        define_expected_types.first().and_then(|ty| ty.as_ref())
                    {
                        compile_expr_with_expected(first_rhs, Some(expected))
                    } else {
                        first_rhs.into()
                    };
                    maybe_clone_binding_init(should_clone, init)
                }
                _ => {
                    let mut elems = syn::punctuated::Punctuated::new();
                    for (idx, expr) in assign_stmt.rhs.into_iter().enumerate() {
                        let should_clone = binding_init_should_clone(&expr);
                        let init = if let Some(expected) =
                            define_expected_types.get(idx).and_then(|ty| ty.as_ref())
                        {
                            compile_expr_with_expected(expr, Some(expected))
                        } else {
                            expr.into()
                        };
                        elems.push(maybe_clone_binding_init(should_clone, init))
                    }
                    syn::Expr::Tuple(syn::ExprTuple {
                        attrs: vec![],
                        elems,
                        paren_token: syn::token::Paren {
                            ..Default::default()
                        },
                    })
                }
            };

            return Ok(vec![syn::Stmt::Local(syn::Local {
                attrs: vec![],
                pat,
                init: Some(syn::LocalInit {
                    eq_token: <Token![=]>::default(),
                    expr: Box::new(init),
                    diverge: None,
                }),
                let_token: <Token![let]>::default(),
                semi_token: <Token![;]>::default(),
            })]);
        }

        // a = 1
        // b, c = 2, 3
        if assign_stmt.tok == token::Token::ASSIGN {
            if let Some(stmts) = compile_index_swap_assignment(&assign_stmt) {
                return Ok(stmts);
            }

            if assign_stmt.lhs.len() == 1 {
                let lhs_ast = assign_stmt
                    .lhs
                    .into_iter()
                    .next()
                    .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?;
                let rhs_ast = assign_stmt
                    .rhs
                    .into_iter()
                    .next()
                    .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?;
                if matches!(&lhs_ast, ast::Expr::Ident(ident) if ident.name == "_") {
                    let right: syn::Expr = rhs_ast.into();
                    return Ok(vec![syn::parse_quote! { let _ = #right; }]);
                }
                if let Some((key_ty, value_ty)) = map_index_types(&lhs_ast) {
                    return Ok(vec![compile_map_index_assignment(
                        lhs_ast, rhs_ast, key_ty, value_ty,
                    )?]);
                }
                let lhs_func_ty =
                    TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&lhs_ast, &env.borrow()));
                let rhs_ast = match rhs_ast {
                    ast::Expr::FuncLit(func_lit)
                        if shared_func_type_from_go_type(&lhs_func_ty).is_some() =>
                    {
                        let closure = compile_func_lit_with_capture_mode(func_lit, true);
                        if let ast::Expr::Ident(ident) = &lhs_ast {
                            let ident = syn::Ident::new(
                                &rust_safe_ident_name(ident.name),
                                Span::mixed_site(),
                            );
                            let func_ty = shared_func_box_type_from_go_type(&lhs_func_ty)
                                .ok_or_else(|| {
                                    CompilerError::InvalidAssignment(
                                        "invalid function assignment".to_string(),
                                    )
                                })?;
                            return Ok(vec![syn::parse_quote! {{
                                let __gors_func_target = #ident.clone();
                                let #ident = #ident.clone();
                                let __gors_func_value: #func_ty = std::sync::Arc::new(#closure);
                                *crate::builtin::lock_func(&__gors_func_target) = Some(__gors_func_value);
                            }}]);
                        }
                        let right =
                            shared_func_value_expr(&lhs_func_ty, closure).ok_or_else(|| {
                                CompilerError::InvalidAssignment(
                                    "invalid function assignment".to_string(),
                                )
                            })?;
                        let left = compile_assignment_lhs_checked(lhs_ast)?;
                        return Ok(vec![assign_expr_stmt(left, right)]);
                    }
                    other => other,
                };
                let (lhs_ty, rhs_ty) = infer_assignment_types(&lhs_ast, &rhs_ast);
                let right_raw = compile_expr_with_expected(rhs_ast, Some(&lhs_ty));
                let mut right = coerce_assignment_expr(&lhs_ty, &rhs_ty, right_raw);
                if !go_type_is_copy(&lhs_ty) {
                    take_rhs_lvalue_reads(&lhs_ast, &mut right);
                }
                if let Some(shared_ident) = shared_capture_ident(&lhs_ast) {
                    let left: syn::Expr = syn::parse_quote! { *#shared_ident.lock().unwrap() };
                    let value_ident = quote::format_ident!("__gors_shared_value");
                    let value_expr: syn::Expr = syn::parse_quote! { #value_ident };
                    let value_stmt: syn::Stmt = syn::parse_quote! { let #value_ident = #right; };
                    let assign_stmt = assign_expr_stmt(left, value_expr);
                    return Ok(vec![syn::Stmt::Expr(
                        syn::Expr::Block(syn::ExprBlock {
                            attrs: vec![],
                            label: None,
                            block: syn::Block {
                                brace_token: syn::token::Brace::default(),
                                stmts: vec![value_stmt, assign_stmt],
                            },
                        }),
                        Some(<Token![;]>::default()),
                    )]);
                }
                let left = compile_assignment_lhs_checked(lhs_ast)?;
                return Ok(vec![assign_expr_stmt(left, right)]);
            }

            let mut out = vec![];

            let mut idents: Vec<syn::Ident> = vec![];
            let mut values: Vec<syn::Expr> = vec![];
            let mut skip_indices = std::collections::HashSet::new();
            for (idx, (lhs, rhs)) in assign_stmt.lhs.iter().zip(assign_stmt.rhs).enumerate() {
                let tmp = quote::format_ident!("__gors_assign_{}", idx);
                if matches!(lhs, ast::Expr::Ident(ident) if ident.name == "_") {
                    skip_indices.insert(idx);
                }
                idents.push(tmp);
                values.push(rhs.into());
            }
            out.push(syn::parse_quote! { let (#(#idents),*) = (#(#values),*); });

            for (idx, lhs) in assign_stmt.lhs.into_iter().enumerate() {
                if skip_indices.contains(&idx) {
                    continue;
                }
                let right = quote::format_ident!("__gors_assign_{}", idx);
                let left = compile_assignment_lhs_checked(lhs)?;
                let right: syn::Expr = syn::parse_quote! { #right };
                out.push(assign_expr_stmt(left, right));
            }

            return Ok(out);
        }

        // e += 4
        if assign_stmt.tok == token::Token::AND_NOT_ASSIGN {
            if assign_stmt.lhs.len() != 1 {
                return Err(CompilerError::InvalidAssignment(
                    "compound assignment only supports a single lhs element".to_string(),
                ));
            }
            let left = compile_assignment_lhs_checked(
                assign_stmt
                    .lhs
                    .into_iter()
                    .next()
                    .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?,
            )?;
            let right: syn::Expr = assign_stmt
                .rhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?
                .into();
            return Ok(vec![syn::parse_quote! { #left = #left & !#right; }]);
        }

        if assign_stmt.tok.is_assign_op() {
            if assign_stmt.lhs.len() != 1 {
                return Err(CompilerError::InvalidAssignment(
                    "compound assignment only supports a single lhs element".to_string(),
                ));
            }
            let lhs_ast = assign_stmt
                .lhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?;
            let rhs_ast = assign_stmt
                .rhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?;
            let lhs_ty =
                TYPE_ENV.with(|env| typeinfer::GoType::infer_expr(&lhs_ast, &env.borrow()));
            if assign_stmt.tok == token::Token::ADD_ASSIGN
                && matches!(resolved_go_type(&lhs_ty), typeinfer::GoType::String)
            {
                let right = compile_expr_with_expected(rhs_ast, Some(&typeinfer::GoType::String));
                if let Some(shared_ident) = shared_capture_ident(&lhs_ast) {
                    return Ok(vec![syn::parse_quote! {{
                        let __gors_shared_value = #right;
                        #shared_ident.lock().unwrap().push_str(&__gors_shared_value);
                    }}]);
                }
                let left = compile_assignment_lhs_checked(lhs_ast)?;
                return Ok(vec![syn::parse_quote! {{
                    let __gors_string_rhs = #right;
                    #left.push_str(&__gors_string_rhs);
                }}]);
            }
            let right = if matches!(
                assign_stmt.tok,
                token::Token::SHL_ASSIGN | token::Token::SHR_ASSIGN
            ) {
                rhs_ast.into()
            } else {
                compile_expr_with_expected(rhs_ast, Some(&lhs_ty))
            };
            if let Some(shared_ident) = shared_capture_ident(&lhs_ast) {
                let op: syn::BinOp = assign_stmt.tok.into();
                let left: syn::Expr = syn::parse_quote! { *#shared_ident.lock().unwrap() };
                let value_ident = quote::format_ident!("__gors_shared_value");
                let value_expr: syn::Expr = syn::parse_quote! { #value_ident };
                let value_stmt: syn::Stmt = syn::parse_quote! { let #value_ident = #right; };
                let assign_stmt = binary_expr_stmt(left, op, value_expr);
                return Ok(vec![syn::Stmt::Expr(
                    syn::Expr::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: None,
                        block: syn::Block {
                            brace_token: syn::token::Brace::default(),
                            stmts: vec![value_stmt, assign_stmt],
                        },
                    }),
                    Some(<Token![;]>::default()),
                )]);
            }
            let left = compile_assignment_lhs_checked(lhs_ast)?;
            let op: syn::BinOp = assign_stmt.tok.into();
            return Ok(vec![binary_expr_stmt(left, op, right)]);
        }

        Err(CompilerError::UnsupportedConstruct(format!(
            "unexpected assignment token {:?}",
            assign_stmt.tok
        )))
    }
}

impl From<ast::ReturnStmt<'_>> for syn::ExprReturn {
    fn from(return_stmt: ast::ReturnStmt) -> Self {
        record_mapping(&return_stmt.return_, Some("return"));

        let expected_return_types = RETURN_TYPES.with(|types| types.borrow().clone());
        let expr = match return_stmt.results.len() {
            0 => None,
            1 => Some(compile_return_expr_with_expected(
                return_stmt.results.into_iter().next().unwrap_or_else(|| {
                    ast::Expr::Ident(ast::Ident {
                        name_pos: token::Position::default(),
                        name: "__gors_missing_return",
                        obj: None,
                    })
                }),
                expected_return_types.first(),
            )),
            _ => {
                let mut elems = syn::punctuated::Punctuated::new();
                for (idx, result) in return_stmt.results.into_iter().enumerate() {
                    elems.push(compile_return_expr_with_expected(
                        result,
                        expected_return_types.get(idx),
                    ));
                }
                Some(syn::Expr::Tuple(syn::ExprTuple {
                    attrs: vec![],
                    paren_token: syn::token::Paren::default(),
                    elems,
                }))
            }
        };
        Self {
            attrs: vec![],
            expr: expr.map(Box::new),
            return_token: <Token![return]>::default(),
        }
    }
}

impl From<token::Token> for syn::BinOp {
    fn from(token: token::Token) -> Self {
        use token::Token::*;
        match token {
            ADD => Self::Add(<Token![+]>::default()),
            SUB => Self::Sub(<Token![-]>::default()),
            MUL => Self::Mul(<Token![*]>::default()),
            QUO => Self::Div(<Token![/]>::default()),
            REM => Self::Rem(<Token![%]>::default()),
            LAND => Self::And(<Token![&&]>::default()),
            LOR => Self::Or(<Token![||]>::default()),
            XOR => Self::BitXor(<Token![^]>::default()),
            AND => Self::BitAnd(<Token![&]>::default()),
            OR => Self::BitOr(<Token![|]>::default()),
            SHL => Self::Shl(<Token![<<]>::default()),
            SHR => Self::Shr(<Token![>>]>::default()),
            EQL => Self::Eq(<Token![==]>::default()),
            LSS => Self::Lt(<Token![<]>::default()),
            LEQ => Self::Le(<Token![<=]>::default()),
            NEQ => Self::Ne(<Token![!=]>::default()),
            GEQ => Self::Ge(<Token![>=]>::default()),
            GTR => Self::Gt(<Token![>]>::default()),
            //
            ADD_ASSIGN => Self::AddAssign(<Token![+=]>::default()),
            SUB_ASSIGN => Self::SubAssign(<Token![-=]>::default()),
            MUL_ASSIGN => Self::MulAssign(<Token![*=]>::default()),
            QUO_ASSIGN => Self::DivAssign(<Token![/=]>::default()),
            REM_ASSIGN => Self::RemAssign(<Token![%=]>::default()),
            XOR_ASSIGN => Self::BitXorAssign(<Token![^=]>::default()),
            AND_ASSIGN => Self::BitAndAssign(<Token![&=]>::default()),
            OR_ASSIGN => Self::BitOrAssign(<Token![|=]>::default()),
            SHL_ASSIGN => Self::ShlAssign(<Token![<<=]>::default()),
            SHR_ASSIGN => Self::ShrAssign(<Token![>>=]>::default()),
            //
            _ => Self::Add(<Token![+]>::default()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    //! This module contains the compiler tests (the initial Go -> Rust step, followed by the
    //! compiler passes).

    use super::{build_source_map, clear_source_map_tracker, compile, compile_with_source_map};
    use crate::parser::parse_file;
    use crate::printer;
    use quote::quote;
    use std::path::Path;
    use syn::parse_quote as rust;

    fn test(go_input: &str, expected: syn::File) {
        let parsed = parse_file("test.go", go_input).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = (quote! {#compiled}).to_string();
        let expected = (quote! {#expected}).to_string();
        assert_eq!(output, expected);
    }

    fn assert_invalid_assignment(go_input: &str) {
        let parsed = parse_file("test.go", go_input).unwrap();
        match compile(parsed) {
            Err(super::CompilerError::InvalidAssignment(_)) => {}
            Err(super::CompilerError::UnsupportedConstruct(err))
                if err.contains("left side is not assignable") => {}
            Err(err) => panic!("expected invalid assignment rejection, got {err:?}"),
            Ok(_) => panic!("expected invalid assignment, got success"),
        }
    }

    fn assert_unsupported_construct(go_input: &str, message: &str) {
        let parsed = parse_file("test.go", go_input).unwrap();
        match compile(parsed) {
            Err(super::CompilerError::UnsupportedConstruct(err)) => {
                assert!(err.contains(message), "{err:?}");
            }
            Err(err) => panic!("expected unsupported construct, got {err:?}"),
            Ok(_) => panic!("expected unsupported construct, got success"),
        }
    }

    fn assert_invalid_function_signature(go_input: &str, message: &str) {
        let parsed = parse_file("test.go", go_input).unwrap();
        match compile(parsed) {
            Err(super::CompilerError::InvalidFunctionSignature(err)) => {
                assert!(err.contains(message), "{err:?}");
            }
            Err(err) => panic!("expected invalid function signature, got {err:?}"),
            Ok(_) => panic!("expected invalid function signature, got success"),
        }
    }

    #[test]
    fn compile_compound_assignments_without_dynamic_parse() {
        let go_input = r#"
package main

func main() {
	x := 1
	x += 2
	x <<= 1
	x &= 3
}
"#;
        let parsed = parse_file("test.go", go_input).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("x += 2"));
        assert!(output.contains("x <<= 1"));
        assert!(output.contains("x &= 3"));
    }

    fn write_fixture_file(path: &Path, source: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, source).unwrap();
    }

    fn compile_temp_program(dir: &Path) -> printer::GeneratedOutput {
        let program = crate::parser::parse_program(dir.to_str().unwrap()).unwrap();
        let compiled = super::compile_program_multi(program).unwrap();
        printer::generate_multi(compiled).unwrap()
    }

    fn compile_temp_program_error(dir: &Path) -> super::CompilerError {
        let program = crate::parser::parse_program(dir.to_str().unwrap()).unwrap();
        match super::compile_program_multi(program) {
            Err(err) => err,
            Ok(_) => panic!("expected program compile error"),
        }
    }

    #[test]
    fn compile_program_multi_applies_ir_validation_to_merged_package() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

var X int

func main() {}
"#,
        );
        write_fixture_file(
            tmp.path().join("other.go").as_path(),
            r#"
package main

const X = 1
"#,
        );

        let program = crate::parser::parse_program(tmp.path().to_str().unwrap()).unwrap();
        match super::compile_program_multi(program) {
            Err(super::CompilerError::UnsupportedConstruct(err)) => {
                assert!(err.contains("duplicate top-level declaration X"), "{err:?}");
            }
            Err(err) => panic!("expected duplicate declaration rejection, got {err:?}"),
            Ok(_) => panic!("expected duplicate declaration rejection"),
        }
    }

    #[test]
    fn compile_program_multi_rejects_main_package_without_main_function() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

var X int
"#,
        );

        let program = crate::parser::parse_program(tmp.path().to_str().unwrap()).unwrap();
        match super::compile_program_multi(program) {
            Err(super::CompilerError::InvalidFunctionSignature(err)) => {
                assert!(
                    err.contains("function main is undeclared in the main package"),
                    "{err:?}"
                );
            }
            Err(err) => panic!("expected missing main rejection, got {err:?}"),
            Ok(_) => panic!("expected missing main rejection"),
        }
    }

    #[test]
    fn rust_type_from_go_type_builds_path_types() {
        let int_ty = super::rust_type_from_go_type(&super::typeinfer::GoType::Int).unwrap();
        let complex_ty =
            super::rust_type_from_go_type(&super::typeinfer::GoType::Complex64).unwrap();
        let named_crate_ty = super::named_go_type_path("crate");

        assert_eq!(quote! { #int_ty }.to_string(), "isize");
        assert_eq!(
            quote! { #complex_ty }.to_string(),
            "crate :: builtin :: Complex64"
        );
        assert_eq!(quote! { #named_crate_ty }.to_string(), "crate_");
    }

    #[test]
    fn it_should_support_binary_operators() {
        test(
            r#"
                package main;

                func main() {
                    i += 2;
                    i *= 2;
                }
            "#,
            rust! {
                pub fn main() {
                    i += 2;
                    i *= 2;
                }
            },
        )
    }

    #[test]
    fn it_should_reject_non_addressable_assignment_lhs() {
        assert_invalid_assignment(
            r#"
                package main

                const c = 1

                func main() {
                    c = 2
                }
            "#,
        );
        assert_invalid_assignment(
            r#"
                package main

                type S struct { X int }

                func main() {
                    S{X: 1}.X = 2
                }
            "#,
        );
        assert_invalid_assignment(
            r#"
                package main

                func main() {
                    [1]int{1}[0] = 2
                }
            "#,
        );
        assert_unsupported_construct(
            r#"
                package main

                const c = 1

                func main() {
                    c++
                }
            "#,
            "operand must be addressable or a map index",
        );
        assert_invalid_assignment(
            r#"
                package main

                const c = 1

                func main() {
                    for c = range []int{1} {
                    }
                }
            "#,
        );
    }

    #[test]
    fn it_should_allow_assignment_to_shadowed_predeclared_name() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    len := 1
                    len <<= 3
                }
            "#,
        )
        .unwrap();
        compile(parsed).unwrap();
    }

    #[test]
    fn it_should_not_treat_shadowed_predeclared_call_as_builtin() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    print()
                }
            "#,
        )
        .unwrap();
        let crate::ast::Decl::FuncDecl(func) = parsed.decls.first().expect("expected function")
        else {
            panic!("expected function declaration");
        };
        let crate::ast::Stmt::ExprStmt(expr) = func
            .body
            .as_ref()
            .and_then(|body| body.list.first())
            .expect("expected expression statement")
        else {
            panic!("expected expression statement");
        };
        let crate::ast::Expr::CallExpr(call) = &expr.x else {
            panic!("expected call expression");
        };

        super::set_type_env(super::typeinfer::TypeEnv::new());
        let builtin_kind = super::builtin_call_kind(call);

        let mut env = super::typeinfer::TypeEnv::new();
        env.set_var(
            "print",
            super::typeinfer::GoType::Func {
                params: Vec::new(),
                results: Vec::new(),
                variadic_start: None,
            },
        );
        super::set_type_env(env);
        let shadowed_kind = super::builtin_call_kind(call);
        super::set_type_env(super::typeinfer::TypeEnv::new());

        assert_eq!(builtin_kind, Some(super::ir::BuiltinCallKind::Print));
        assert_eq!(shadowed_kind, None);
    }

    #[test]
    fn it_should_not_treat_shadowed_predeclared_type_call_as_conversion() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    _ = string("x")
                }
            "#,
        )
        .unwrap();
        let crate::ast::Decl::FuncDecl(func) = parsed.decls.first().expect("expected function")
        else {
            panic!("expected function declaration");
        };
        let crate::ast::Stmt::AssignStmt(assign) = func
            .body
            .as_ref()
            .and_then(|body| body.list.first())
            .expect("expected assignment")
        else {
            panic!("expected assignment");
        };
        let crate::ast::Expr::CallExpr(call) = assign.rhs.first().expect("expected rhs call")
        else {
            panic!("expected call expression");
        };

        super::set_type_env(super::typeinfer::TypeEnv::new());
        let conversion_kind = super::special_type_conversion_kind(call);

        let mut env = super::typeinfer::TypeEnv::new();
        env.set_var(
            "string",
            super::typeinfer::GoType::Func {
                params: vec![super::typeinfer::GoType::String],
                results: vec![super::typeinfer::GoType::String],
                variadic_start: None,
            },
        );
        super::set_type_env(env);
        let shadowed_kind = super::special_type_conversion_kind(call);
        super::set_type_env(super::typeinfer::TypeEnv::new());

        assert_eq!(
            conversion_kind,
            Some(super::ir::SpecialTypeConversionKind::String)
        );
        assert_eq!(shadowed_kind, None);
    }

    #[test]
    fn it_should_reject_goto_that_skips_local_declaration() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    goto Done
                    x := 1
                Done:
                    println(x)
                }
            "#,
            "invalid goto to Done skips declarations: x",
        );
    }

    #[test]
    fn it_should_reject_goto_into_nested_block() {
        assert_unsupported_construct(
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
            "invalid goto to Inside enters a nested block",
        );
    }

    #[test]
    fn it_should_reject_goto_to_undefined_label() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    goto Missing
                }
            "#,
            "invalid goto to undefined label Missing",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    goto _
                _:
                    println("not a target")
                }
            "#,
            "invalid goto to undefined label _",
        );
    }

    #[test]
    fn it_should_reject_invalid_goto_in_function_literal() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    _ = func() {
                        goto Missing
                    }
                }
            "#,
            "invalid goto to undefined label Missing",
        );
        assert_unsupported_construct(
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
            "invalid goto to Done skips declarations: x",
        );
    }

    #[test]
    fn it_should_accept_blank_labels() {
        let parsed = parse_file(
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
        compile(parsed).unwrap();
    }

    #[test]
    fn it_should_reject_invalid_branch_statements() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    break
                }
            "#,
            "invalid break outside for, switch, or select",
        );
        assert_unsupported_construct(
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
            "invalid continue to non-enclosing for label Switch",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    switch 1 {
                    default:
                        fallthrough
                    }
                }
            "#,
            "invalid fallthrough in final switch case",
        );
    }

    #[test]
    fn it_should_reject_invalid_statement_context_expressions() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    1 + 2
                }
            "#,
            "invalid expression statement: expected a function call, method call, or receive operation",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    len("go")
                }
            "#,
            "invalid expression statement: len is not permitted in statement context",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    string(65)
                }
            "#,
            "invalid expression statement: type conversions are not permitted in statement context",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    go len("go")
                }
            "#,
            "invalid go statement: len is not permitted in statement context",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    defer int(1)
                }
            "#,
            "invalid defer statement: type conversions are not permitted in statement context",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    delete(1, 2)
                }
            "#,
            "invalid expression statement: invalid delete call: first argument must have map type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    clear(1)
                }
            "#,
            "invalid expression statement: invalid clear call: argument must have map or slice type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    close(1)
                }
            "#,
            "invalid expression statement: invalid close call: argument must have channel type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    _ = len(1)
                }
            "#,
            "invalid expression: invalid len call: argument must have string, array, slice, map, or channel type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                var N = copy(1, []int{})

                func main() {
                }
            "#,
            "invalid expression: invalid copy call: first argument must have slice type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    s := "go"
                    s++
                }
            "#,
            "invalid increment/decrement statement: operand must have numeric type",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    if 1 {
                    }
                }
            "#,
            "invalid if condition: condition must be boolean, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                    x <- 2
                }
            "#,
            "invalid send statement: channel operand must have channel type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    var ch <-chan int
                    ch <- 1
                }
            "#,
            "invalid send statement: cannot send to receive-only channel",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    ch := make(chan int, 1)
                    ch <- "go"
                }
            "#,
            "invalid send statement: cannot send string to channel of int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    <-1
                }
            "#,
            "invalid receive operation: operand must have channel type, got int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    var ch chan<- int
                    <-ch
                }
            "#,
            "invalid receive operation: cannot receive from send-only channel",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() {}

                func main() {
                    select {
                    case f():
                    }
                }
            "#,
            "invalid select communication clause: case must be a send statement, receive statement, or default",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x, x := 1, 2
                }
            "#,
            "invalid short variable declaration: name x appears more than once on left side of :=",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    for i, i := range []int{1} {
                        _ = i
                    }
                }
            "#,
            "invalid short variable declaration: name i appears more than once on left side of :=",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                    x := 2
                    _ = x
                }
            "#,
            "invalid short variable declaration: no new variables on left side of :=",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f(x int) {
                    x := 1
                    _ = x
                }
            "#,
            "invalid short variable declaration: no new variables on left side of :=",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    1 = 2
                }
            "#,
            "invalid assignment: left side is not assignable",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    xs := []int{1}
                    xs[0] := 2
                }
            "#,
            "invalid short variable declaration: left side of := must be identifiers",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    for i := 0; i < 3; i := i + 1 {
                        _ = i
                    }
                }
            "#,
            "invalid for statement: post statement cannot be a short variable declaration",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                    x = "go"
                    _ = x
                }
            "#,
            "invalid assignment: cannot assign string to int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                    x, y := "go", 2
                    _, _ = x, y
                }
            "#,
            "invalid assignment: cannot assign string to int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f(x int) {
                    x, y := "go", 2
                    _, _ = x, y
                }

                func main() {
                    f(1)
                }
            "#,
            "invalid assignment: cannot assign string to int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := nil
                    _ = x
                }
            "#,
            "invalid assignment: use of untyped nil in assignment",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                    x = nil
                    _ = x
                }
            "#,
            "invalid assignment: cannot assign nil to int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func takes(a int) {}

                func main() {
                    takes(nil)
                }
            "#,
            "invalid expression: invalid call to takes: argument 1 must be assignable to int, got nil",
        );
        assert_unsupported_construct(
            r#"
                package main

                func pair() (int, int) {
                    return 1, 2
                }

                func main() {
                    x := pair()
                    _ = x
                }
            "#,
            "invalid assignment: assignment count mismatch: 1 left operand(s), 2 value(s)",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    xs := []int{1}
                    x, ok := xs[0]
                    _, _ = x, ok
                }
            "#,
            "invalid assignment: assignment count mismatch: 2 left operand(s), 1 value(s)",
        );
        assert_unsupported_construct(
            r#"
                package main

                func pair() (int, int) {
                    return 1, 2
                }

                func f() int {
                    return pair()
                }
            "#,
            "invalid return statement: expected 1 result value(s), got 2",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() int {
                    return
                }
            "#,
            "invalid return statement: expected 1 result value(s), got 0",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() int {
                    return "go"
                }
            "#,
            "invalid return statement: cannot return string as int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() int {
                    return nil
                }
            "#,
            "invalid return statement: cannot return nil as int",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f(x any) {
                    var v any
                    _ = v
                    switch v = x.(type) {
                    default:
                    }
                }
            "#,
            "invalid type switch guard: guard assignment must use :=",
        );
        assert_unsupported_construct(
            r#"
                package main

                const A, B = 1

                func main() {
                }
            "#,
            "const declaration has 2 name(s) but 1 value(s)",
        );
        assert_unsupported_construct(
            r#"
                package main

                func pair() (int, int) {
                    return 1, 2
                }

                var X = pair()

                func main() {
                }
            "#,
            "var declaration has 1 name(s) but 2 value(s)",
        );
        assert_unsupported_construct(
            r#"
                package main

                func pair() (int, int) {
                    return 1, 2
                }

                func main() {
                    var x, y, z = pair(), 3
                    _, _, _ = x, y, z
                }
            "#,
            "invalid declaration: multi-valued expression in explicit var initializer list",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() int { return 1 }

                const X = f()

                func main() {
                }
            "#,
            "const initializer must be a constant expression",
        );
        assert_unsupported_construct(
            r#"
                package main

                var z complex128
                const X = len([10]float64{imag(z)})

                func main() {
                }
            "#,
            "const initializer must be a constant expression",
        );
        assert_unsupported_construct(
            r#"
                package main

                const X int = "go"

                func main() {
                }
            "#,
            "cannot initialize int constant with string",
        );
        assert_unsupported_construct(
            r#"
                package main

                var X = nil

                func main() {
                }
            "#,
            "var initializer cannot use untyped nil without an explicit nilable type",
        );
        assert_unsupported_construct(
            r#"
                package main

                var X int = "go"

                func main() {
                }
            "#,
            "cannot initialize int variable with string",
        );
        assert_unsupported_construct(
            r#"
                package main

                var X int = nil

                func main() {
                }
            "#,
            "cannot initialize int variable with nil",
        );
        assert_unsupported_construct(
            r#"
                package main

                func f() int {
                    if true {
                        return 1
                    }
                }
            "#,
            "invalid function body: missing terminating statement",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    switch 1 {
                    default:
                    default:
                    }
                }
            "#,
            "invalid switch statement: multiple default clauses",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    select {
                    default:
                    default:
                    }
                }
            "#,
            "invalid select statement: multiple default clauses",
        );
    }

    #[test]
    fn it_should_reject_invalid_labels() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                Done:
                    println("done")
                }
            "#,
            "invalid unused label Done",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                Done:
                    goto Done
                Done:
                    println("done")
                }
            "#,
            "invalid duplicate label Done",
        );
    }

    #[test]
    fn it_should_reject_range_clauses_with_too_many_bindings() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    for i, v := range 3 {
                        _, _ = i, v
                    }
                }
            "#,
            "invalid range clause: integer range permits at most 1 iteration variable(s), got 2",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    ch := make(chan int)
                    for i, v := range ch {
                        _, _ = i, v
                    }
                }
            "#,
            "invalid range clause: channel range permits at most 1 iteration variable(s), got 2",
        );
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    for range 1.5 {
                    }
                }
            "#,
            "invalid range clause: cannot range over float64",
        );
    }

    #[test]
    fn it_should_reject_invalid_range_assignment_types() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    var value string
                    for _, value = range []int{1} {
                    }
                    _ = value
                }
            "#,
            "invalid range assignment: cannot assign int to string",
        );
    }

    #[test]
    fn it_should_reject_invalid_function_signatures() {
        assert_invalid_function_signature(
            r#"
                package main

                func main(a int, a string) {}
            "#,
            "duplicate parameter/result name a",
        );
        assert_invalid_function_signature(
            r#"
                package main

                func main(nums ...int, label string) {}
            "#,
            "variadic parameter must be the final incoming parameter",
        );
        assert_invalid_function_signature(
            r#"
                package main

                var _ = func(a int, a int) {}
            "#,
            "duplicate parameter/result name a",
        );
    }

    #[test]
    fn it_should_reject_invalid_struct_and_method_declarations() {
        assert_unsupported_construct(
            r#"
                package main

                type S struct {
                    X int
                    X string
                }
            "#,
            "duplicate field X in struct S",
        );
        assert_unsupported_construct(
            r#"
                package main

                type S struct{}
                func (S) M() {}
                func (*S) M() {}
            "#,
            "duplicate method M for receiver base type S",
        );
        assert_unsupported_construct(
            r#"
                package main

                type S struct { M int }
                func (S) M() {}
            "#,
            "method M conflicts with field on struct S",
        );
    }

    #[test]
    fn it_should_compile_deep_binary_chains_iteratively() {
        let expr = (0..128)
            .map(|idx| (idx + 1).to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        let go_source = format!(
            r#"
                package main;

                func main() {{
                    x := {expr};
                    println(x);
                }}
            "#
        );
        let parsed = parse_file("test.go", &go_source).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("println_value"));
    }

    #[test]
    fn it_should_create_sourcemap_for_fmt_println() {
        clear_source_map_tracker();
        let go_source = r#"package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let compiled = compile_with_source_map(parsed, "test.go", go_source).unwrap();

        // Generate the Rust code
        let rust_source = printer::generate(compiled).unwrap();

        // Build the source map
        let sm = build_source_map(&rust_source);

        // Verify we can serialize and parse it back
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        let parsed_sm = sourcemap::SourceMap::from_reader(&buf[..]).unwrap();

        // Should have some tokens
        assert!(
            parsed_sm.get_token_count() > 0,
            "Expected source map to have tokens"
        );

        // Verify the string literal keeps a mapping through import-selector lowering.
        let has_hello = (0..parsed_sm.get_name_count())
            .any(|i| parsed_sm.get_name(i).is_some_and(|n| n.contains("Hello")));
        assert!(has_hello, "Expected string literal mapping in source map");
    }

    #[test]
    fn it_should_create_sourcemap_for_func_declaration() {
        clear_source_map_tracker();
        let go_source = r#"package main

func main() {
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let compiled = compile_with_source_map(parsed, "test.go", go_source).unwrap();

        // Generate the Rust code
        let rust_source = printer::generate(compiled).unwrap();

        // The generated Rust code should contain "fn main"
        assert!(
            rust_source.contains("fn main"),
            "Expected Rust output to contain 'fn main'"
        );

        // Build the source map
        let sm = build_source_map(&rust_source);

        // Verify we can serialize and parse it back
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        let parsed_sm = sourcemap::SourceMap::from_reader(&buf[..]).unwrap();

        // Should have some tokens
        assert!(
            parsed_sm.get_token_count() > 0,
            "Expected source map to have tokens"
        );

        // Check that source file is correct
        assert_eq!(parsed_sm.get_source(0), Some("test.go"));

        // Check that Go names ("func", "main") are in the source map (not Rust names like "fn")
        let names: Vec<_> = (0..parsed_sm.get_name_count())
            .filter_map(|i| parsed_sm.get_name(i))
            .collect();
        assert!(
            names.contains(&"func"),
            "Expected 'func' (Go name) in source map names"
        );
        assert!(
            names.contains(&"main"),
            "Expected 'main' in source map names"
        );
    }

    #[test]
    fn compile_with_source_map_applies_single_file_validation() {
        clear_source_map_tracker();
        let go_source = r#"package main

import "fmt"

func main() {
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        match compile_with_source_map(parsed, "test.go", go_source) {
            Err(super::CompilerError::UnsupportedConstruct(err)) => {
                assert!(err.contains("fmt imported and not used"), "{err:?}");
            }
            Err(err) => panic!("expected validation rejection, got {err:?}"),
            Ok(_) => panic!("expected validation rejection"),
        }
    }

    #[test]
    fn import_path_to_filename_simple() {
        assert_eq!(super::import_path_to_filename("fmt"), "fmt.rs");
        assert_eq!(
            super::import_path_to_filename("example/greet"),
            "example__greet.rs"
        );
        assert_eq!(
            super::import_path_to_filename("example/math/calc"),
            "example__math__calc.rs"
        );
    }

    #[test]
    fn import_path_to_filename_main() {
        assert_eq!(super::import_path_to_filename("main"), "main.rs");
        assert_eq!(super::import_path_to_filename(""), "main.rs");
    }

    #[test]
    fn import_path_to_filename_no_collisions() {
        let paths = ["example/foo", "example/bar", "other/foo", "example/foo/bar"];
        let filenames: Vec<String> = paths
            .iter()
            .map(|p| super::import_path_to_filename(p))
            .collect();
        let unique: std::collections::HashSet<_> = filenames.iter().collect();
        assert_eq!(filenames.len(), unique.len(), "filenames must be unique");
    }

    #[test]
    fn content_hash_is_deterministic() {
        let files1 = vec![
            ("b.go".to_string(), "package b".to_string()),
            ("a.go".to_string(), "package a".to_string()),
        ];
        let files2 = vec![
            ("a.go".to_string(), "package a".to_string()),
            ("b.go".to_string(), "package b".to_string()),
        ];
        assert_eq!(
            super::compute_content_hash(&files1),
            super::compute_content_hash(&files2)
        );
    }

    #[test]
    fn content_hash_changes_on_modification() {
        let files1 = vec![("a.go".to_string(), "package a".to_string())];
        let files2 = vec![("a.go".to_string(), "package a // changed".to_string())];
        assert_ne!(
            super::compute_content_hash(&files1),
            super::compute_content_hash(&files2)
        );
    }

    #[test]
    fn it_should_coerce_function_values_at_expected_argument_sites() {
        let go_source = r#"
package main

func apply(f func(int) int, x int) int {
	return f(x)
}

func inc(x int) int {
	return x + 1
}

func main() {
	println(apply(inc, 1))
	println(apply(func(x int) int { return x + 2 }, 1))
}
"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("std::sync::Arc::new(\n                    inc"));
        assert!(output.contains("std::sync::Arc::new(move |"));
        assert!(output.contains("dyn Fn(isize) -> isize + Send + Sync"));
        assert!(!output.contains("FnMut"));
    }

    #[test]
    fn append_missing_return_panic_preserves_tail_value_exprs() {
        let mut block: syn::Block = rust!({ 1 });
        let output: syn::ReturnType = rust!(-> isize);

        super::append_missing_return_panic(&mut block, &output, None);

        assert_eq!(quote! { #block }.to_string(), "{ 1 }");
    }

    #[test]
    fn stdlib_sort_type_env_records_search_function_parameter() {
        let (_, env) = crate::resolve::scan_type_env("sort").unwrap();
        let params = env.get_func_params("Search");

        assert!(
            matches!(
                params.as_slice(),
                [
                    super::typeinfer::GoType::Int,
                    super::typeinfer::GoType::Func { .. }
                ]
            ),
            "{params:?}"
        );
    }

    #[test]
    fn it_should_coerce_complex_builtin_parts() {
        let go_source = r#"
package main

func main() {
	_ = complex(1, 2)
}
"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("crate::builtin::complex128((1 as f64), (2 as f64))"));
    }

    #[test]
    fn it_should_lower_imaginary_literals() {
        let go_source = r#"
package main

const wide = 0123i
const small complex64 = 2.5i
const combo = 1 + 2i
const tiny complex64 = 3 + 4i

func main() {
	z := 0x1p-2i
	sum := 1 + 2i
	var narrowed complex64 = 3 + 4i
	_, _, _, _, _, _, _ = wide, small, combo, tiny, z, sum, narrowed
}
"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("crate::builtin::complex128(0.0"));
        assert!(output.contains("crate::builtin::complex64"));
        assert!(output.contains("crate::builtin::to_complex64"));
        assert!(!output.contains("unsupported literal"));
    }

    #[test]
    fn compile_program_multi_builtin_println() {
        let go_source = "package main\n\nfunc main() {\n\tprintln(\"hello\")\n}\n";
        let ast = parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec![],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        assert!(compiled.has_main);
        assert!(compiled.modules.contains_key("__main__"));
        assert!(compiled.modules.contains_key("builtin"));
        assert!(!compiled.modules.contains_key("fmt"));
        let builtin_mod = compiled.modules.get("builtin").unwrap();
        let builtin_item_names: std::collections::HashSet<_> = builtin_mod
            .file
            .items
            .iter()
            .filter_map(super::item_name)
            .collect();
        assert!(builtin_item_names.contains("println_value"));
        assert!(!builtin_item_names.contains("println_empty"));
        assert!(!builtin_item_names.contains("append"));
    }

    #[test]
    fn compile_program_multi_preserves_main_package_var_types() {
        let go_source = r#"package main

var greeting string = "go"
var count int8 = 40
var suffix string

func main() {
	greeting += "rs"
	suffix = "!"
	count += 2
	println(greeting + suffix, count)
}
"#;
        let ast = parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec![],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        let output = printer::generate_multi(compiled).unwrap();
        let main_rs = output.files.get("main.rs").unwrap();

        assert!(main_rs.contains("let mut greeting: String = \"go\".to_string();"));
        assert!(main_rs.contains("let mut count: i8 = (40 as i8);"));
        assert!(main_rs.contains("let mut suffix: String = Default::default();"));
    }

    #[test]
    fn compile_program_multi_generates_valid_rust() {
        let go_source = "package main\n\nfunc main() {\n\tprintln(\"test\")\n}\n";
        let ast = parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec![],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        let output = printer::generate_multi(compiled).unwrap();
        assert!(output.files.contains_key("main.rs"));
        assert!(output.files.contains_key("lib.rs"));
        assert!(output.files.contains_key("builtin.rs"));
        assert!(!output.files.contains_key("fmt.rs"));
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(main_rs.contains("mod lib"));
        assert!(main_rs.contains("use lib::{"));
        assert!(main_rs.contains("builtin"));
        assert!(main_rs.contains("println_value"));
        let lib_rs = output.files.get("lib.rs").unwrap();
        assert!(lib_rs.contains("pub mod builtin"));
        assert!(!lib_rs.contains("pub mod fmt"));
    }

    #[test]
    fn compile_program_multi_source_map_tracks_main_package_files() {
        clear_source_map_tracker();
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

func main() {
	helper()
}
"#,
        );
        write_fixture_file(
            tmp.path().join("helper.go").as_path(),
            r#"
package main

func helper() {
	println("helper")
}
"#,
        );

        let program = crate::parser::parse_program(tmp.path().to_str().unwrap()).unwrap();
        let expected_sources: std::collections::HashSet<_> = program
            .main_package
            .files
            .iter()
            .map(|(file, _)| file.clone())
            .collect();
        let compiled = super::compile_program_multi_with_source_maps(program).unwrap();
        let output = printer::generate_multi(compiled).unwrap();
        let main_rs = output.files.get("main.rs").unwrap();
        let sm = build_source_map(main_rs);
        let mut buf = Vec::new();
        sm.to_writer(&mut buf).unwrap();
        let parsed_sm = sourcemap::SourceMap::from_reader(&buf[..]).unwrap();
        let actual_sources: std::collections::HashSet<_> = (0..expected_sources.len())
            .filter_map(|idx| parsed_sm.get_source(idx as u32).map(ToString::to_string))
            .collect();

        assert_eq!(actual_sources, expected_sources);
    }

    #[test]
    fn compile_program_multi_emits_referenced_local_package_module() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "example/greet"

func main() {
	greet.Hello()
}
"#,
        );
        write_fixture_file(
            tmp.path().join("greet/greet.go").as_path(),
            r#"
package greet

func Hello() {}
"#,
        );

        let program = crate::parser::parse_program(tmp.path().to_str().unwrap()).unwrap();
        let compiled = super::compile_program_multi(program).unwrap();

        assert!(compiled.has_main);
        assert!(compiled.modules.values().any(|m| m.mod_name == "greet"));
        let greet = compiled
            .modules
            .values()
            .find(|m| m.mod_name == "greet")
            .unwrap();
        assert_eq!(greet.filename, "example__greet.rs");
        assert!(!greet.content_hash.is_empty());
    }

    #[test]
    fn compile_program_multi_uses_imported_package_clause_names() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "example/lib"

func main() {
	renamed.Hello()
}
"#,
        );
        write_fixture_file(
            tmp.path().join("lib/lib.go").as_path(),
            r#"
package renamed

func Hello() {}
"#,
        );

        let output = compile_temp_program(tmp.path());
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(main_rs.contains("renamed::Hello()"), "{main_rs}");
        assert!(output.files.contains_key("example__lib.rs"));
    }

    #[test]
    fn compile_program_multi_rejects_duplicate_default_import_package_names() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import (
	"example/a"
	"example/b"
)

func main() {}
"#,
        );
        write_fixture_file(
            tmp.path().join("a/a.go").as_path(),
            r#"
package same

func A() {}
"#,
        );
        write_fixture_file(
            tmp.path().join("b/b.go").as_path(),
            r#"
package same

func B() {}
"#,
        );

        match compile_temp_program_error(tmp.path()) {
            super::CompilerError::UnsupportedConstruct(err) => {
                assert!(err.contains("duplicate import name same"), "{err:?}");
            }
            err => panic!("expected duplicate import rejection, got {err:?}"),
        }
    }

    #[test]
    fn generated_output_prunes_unreachable_items_and_builtin_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "fmt"

const deadConst = 42

type deadStruct struct {
	value int
}

func deadLocal() {
	fmt.Println("dead")
}

func main() {
	fmt.Println("live")
}
"#,
        );

        let output = compile_temp_program(tmp.path());
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(!main_rs.contains("deadConst"), "{main_rs}");
        assert!(!main_rs.contains("deadStruct"), "{main_rs}");
        assert!(!main_rs.contains("deadLocal"), "{main_rs}");

        let builtin_rs = output.files.get("builtin.rs").unwrap();
        assert!(!builtin_rs.contains("Chan"), "{builtin_rs}");
        assert!(!builtin_rs.contains("make_chan"), "{builtin_rs}");
    }

    #[test]
    fn compile_program_multi_rejects_unreferenced_imported_package_modules() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import (
	"example/dead"
	"example/live"
)

func main() {
	live.Hello()
}
"#,
        );
        write_fixture_file(
            tmp.path().join("dead/dead.go").as_path(),
            r#"
package dead

func Dead() {}
"#,
        );
        write_fixture_file(
            tmp.path().join("live/live.go").as_path(),
            r#"
package live

func Hello() {}
"#,
        );

        match compile_temp_program_error(tmp.path()) {
            super::CompilerError::UnsupportedConstruct(err) => {
                assert!(
                    err.contains("example/dead imported and not used"),
                    "{err:?}"
                );
            }
            err => panic!("expected unused import rejection, got {err:?}"),
        }
    }

    #[test]
    fn imported_string_constants_are_called_as_functions() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "example/config"

func main() {
	_ = len(config.Header)
	_ = config.Header[:5]
}
"#,
        );
        write_fixture_file(
            tmp.path().join("config/config.go").as_path(),
            r#"
package config

const Header = "hello, world"
"#,
        );

        let output = compile_temp_program(tmp.path());
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(main_rs.contains("config::Header()"), "{main_rs}");
        assert!(main_rs.contains("&config::Header()"), "{main_rs}");
        assert!(main_rs.contains("config::Header()[.."), "{main_rs}");
        assert!(!main_rs.contains("config::Header[.."), "{main_rs}");
    }

    #[test]
    fn dce_prunes_display_impl_when_string_method_is_unreachable() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "example/level"

func main() {
	_ = level.Debug == level.Debug
}
"#,
        );
        write_fixture_file(
            tmp.path().join("level/level.go").as_path(),
            r#"
package level

type Level int

const Debug Level = -4

func (l Level) String() string {
	return "debug"
}
"#,
        );

        let output = compile_temp_program(tmp.path());
        let level_rs = output.files.get("example__level.rs").unwrap();
        assert!(!level_rs.contains("fn String"), "{level_rs}");
        assert!(
            !level_rs.contains("impl std::fmt::Display for Level"),
            "{level_rs}"
        );
    }

    #[test]
    fn generated_output_prunes_lowered_away_errors_import() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(tmp.path().join("go.mod").as_path(), "module example\n");
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

import "errors"

func main() {
	_ = errors.New("boom")
}
"#,
        );

        let output = compile_temp_program(tmp.path());
        assert!(!output.files.contains_key("errors.rs"));
        assert!(!output.files.get("lib.rs").unwrap().contains("errors"));
    }

    #[test]
    fn compile_program_multi_retains_referenced_stdlib_imports() {
        let go_source = r#"package main

import "fmt"
import "errors"
import "strconv"

func main() {
	fmt.Println("hello")
	e := errors.New("fail")
	s := strconv.Itoa(42)
	_, _ = e, s
}
"#;
        let ast = crate::parser::parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec![
                "errors".to_string(),
                "fmt".to_string(),
                "strconv".to_string(),
            ],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        assert!(compiled.modules.contains_key("fmt"));
        assert!(compiled.modules.contains_key("strconv"));
        assert!(!compiled.modules.contains_key("errors"));
        let output = printer::generate_multi(compiled).unwrap();
        assert!(output.files.contains_key("fmt.rs"));
        assert!(output.files.contains_key("strconv.rs"));
        assert!(!output.files.contains_key("errors.rs"));
        let lib_rs = output.files.get("lib.rs").unwrap();
        assert!(lib_rs.contains("pub mod fmt"));
        assert!(lib_rs.contains("pub mod strconv"));
        assert!(!lib_rs.contains("pub mod errors"));
    }

    #[test]
    fn compile_program_multi_retains_direct_stdlib_constants() {
        let go_source = r#"package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Println(os.PathSeparator)
	fmt.Println(os.PathListSeparator)
}
"#;
        let ast = crate::parser::parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec!["fmt".to_string(), "os".to_string()],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        let output = printer::generate_multi(compiled).unwrap();
        let os_rs = output.files.get("os.rs").unwrap();
        assert!(os_rs.contains("pub const PathSeparator"), "{os_rs}");
        assert!(os_rs.contains("pub const PathListSeparator"), "{os_rs}");
    }

    #[test]
    fn resolve_with_roots_retains_grouped_stdlib_constants() {
        let roots = std::collections::HashSet::from([
            "PathSeparator".to_string(),
            "PathListSeparator".to_string(),
            "Stdout".to_string(),
            "clone".to_string(),
        ]);
        let module = crate::resolve::resolve_with_roots("os", &roots).unwrap();
        let items = module.content.unwrap().1;
        let source = prettyplease::unparse(&syn::File {
            shebang: None,
            attrs: vec![],
            items,
        });
        assert!(source.contains("pub const PathSeparator"), "{source}");
        assert!(source.contains("pub const PathListSeparator"), "{source}");
    }

    #[test]
    fn collect_external_refs_follows_stdlib_const_paths() {
        let file: syn::File = rust! {
            pub fn main() {
                fmt::Println(Vec::from([Box::new((os::PathSeparator).clone()) as Box<dyn std::any::Any>]));
                fmt::Println(Vec::from([Box::new((os::PathListSeparator).clone()) as Box<dyn std::any::Any>]));
            }
        };
        let module_names = std::collections::HashSet::from(["fmt".to_string(), "os".to_string()]);

        let refs = super::collect_external_refs(&file.items, &module_names);

        assert!(
            refs.get("fmt")
                .is_some_and(|roots| roots.contains("Println"))
        );
        assert!(
            refs.get("os")
                .is_some_and(|roots| roots.contains("PathSeparator"))
        );
        assert!(
            refs.get("os")
                .is_some_and(|roots| roots.contains("PathListSeparator"))
        );
    }

    #[test]
    fn prune_items_to_roots_retains_stdlib_constants() {
        let roots = std::collections::HashSet::from([
            "PathSeparator".to_string(),
            "PathListSeparator".to_string(),
        ]);
        let module = crate::resolve::resolve_with_roots("os", &roots).unwrap();
        let mut items = module.content.unwrap().1;
        let module_names = std::collections::HashSet::from(["os".to_string()]);

        super::prune_items_to_roots(&mut items, &roots, &module_names);

        let source = prettyplease::unparse(&syn::File {
            shebang: None,
            attrs: vec![],
            items,
        });
        assert!(source.contains("pub const PathSeparator"), "{source}");
        assert!(source.contains("pub const PathListSeparator"), "{source}");
    }

    #[test]
    fn it_should_compile_struct_type_declaration() {
        test(
            r#"
                package main

                type Point struct {
                    X int
                    Y int
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Point {
                    pub X: isize,
                    pub Y: isize,
                }
            },
        );
    }

    #[test]
    fn it_should_compile_struct_with_mixed_visibility() {
        test(
            r#"
                package main

                type point struct {
                    x int
                    Y int
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                struct point {
                    x: isize,
                    pub Y: isize,
                }
            },
        );
    }

    #[test]
    fn it_should_compile_named_type_definition() {
        test(
            r#"
                package main

                type MyInt int
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq, PartialOrd)]
                pub struct MyInt(pub isize);
                impl std::ops::Deref for MyInt {
                    type Target = isize;
                    fn deref(&self) -> &isize { &self.0 }
                }
                impl std::ops::DerefMut for MyInt {
                    fn deref_mut(&mut self) -> &mut isize { &mut self.0 }
                }
                impl From<MyInt> for isize {
                    fn from(value: MyInt) -> isize { value.0 }
                }
                impl std::ops::BitXorAssign for MyInt {
                    fn bitxor_assign(&mut self, rhs: Self) { self.0 ^= rhs.0; }
                }
                impl std::ops::Shl<i32> for MyInt {
                    type Output = Self;
                    fn shl(self, rhs: i32) -> Self { Self(self.0 << rhs) }
                }
                impl std::ops::Shr<i32> for MyInt {
                    type Output = Self;
                    fn shr(self, rhs: i32) -> Self { Self(self.0 >> rhs) }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_slice_type_alias() {
        test(
            r#"
                package main

                type buffer []byte
            "#,
            rust! {
                #[derive(Clone, Default)]
                struct buffer(pub Vec<u8>);
                impl std::ops::Deref for buffer {
                    type Target = Vec<u8>;
                    fn deref(&self) -> &Vec<u8> { &self.0 }
                }
                impl std::ops::DerefMut for buffer {
                    fn deref_mut(&mut self) -> &mut Vec<u8> { &mut self.0 }
                }
                impl crate::builtin::Len for buffer {
                    fn len_value(&self) -> usize { self.0.len() }
                }
                impl crate::builtin::Cap for buffer {
                    fn cap_value(&self) -> usize { self.0.capacity() }
                }
                impl crate::builtin::StringValue for buffer {
                    fn string_value(self) -> String {
                        String::from_utf8(self.0).unwrap_or_default()
                    }
                }
                impl crate::builtin::StringValue for &buffer {
                    fn string_value(self) -> String {
                        String::from_utf8(self.0.clone()).unwrap_or_default()
                    }
                }
                impl AsRef<[u8]> for buffer {
                    fn as_ref(&self) -> &[u8] { self.0.as_ref() }
                }
                impl AsMut<[u8]> for buffer {
                    fn as_mut(&mut self) -> &mut [u8] { self.0.as_mut() }
                }
                impl From<Vec<u8>> for buffer {
                    fn from(value: Vec<u8>) -> Self { Self(value) }
                }
                impl From<buffer> for Vec<u8> {
                    fn from(value: buffer) -> Self { value.0 }
                }
                impl crate::builtin::Append<u8> for buffer {
                    fn append_value(mut self, elem: u8) -> Self {
                        self.0.push(elem);
                        self
                    }
                }
                impl crate::builtin::Append<Vec<u8>> for buffer {
                    fn append_value(mut self, elem: Vec<u8>) -> Self {
                        self.0.extend(elem);
                        self
                    }
                }
                impl crate::builtin::Append<buffer> for Vec<u8> {
                    fn append_value(mut self, elem: buffer) -> Self {
                        self.extend(elem.0);
                        self
                    }
                }
                impl crate::builtin::Append<String> for buffer {
                    fn append_value(mut self, elem: String) -> Self {
                        self.0.extend(elem.into_bytes());
                        self
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_pointer_type_in_params() {
        test(
            r#"
                package main

                func deref(p *int) int {
                    return *p
                }
            "#,
            rust! {
                fn deref(mut p: &mut isize) -> isize {
                    *p
                }
            },
        );
    }

    #[test]
    fn it_should_compile_struct_with_pointer_field() {
        test(
            r#"
                package main

                type Node struct {
                    Value int
                    Next *Node
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Node {
                    pub Value: isize,
                    pub Next: *mut Node,
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_type() {
        test(
            r#"
                package main

                type Dict map[string]int
            "#,
            rust! {
                #[derive(Clone, Default)]
                pub struct Dict(pub std::collections::HashMap<String, isize>);
                impl std::ops::Deref for Dict {
                    type Target = std::collections::HashMap<String, isize>;
                    fn deref(&self) -> &std::collections::HashMap<String, isize> { &self.0 }
                }
                impl std::ops::DerefMut for Dict {
                    fn deref_mut(&mut self) -> &mut std::collections::HashMap<String, isize> { &mut self.0 }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_index_assignment() {
        test(
            r#"
                package main

                func main() {
                    m := make(map[string]int)
                    m["a"] = 1
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::<String, isize>::new();
                    m.insert("a".to_string(), 1);
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_index_inc_dec() {
        test(
            r#"
                package main

                func main() {
                    m := map[string]int{"a": 1}
                    m["a"]++
                    m["b"]--
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::from([("a".to_string(), 1)]);
                    *m.entry("a".to_string()).or_default() += 1;
                    *m.entry("b".to_string()).or_default() -= 1;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_index_read_with_zero_value_fallback() {
        test(
            r#"
                package main

                func main() {
                    m := map[string]int{"a": 1}
                    _ = m["missing"]
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::from([("a".to_string(), 1)]);
                    let _ = {
                        let __gors_map_key = "missing".to_string();
                        (m).get(&__gors_map_key).cloned().unwrap_or_default()
                    };
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_literal_keys_with_expected_type() {
        test(
            r#"
                package main

                func main() {
                    m := map[string]int{"a": 1}
                    _ = m
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::from([("a".to_string(), 1)]);
                    let _ = m;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_map_comma_ok_key_with_expected_type() {
        test(
            r#"
                package main

                func main() {
                    m := map[string]int{"alice": 25}
                    val, ok := m["alice"]
                    _ = val
                    _ = ok
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::from([("alice".to_string(), 25)]);
                    let (mut val, mut ok) = {
                        let __gors_map_key = "alice".to_string();
                        match (m).get(&__gors_map_key) {
                            Some(__v) => (__v.clone(), true),
                            None => (Default::default(), false),
                        }
                    };
                    let _ = val;
                    let _ = ok;
                }
            },
        );
    }

    #[test]
    fn it_should_map_byte_type() {
        test(
            r#"
                package main

                func f(b byte) byte {
                    return b
                }
            "#,
            rust! {
                fn f(mut b: u8) -> u8 {
                    b
                }
            },
        );
    }

    #[test]
    fn it_should_compile_method_with_pointer_receiver() {
        test(
            r#"
                package main

                type Counter struct {
                    Value int
                }

                func (c *Counter) Increment() {
                    c.Value = c.Value + 1
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Counter {
                    pub Value: isize,
                }
                impl Counter {
                    pub fn Increment(&mut self) {
                        self.Value = self.Value + 1;
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_method_with_value_receiver() {
        test(
            r#"
                package main

                type Point struct {
                    X int
                    Y int
                }

                func (p Point) Sum() int {
                    return p.X + p.Y
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Point {
                    pub X: isize,
                    pub Y: isize,
                }
                impl Point {
                    pub fn Sum(&self) -> isize {
                        self.X + self.Y
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_multiple_return_values() {
        test(
            r#"
                package main

                func divmod(a int, b int) (int, int) {
                    return a / b, a % b
                }
            "#,
            rust! {
                fn divmod(mut a: isize, mut b: isize) -> (isize, isize) {
                    (a / b, a % b)
                }
            },
        );
    }

    #[test]
    fn it_should_compile_multiple_methods_on_same_type() {
        test(
            r#"
                package main

                type Pair struct {
                    A int
                    B int
                }

                func (p *Pair) Sum() int {
                    return p.A + p.B
                }

                func (p *Pair) Swap() {
                    p.A = p.B
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Pair {
                    pub A: isize,
                    pub B: isize,
                }
                impl Pair {
                    pub fn Sum(&mut self) -> isize {
                        self.A + self.B
                    }
                    pub fn Swap(&mut self) {
                        self.A = self.B;
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_struct_literal() {
        test(
            r#"
                package main

                type Point struct {
                    X int
                    Y int
                }

                func main() {
                    p := Point{X: 1, Y: 2}
                    _ = p
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Point {
                    pub X: isize,
                    pub Y: isize,
                }
                pub fn main() {
                    let mut p = Point { X: 1, Y: 2, };
                    let _ = p;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_slice_literal() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2, 3}
                    _ = s
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    let _ = s;
                }
            },
        );
    }

    #[test]
    fn compile_program_multi_borrows_slice_params_written_by_index() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_file(
            tmp.path().join("main.go").as_path(),
            r#"
package main

func writeByte(p []byte) int {
	p[0] = 7
	p[0] += 1
	return len(p)
}

func main() {
	buf := []byte{0}
	writeByte(buf)
}
"#,
        );

        let output = compile_temp_program(tmp.path());
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(
            main_rs.contains("fn writeByte(mut p: &mut Vec<u8>) -> isize"),
            "{main_rs}"
        );
        assert!(main_rs.contains("writeByte(&mut buf);"), "{main_rs}");
        assert!(!main_rs.contains("writeByte(buf.clone());"), "{main_rs}");
    }

    #[test]
    fn it_should_reject_untyped_nil_assignment() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := nil
                    _ = x
                }
            "#,
            "invalid assignment: use of untyped nil in assignment",
        );
    }

    #[test]
    fn it_should_compile_range_with_key_value() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2, 3}
                    for i, v := range s {
                        x := i + v
                        _ = x
                    }
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    for (mut i, mut v) in (s).iter().cloned().enumerate().map(|(i, v)| (i as isize, v)) {
                        let mut x = i + v;
                        let _ = x;
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_range_over_pointer_to_array_without_holding_lock() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    values := [3]int{2, 4, 6}
                    ptr := &values
                    for i, v := range ptr {
                        _ = i
                        _ = v
                    }
                    for i := range ptr {
                        _ = values[i]
                    }
                    for range ptr {
                    }
                }
            "#,
        )
        .unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(output.contains("let __gors_range_values ="), "{output}");
        assert!(
            output.contains("__gors_range_values.into_iter().enumerate()"),
            "{output}"
        );
        assert!(output.contains("let __gors_range_len ="), "{output}");
        assert!(
            !output.contains(".lock().unwrap().iter().cloned().collect::<Vec<_>>().into_iter()")
        );
    }

    #[test]
    fn it_should_compile_range_index_for_generic_slice_alias() {
        test(
            r#"
                package main

                func Index[S ~[]E, E comparable](s S, v E) int {
                    for i := range s {
                        if v == s[i] {
                            return i
                        }
                    }
                    return -1
                }
            "#,
            rust! {
                pub fn Index<E: PartialEq + Clone>(mut s: &mut Vec<E>, mut v: E) -> isize {
                    for mut i in 0..((crate::builtin::len(&s) as isize) as isize) {
                        if v == s[(i) as usize] {
                            return i
                        }
                    }
                    -1
                }
            },
        );
    }

    #[test]
    fn it_should_compile_slice_expression() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2, 3}
                    t := s[1:2]
                    _ = t
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    let mut t = (s[(1) as usize..(2) as usize]).to_vec();
                    let _ = t;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_slice_reassignment_without_moving_bounds() {
        test(
            r#"
                package main

                func main() {
                    s := "abba"
                    s = s[:len(s)-1]
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = "abba".to_string();
                    s = (s[..((crate::builtin::len(&s) as isize) - 1) as usize]).to_string();
                }
            },
        );
    }

    #[test]
    fn it_should_compile_string_add_assign_by_borrowing_rhs() {
        test(
            r#"
                package main

                func main() {
                    s := ""
                    part := "go"
                    s += part
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = "".to_string();
                    let mut part = "go".to_string();
                    {
                        let __gors_string_rhs = (part).clone();
                        s.push_str(&__gors_string_rhs);
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_string_add_assign_literal_by_borrowing_rhs() {
        test(
            r#"
                package main

                func main() {
                    s := ""
                    s += "go"
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = "".to_string();
                    {
                        let __gors_string_rhs = "go".to_string();
                        s.push_str(&__gors_string_rhs);
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_address_of_as_shared_cell() {
        test(
            r#"
                package main

                func main() {
                    x := 42
                    p := &x
                    _ = p
                }
            "#,
            rust! {
                pub fn main() {
                    let mut x = std::sync::Arc::new(std::sync::Mutex::new(42));
                    let mut p = x.clone();
                    let _ = p;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_closure() {
        test(
            r#"
                package main

                func main() {
                    f := func() { x := 1; _ = x }
                    _ = f
                }
            "#,
            rust! {
                pub fn main() {
                    let mut f = {
                        let __gors_func: std::sync::Arc<dyn Fn() -> () + Send + Sync> =
                            std::sync::Arc::new(move || { let mut x = 1; let _ = x; });
                        std::sync::Arc::new(std::sync::Mutex::new(Some(__gors_func)))
                    };
                    let _ = (f).clone();
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_len() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2, 3}
                    n := len(s)
                    _ = n
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    let mut n = (crate::builtin::len(&s) as isize);
                    let _ = n;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_append() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2}
                    s = append(s, 3)
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2]);
                    s = crate::builtin::append(std::mem::take(&mut s), 3);
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_append_multiple_elements() {
        test(
            r#"
                package main

                func main() {
                    s := []int{1, 2}
                    s = append(s, 3, 4)
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2]);
                    s = crate::builtin::append(crate::builtin::append(std::mem::take(&mut s), 3), 4);
                }
            },
        );
    }

    #[test]
    fn it_should_compile_delete_key_with_expected_type() {
        test(
            r#"
                package main

                func main() {
                    m := map[string]int{"a": 1}
                    delete(m, "a")
                }
            "#,
            rust! {
                pub fn main() {
                    let mut m = std::collections::HashMap::from([("a".to_string(), 1)]);
                    {
                        let __gors_delete_key = "a".to_string();
                        crate::builtin::delete(&mut m, &__gors_delete_key)
                    };
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_panic() {
        test(
            r#"
                package main

                func main() {
                    panic("oh no")
                }
            "#,
            rust! {
                pub fn main() {
                    std::panic::panic_any("oh no".to_string());
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_string_max_min() {
        test(
            r#"
                package main

                func main() {
                    println(max("apple", "banana"))
                    println(min("apple", "banana"))
                }
            "#,
            rust! {
                pub fn main() {
                    crate::builtin::println_value(crate::builtin::max("apple".to_string(), "banana".to_string()));
                    crate::builtin::println_value(crate::builtin::min("apple".to_string(), "banana".to_string()));
                }
            },
        );
    }

    #[test]
    fn it_should_follow_references_inside_macro_items() {
        let file: syn::File = rust! {
            pub trait NeededTrait {}

            macro_rules! make_impl {
                ($ty:ty) => {
                    impl NeededTrait for $ty {}
                };
            }

            make_impl!(isize);

            pub fn root<T: NeededTrait>(value: T) {
                let _ = value;
            }
        };
        let roots = std::collections::HashSet::from(["root".to_string()]);
        let module_names = std::collections::HashSet::new();

        let (keep, _, names) = super::reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(names.contains("NeededTrait"));
        assert!(names.contains("make_impl"));
        assert_eq!(keep.len(), 4);
    }

    #[test]
    fn it_should_preserve_runtime_interface_hooks_during_dce() {
        let file: syn::File = rust! {
            pub trait Interface {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                fn Len(&mut self) -> isize;
            }

            pub struct Values;

            impl Interface for Values {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn Len(&mut self) -> isize {
                    0
                }
            }

            pub fn root(mut value: Values) -> isize {
                Interface::Len(&mut value)
            }
        };
        let roots = std::collections::HashSet::from(["root".to_string()]);
        let module_names = std::collections::HashSet::new();

        let (_, _, names) = super::reachable_stdlib_items(&file.items, &roots, &module_names);
        let item_names = super::item_reachability_names(&file.items);
        let top_level_names = super::top_level_item_names(&file.items);

        let trait_item = file
            .items
            .iter()
            .find(|item| matches!(item, syn::Item::Trait(_)))
            .and_then(|item| {
                super::reachable_item_for_names(item, &names, &item_names, &top_level_names)
            })
            .and_then(|item| match item {
                syn::Item::Trait(item_trait) => Some(item_trait),
                _ => None,
            })
            .expect("expected trait");
        assert!(trait_item.items.iter().any(|item| {
            matches!(item, syn::TraitItem::Fn(func) if func.sig.ident == "__gors_as_any")
        }));

        let impl_item = file
            .items
            .iter()
            .find(|item| matches!(item, syn::Item::Impl(_)))
            .and_then(|item| {
                super::reachable_item_for_names(item, &names, &item_names, &top_level_names)
            })
            .and_then(|item| match item {
                syn::Item::Impl(item_impl) => Some(item_impl),
                _ => None,
            })
            .expect("expected impl");
        assert!(impl_item.items.iter().any(|item| {
            matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == "__gors_as_any")
        }));
    }

    #[test]
    fn it_should_collect_external_methods_called_on_associated_constructors() {
        let mut item: syn::Item = rust! {
            pub fn main() {
                let mut values = Vec::from([3.5e0, 1.25e0]);
                sort::Float64Slice::from(values).Less(1, 0);
            }
        };
        let module_names = std::collections::HashSet::from(["sort".to_string()]);
        let item_names = std::collections::HashSet::new();
        let top_level_names = std::collections::HashSet::new();
        let top_level_types = std::collections::HashMap::new();
        let top_level_field_types = std::collections::HashMap::new();
        let top_level_return_types = std::collections::HashMap::new();
        let top_level_tuple_return_types = std::collections::HashMap::new();

        let context = super::RefCollectionContext {
            module_names: &module_names,
            item_names: &item_names,
            top_level_names: &top_level_names,
            top_level_types: &top_level_types,
            top_level_field_types: &top_level_field_types,
            top_level_return_types: &top_level_return_types,
            top_level_tuple_return_types: &top_level_tuple_return_types,
        };
        let (_, refs) = super::collect_refs_from_item(&mut item, &context);

        let sort_refs = refs.get("sort").expect("sort refs");
        assert!(sort_refs.contains("Float64Slice"));
        assert!(sort_refs.contains("Float64Slice::Less"));
    }

    #[test]
    fn it_should_compile_builtin_println() {
        test(
            r#"
                package main

                func main() {
                    println("hello")
                }
            "#,
            rust! {
                pub fn main() {
                    crate::builtin::println_value("hello");
                }
            },
        );
    }

    #[test]
    fn it_should_compile_builtin_println_multiple_arguments() {
        test(
            r#"
                package main

                func main() {
                    println(1, "seen")
                }
            "#,
            rust! {
                pub fn main() {
                    {
                        crate::builtin::print_value(1);
                        crate::builtin::print_value(" ");
                        crate::builtin::println_value("seen");
                    };
                }
            },
        );
    }

    #[test]
    fn it_should_box_numeric_constants_as_go_types_for_any() {
        test(
            r#"
                package main

                func main() {
                    var x any = 42
                    _ = x
                }
            "#,
            rust! {
                pub fn main() {
                    let mut x: Box<dyn std::any::Any> = Box::new((42 as isize)) as Box<dyn std::any::Any>;
                    let _ = x;
                }
            },
        );
    }

    #[test]
    fn it_should_compile_multiple_return_values_with_error() {
        test(
            r#"
                package main

                func divide(a int, b int) (int, bool) {
                    if b == 0 {
                        return 0, false
                    }
                    return a / b, true
                }
            "#,
            rust! {
                fn divide(mut a: isize, mut b: isize) -> (isize, bool) {
                    if b == 0 {
                        return (0, false)
                    }
                    (a / b, true)
                }
            },
        );
    }

    #[test]
    fn it_should_compile_defer_statement() {
        test(
            r#"
                package main

                func cleanup() {}

                func main() {
                    defer cleanup()
                }
            "#,
            rust! {
                fn cleanup() {}
                pub fn main() {
                    let mut __gors_defer_stack = {
                        struct __GorsDeferStack(Vec<Box<dyn FnOnce()>>);
                        impl Drop for __GorsDeferStack {
                            fn drop(&mut self) {
                                while let Some(__gors_defer) = self.0.pop() {
                                    __gors_defer();
                                }
                            }
                        }
                        __GorsDeferStack(Vec::new())
                    };
                    {
                        __gors_defer_stack.0.push(Box::new(move || { cleanup(); }));
                    }
                }
            },
        );
    }

    #[test]
    fn it_should_compile_interface_type_declaration() {
        test(
            r#"
                package main

                type Stringer interface {}
            "#,
            rust! {
                pub trait Stringer {}
            },
        );
    }

    #[test]
    fn it_should_compile_empty_struct_literal() {
        test(
            r#"
                package main

                type Flags struct {
                    X bool
                }

                func main() {
                    f := Flags{}
                    _ = f
                }
            "#,
            rust! {
                #[derive(Clone, Copy, Default, PartialEq)]
                pub struct Flags {
                    pub X: bool,
                }
                pub fn main() {
                    let mut f = Flags::default();
                    let _ = (f).clone();
                }
            },
        );
    }

    // --- Iota + Const tests (Agent 2) ---

    #[test]
    fn it_should_support_iota_basic() {
        test(
            r#"
                package main

                const (
                    Red = iota
                    Green
                    Blue
                )

                func main() {}
            "#,
            rust! {
                pub const Red: isize = 0;
                pub const Green: isize = 1;
                pub const Blue: isize = 2;
                pub fn main() {}
            },
        )
    }

    #[test]
    fn it_should_support_iota_expression() {
        test(
            r#"
                package main

                const (
                    A = iota * 2
                    B
                    C
                )

                func main() {}
            "#,
            rust! {
                pub const A: isize = 0;
                pub const B: isize = 2;
                pub const C: isize = 4;
                pub fn main() {}
            },
        )
    }

    #[test]
    fn it_should_support_blank_identifier_in_const() {
        test(
            r#"
                package main

                const (
                    _ = iota
                    KB = 1 << (10 * iota)
                    MB
                )

                func main() {}
            "#,
            rust! {
                pub const KB: isize = 1024;
                pub const MB: isize = 1048576;
                pub fn main() {}
            },
        )
    }

    #[test]
    fn it_should_evaluate_constant_builtin_calls() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                const (
                    S = "gors"
                    StringLen = len(S)
                    ArrayLen = len([3]int{})
                    ArrayCap = cap(&[4]string{})
                    InferredLen = len([...]int{2: 1})
                    ImagArrayLen = len([10]float64{imag(2i)})
                    ImagPart = int(imag(2i))
                    RealPart = int(real(complex(3, 4)))
                    MaxPart = max(1, 4, 2)
                )

                func main() {}
            "#,
        )
        .unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(
            output.contains("pub const StringLen: isize = 4;"),
            "{output}"
        );
        assert!(
            output.contains("pub const ArrayLen: isize = 3;"),
            "{output}"
        );
        assert!(
            output.contains("pub const ArrayCap: isize = 4;"),
            "{output}"
        );
        assert!(
            output.contains("pub const InferredLen: isize = 3;"),
            "{output}"
        );
        assert!(
            output.contains("pub const ImagArrayLen: isize = 10;"),
            "{output}"
        );
        assert!(
            output.contains("pub const ImagPart: isize = 2;"),
            "{output}"
        );
        assert!(
            output.contains("pub const RealPart: isize = 3;"),
            "{output}"
        );
        assert!(output.contains("pub const MaxPart: isize = 4;"), "{output}");
    }

    #[test]
    fn it_should_evaluate_len_of_array_struct_fields() {
        let parsed = parse_file(
            "test.go",
            r#"
                package main

                type dirent struct {
                    name [256]byte
                }

                const NameLen = len(dirent{}.name)

                var _ [len(dirent{}.name)]int

                func main() {}
            "#,
        )
        .unwrap();
        let compiled = compile(parsed).unwrap();
        let output = printer::generate(compiled).unwrap();

        assert!(
            output.contains("pub const NameLen: isize = 256;"),
            "{output}"
        );
        assert!(output.contains("[isize; 256]"), "{output}");
    }

    #[test]
    fn it_should_preserve_const_exprs_with_forward_refs() {
        test(
            r#"
                package main

                const (
                    Mask = 1 << -Ident
                    Combo = Mask | Other
                )

                const (
                    EOF = -(iota + 1)
                    Ident
                    Other
                )

                func main() {}
            "#,
            rust! {
                pub const Mask: isize = (1 as isize) << -(Ident as isize);
                pub const Combo: isize = (Mask as isize) | (Other as isize);
                pub const EOF: isize = -1;
                pub const Ident: isize = -2;
                pub const Other: isize = -3;
                pub fn main() {}
            },
        )
    }

    #[test]
    fn it_should_widen_large_untyped_integer_constants() {
        test(
            r#"
                package main

                const Huge = 0xFFF0000000000000

                func main() {}
            "#,
            rust! {
                pub const Huge: u64 = 18442240474082181120;
                pub fn main() {}
            },
        )
    }

    #[test]
    fn it_should_support_blank_identifier_in_define() {
        test(
            r#"
                package main

                func main() {
                    _, b := 1, 2
                    _ = b
                }
            "#,
            rust! {
                pub fn main() {
                    let (_, mut b) = (1, 2);
                    let _ = b;
                }
            },
        )
    }

    #[test]
    fn it_should_reject_blank_identifier_as_value() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    println(_)
                }
            "#,
            "cannot use _ as value or type",
        );
    }

    #[test]
    fn it_should_reject_unused_local_variables_in_single_file_compile() {
        assert_unsupported_construct(
            r#"
                package main

                func main() {
                    x := 1
                }
            "#,
            "declared and not used: x",
        );
    }

    #[test]
    fn it_should_reject_unused_imports_in_single_file_compile() {
        assert_unsupported_construct(
            r#"
                package main

                import "fmt"

                func main() {}
            "#,
            "fmt imported and not used",
        );
    }

    #[test]
    fn it_should_validate_single_file_default_import_package_names() {
        assert_unsupported_construct(
            r#"
                package main

                import (
                    "crypto/rand"
                    "math/rand/v2"
                )

                func main() {}
            "#,
            "duplicate import name rand",
        );
    }

    // --- Concurrency tests (Agent 3) ---

    /// Helper that compiles Go source and generates Rust source code.
    fn go_to_rust(go_input: &str) -> String {
        let parsed = parse_file("test.go", go_input).unwrap();
        let compiled = compile(parsed).unwrap();
        printer::generate(compiled).unwrap()
    }

    #[test]
    fn it_should_compile_channel_send() {
        let rust_src = go_to_rust(
            r#"
            package main
            func main() {
                ch := make(chan int)
                ch <- 42
            }
            "#,
        );
        assert!(
            rust_src.contains("make_chan"),
            "Expected make_chan in output:\n{}",
            rust_src
        );
        assert!(
            rust_src.contains(".send(42)"),
            "Expected .send(42) in output:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_channel_recv() {
        let rust_src = go_to_rust(
            r#"
            package main
            func main() {
                ch := make(chan int)
                v := <-ch
                _ = v
            }
            "#,
        );
        assert!(
            rust_src.contains(".recv()"),
            "Expected .recv() in output:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_buffered_channel() {
        let rust_src = go_to_rust(
            r#"
            package main
            func main() {
                ch := make(chan int, 5)
                _ = ch
            }
            "#,
        );
        assert!(
            rust_src.contains("make_chan(5)"),
            "Expected make_chan(5) in output:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_empty_select_as_blocking() {
        let rust_src = go_to_rust(
            r#"
            package main
            func main() {
                select {}
            }
            "#,
        );
        assert!(
            rust_src.contains("std::thread::park();"),
            "Expected empty select to park forever:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_multi_case_select_with_block_body() {
        let rust_src = go_to_rust(
            r#"
            package main

            func main() {
                ch := make(chan int, 1)
                ch <- 1
                select {
                case <-ch:
                    {
                        println("first")
                    }
                case <-ch:
                    println("second")
                default:
                    {
                        println("default")
                    }
                }
            }
            "#,
        );
        assert!(
            rust_src.contains("try_recv"),
            "Expected try_recv in output:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_go_stmt_with_func_lit() {
        let rust_src = go_to_rust(
            r#"
            package main
            import "fmt"
            func main() {
                go func() {
                    fmt.Println("hello")
                }()
            }
            "#,
        );
        assert!(
            rust_src.contains("spawn"),
            "Expected spawn in output:\n{}",
            rust_src
        );
        assert!(
            rust_src.contains("move ||"),
            "Expected move || in output:\n{}",
            rust_src
        );
    }

    #[test]
    fn it_should_compile_go_stmt_with_named_func() {
        let rust_src = go_to_rust(
            r#"
            package main
            func work() {}
            func main() {
                go work()
            }
            "#,
        );
        assert!(
            rust_src.contains("spawn"),
            "Expected spawn in output:\n{}",
            rust_src
        );
    }

    // --- Pointer + Named Return tests (Agent 4) ---

    #[test]
    fn it_should_support_named_return_values() {
        test(
            r#"
                package main

                func swap(a int, b int) (x int, y int) {
                    x = b
                    y = a
                    return
                }
            "#,
            rust! {
                fn swap(mut a: isize, mut b: isize) -> (isize, isize) {
                    let mut x: isize = 0;
                    let mut y: isize = 0;
                    '__gors_named_return_0: {
                        x = b;
                        y = a;
                        {
                            break '__gors_named_return_0;
                        }
                    };
                    (x, y)
                }
            },
        );
    }

    #[test]
    fn it_should_support_pointer_types_and_address_of() {
        test(
            r#"
                package main

                func newInt(x int) *int {
                    return &x
                }
            "#,
            rust! {
                fn newInt(mut x: isize) -> std::sync::Arc<std::sync::Mutex<isize>> {
                    std::sync::Arc::new(std::sync::Mutex::new(x))
                }
            },
        );
    }

    #[test]
    fn it_should_support_pointer_deref_lvalue_assignment() {
        test(
            r#"
                package main

                func main() {
                    p := new(int)
                    *p = 2
                    (*p)++
                }
            "#,
            rust! {
                pub fn main() {
                    let mut p = std::sync::Arc::new(std::sync::Mutex::new(<isize>::default()));
                    *p.lock().unwrap() = 2;
                    *p.lock().unwrap() += 1;
                }
            },
        );
    }

    // --- Generics tests (Agent 5) ---

    #[test]
    fn it_should_compile_generic_function() {
        test(
            r#"
                package main

                func Identity[T any](x T) T {
                    return x
                }

                func main() {}
            "#,
            rust! {
                pub fn Identity<T>(mut x: T) -> T {
                    (x).clone()
                }
                pub fn main() {}
            },
        );
    }

    #[test]
    fn it_should_compile_generic_function_with_constraint() {
        test(
            r#"
                package main

                func Max[T int | float64](a T, b T) T {
                    if a > b {
                        return a
                    }
                    return b
                }

                func main() {}
            "#,
            rust! {
                pub fn Max<T: PartialEq + PartialOrd + Clone>(mut a: T, mut b: T) -> T {
                    if a > b {
                        return (a).clone()
                    }
                    (b).clone()
                }
                pub fn main() {}
            },
        );
    }
}
