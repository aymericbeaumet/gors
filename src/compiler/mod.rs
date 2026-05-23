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
//! - Defer statements
//! - Goto statements
//! - Some complex type expressions

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
use std::time::Instant;
use syn::Token;

// Thread-local storage for source map tracker during compilation
thread_local! {
    static TRACKER: RefCell<SourceMapTracker> = RefCell::new(SourceMapTracker::new());
    static DEFER_COUNTER: RefCell<usize> = const { RefCell::new(0) };
    static IMPORT_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static IMPORT_RENAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static INTERFACE_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static TYPE_ENV: RefCell<typeinfer::TypeEnv> = RefCell::new(typeinfer::TypeEnv::new());
    static STRING_CONST_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
    static UNNAMED_ARG_COUNTER: RefCell<usize> = const { RefCell::new(0) };
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

/// Record a mapping if tracking is enabled.
fn record_mapping(pos: &token::Position, name: Option<&str>) {
    TRACKER.with(|t| {
        t.borrow_mut()
            .record(pos.line as u32, pos.column as u32, name);
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
        ast::Expr::BinaryExpr(bin) if bin.op == token::Token::OR => {
            // Union type like `int | float64` → approximate with common traits
            bounds.push(syn::parse_quote! { PartialOrd });
            bounds.push(syn::parse_quote! { Copy });
            bounds.push(syn::parse_quote! { std::fmt::Display });
        }
        _ => {
            // Fallback: no bounds
        }
    }

    bounds
}

fn compile_go_type_params(type_params: Option<ast::FieldList>) -> syn::Generics {
    let Some(type_params) = type_params else {
        return syn::Generics::default();
    };

    let mut params = syn::punctuated::Punctuated::new();
    for field in type_params.list {
        let bounds = field
            .type_
            .as_ref()
            .map(go_constraint_to_rust_bounds)
            .unwrap_or_default();
        if let Some(names) = field.names {
            for name in names {
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

/// Attempt to evaluate a Go constant expression at compile time, substituting `iota` with the
/// given value. Returns `Some(value)` if fully evaluable, `None` otherwise.
fn const_eval_expr(expr: &ast::Expr, iota_value: i64) -> Option<i64> {
    match expr {
        ast::Expr::BasicLit(lit) => {
            if lit.kind == token::Token::INT {
                let s = lit.value;
                if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    i64::from_str_radix(hex, 16).ok()
                } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                    i64::from_str_radix(bin, 2).ok()
                } else if let Some(oct) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                    i64::from_str_radix(oct, 8).ok()
                } else {
                    s.parse::<i64>().ok()
                }
            } else {
                None
            }
        }
        ast::Expr::Ident(ident) if ident.name == "iota" => Some(iota_value),
        ast::Expr::BinaryExpr(bin) => {
            let lhs = const_eval_expr(&bin.x, iota_value)?;
            let rhs = const_eval_expr(&bin.y, iota_value)?;
            match bin.op {
                token::Token::ADD => lhs.checked_add(rhs),
                token::Token::SUB => lhs.checked_sub(rhs),
                token::Token::MUL => lhs.checked_mul(rhs),
                token::Token::QUO => {
                    if rhs == 0 {
                        None
                    } else {
                        lhs.checked_div(rhs)
                    }
                }
                token::Token::REM => {
                    if rhs == 0 {
                        None
                    } else {
                        lhs.checked_rem(rhs)
                    }
                }
                token::Token::SHL => u32::try_from(rhs).ok().and_then(|rhs| lhs.checked_shl(rhs)),
                token::Token::SHR => u32::try_from(rhs).ok().and_then(|rhs| lhs.checked_shr(rhs)),
                token::Token::AND => Some(lhs & rhs),
                token::Token::AND_NOT => Some(lhs & !rhs),
                token::Token::OR => Some(lhs | rhs),
                token::Token::XOR => Some(lhs ^ rhs),
                _ => None,
            }
        }
        ast::Expr::ParenExpr(paren) => const_eval_expr(&paren.x, iota_value),
        ast::Expr::UnaryExpr(unary) => {
            let val = const_eval_expr(&unary.x, iota_value)?;
            match unary.op {
                token::Token::SUB => val.checked_neg(),
                token::Token::ADD => Some(val),
                token::Token::XOR => Some(!val),
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

        let inferred_from_value = source_values.and_then(|vals| {
            vals.first().and_then(|expr| match expr {
                ast::Expr::BasicLit(lit) if lit.kind == token::Token::STRING => {
                    Some(syn::parse_quote! { &str })
                }
                ast::Expr::BasicLit(lit) if lit.kind == token::Token::FLOAT => {
                    Some(syn::parse_quote! { f64 })
                }
                ast::Expr::Ident(id) if id.name == "true" || id.name == "false" => {
                    Some(syn::parse_quote! { bool })
                }
                _ => None,
            })
        });

        let rust_type: syn::Type = if let Some(name) = type_name_str {
            let type_ident = syn::Ident::new(&import_rust_name(name), Span::mixed_site());
            syn::parse_quote! { #type_ident }
        } else if let Some(ty) = inferred_from_value {
            ty
        } else {
            syn::parse_quote! { isize }
        };

        for (name_idx, name) in value_spec.names.iter().enumerate() {
            if name.name == "_" {
                continue;
            }

            let vis: syn::Visibility = name.into();
            let ident = syn::Ident::new(&import_rust_name(name.name), Span::mixed_site());

            let value_expr = source_values.and_then(|vals| vals.get(name_idx));

            let value: syn::Expr = if let Some(expr) = value_expr {
                if let Some(evaluated) = const_eval_expr(expr, iota as i64) {
                    let lit = syn::LitInt::new(&evaluated.to_string(), Span::mixed_site());
                    syn::parse_quote! { #lit }
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
                            let f = syn::LitFloat::new(lit.value, Span::mixed_site());
                            syn::parse_quote! { #f }
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
                            if is_string_const_fn(id.name) {
                                syn::parse_quote! { #id_ident() }
                            } else {
                                syn::parse_quote! { #id_ident }
                            }
                        }
                    }
                } else {
                    syn::parse_quote! { 0 }
                }
            } else {
                syn::parse_quote! { 0 }
            };

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
        }
    }
    Ok(items)
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

/// Rewrite bare return statements in a block to return a tuple of the named variables.
fn rewrite_bare_returns(block: &mut syn::Block, named_return_idents: &[syn::Ident]) {
    for stmt in &mut block.stmts {
        if let syn::Stmt::Expr(expr, _semi) = stmt {
            rewrite_bare_returns_in_expr(expr, named_return_idents);
        }
    }
}

fn rewrite_bare_returns_in_expr(expr: &mut syn::Expr, named_return_idents: &[syn::Ident]) {
    match expr {
        syn::Expr::Return(ret) if ret.expr.is_none() => {
            if let Some(ident) = (named_return_idents.len() == 1)
                .then(|| named_return_idents.first())
                .flatten()
            {
                ret.expr = Some(Box::new(syn::parse_quote! { #ident }));
            } else {
                let idents = named_return_idents;
                ret.expr = Some(Box::new(syn::parse_quote! { (#(#idents),*) }));
            }
        }
        syn::Expr::Block(block) => {
            rewrite_bare_returns(&mut block.block, named_return_idents);
        }
        syn::Expr::If(if_expr) => {
            rewrite_bare_returns(&mut if_expr.then_branch, named_return_idents);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_bare_returns_in_expr(else_expr, named_return_idents);
            }
        }
        syn::Expr::Loop(loop_expr) => {
            rewrite_bare_returns(&mut loop_expr.body, named_return_idents);
        }
        syn::Expr::While(while_expr) => {
            rewrite_bare_returns(&mut while_expr.body, named_return_idents);
        }
        syn::Expr::ForLoop(for_expr) => {
            rewrite_bare_returns(&mut for_expr.body, named_return_idents);
        }
        _ => {}
    }
}

/// Extract identifiers from a syn::Block that should be cloned before being
/// moved into a goroutine closure. Returns `let name = name.clone();` statements.
fn extract_idents_for_clone(block: &syn::Block) -> Vec<syn::Stmt> {
    use std::collections::BTreeSet;

    struct IdentCollector {
        idents: BTreeSet<String>,
        locals: BTreeSet<String>,
    }

    impl IdentCollector {
        fn visit_expr(&mut self, expr: &syn::Expr) {
            match expr {
                syn::Expr::MethodCall(mc) => {
                    if let syn::Expr::Path(path) = mc.receiver.as_ref() {
                        if let Some(segment) = path.path.segments.first()
                            && path.path.segments.len() == 1
                        {
                            let name = segment.ident.to_string();
                            if !self.locals.contains(&name) {
                                self.idents.insert(name);
                            }
                        }
                    }
                    self.visit_expr(&mc.receiver);
                    for arg in &mc.args {
                        self.visit_expr(arg);
                    }
                }
                syn::Expr::Call(call) => {
                    self.visit_expr(&call.func);
                    for arg in &call.args {
                        self.visit_expr(arg);
                    }
                }
                syn::Expr::Path(path) if path.path.segments.len() == 1 => {
                    let Some(name) = path.path.segments.first().map(|seg| seg.ident.to_string())
                    else {
                        return;
                    };
                    if !self.locals.contains(&name) {
                        self.idents.insert(name);
                    }
                }
                syn::Expr::Binary(binary) => {
                    self.visit_expr(&binary.left);
                    self.visit_expr(&binary.right);
                }
                _ => {}
            }
        }

        fn visit_stmt(&mut self, stmt: &syn::Stmt) {
            match stmt {
                syn::Stmt::Expr(expr, _) => self.visit_expr(expr),
                syn::Stmt::Local(local) => {
                    if let syn::Pat::Ident(pat_ident) = &local.pat {
                        self.locals.insert(pat_ident.ident.to_string());
                    }
                    if let Some(init) = &local.init {
                        self.visit_expr(&init.expr);
                    }
                }
                _ => {}
            }
        }
    }

    let mut collector = IdentCollector {
        idents: BTreeSet::new(),
        locals: BTreeSet::new(),
    };
    for stmt in &block.stmts {
        collector.visit_stmt(stmt);
    }

    let skip = [
        "println",
        "print",
        "eprintln",
        "make_chan",
        "spawn",
        "true",
        "false",
    ];
    collector
        .idents
        .into_iter()
        .filter(|name| !skip.contains(&name.as_str()))
        .map(|name| {
            let ident = syn::Ident::new(&name, Span::mixed_site());
            syn::parse_quote! { let #ident = #ident.clone(); }
        })
        .collect()
}

/// Compile a Go select statement into Rust.
fn compile_select_stmt(select_stmt: ast::SelectStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
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
            let mut stmts = vec![];
            for stmt in body {
                stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
            }
            return Ok(stmts);
        }
        return Ok(vec![]);
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

        let mut case_body_stmts = vec![];
        for stmt in case.body {
            case_body_stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }

        if let Some(default) = default_body.take() {
            // Non-blocking: try_recv/try_send with fallback
            let mut default_stmts = vec![];
            for stmt in default {
                default_stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
            }

            match comm {
                ast::Stmt::ExprStmt(expr_stmt) => {
                    // <-ch in expression position
                    if let Some(ch) = extract_channel_recv(expr_stmt.x) {
                        return Ok(vec![syn::Stmt::Expr(
                            syn::parse_quote! {
                                if let Ok(_v) = #ch.try_recv() {
                                    #(#case_body_stmts)*
                                } else {
                                    #(#default_stmts)*
                                }
                            },
                            None,
                        )]);
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
                            return Ok(vec![syn::Stmt::Expr(
                                syn::parse_quote! {
                                    if let Ok(#lhs_pat) = #ch.try_recv() {
                                        #(#case_body_stmts)*
                                    } else {
                                        #(#default_stmts)*
                                    }
                                },
                                None,
                            )]);
                        }
                    }
                }
                ast::Stmt::SendStmt(send) => {
                    let ch: syn::Expr = send.chan.into();
                    let val: syn::Expr = send.value.into();
                    return Ok(vec![syn::Stmt::Expr(
                        syn::parse_quote! {
                            if #ch.try_send(#val).is_ok() {
                                #(#case_body_stmts)*
                            } else {
                                #(#default_stmts)*
                            }
                        },
                        None,
                    )]);
                }
                _ => {}
            }
        } else {
            // Blocking single case
            let comm_stmts: Vec<syn::Stmt> = Vec::<syn::Stmt>::try_from(comm)?;
            let mut all_stmts = comm_stmts;
            all_stmts.extend(case_body_stmts);
            return Ok(all_stmts);
        }
    }

    // Multiple cases: generate loop with try_recv checks
    let mut arms: Vec<proc_macro2::TokenStream> = Vec::new();
    for case in cases {
        let Some(comm) = case.comm.map(|c| *c) else {
            continue;
        };
        let mut body_stmts = vec![];
        for stmt in case.body {
            body_stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }

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
        let mut stmts = vec![];
        for stmt in body {
            stmts.extend(Vec::<syn::Stmt>::try_from(stmt)?);
        }
        quote::quote! {
            #(#stmts)*
            break;
        }
    } else {
        quote::quote! {
            std::thread::yield_now();
        }
    };

    Ok(vec![syn::Stmt::Expr(
        syn::parse_quote! {
            loop {
                #(#arms)*
                #default_arm
            }
        },
        None,
    )])
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
    set_import_renames(BTreeMap::new());
    // Pre-scan the AST to build a type environment
    let mut type_env = typeinfer::TypeEnv::new();
    type_env.scan_file(&file);
    set_type_env(type_env);
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
    set_import_renames(import_renames);
    set_type_env(type_env);
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

/// Compile a parsed program (main package + imports) into a single Rust file.
///
/// Imported packages are emitted as `mod` blocks before the main package items.
pub fn compile_program(program: crate::parser::ParsedProgram) -> Result<syn::File, CompilerError> {
    let mut all_items: Vec<syn::Item> = Vec::new();

    for stdlib_path in &program.stdlib_imports {
        if let Some(stdlib_mod) = crate::go_stdlib::resolve(stdlib_path) {
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
                .map(|path| crate::go_stdlib::module_name(path)),
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
        if crate::go_stdlib::is_known(import_path)
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
    compile_program_impl(program, Some((go_file, go_source)))
}

fn compile_program_impl(
    program: crate::parser::ParsedProgram,
    source_map_config: Option<(&str, &str)>,
) -> Result<CompiledProgram, CompilerError> {
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

    let builtins_file: syn::File =
        syn::parse_str(crate::backend_rust::GORS_BUILTINS).map_err(|e| {
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

    for stdlib_path in &stdlib_imports {
        if let Some((package_name, env)) = crate::go_stdlib::scan_type_env(stdlib_path) {
            stdlib_type_envs.insert(stdlib_path.clone(), (package_name, env));
        }
    }

    let stdlib_mod_names: std::collections::HashSet<String> =
        std::iter::once("builtin".to_string())
            .chain(
                crate::go_stdlib::list_packages()
                    .into_iter()
                    .map(|path| crate::go_stdlib::module_name(&path)),
            )
            .chain(
                stdlib_imports
                    .iter()
                    .map(|path| crate::go_stdlib::module_name(path)),
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
        .map(|path| (path.clone(), crate::go_stdlib::module_name(path)))
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
            .unwrap_or_else(typeinfer::TypeEnv::new);
        merge_import_type_envs(&mut type_env, &pkg.ast, &local_type_envs, &stdlib_type_envs);
        let import_rewrites = import_module_rewrites(
            &pkg.ast,
            &local_type_envs,
            &local_module_names,
            &stdlib_type_envs,
            &stdlib_module_names,
        );
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

    if let Some((go_file, go_source)) = source_map_config {
        TRACKER.with(|t| {
            t.borrow_mut().start(go_file, "output.rs", Some(go_source));
        });
    }

    let has_main_fn = program.main_package.name == "main"
        && program
            .main_package
            .ast
            .decls
            .iter()
            .any(|d| matches!(d, ast::Decl::FuncDecl(f) if f.name.name == "main"));

    let main_hash = compute_content_hash(&program.main_package.files);
    let mut main_type_env = typeinfer::TypeEnv::new();
    main_type_env.scan_file(&program.main_package.ast);
    merge_import_type_envs(
        &mut main_type_env,
        &program.main_package.ast,
        &local_type_envs,
        &stdlib_type_envs,
    );
    let main_import_rewrites = import_module_rewrites(
        &program.main_package.ast,
        &local_type_envs,
        &local_module_names,
        &stdlib_type_envs,
        &stdlib_module_names,
    );
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

    let dce_timer = ProfileTimer::start("compiler.dce");
    prune_generated_dead_code(&mut modules, has_main_fn);
    inject_post_prune_stdlib_helpers(&mut modules, &stdlib_imports);
    prune_generated_dead_code(&mut modules, has_main_fn);
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
            "reflect" => {
                if module
                    .file
                    .items
                    .iter()
                    .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == "Value"))
                {
                    module.file.items = vec![syn::parse_quote! {
                        #[derive(Clone, Default)]
                        pub struct Value;
                    }];
                    module.content_hash = String::new();
                }
            }
            "os" => {
                if module
                    .file
                    .items
                    .iter()
                    .any(|item| matches!(item, syn::Item::Static(item_static) if item_static.ident == "Stdout"))
                {
                    module.file.items = vec![
                        syn::parse_quote! {
                            #[derive(Clone, Copy, Default)]
                            pub struct File;
                        },
                        syn::parse_quote! {
                            #[allow(non_upper_case_globals)]
                            pub static Stdout: File = File;
                        },
                        syn::parse_quote! {
                            impl crate::io::Writer for File {
                                fn Write(&mut self, b: Vec<u8>) -> (isize, String) {
                                    let mut stdout = std::io::stdout();
                                    match std::io::Write::write_all(&mut stdout, &b) {
                                        Ok(()) => (b.len() as isize, String::new()),
                                        Err(err) => (0, err.to_string()),
                                    }
                                }
                            }
                        },
                    ];
                    module.content_hash = String::new();
                }
            }
            _ => {}
        }
    }
    let mut preserved = std::collections::HashSet::from(["builtin".to_string()]);
    preserved.extend(roots.iter().map(|root| crate::go_stdlib::module_name(root)));
    prune_unreferenced_stdlib_modules(modules, &preserved);
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
                let (_, refs, _) = reachable_stdlib_items(&module.file.items, roots, &module_names);
                changed |= merge_required_refs(&mut required, refs);
            }
            if !changed {
                break;
            }
        }

        let removable: Vec<String> = modules
            .iter()
            .filter_map(|(key, module)| {
                (!module.is_main
                    && !required
                        .get(&module.mod_name)
                        .is_some_and(|roots| !roots.is_empty()))
                .then(|| key.clone())
            })
            .collect();
        for key in removable {
            modules.remove(&key);
        }

        for module in modules.values_mut().filter(|module| !module.is_main) {
            let Some(roots) = required.get(&module.mod_name) else {
                continue;
            };
            prune_items_to_roots(&mut module.file.items, roots, &module_names);
            if module.mod_name == "builtin" {
                prune_builtin_channel_helpers(&mut module.file.items, roots);
                prune_unneeded_builtin_traits(&mut module.file.items, roots);
            } else if let Some(builtin_roots) = required.get("builtin") {
                prune_unneeded_builtin_traits(&mut module.file.items, builtin_roots);
            }
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
            .filter_map(|(key, module)| {
                (!module.is_main && module.file.items.is_empty()).then(|| key.clone())
            })
            .collect();
        for key in empty_modules {
            modules.remove(&key);
        }

        if modules_reachability_fingerprint(modules) == before {
            break;
        }
    }
}

fn prune_items_to_roots(
    items: &mut Vec<syn::Item>,
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) {
    let (keep, _, names) = reachable_stdlib_items(items, roots, module_names);
    let item_names = item_reachability_names(items);
    *items = items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            keep.contains(&idx)
                .then(|| reachable_item_for_names(item, &names, &item_names))
                .flatten()
        })
        .collect();
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
            "GoChan"
                | "GoChanIter"
                | "ChanInner"
                | "make_chan"
                | "close"
                | "send"
                | "recv"
                | "recv_with_ok"
                | "GoChan::send"
                | "GoChan::recv"
                | "GoChan::recv_with_ok"
                | "GoChan::len"
                | "GoChan::cap"
        )
    }) {
        return;
    }

    let channel_names = std::collections::HashSet::from([
        "GoChan".to_string(),
        "GoChanIter".to_string(),
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
        "GoAppend" => Some("append"),
        "GoCap" => Some("cap"),
        "GoLen" => Some("len"),
        "GoString" => Some("go_string"),
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

fn merge_import_type_envs(
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

    let mut import_path_by_module: HashMap<String, String> = crate::go_stdlib::list_packages()
        .into_iter()
        .map(|path| (crate::go_stdlib::module_name(&path), path))
        .collect();
    import_path_by_module.remove("builtin");
    for path in roots {
        import_path_by_module
            .entry(crate::go_stdlib::module_name(path))
            .or_insert_with(|| path.clone());
    }
    let mut stdlib_mod_names: HashSet<String> = import_path_by_module.keys().cloned().collect();
    for module in modules.values().filter(|module| module.is_stdlib) {
        stdlib_mod_names.insert(module.mod_name.clone());
    }

    let mut required: HashMap<String, HashSet<String>> = HashMap::new();
    for path in roots {
        required
            .entry(crate::go_stdlib::module_name(path))
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
            trace_stdlib_resolution(format_args!(
                "[gors] resolve stdlib {import_path} as {module_name}"
            ));
            let required_roots = required.get(&module_name).cloned().unwrap_or_default();
            let items = if let Some(stdlib_mod) =
                crate::go_stdlib::resolve_with_roots(&import_path, &required_roots)
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

            for dep in crate::go_stdlib::collect_transitive_imports(&import_path) {
                let dep_module = crate::go_stdlib::module_name(&dep);
                stdlib_mod_names.insert(dep_module.clone());
                import_path_by_module.entry(dep_module).or_insert(dep);
            }

            let filename = format!("{}.rs", crate::go_stdlib::module_name(&import_path));
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
        module.file.items = module
            .file
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                keep.contains(&idx)
                    .then(|| reachable_item_for_names(item, &names, &item_names))
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
            .filter_map(|(key, module)| {
                (module.is_stdlib
                    && !preserved_mod_names.contains(&module.mod_name)
                    && !referenced.contains(&module.mod_name))
                .then(|| key.clone())
            })
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
    let mut names = roots.clone();
    let mut keep = std::collections::HashSet::new();
    let mut external_refs = std::collections::HashMap::new();
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    let top_level_types = top_level_item_types(items, module_names);

    loop {
        let mut changed = false;
        for (idx, item) in items.iter().enumerate() {
            let Some(mut reachable_item) = reachable_item_for_names(item, &names, &item_names)
            else {
                continue;
            };
            changed |= keep.insert(idx);

            let (local_names, refs) = collect_refs_from_item(
                &mut reachable_item,
                module_names,
                &item_names,
                &top_level_names,
                &top_level_types,
            );
            for name in local_names {
                changed |= names.insert(name);
            }
            changed |= merge_required_refs(&mut external_refs, refs);
        }
        if !changed {
            break;
        }
    }

    (keep, external_refs, names)
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
) -> Option<syn::Item> {
    if matches!(item, syn::Item::Use(_)) {
        return Some(item.clone());
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
        return Some(item.clone());
    }
    if !self_reachable {
        return None;
    }
    if item_impl.trait_.is_some() {
        if let Some((_, path, _)) = &item_impl.trait_
            && let Some(trait_name) = path.segments.last().map(|seg| seg.ident.to_string())
            && item_names.contains(&trait_name)
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
    names.contains(item_name)
        || self_name.as_ref().is_some_and(|self_name| {
            names.contains(&impl_method_reachability_name(self_name, item_name))
        })
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
    for item in items {
        let mut item_clone = item.clone();
        let empty_item_names = std::collections::HashSet::new();
        let empty_top_level_names = std::collections::HashSet::new();
        let (_, refs) = collect_refs_from_item(
            &mut item_clone,
            module_names,
            &empty_item_names,
            &empty_top_level_names,
            &empty_types,
        );
        merge_required_refs(&mut external_refs, refs);
    }
    external_refs
}

#[derive(Clone)]
struct ReceiverTypeRef {
    module: Option<String>,
    name: String,
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
        _ => None,
    }
}

fn receiver_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
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

fn collect_refs_from_item(
    item: &mut syn::Item,
    module_names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
    top_level_names: &std::collections::HashSet<String>,
    top_level_types: &std::collections::HashMap<String, ReceiverTypeRef>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashMap<String, std::collections::HashSet<String>>,
) {
    use syn::visit_mut::VisitMut;

    struct BoundCollector {
        names: std::collections::HashSet<String>,
        types: std::collections::HashMap<String, ReceiverTypeRef>,
        module_names: std::collections::HashSet<String>,
    }

    impl VisitMut for BoundCollector {
        fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
            self.names.insert(pat.ident.to_string());
            syn::visit_mut::visit_pat_ident_mut(self, pat);
        }

        fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
            if let syn::FnArg::Typed(pat_type) = arg
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, &self.module_names)
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_fn_arg_mut(self, arg);
        }

        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            if let syn::Pat::Type(pat_type) = &local.pat
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, &self.module_names)
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_local_mut(self, local);
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

    struct RefCollector<'a> {
        module_names: &'a std::collections::HashSet<String>,
        item_names: &'a std::collections::HashSet<String>,
        top_level_names: &'a std::collections::HashSet<String>,
        top_level_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
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
                    return;
                }
                (Some(module), Some(symbol), assoc, _) if self.module_names.contains(module) => {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(assoc.to_string());
                    }
                    return;
                }
                (Some(local), Some(symbol), assoc, _) if self.item_names.contains(local) => {
                    if is_reachability_name(local) {
                        self.local_names.insert(local.to_string());
                    }
                    self.local_names.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        self.local_names.insert(assoc.to_string());
                    }
                    return;
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
            if let Some(module) = external_module_from_expr(&method.receiver, self.module_names) {
                self.external_refs.entry(module).or_default().insert(name);
            } else if let Some(receiver_type) = self.receiver_type_from_expr(&method.receiver) {
                self.insert_receiver_method_ref(receiver_type, &name);
            } else if !self.top_level_names.contains(&name) {
                self.local_names.insert(name);
            }
            syn::visit_mut::visit_expr_method_call_mut(self, method);
        }
    }

    let mut bound_collector = BoundCollector {
        names: std::collections::HashSet::new(),
        types: std::collections::HashMap::new(),
        module_names: module_names.clone(),
    };
    let mut item_for_bounds = item.clone();
    bound_collector.visit_item_mut(&mut item_for_bounds);

    let mut collector = RefCollector {
        module_names,
        item_names,
        top_level_names,
        top_level_types,
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
            | "GoAppend"
            | "GoCap"
            | "GoLen"
            | "GoString"
            | "Into"
            | "ToString"
    )
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
/// let go_source = "package main\n\nfunc main() { x := 42 }";
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
    if let Some(value) = const_eval_expr(expr, 0) {
        let lit = syn::LitInt::new(&value.to_string(), Span::mixed_site());
        syn::parse_quote! { #lit }
    } else {
        syn::parse_quote! { 0 }
    }
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
    let mut params = Vec::new();
    for field in &func_type.params.list {
        let ty: syn::Type = field
            .type_
            .as_ref()
            .map(type_from_expr_ref)
            .unwrap_or_else(|| syn::parse_quote! { () });
        let count = field.names.as_ref().map_or(1, Vec::len);
        for _ in 0..count {
            let ident = next_unnamed_arg_ident();
            params.push(quote::quote! { #ident: #ty });
        }
    }

    let body = match func_type
        .results
        .as_ref()
        .map(|results| results.list.as_slice())
    {
        None | Some([]) => quote::quote! {},
        Some([field]) => {
            let expr = field
                .type_
                .as_ref()
                .map(default_expr_for_type)
                .unwrap_or_else(|| syn::parse_quote! { Default::default() });
            quote::quote! { #expr }
        }
        Some(fields) => {
            let values = fields.iter().map(|field| {
                field
                    .type_
                    .as_ref()
                    .map(default_expr_for_type)
                    .unwrap_or_else(|| syn::parse_quote! { Default::default() })
            });
            quote::quote! { (#(#values),*) }
        }
    };

    syn::parse_quote! { |#(#params),*| { #body } }
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
        ast::Expr::Ident(ident) if ident.name == "rune" => syn::parse_quote! { u32 },
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
            syn::parse_quote! { Box<#inner> }
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
            syn::parse_quote! { crate::builtin::GoChan<#inner> }
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

fn compile_type_spec(ts: ast::TypeSpec) -> Result<Vec<syn::Item>, CompilerError> {
    let name = ts
        .name
        .ok_or_else(|| CompilerError::UnsupportedConstruct("type spec has no name".to_string()))?;
    let vis: syn::Visibility = (&name).into();
    let ident: syn::Ident = name.into();
    let generics = compile_go_type_params(ts.type_params);

    match ts.type_ {
        ast::Expr::StructType(struct_type) => {
            let mut fields = syn::punctuated::Punctuated::new();
            let mut embedded_types: Vec<(syn::Ident, syn::Type)> = vec![];
            let mut default_fields: Vec<(syn::Ident, syn::Expr)> = vec![];
            let mut needs_manual_default = false;
            let mut cannot_derive_clone = false;
            let mut blank_field_index = 0usize;
            if let Some(field_list) = struct_type.fields {
                for field in field_list.list {
                    let field_type = field.type_.ok_or_else(|| {
                        CompilerError::UnsupportedConstruct("struct field has no type".to_string())
                    })?;
                    let field_needs_manual_default =
                        contains_array_type(&field_type) || contains_func_type(&field_type);
                    let field_cannot_derive_clone = contains_any_type(&field_type);
                    let field_default = default_expr_for_type(&field_type);

                    if let Some(names) = field.names {
                        let rust_type: syn::Type = field_type.into();
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
                        let rust_type: syn::Type = field_type.into();
                        if let Some(name) = embedded_name {
                            let field_ident =
                                syn::Ident::new(&rust_safe_ident_name(&name), Span::mixed_site());
                            let field_vis: syn::Visibility =
                                if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                                    syn::parse_quote! { pub }
                                } else {
                                    syn::Visibility::Inherited
                                };
                            embedded_types.push((field_ident.clone(), rust_type.clone()));
                            default_fields.push((field_ident.clone(), field_default.clone()));
                            needs_manual_default |= field_needs_manual_default;
                            cannot_derive_clone |= field_cannot_derive_clone;
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

            if !fields.empty_or_trailing() {
                fields.push_punct(<Token![,]>::default());
            }

            let generics_for_impl = generics.clone();
            let (impl_generics, ty_generics, where_clause) = generics_for_impl.split_for_impl();
            let struct_item = syn::Item::Struct(syn::ItemStruct {
                attrs: if cannot_derive_clone {
                    vec![]
                } else if needs_manual_default {
                    vec![syn::parse_quote! { #[derive(Clone)] }]
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

            let default_impl = if needs_manual_default || cannot_derive_clone {
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

            if let Some((emb_field, emb_ty)) =
                embedded_types.first().filter(|_| embedded_types.len() == 1)
            {
                let deref_impl: syn::Item = syn::parse_quote! {
                    impl std::ops::Deref for #ident {
                        type Target = #emb_ty;
                        fn deref(&self) -> &#emb_ty {
                            &self.#emb_field
                        }
                    }
                };
                let deref_mut_impl: syn::Item = syn::parse_quote! {
                    impl std::ops::DerefMut for #ident {
                        fn deref_mut(&mut self) -> &mut #emb_ty {
                            &mut self.#emb_field
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
                    }
                }
            }

            Ok(vec![syn::Item::Trait(syn::ItemTrait {
                attrs: vec![],
                vis,
                unsafety: None,
                auto_token: None,
                restriction: None,
                trait_token: <Token![trait]>::default(),
                ident,
                generics: syn::Generics::default(),
                colon_token: None,
                supertraits: syn::punctuated::Punctuated::new(),
                brace_token: syn::token::Brace::default(),
                items: trait_items,
            })])
        }
        other => {
            let is_byte_slice = is_byte_slice_type(&other);
            let underlying_go_type = typeinfer::GoType::from_expr(&other);
            let is_copy_alias = underlying_go_type.is_numeric()
                || matches!(underlying_go_type, typeinfer::GoType::Bool);
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
            if is_byte_slice {
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoLen for #ident {
                        fn go_len(&self) -> usize { self.0.len() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoCap for #ident {
                        fn go_cap(&self) -> usize { self.0.capacity() }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoString for #ident {
                        fn go_string(self) -> String {
                            String::from_utf8(self.0).unwrap_or_default()
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoString for &#ident {
                        fn go_string(self) -> String {
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
                    impl crate::builtin::GoAppend<u8> for #ident {
                        fn go_append(mut self, elem: u8) -> Self {
                            self.0.push(elem);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoAppend<Vec<u8>> for #ident {
                        fn go_append(mut self, elem: Vec<u8>) -> Self {
                            self.0.extend(elem);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoAppend<#ident> for Vec<u8> {
                        fn go_append(mut self, elem: #ident) -> Self {
                            self.extend(elem.0);
                            self
                        }
                    }
                });
                items.push(syn::parse_quote! {
                    impl crate::builtin::GoAppend<String> for #ident {
                        fn go_append(mut self, elem: String) -> Self {
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
    let type_args = receiver_type_args(&recv_type);

    let self_arg: syn::FnArg = if is_pointer {
        syn::parse_quote! { &mut self }
    } else {
        syn::parse_quote! { &self }
    };

    if !recv_name.is_empty() {
        TYPE_ENV.with(|env| {
            env.borrow_mut()
                .set_var(&recv_name, typeinfer::GoType::Named(type_name.clone()));
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

    let mut inputs = syn::punctuated::Punctuated::new();
    inputs.push(self_arg);
    for param in func_decl.type_.params.list {
        for arg in compile_field_to_fn_args(param)? {
            inputs.push(arg);
        }
    }

    let vis: syn::Visibility = (&func_decl.name).into();
    let attrs = comment_group_to_attrs(&func_decl.doc);

    let mut block = if let Some(body) = func_decl.body {
        body.try_into()?
    } else {
        syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: vec![],
        }
    };

    if !recv_name.is_empty() {
        rewrite_receiver(&mut block, &recv_name);
    }

    // Handle named return values for methods (same logic as top-level functions)
    let mut named_return_idents: Vec<syn::Ident> = vec![];
    if let Some(ref results) = func_decl.type_.results {
        let has_named_returns = results
            .list
            .iter()
            .any(|f| f.names.as_ref().is_some_and(|names| !names.is_empty()));
        if has_named_returns {
            let mut named_return_info: Vec<(syn::Ident, syn::Expr)> = vec![];
            for field in &results.list {
                if let Some(ref names) = field.names {
                    for name in names {
                        let type_name = field.type_.as_ref().and_then(go_type_name_from_expr);
                        let zero = zero_value_for_type(type_name);
                        let ident =
                            syn::Ident::new(&rust_safe_ident_name(name.name), Span::mixed_site());
                        named_return_info.push((ident.clone(), zero));
                        named_return_idents.push(ident);
                    }
                }
            }
            let mut prepend: Vec<syn::Stmt> = vec![];
            for (ident, zero) in &named_return_info {
                prepend.push(syn::parse_quote! { let mut #ident = #zero; });
            }
            let existing = std::mem::take(&mut block.stmts);
            block.stmts = prepend;
            block.stmts.extend(existing);
            rewrite_bare_returns(&mut block, &named_return_idents);
            let needs_implicit_return = !block
                .stmts
                .last()
                .is_some_and(|last| matches!(last, syn::Stmt::Expr(syn::Expr::Return(_), _)));
            if needs_implicit_return {
                if let Some(ident) = (named_return_idents.len() == 1)
                    .then(|| named_return_idents.first())
                    .flatten()
                {
                    block.stmts.push(syn::Stmt::Expr(
                        syn::Expr::Return(syn::ExprReturn {
                            attrs: vec![],
                            return_token: <Token![return]>::default(),
                            expr: Some(Box::new(syn::parse_quote! { #ident })),
                        }),
                        None,
                    ));
                } else {
                    let idents = &named_return_idents;
                    block.stmts.push(syn::Stmt::Expr(
                        syn::Expr::Return(syn::ExprReturn {
                            attrs: vec![],
                            return_token: <Token![return]>::default(),
                            expr: Some(Box::new(syn::parse_quote! { (#(#idents),*) })),
                        }),
                        None,
                    ));
                }
            }
        }
    }

    let output = compile_return_type(func_decl.type_.results)?;

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

const BUILTINS: &[&str] = &[
    "len", "cap", "append", "make", "new", "copy", "delete", "clear", "close", "panic", "println",
    "print", "max", "min", "complex", "real", "imag", "recover",
];

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

fn detect_type_conversion(call_expr: &ast::CallExpr) -> Option<&'static str> {
    let args = call_expr.args.as_ref()?;
    if args.len() != 1 {
        return None;
    }
    match &*call_expr.fun {
        ast::Expr::Ident(id) if id.name == "string" => Some("string"),
        ast::Expr::Ident(id) if id.name == "any" => Some("any"),
        ast::Expr::Ident(id) if id.name == "complex64" => Some("complex64"),
        ast::Expr::Ident(id) if id.name == "complex128" => Some("complex128"),
        ast::Expr::ArrayType(arr) if arr.len.is_none() => {
            if let ast::Expr::Ident(elt_id) = &*arr.elt {
                match elt_id.name {
                    "byte" | "uint8" => Some("[]byte"),
                    "rune" | "int32" => Some("[]rune"),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_general_type_conversion_fun(fun: &ast::Expr) -> bool {
    match fun {
        ast::Expr::ParenExpr(paren) => is_general_type_conversion_fun(&paren.x),
        ast::Expr::StarExpr(_)
        | ast::Expr::ChanType(_)
        | ast::Expr::MapType(_)
        | ast::Expr::InterfaceType(_)
        | ast::Expr::StructType(_) => true,
        ast::Expr::ArrayType(arr) => arr.len.is_some(),
        ast::Expr::IndexExpr(index) => TYPE_ENV.with(|env| {
            let env = env.borrow();
            extract_type_name(&index.x)
                .and_then(|name| env.get_type_kind(&name).cloned())
                .is_some()
        }),
        ast::Expr::IndexListExpr(index) => TYPE_ENV.with(|env| {
            let env = env.borrow();
            extract_type_name(&index.x)
                .and_then(|name| env.get_type_kind(&name).cloned())
                .is_some()
        }),
        ast::Expr::SelectorExpr(sel) => {
            if let ast::Expr::Ident(pkg) = &*sel.x {
                if pkg.name == "unsafe" && sel.sel.name == "Pointer" {
                    return true;
                }
                let key = format!("{}.{}", pkg.name, sel.sel.name);
                return TYPE_ENV.with(|env| env.borrow().get_type_kind(&key).is_some());
            }
            false
        }
        ast::Expr::Ident(id) => {
            matches!(
                id.name,
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
            ) || TYPE_ENV.with(|env| env.borrow().get_type_kind(id.name).is_some())
        }
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
    let arg: syn::Expr = raw_arg.into();

    if matches!(&target_fun, ast::Expr::SelectorExpr(sel) if matches!(&*sel.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe") && sel.sel.name == "Pointer")
    {
        return syn::parse_quote! { 0usize };
    }
    if matches!(&target_fun, ast::Expr::Ident(id) if id.name == "any") {
        return syn::parse_quote! { Box::new(#arg) as Box<dyn std::any::Any> };
    }

    let target_ty = type_from_expr_ref(&target_fun);
    if let ast::Expr::Ident(id) = &target_fun
        && let Some(inner_ty) = TYPE_ENV.with(|env| {
            let env = env.borrow();
            match env.get_type_kind(id.name) {
                Some(typeinfer::TypeKind::Alias(inner)) if inner.is_numeric() => {
                    rust_type_from_go_type(inner)
                }
                _ => None,
            }
        })
    {
        return syn::parse_quote! { #target_ty((#arg) as #inner_ty) };
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
        "string" => {
            syn::parse_quote! { crate::builtin::go_string(&#arg) }
        }
        "complex64" => syn::parse_quote! { crate::builtin::to_complex64(#arg) },
        "complex128" => syn::parse_quote! { crate::builtin::to_complex128(#arg) },
        "any" => syn::parse_quote! { Box::new(#arg) as Box<dyn std::any::Any> },
        "[]byte" => syn::parse_quote! { (#arg).as_bytes().to_vec() },
        "[]rune" => syn::parse_quote! { (#arg).chars().collect::<Vec<char>>() },
        _ => compile_error_expr(format!("unsupported type conversion: {kind}")),
    }
}

fn rust_type_from_go_type(go_type: &typeinfer::GoType) -> Option<syn::Type> {
    match go_type {
        typeinfer::GoType::Bool => Some(syn::parse_quote! { bool }),
        typeinfer::GoType::Int => Some(syn::parse_quote! { isize }),
        typeinfer::GoType::Int8 => Some(syn::parse_quote! { i8 }),
        typeinfer::GoType::Int16 => Some(syn::parse_quote! { i16 }),
        typeinfer::GoType::Int32 => Some(syn::parse_quote! { i32 }),
        typeinfer::GoType::Int64 => Some(syn::parse_quote! { i64 }),
        typeinfer::GoType::Uint => Some(syn::parse_quote! { usize }),
        typeinfer::GoType::Uint8 => Some(syn::parse_quote! { u8 }),
        typeinfer::GoType::Uint16 => Some(syn::parse_quote! { u16 }),
        typeinfer::GoType::Uint32 => Some(syn::parse_quote! { u32 }),
        typeinfer::GoType::Uint64 => Some(syn::parse_quote! { u64 }),
        typeinfer::GoType::Uintptr => Some(syn::parse_quote! { usize }),
        typeinfer::GoType::Float32 => Some(syn::parse_quote! { f32 }),
        typeinfer::GoType::Float64 => Some(syn::parse_quote! { f64 }),
        typeinfer::GoType::Complex64 => Some(syn::parse_quote! { crate::builtin::Complex64 }),
        typeinfer::GoType::Complex128 => Some(syn::parse_quote! { crate::builtin::Complex128 }),
        _ => None,
    }
}

fn is_builtin_call(call_expr: &ast::CallExpr) -> bool {
    if let ast::Expr::Ident(ident) = &*call_expr.fun {
        BUILTINS.contains(&ident.name)
    } else {
        false
    }
}

#[derive(Clone, Copy)]
enum LoweredStdlibCall {
    FmtAppend,
    FmtAppendf,
    FmtAppendln,
    FmtPrint,
    FmtPrintf,
    FmtPrintln,
    FmtSprint,
    FmtSprintf,
    FmtSprintln,
    FmtFprint,
    FmtFprintf,
    FmtFprintln,
    FmtErrorf,
    SortFind,
    SortInts,
    SortStrings,
    SortFloat64s,
    SortIntsAreSorted,
    SortStringsAreSorted,
    SortFloat64sAreSorted,
    SortIsSorted,
    SortSearch,
    SortSearchFloat64s,
    SortSearchInts,
    SortSearchStrings,
    SortSlice,
    SortSliceIsSorted,
    SortSliceStable,
    SortSort,
    SortStable,
}

#[derive(Clone, Copy)]
enum SortSliceKind {
    Int,
    String,
    Float64,
}

#[derive(Clone, Copy)]
enum SortInterfaceMode {
    Normal,
    Reverse,
}

fn lowered_stdlib_call(call_expr: &ast::CallExpr) -> Option<LoweredStdlibCall> {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return None;
    };
    let ast::Expr::Ident(pkg) = &*selector.x else {
        return None;
    };
    if !IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name)) {
        return None;
    }

    match (pkg.name, selector.sel.name) {
        ("fmt", "Append") => Some(LoweredStdlibCall::FmtAppend),
        ("fmt", "Appendf") => Some(LoweredStdlibCall::FmtAppendf),
        ("fmt", "Appendln") => Some(LoweredStdlibCall::FmtAppendln),
        ("fmt", "Print") => Some(LoweredStdlibCall::FmtPrint),
        ("fmt", "Printf") => Some(LoweredStdlibCall::FmtPrintf),
        ("fmt", "Println") => Some(LoweredStdlibCall::FmtPrintln),
        ("fmt", "Sprint") => Some(LoweredStdlibCall::FmtSprint),
        ("fmt", "Sprintf") => Some(LoweredStdlibCall::FmtSprintf),
        ("fmt", "Sprintln") => Some(LoweredStdlibCall::FmtSprintln),
        ("fmt", "Fprint") => Some(LoweredStdlibCall::FmtFprint),
        ("fmt", "Fprintf") => Some(LoweredStdlibCall::FmtFprintf),
        ("fmt", "Fprintln") => Some(LoweredStdlibCall::FmtFprintln),
        ("fmt", "Errorf") => Some(LoweredStdlibCall::FmtErrorf),
        ("sort", "Find") => Some(LoweredStdlibCall::SortFind),
        ("sort", "Ints") => Some(LoweredStdlibCall::SortInts),
        ("sort", "Strings") => Some(LoweredStdlibCall::SortStrings),
        ("sort", "Float64s") => Some(LoweredStdlibCall::SortFloat64s),
        ("sort", "IntsAreSorted") => Some(LoweredStdlibCall::SortIntsAreSorted),
        ("sort", "StringsAreSorted") => Some(LoweredStdlibCall::SortStringsAreSorted),
        ("sort", "Float64sAreSorted") => Some(LoweredStdlibCall::SortFloat64sAreSorted),
        ("sort", "IsSorted") => Some(LoweredStdlibCall::SortIsSorted),
        ("sort", "Search") => Some(LoweredStdlibCall::SortSearch),
        ("sort", "SearchFloat64s") => Some(LoweredStdlibCall::SortSearchFloat64s),
        ("sort", "SearchInts") => Some(LoweredStdlibCall::SortSearchInts),
        ("sort", "SearchStrings") => Some(LoweredStdlibCall::SortSearchStrings),
        ("sort", "Slice") => Some(LoweredStdlibCall::SortSlice),
        ("sort", "SliceIsSorted") => Some(LoweredStdlibCall::SortSliceIsSorted),
        ("sort", "SliceStable") => Some(LoweredStdlibCall::SortSliceStable),
        ("sort", "Sort") => Some(LoweredStdlibCall::SortSort),
        ("sort", "Stable") => Some(LoweredStdlibCall::SortStable),
        _ => None,
    }
}

fn compile_lowered_stdlib_call(call_expr: ast::CallExpr, lowered: LoweredStdlibCall) -> syn::Expr {
    let raw_args = call_expr.args.unwrap_or_default();
    if matches!(
        lowered,
        LoweredStdlibCall::FmtPrint
            | LoweredStdlibCall::FmtPrintln
            | LoweredStdlibCall::FmtSprint
            | LoweredStdlibCall::FmtSprintln
    ) {
        let args = compile_fmt_any_vec(raw_args);
        return match lowered {
            LoweredStdlibCall::FmtPrint => {
                syn::parse_quote! { crate::builtin::go_fmt_print(#args) }
            }
            LoweredStdlibCall::FmtPrintln => {
                syn::parse_quote! { crate::builtin::go_fmt_println(#args) }
            }
            LoweredStdlibCall::FmtSprint => {
                syn::parse_quote! { crate::builtin::go_fmt_sprint(#args) }
            }
            LoweredStdlibCall::FmtSprintln => {
                syn::parse_quote! { crate::builtin::go_fmt_sprintln(#args) }
            }
            _ => compile_error_expr("unsupported fmt print lowering"),
        };
    }

    if matches!(
        lowered,
        LoweredStdlibCall::FmtAppend | LoweredStdlibCall::FmtAppendln
    ) {
        let mut raw_args = raw_args.into_iter();
        let Some(buffer) = raw_args.next() else {
            return compile_error_expr("fmt append call requires a buffer argument");
        };
        let buffer: syn::Expr = buffer.into();
        let args = compile_fmt_any_vec(raw_args.collect());
        return match lowered {
            LoweredStdlibCall::FmtAppend => {
                syn::parse_quote! { crate::builtin::go_fmt_append(#buffer, #args) }
            }
            LoweredStdlibCall::FmtAppendln => {
                syn::parse_quote! { crate::builtin::go_fmt_appendln(#buffer, #args) }
            }
            _ => compile_error_expr("unsupported fmt append lowering"),
        };
    }

    if matches!(lowered, LoweredStdlibCall::FmtAppendf) {
        let mut raw_args = raw_args.into_iter();
        let Some(buffer) = raw_args.next() else {
            return compile_error_expr("fmt appendf call requires a buffer argument");
        };
        let Some(format_arg) = raw_args.next() else {
            return compile_error_expr("fmt appendf call requires a format argument");
        };
        let buffer: syn::Expr = buffer.into();
        let format = compile_expr_with_expected(format_arg, Some(&typeinfer::GoType::String));
        let args = compile_fmt_any_vec(raw_args.collect());
        return syn::parse_quote! { crate::builtin::go_fmt_appendf(#buffer, &#format, #args) };
    }

    if matches!(
        lowered,
        LoweredStdlibCall::FmtPrintf | LoweredStdlibCall::FmtSprintf | LoweredStdlibCall::FmtErrorf
    ) {
        return compile_fmt_format_call(raw_args, 0, lowered);
    }

    if matches!(
        lowered,
        LoweredStdlibCall::FmtFprint | LoweredStdlibCall::FmtFprintln
    ) {
        let args = compile_fmt_any_vec(raw_args.into_iter().skip(1).collect());
        return match lowered {
            LoweredStdlibCall::FmtFprint => {
                syn::parse_quote! { crate::builtin::go_fmt_print(#args) }
            }
            LoweredStdlibCall::FmtFprintln => {
                syn::parse_quote! { crate::builtin::go_fmt_println(#args) }
            }
            _ => compile_error_expr("unsupported fmt writer lowering"),
        };
    }

    if matches!(lowered, LoweredStdlibCall::FmtFprintf) {
        return compile_fmt_format_call(raw_args, 1, lowered);
    }

    if matches!(lowered, LoweredStdlibCall::SortSearch) {
        let mut args = raw_args.into_iter();
        let Some(n) = args.next() else {
            return compile_error_expr("sort.Search requires a length argument");
        };
        let Some(predicate) = args.next() else {
            return compile_error_expr("sort.Search requires a predicate argument");
        };
        if args.next().is_some() {
            return compile_error_expr("sort.Search requires two arguments");
        }
        let n: syn::Expr = n.into();
        let predicate: syn::Expr = predicate.into();
        return syn::parse_quote! { crate::builtin::go_sort_search(#n, #predicate) };
    }

    if matches!(lowered, LoweredStdlibCall::SortFind) {
        let mut args = raw_args.into_iter();
        let Some(n) = args.next() else {
            return compile_error_expr("sort.Find requires a length argument");
        };
        let Some(cmp) = args.next() else {
            return compile_error_expr("sort.Find requires a comparison argument");
        };
        if args.next().is_some() {
            return compile_error_expr("sort.Find requires two arguments");
        }
        let n: syn::Expr = n.into();
        let cmp: syn::Expr = cmp.into();
        return syn::parse_quote! { crate::builtin::go_sort_find(#n, #cmp) };
    }

    if matches!(
        lowered,
        LoweredStdlibCall::SortSearchFloat64s
            | LoweredStdlibCall::SortSearchInts
            | LoweredStdlibCall::SortSearchStrings
    ) {
        let mut args = raw_args.into_iter();
        let Some(values) = args.next() else {
            return compile_error_expr("sort search helper requires a slice argument");
        };
        let Some(target) = args.next() else {
            return compile_error_expr("sort search helper requires a target argument");
        };
        if args.next().is_some() {
            return compile_error_expr("sort search helper requires two arguments");
        }
        let values: syn::Expr = values.into();
        let target = match lowered {
            LoweredStdlibCall::SortSearchStrings => {
                compile_expr_with_expected(target, Some(&typeinfer::GoType::String))
            }
            _ => target.into(),
        };
        return match lowered {
            LoweredStdlibCall::SortSearchFloat64s => {
                syn::parse_quote! { crate::builtin::go_sort_search_float64s(&#values, #target) }
            }
            LoweredStdlibCall::SortSearchInts => {
                syn::parse_quote! { crate::builtin::go_sort_search_ints(&#values, #target) }
            }
            LoweredStdlibCall::SortSearchStrings => {
                syn::parse_quote! { crate::builtin::go_sort_search_strings(&#values, #target) }
            }
            _ => compile_error_expr("unsupported sort search lowering"),
        };
    }

    if matches!(
        lowered,
        LoweredStdlibCall::SortSlice
            | LoweredStdlibCall::SortSliceStable
            | LoweredStdlibCall::SortSliceIsSorted
    ) {
        let mut args = raw_args.into_iter();
        let Some(values) = args.next() else {
            return compile_error_expr("sort.Slice requires a slice argument");
        };
        let Some(less) = args.next() else {
            return compile_error_expr("sort.Slice requires a less function argument");
        };
        if args.next().is_some() {
            return compile_error_expr("sort.Slice requires two arguments");
        }
        let values: syn::Expr = values.into();
        let less: syn::Expr = less.into();
        return match lowered {
            LoweredStdlibCall::SortSlice | LoweredStdlibCall::SortSliceStable => {
                syn::parse_quote! { crate::builtin::go_sort_slice(&mut #values, #less) }
            }
            LoweredStdlibCall::SortSliceIsSorted => {
                syn::parse_quote! {
                    crate::builtin::go_sort_slice_is_sorted(crate::builtin::len(&#values) as isize, #less)
                }
            }
            _ => compile_error_expr("unsupported sort slice lowering"),
        };
    }

    if matches!(
        lowered,
        LoweredStdlibCall::SortSort
            | LoweredStdlibCall::SortStable
            | LoweredStdlibCall::SortIsSorted
    ) {
        let mut args = raw_args.into_iter();
        let Some(arg) = args.next() else {
            return compile_error_expr("sort interface call requires one argument");
        };
        if args.next().is_some() {
            return compile_error_expr("sort interface call requires one argument");
        }
        if let Some((mode, kind, values)) = into_sort_interface_arg(arg) {
            return compile_sort_interface_arg(mode, kind, values, lowered);
        }
        return compile_error_expr("unsupported sort interface argument");
    }

    let mut args = raw_args.into_iter();
    let Some(arg) = args.next() else {
        return compile_error_expr("sort call requires one argument");
    };
    if args.next().is_some() {
        return compile_error_expr("sort call requires one argument");
    }
    let arg: syn::Expr = arg.into();

    match lowered {
        LoweredStdlibCall::SortInts | LoweredStdlibCall::SortStrings => {
            syn::parse_quote! { crate::builtin::go_sort(&mut #arg) }
        }
        LoweredStdlibCall::SortFloat64s => {
            syn::parse_quote! { crate::builtin::go_sort_float64s(&mut #arg) }
        }
        LoweredStdlibCall::SortIntsAreSorted | LoweredStdlibCall::SortStringsAreSorted => {
            syn::parse_quote! { crate::builtin::go_is_sorted(&#arg) }
        }
        LoweredStdlibCall::SortFloat64sAreSorted => {
            syn::parse_quote! { crate::builtin::go_float64s_are_sorted(&#arg) }
        }
        _ => compile_error_expr("unsupported sort lowering"),
    }
}

fn is_lowered_stdlib_method_call(call_expr: &ast::CallExpr) -> bool {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return false;
    };
    if !matches!(
        selector.sel.name,
        "Len" | "Less" | "Search" | "Sort" | "Swap"
    ) {
        return false;
    }
    let ast::Expr::CallExpr(receiver_call) = &*selector.x else {
        return false;
    };
    sort_slice_kind_from_fun(&receiver_call.fun).is_some()
}

fn compile_lowered_stdlib_method_call(call_expr: ast::CallExpr) -> syn::Expr {
    let ast::Expr::SelectorExpr(selector) = *call_expr.fun else {
        return compile_error_expr("unsupported stdlib method call");
    };
    let ast::Expr::CallExpr(receiver_call) = *selector.x else {
        return compile_error_expr("unsupported stdlib method receiver");
    };
    let Some((kind, receiver)) = into_sort_slice_conversion_call(receiver_call) else {
        return compile_error_expr("unsupported sort slice method receiver");
    };
    let method = selector.sel.name;
    let raw_args = call_expr.args.unwrap_or_default();

    match method {
        "Len" => {
            if !raw_args.is_empty() {
                return compile_error_expr("sort slice Len method requires no arguments");
            }
            let receiver: syn::Expr = receiver.into();
            syn::parse_quote! { crate::builtin::len(&#receiver) as isize }
        }
        "Less" => {
            let mut args = raw_args.into_iter();
            let Some(i) = args.next() else {
                return compile_error_expr("sort slice Less method requires two arguments");
            };
            let Some(j) = args.next() else {
                return compile_error_expr("sort slice Less method requires two arguments");
            };
            if args.next().is_some() {
                return compile_error_expr("sort slice Less method requires two arguments");
            }
            let receiver: syn::Expr = receiver.into();
            let i: syn::Expr = i.into();
            let j: syn::Expr = j.into();
            match kind {
                SortSliceKind::Float64 => {
                    syn::parse_quote! { crate::builtin::go_sort_float64s_less(&#receiver, #i, #j) }
                }
                SortSliceKind::Int | SortSliceKind::String => {
                    syn::parse_quote! { crate::builtin::go_sort_slice_less(&#receiver, #i, #j) }
                }
            }
        }
        "Search" => {
            let mut args = raw_args.into_iter();
            let Some(target) = args.next() else {
                return compile_error_expr("sort slice Search method requires one argument");
            };
            if args.next().is_some() {
                return compile_error_expr("sort slice Search method requires one argument");
            }
            let receiver: syn::Expr = receiver.into();
            compile_sort_slice_search(kind, receiver, target)
        }
        "Sort" => {
            if !raw_args.is_empty() {
                return compile_error_expr("sort slice Sort method requires no arguments");
            }
            let receiver: syn::Expr = receiver.into();
            compile_sort_slice_sort(kind, receiver, SortInterfaceMode::Normal)
        }
        "Swap" => {
            let mut args = raw_args.into_iter();
            let Some(i) = args.next() else {
                return compile_error_expr("sort slice Swap method requires two arguments");
            };
            let Some(j) = args.next() else {
                return compile_error_expr("sort slice Swap method requires two arguments");
            };
            if args.next().is_some() {
                return compile_error_expr("sort slice Swap method requires two arguments");
            }
            let receiver: syn::Expr = receiver.into();
            let i: syn::Expr = i.into();
            let j: syn::Expr = j.into();
            syn::parse_quote! { crate::builtin::go_sort_swap(&mut #receiver, #i, #j) }
        }
        _ => compile_error_expr("unsupported sort slice method"),
    }
}

fn into_sort_interface_arg(
    arg: ast::Expr,
) -> Option<(SortInterfaceMode, SortSliceKind, ast::Expr)> {
    if let ast::Expr::CallExpr(call) = arg {
        if is_sort_package_selector(&call.fun, "Reverse") {
            let mut args = call.args.unwrap_or_default().into_iter();
            let receiver = args.next()?;
            if args.next().is_some() {
                return None;
            }
            let (kind, values) = into_sort_slice_conversion(receiver)?;
            return Some((SortInterfaceMode::Reverse, kind, values));
        }
        let (kind, values) = into_sort_slice_conversion_call(call)?;
        return Some((SortInterfaceMode::Normal, kind, values));
    }
    None
}

fn into_sort_slice_conversion(arg: ast::Expr) -> Option<(SortSliceKind, ast::Expr)> {
    let ast::Expr::CallExpr(call) = arg else {
        return None;
    };
    into_sort_slice_conversion_call(call)
}

fn into_sort_slice_conversion_call(call: ast::CallExpr) -> Option<(SortSliceKind, ast::Expr)> {
    let kind = sort_slice_kind_from_fun(&call.fun)?;
    let mut args = call.args.unwrap_or_default().into_iter();
    let values = args.next()?;
    if args.next().is_some() {
        return None;
    }
    Some((kind, values))
}

fn sort_slice_kind_from_fun(fun: &ast::Expr) -> Option<SortSliceKind> {
    let ast::Expr::SelectorExpr(selector) = fun else {
        return None;
    };
    let ast::Expr::Ident(pkg) = &*selector.x else {
        return None;
    };
    if pkg.name != "sort" || !IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name)) {
        return None;
    }
    match selector.sel.name {
        "IntSlice" => Some(SortSliceKind::Int),
        "StringSlice" => Some(SortSliceKind::String),
        "Float64Slice" => Some(SortSliceKind::Float64),
        _ => None,
    }
}

fn is_sort_package_selector(fun: &ast::Expr, name: &str) -> bool {
    let ast::Expr::SelectorExpr(selector) = fun else {
        return false;
    };
    let ast::Expr::Ident(pkg) = &*selector.x else {
        return false;
    };
    pkg.name == "sort"
        && selector.sel.name == name
        && IMPORT_NAMES.with(|names| names.borrow().contains(pkg.name))
}

fn compile_sort_interface_arg(
    mode: SortInterfaceMode,
    kind: SortSliceKind,
    values: ast::Expr,
    lowered: LoweredStdlibCall,
) -> syn::Expr {
    let values: syn::Expr = values.into();
    match lowered {
        LoweredStdlibCall::SortSort | LoweredStdlibCall::SortStable => {
            compile_sort_slice_sort(kind, values, mode)
        }
        LoweredStdlibCall::SortIsSorted => match mode {
            SortInterfaceMode::Normal => compile_sort_slice_is_sorted(kind, values),
            SortInterfaceMode::Reverse => {
                compile_error_expr("sort.IsSorted does not lower Reverse")
            }
        },
        _ => compile_error_expr("unsupported sort interface lowering"),
    }
}

fn compile_sort_slice_sort(
    kind: SortSliceKind,
    values: syn::Expr,
    mode: SortInterfaceMode,
) -> syn::Expr {
    match (kind, mode) {
        (SortSliceKind::Float64, SortInterfaceMode::Normal) => {
            syn::parse_quote! { crate::builtin::go_sort_float64s(&mut #values) }
        }
        (SortSliceKind::Float64, SortInterfaceMode::Reverse) => {
            syn::parse_quote! { crate::builtin::go_sort_float64s_reverse(&mut #values) }
        }
        (SortSliceKind::Int | SortSliceKind::String, SortInterfaceMode::Normal) => {
            syn::parse_quote! { crate::builtin::go_sort(&mut #values) }
        }
        (SortSliceKind::Int | SortSliceKind::String, SortInterfaceMode::Reverse) => {
            syn::parse_quote! { crate::builtin::go_sort_reverse(&mut #values) }
        }
    }
}

fn compile_sort_slice_is_sorted(kind: SortSliceKind, values: syn::Expr) -> syn::Expr {
    match kind {
        SortSliceKind::Float64 => {
            syn::parse_quote! { crate::builtin::go_float64s_are_sorted(&#values) }
        }
        SortSliceKind::Int | SortSliceKind::String => {
            syn::parse_quote! { crate::builtin::go_is_sorted(&#values) }
        }
    }
}

fn compile_sort_slice_search(
    kind: SortSliceKind,
    receiver: syn::Expr,
    target: ast::Expr,
) -> syn::Expr {
    match kind {
        SortSliceKind::Float64 => {
            let target: syn::Expr = target.into();
            syn::parse_quote! { crate::builtin::go_sort_search_float64s(&#receiver, #target) }
        }
        SortSliceKind::Int => {
            let target: syn::Expr = target.into();
            syn::parse_quote! { crate::builtin::go_sort_search_ints(&#receiver, #target) }
        }
        SortSliceKind::String => {
            let target = compile_expr_with_expected(target, Some(&typeinfer::GoType::String));
            syn::parse_quote! { crate::builtin::go_sort_search_strings(&#receiver, #target) }
        }
    }
}

fn compile_fmt_any_vec(raw_args: Vec<ast::Expr>) -> syn::Expr {
    let args: Vec<syn::Expr> = raw_args
        .into_iter()
        .map(|arg| {
            let arg = compile_variadic_any_arg(arg, Some(&typeinfer::GoType::Any));
            syn::parse_quote! { Box::new((#arg).clone()) as Box<dyn std::any::Any> }
        })
        .collect();

    if args.is_empty() {
        syn::parse_quote! { Vec::<Box<dyn std::any::Any>>::new() }
    } else {
        syn::parse_quote! { Vec::from([#(#args),*]) }
    }
}

fn compile_fmt_format_call(
    raw_args: Vec<ast::Expr>,
    format_index: usize,
    lowered: LoweredStdlibCall,
) -> syn::Expr {
    let mut raw_args = raw_args.into_iter();
    let format_arg = raw_args.nth(format_index);
    let Some(format_arg) = format_arg else {
        return compile_error_expr("fmt format call requires a format argument");
    };
    let format = compile_expr_with_expected(format_arg, Some(&typeinfer::GoType::String));
    let args = compile_fmt_any_vec(raw_args.collect());

    match lowered {
        LoweredStdlibCall::FmtPrintf | LoweredStdlibCall::FmtFprintf => {
            syn::parse_quote! { crate::builtin::go_fmt_printf(&#format, #args) }
        }
        LoweredStdlibCall::FmtSprintf | LoweredStdlibCall::FmtErrorf => {
            syn::parse_quote! { crate::builtin::go_fmt_sprintf(&#format, #args) }
        }
        _ => compile_error_expr("unsupported fmt format lowering"),
    }
}

fn call_func_key(fun: &ast::Expr) -> Option<String> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
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

                    if let Some(typeinfer::GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                        return Some(format!("{}.{}", name, sel.sel.name));
                    }
                }
                None
            }
            _ => None,
        }
    })
}

fn is_variadic_any_call(call_expr: &ast::CallExpr) -> Option<usize> {
    let key = call_func_key(&call_expr.fun)?;
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        let start = env.get_func_variadic_start(&key)?;
        let params = env.get_func_params(&key);
        match params.get(start) {
            Some(typeinfer::GoType::Slice(inner))
                if matches!(
                    &**inner,
                    typeinfer::GoType::Any | typeinfer::GoType::Interface(_)
                ) =>
            {
                Some(start)
            }
            _ => None,
        }
    })
}

fn compile_variadic_any_call(call_expr: ast::CallExpr, variadic_start: usize) -> syn::Expr {
    let param_types = call_param_types(&call_expr.fun);
    let fun_expr: syn::Expr = match *call_expr.fun {
        ast::Expr::Ident(ident) => syn::Expr::Path(ident.into()),
        ast::Expr::SelectorExpr(sel) => {
            let ast::Expr::Ident(pkg) = *sel.x else {
                return compile_error_expr("unsupported variadic selector receiver");
            };
            let pkg_ident = syn::Ident::new(&import_rust_name(pkg.name), Span::mixed_site());
            let method_ident: syn::Ident = sel.sel.into();
            syn::parse_quote! { #pkg_ident::#method_ident }
        }
        _ => compile_error_expr("unsupported variadic call target"),
    };

    let variadic_elem = param_types.get(variadic_start).and_then(|ty| match ty {
        typeinfer::GoType::Slice(inner) => Some((**inner).clone()),
        _ => None,
    });
    let raw_args: Vec<ast::Expr> = call_expr.args.unwrap_or_default().into_iter().collect();
    let has_variadic_spread = call_expr.ellipsis.is_some();

    let mut final_args: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> =
        syn::punctuated::Punctuated::new();

    if has_variadic_spread {
        for (i, arg) in raw_args.into_iter().enumerate() {
            if i < variadic_start {
                final_args.push(compile_expr_with_expected(arg, param_types.get(i)));
            } else {
                final_args.push(compile_expr_with_expected(arg, param_types.get(i)));
            }
        }
        return syn::parse_quote! { #fun_expr(#final_args) };
    }

    for (i, arg) in raw_args.into_iter().enumerate() {
        if i < variadic_start {
            final_args.push(compile_expr_with_expected(arg, param_types.get(i)));
        } else {
            let arg = compile_variadic_any_arg(arg, variadic_elem.as_ref());
            final_args.push(syn::parse_quote! { Box::new(#arg.clone()) as Box<dyn std::any::Any> });
        }
    }

    let variadic_args: Vec<&syn::Expr> = final_args.iter().skip(variadic_start).collect();
    let fixed_args: Vec<&syn::Expr> = final_args.iter().take(variadic_start).collect();

    let vec_expr: syn::Expr = if variadic_args.is_empty() {
        syn::parse_quote! { Vec::<Box<dyn std::any::Any>>::new() }
    } else {
        syn::parse_quote! { Vec::from([#(#variadic_args),*]) }
    };

    let mut call_args: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> =
        syn::punctuated::Punctuated::new();
    for arg in fixed_args {
        call_args.push(arg.clone());
    }
    call_args.push(vec_expr);

    syn::parse_quote! { #fun_expr(#call_args) }
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
            syn::parse_quote! { (#expr as u32) }
        }
        ast::Expr::Ident(id) if id.name == "true" || id.name == "false" => arg.into(),
        _ => compile_expr_with_expected(arg, variadic_elem),
    }
}

fn compile_builtin(call_expr: ast::CallExpr) -> syn::Expr {
    let name = match *call_expr.fun {
        ast::Expr::Ident(ident) => ident.name.to_string(),
        _ => return compile_error_expr("builtin call without builtin identifier"),
    };

    let has_variadic_spread = call_expr.ellipsis.is_some();
    let raw_args: Vec<ast::Expr> = call_expr.args.unwrap_or_default().into_iter().collect();

    match name.as_str() {
        "make" => {
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
        "new" => {
            let Some(type_arg) = raw_args.into_iter().next() else {
                return compile_error_expr("new requires a type argument");
            };
            let type_arg: syn::Type = type_arg.into();
            syn::parse_quote! { Box::new(<#type_arg>::default()) }
        }
        "append" => compile_append_builtin(raw_args, has_variadic_spread),
        "panic" => compile_panic_builtin(raw_args),
        _ => {
            let args: Vec<syn::Expr> = raw_args.into_iter().map(syn::Expr::from).collect();
            match name.as_str() {
                "len" if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::len(&#x) }
                }
                "cap" if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::cap(&#x) }
                }
                "copy" if let [dst, src] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::copy_slice(&mut #dst, &#src) }
                }
                "delete" if let [map, key] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::delete(&mut #map, &#key) }
                }
                "clear" if let [x] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::clear(&mut #x) }
                }
                "close" if let [ch] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::close(&#ch) }
                }
                "max" if let [a, b] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::max(#a, #b) }
                }
                "max" if let [a, b, c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::max3(#a, #b, #c) }
                }
                "min" if let [a, b] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::min(#a, #b) }
                }
                "min" if let [a, b, c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::min3(#a, #b, #c) }
                }
                "complex" if let [re, im] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::complex128(#re, #im) }
                }
                "real" if let [c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::real128(#c) }
                }
                "imag" if let [c] = args.as_slice() => {
                    syn::parse_quote! { crate::builtin::imag128(#c) }
                }
                "recover" => {
                    syn::parse_quote! { String::new() }
                }
                "println" => match args.first() {
                    Some(first) => syn::parse_quote! { crate::builtin::go_println_value(#first) },
                    None => syn::parse_quote! { crate::builtin::go_println_empty() },
                },
                "print" => match args.first() {
                    Some(first) => syn::parse_quote! { crate::builtin::go_print_value(#first) },
                    None => syn::parse_quote! { crate::builtin::go_print_empty() },
                },
                _ => compile_error_expr(format!("invalid builtin call: {name}")),
            }
        }
    }
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

fn elts_to_field_values(type_name: Option<&str>, elts: Vec<syn::Expr>) -> Vec<syn::FieldValue> {
    let struct_fields = type_name
        .map(|name| TYPE_ENV.with(|env| env.borrow().get_struct_fields(name)))
        .unwrap_or_default();
    let mut positional_index = 0usize;

    elts.into_iter()
        .map(|e| {
            if let syn::Expr::Tuple(ref tuple) = e {
                let mut iter = tuple.elems.clone().into_iter();
                if let (Some(syn::Expr::Path(ref path)), Some(value)) = (iter.next(), iter.next()) {
                    if let Some(last_seg) = path.path.segments.last() {
                        return syn::FieldValue {
                            attrs: vec![],
                            member: syn::Member::Named(last_seg.ident.clone()),
                            colon_token: Some(<Token![:]>::default()),
                            expr: value,
                        };
                    }
                }
            }
            if let Some((field_name, _)) = struct_fields.get(positional_index) {
                positional_index += 1;
                let field_name = rust_safe_ident_name(field_name);
                return syn::FieldValue {
                    attrs: vec![],
                    member: syn::Member::Named(syn::Ident::new(&field_name, Span::mixed_site())),
                    colon_token: Some(<Token![:]>::default()),
                    expr: e,
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
                expr: e,
            }
        })
        .collect()
}

fn has_unnamed_field_values(field_values: &[syn::FieldValue]) -> bool {
    field_values
        .iter()
        .any(|fv| matches!(fv.member, syn::Member::Unnamed(_)))
}

fn compile_composite_lit(comp_lit: ast::CompositeLit) -> syn::Expr {
    let elts: Vec<syn::Expr> = comp_lit
        .elts
        .unwrap_or_default()
        .into_iter()
        .map(syn::Expr::from)
        .collect();

    if let Some(type_expr) = comp_lit.type_ {
        match *type_expr {
            ast::Expr::Ident(ident) => {
                let type_ident: syn::Ident = ident.into();
                let type_name = type_ident.to_string();
                let type_kind =
                    TYPE_ENV.with(|env| env.borrow().get_type_kind(&type_name).cloned());
                if matches!(type_kind, Some(typeinfer::TypeKind::Alias(_))) {
                    if elts.is_empty() || elts.iter().any(|elt| matches!(elt, syn::Expr::Tuple(_)))
                    {
                        return syn::parse_quote! { #type_ident::default() };
                    }
                    return syn::parse_quote! { #type_ident(#(#elts),*) };
                }
                let field_values = elts_to_field_values(Some(&type_name), elts);
                if field_values.is_empty() {
                    syn::parse_quote! { #type_ident::default() }
                } else if has_unnamed_field_values(&field_values) {
                    syn::parse_quote! { #type_ident::default() }
                } else {
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
                        dot2_token: Some(<syn::Token![..]>::default()),
                        rest: Some(Box::new(syn::parse_quote! { Default::default() })),
                    })
                }
            }
            ast::Expr::SelectorExpr(sel) => {
                let path: syn::ExprPath = sel.into();
                let type_name = path.path.segments.last().map(|seg| seg.ident.to_string());
                let field_values = elts_to_field_values(type_name.as_deref(), elts);
                if field_values.is_empty() {
                    let p = &path.path;
                    syn::parse_quote! { #p::default() }
                } else if has_unnamed_field_values(&field_values) {
                    let p = &path.path;
                    syn::parse_quote! { #p::default() }
                } else {
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
                        dot2_token: Some(<syn::Token![..]>::default()),
                        rest: Some(Box::new(syn::parse_quote! { Default::default() })),
                    })
                }
            }
            ast::Expr::ArrayType(array_type) => {
                // Slice/array literal: []T{e1, e2, ...} → vec![e1, e2, ...]
                if array_type.len.is_none() {
                    if elts.is_empty() {
                        syn::parse_quote! { Vec::new() }
                    } else {
                        syn::parse_quote! { Vec::from([#(#elts),*]) }
                    }
                } else if elts.is_empty() {
                    let Some(array_len) = array_type.len.as_ref() else {
                        return syn::parse_quote! { Default::default() };
                    };
                    let len = array_len_expr(array_len);
                    syn::parse_quote! { [Default::default(); #len] }
                } else {
                    syn::parse_quote! { [#(#elts),*] }
                }
            }
            ast::Expr::MapType(_) => {
                // Map literal: map[K]V{k1: v1, ...}
                // elts are already (key, value) tuples from KeyValueExpr
                syn::parse_quote! {
                    std::collections::HashMap::from([#(#elts),*])
                }
            }
            _ => {
                // Fallback: treat as array/vec
                if elts.is_empty() {
                    syn::parse_quote! { Vec::new() }
                } else {
                    syn::parse_quote! { Vec::from([#(#elts),*]) }
                }
            }
        }
    } else {
        // No type — nested composite lit in an array/slice context
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

    let ret = compile_return_type(func_lit.type_.results).unwrap_or(syn::ReturnType::Default);

    let block: syn::Block = func_lit.body.try_into().unwrap_or(syn::Block {
        brace_token: syn::token::Brace::default(),
        stmts: vec![],
    });

    if param_types.is_empty() && matches!(ret, syn::ReturnType::Default) {
        syn::parse_quote! { move || #block }
    } else if param_types.is_empty() {
        syn::parse_quote! { move || #ret #block }
    } else {
        let typed_params: Vec<proc_macro2::TokenStream> = params
            .iter()
            .zip(param_types.iter())
            .map(|(p, t)| quote::quote! { #p: #t })
            .collect();
        syn::parse_quote! { move |#(#typed_params),*| #ret #block }
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

    rhs_expr
}

fn compile_expr_with_expected(expr: ast::Expr, expected: Option<&typeinfer::GoType>) -> syn::Expr {
    if matches!(&expr, ast::Expr::Ident(id) if id.name == "nil") {
        return match expected {
            Some(ty) if is_go_byte_slice_type(ty) => syn::parse_quote! { Default::default() },
            Some(typeinfer::GoType::Any | typeinfer::GoType::Interface(_)) => {
                syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> }
            }
            _ => syn::parse_quote! { Default::default() },
        };
    }

    if matches!(expected, Some(typeinfer::GoType::String)) && is_string_literal(&expr) {
        let expr: syn::Expr = expr.into();
        return syn::parse_quote! { #expr.to_string() };
    }

    if let Some(typeinfer::GoType::Named(name)) = expected {
        if is_type_interface(name) {
            let expr: syn::Expr = expr.into();
            return syn::parse_quote! { &mut #expr };
        }
    }

    expr.into()
}

fn call_param_types(fun: &ast::Expr) -> Vec<typeinfer::GoType> {
    TYPE_ENV.with(|env| {
        let env = env.borrow();
        match fun {
            ast::Expr::Ident(id) => env.get_func_params(id.name),
            ast::Expr::SelectorExpr(sel) => {
                if let ast::Expr::Ident(pkg_or_recv) = &*sel.x {
                    let package_key = format!("{}.{}", pkg_or_recv.name, sel.sel.name);
                    let package_params = env.get_func_params(&package_key);
                    if !package_params.is_empty() {
                        return package_params;
                    }

                    if let Some(typeinfer::GoType::Named(name)) = env.get_var(pkg_or_recv.name) {
                        return env.get_func_params(&format!("{}.{}", name, sel.sel.name));
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

fn is_integer_expr(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::BasicLit(lit) => lit.kind == token::Token::INT,
        ast::Expr::Ident(_) => false,
        ast::Expr::CallExpr(_) => false,
        _ => false,
    }
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

fn compile_range_stmt(range_stmt: ast::RangeStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    let inferred_range_type =
        typeinfer::GoType::infer_expr(&range_stmt.x, &TYPE_ENV.with(|e| e.borrow().clone()));
    let is_string = is_string_literal(&range_stmt.x) || inferred_range_type.is_string();
    let is_int = is_integer_expr(&range_stmt.x);
    let x: syn::Expr = range_stmt.x.into();
    let body: syn::Block = range_stmt.body.try_into()?;

    match (range_stmt.key, range_stmt.value) {
        // for i, v := range x
        (Some(key_expr), Some(val_expr)) => {
            let key_pat = expr_to_pat(&key_expr);
            let val_pat = expr_to_pat(&val_expr);
            let pat: syn::Pat = syn::parse_quote! { (#key_pat, #val_pat) };
            if is_string {
                // range over string: iterate (byte_index, char)
                Ok(make_for_loop(
                    pat,
                    syn::parse_quote! { (#x).char_indices() },
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
            let key_pat = expr_to_pat(&key_expr);
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
                } else if is_indexed_range_type(&inferred_range_type) {
                    Ok(make_for_loop(
                        key_pat,
                        syn::parse_quote! { 0..(crate::builtin::len(&#x) as isize) },
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
            } else {
                Ok(make_for_loop(pat, x, body))
            }
        }
        _ => Err(CompilerError::UnsupportedConstruct(
            "range with value but no key".to_string(),
        )),
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
        ast::Expr::CompositeLit(comp_lit) => comp_lit
            .type_
            .as_ref()
            .map(|type_expr| type_from_expr_ref(type_expr)),
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
        _ => None,
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
            .map(type_from_expr_ref)
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
                // Emit as integer (u32) since Go's rune is int32/u32, not Rust's char
                let value = ch as u32;
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

fn compile_binary_expr(binary_expr: ast::BinaryExpr) -> syn::Expr {
    let op = binary_expr.op;
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
    }

    syn::Expr::Binary(binary_expr.into())
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

fn is_type_method_expression_receiver(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::ParenExpr(paren) => is_type_method_expression_receiver(&paren.x),
        ast::Expr::StarExpr(_) | ast::Expr::IndexExpr(_) | ast::Expr::IndexListExpr(_) => true,
        _ => false,
    }
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

impl TryFrom<ast::BlockStmt<'_>> for syn::Block {
    type Error = CompilerError;

    fn try_from(block_stmt: ast::BlockStmt) -> Result<Self, Self::Error> {
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

        let func = compile_call_function_expr(*call_expr.fun);

        let mut args = syn::punctuated::Punctuated::new();
        if let Some(cargs) = call_expr.args {
            for (idx, arg) in cargs.into_iter().enumerate() {
                args.push(compile_expr_with_expected(arg, param_types.get(idx)))
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
            ast::Expr::BasicLit(basic_lit) => Self::Lit(basic_lit.into()),
            ast::Expr::BinaryExpr(binary_expr) => compile_binary_expr(binary_expr),
            ast::Expr::CallExpr(call_expr) => {
                if let Some(kind) = detect_type_conversion(&call_expr) {
                    return compile_type_conversion(call_expr, kind);
                }
                if call_expr.args.as_ref().is_some_and(|args| args.len() == 1)
                    && is_general_type_conversion_fun(&call_expr.fun)
                {
                    return compile_general_type_conversion(call_expr);
                }
                if is_builtin_call(&call_expr) {
                    return compile_builtin(call_expr);
                }
                if let Some(lowered) = lowered_stdlib_call(&call_expr) {
                    return compile_lowered_stdlib_call(call_expr, lowered);
                }
                if is_lowered_stdlib_method_call(&call_expr) {
                    return compile_lowered_stdlib_method_call(call_expr);
                }
                if let Some(variadic_start) = is_variadic_any_call(&call_expr) {
                    return compile_variadic_any_call(call_expr, variadic_start);
                }
                let param_types = call_param_types(&call_expr.fun);
                // Detect method call vs package function call
                let is_method_call = matches!(&*call_expr.fun, ast::Expr::SelectorExpr(sel) if {
                    match &*sel.x {
                        ast::Expr::Ident(id) => !IMPORT_NAMES.with(|names| names.borrow().contains(id.name)),
                        _ => true,
                    }
                });
                if is_method_call {
                    if let ast::Expr::SelectorExpr(sel) = *call_expr.fun {
                        let receiver: syn::Expr = (*sel.x).into();
                        let method: syn::Ident = sel.sel.into();
                        let mut args = syn::punctuated::Punctuated::new();
                        if let Some(cargs) = call_expr.args {
                            for (idx, arg) in cargs.into_iter().enumerate() {
                                args.push(compile_expr_with_expected(arg, param_types.get(idx)));
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
            ast::Expr::Ident(ident) if is_string_const_fn(ident.name) => {
                let ident: syn::Ident = ident.into();
                syn::parse_quote! { #ident() }
            }
            ast::Expr::Ident(ident) => Self::Path(ident.into()),
            ast::Expr::SelectorExpr(selector_expr) => {
                if is_type_method_expression_receiver(&selector_expr.x) {
                    return syn::parse_quote! { |_| {} };
                }
                let is_package = match &*selector_expr.x {
                    ast::Expr::Ident(id) => {
                        IMPORT_NAMES.with(|names| names.borrow().contains(id.name))
                    }
                    _ => false,
                };
                if is_package {
                    Self::Path(selector_expr.into())
                } else {
                    let base: syn::Expr = (*selector_expr.x).into();
                    let field: syn::Ident = selector_expr.sel.into();
                    syn::Expr::Field(syn::ExprField {
                        attrs: vec![],
                        base: Box::new(base),
                        dot_token: <Token![.]>::default(),
                        member: syn::Member::Named(field),
                    })
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
                    // &x → Box::new(x)
                    let inner: syn::Expr = (*unary_expr.x).into();
                    Self::Call(syn::ExprCall {
                        attrs: vec![],
                        func: Box::new(syn::parse_quote! { Box::new }),
                        paren_token: syn::token::Paren::default(),
                        args: {
                            let mut a = syn::punctuated::Punctuated::new();
                            a.push(inner);
                            a
                        },
                    })
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
                let idx: syn::Expr = (*index_expr.index).into();

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
            ast::Expr::StarExpr(star_expr) => Self::Unary(syn::ExprUnary {
                attrs: vec![],
                op: syn::UnOp::Deref(<Token![*]>::default()),
                expr: Box::new((*star_expr.x).into()),
            }),
            ast::Expr::CompositeLit(comp_lit) => compile_composite_lit(comp_lit),
            ast::Expr::FuncLit(func_lit) => compile_func_lit(func_lit),
            ast::Expr::SliceExpr(slice_expr) => compile_slice_expr(slice_expr),
            ast::Expr::TypeAssertExpr(ta) => {
                // x.(T) → downcast or type check
                let x: syn::Expr = (*ta.x).into();
                if let Some(type_expr) = ta.type_ {
                    let ty: syn::Type = (*type_expr).into();
                    if let Some(inner_ty) = box_inner_type(&ty) {
                        syn::parse_quote! {{
                            let __gors_any = (#x) as Box<dyn std::any::Any>;
                            match __gors_any.downcast::<#ty>() {
                                Ok(__gors_value) => *__gors_value,
                                Err(__gors_any) => match __gors_any.downcast::<#inner_ty>() {
                                    Ok(__gors_value) => Box::new(*__gors_value),
                                    Err(_) => Default::default(),
                                },
                            }
                        }}
                    } else {
                        syn::parse_quote! {
                            match ((#x) as Box<dyn std::any::Any>).downcast::<#ty>() {
                                Ok(__gors_value) => *__gors_value,
                                Err(_) => Default::default(),
                            }
                        }
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
                syn::parse_quote! { Box<#inner> }
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
            ast::Expr::FuncType(func_type) => {
                let mut param_types = syn::punctuated::Punctuated::<syn::Type, Token![,]>::new();
                for field in func_type.params.list {
                    if let Some(type_expr) = field.type_ {
                        let ty: syn::Type = type_expr.into();
                        let count = field.names.as_ref().map_or(1, |n| n.len());
                        for _ in 0..count {
                            param_types.push(ty.clone());
                        }
                    }
                }
                if let Some(results) = func_type.results {
                    let mut result_fields = results.list;
                    if result_fields.len() == 1 {
                        let field = result_fields.remove(0);
                        let ret_type: syn::Type = field
                            .type_
                            .map(Into::into)
                            .unwrap_or_else(|| syn::parse_quote! { () });
                        syn::parse_quote! { fn(#param_types) -> #ret_type }
                    } else {
                        let mut ret_types =
                            syn::punctuated::Punctuated::<syn::Type, Token![,]>::new();
                        for field in result_fields {
                            if let Some(type_expr) = field.type_ {
                                ret_types.push(type_expr.into());
                            }
                        }
                        syn::parse_quote! { fn(#param_types) -> (#ret_types) }
                    }
                } else {
                    syn::parse_quote! { fn(#param_types) }
                }
            }
            ast::Expr::ChanType(chan_type) => {
                // chan T → crate::builtin::GoChan<T>
                let inner: syn::Type = (*chan_type.value).into();
                syn::parse_quote! { crate::builtin::GoChan<#inner> }
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

        // Track import names for selector expr disambiguation
        IMPORT_NAMES.with(|names| {
            let mut set = names.borrow_mut();
            set.clear();
            for import in file.imports() {
                let path_str = import.path.value.trim_matches('"');
                if let Some(pkg_name) = path_str.rsplit('/').next() {
                    set.insert(pkg_name.to_string());
                }
            }
        });

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
                                    let has_type = vs.type_.is_some();
                                    let type_expr = vs.type_;
                                    let mut values_iter = vs.values.unwrap_or_default().into_iter();

                                    for name in names {
                                        let ident: syn::Ident = name.into();
                                        let init_expr: Option<syn::Expr> =
                                            values_iter.next().map(|v| v.into());

                                        if let Some(init) = init_expr {
                                            package_var_stmts.push(syn::parse_quote! {
                                                let mut #ident = #init;
                                            });
                                        } else if has_type {
                                            let type_name =
                                                type_expr.as_ref().and_then(go_type_name_from_expr);
                                            let zero = zero_value_for_type(type_name);
                                            package_var_stmts.push(syn::parse_quote! {
                                                let mut #ident = #zero;
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
            let generics = generics_for_idents(&type_args);
            let self_ty: syn::Type = if type_args.is_empty() {
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
                    let trait_ident = syn::Ident::new(trait_name, Span::mixed_site());
                    let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());

                    // Get method implementations from the methods map
                    let mut impl_items: Vec<syn::ImplItem> = vec![];
                    if let Some(method_list) = methods.get(struct_name) {
                        for method in method_list {
                            if required_methods.contains(&method.sig.ident.to_string()) {
                                let mut m = method.clone();
                                m.vis = syn::Visibility::Inherited;
                                if let Some(syn::FnArg::Receiver(receiver)) =
                                    m.sig.inputs.first_mut()
                                {
                                    receiver.mutability = Some(<Token![mut]>::default());
                                    receiver.ty = Box::new(syn::parse_quote! { &mut Self });
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
                        generics: syn::Generics::default(),
                        trait_: Some((
                            None,
                            syn::parse_quote! { #trait_ident },
                            <Token![for]>::default(),
                        )),
                        self_ty: Box::new(syn::parse_quote! { #struct_ident }),
                        brace_token: syn::token::Brace::default(),
                        items: impl_items,
                    }));
                }
            }
        }

        // Stringer pattern: generate `impl Display` for structs with String() string
        for struct_name in &struct_has_string_method {
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

fn compile_field_to_fn_args(field: ast::Field) -> Result<Vec<syn::FnArg>, CompilerError> {
    let type_expr = field
        .type_
        .ok_or_else(|| CompilerError::InvalidFunctionSignature("field has no type".to_string()))?;
    let go_type = typeinfer::GoType::from_expr(&type_expr);
    let mut rust_type: syn::Type = type_expr.into();
    let names = field.names.unwrap_or_else(|| {
        vec![ast::Ident {
            name_pos: token::Position::default(),
            name: "",
            obj: None,
        }]
    });

    // Go strings map to String in Rust (owned). Parameters keep String type
    // since Go allows reassigning string parameters within functions.

    // Use &mut dyn Trait for interface parameters
    if let typeinfer::GoType::Named(ref name) = go_type {
        if is_type_interface(name) || TYPE_ENV.with(|env| env.borrow().is_interface(name)) {
            rust_type = syn::parse_quote! { &mut dyn #rust_type };
        }
    }

    let mut args = Vec::new();
    for name in names {
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

        // Register parameter types in the type environment
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

        let mut inputs = syn::punctuated::Punctuated::new();
        for param in func_decl.type_.params.list {
            for arg in compile_field_to_fn_args(param)? {
                inputs.push(arg);
            }
        }

        let vis = (&func_decl.name).into();

        // Analyze return values for named returns
        let mut named_return_info: Vec<(syn::Ident, syn::Expr)> = vec![];
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
                            let zero = zero_value_for_type(type_name);
                            let ident = syn::Ident::new(
                                &rust_safe_ident_name(name.name),
                                Span::mixed_site(),
                            );
                            named_return_info.push((ident.clone(), zero));
                            named_return_idents.push(ident);
                        }
                    }
                }
            }
        }

        let output = compile_return_type(func_decl.type_.results)?;

        let mut block = Box::new(if let Some(body) = func_decl.body {
            body.try_into()?
        } else {
            syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: vec![],
            }
        });

        // For named returns: prepend variable declarations and rewrite bare returns
        if !named_return_info.is_empty() {
            let mut prepend: Vec<syn::Stmt> = vec![];
            for (ident, zero) in &named_return_info {
                prepend.push(syn::parse_quote! {
                    let mut #ident = #zero;
                });
            }
            let existing = std::mem::take(&mut block.stmts);
            block.stmts = prepend;
            block.stmts.extend(existing);

            rewrite_bare_returns(&mut block, &named_return_idents);

            // If the last statement is NOT a return, add an implicit return
            let needs_implicit_return = !block
                .stmts
                .last()
                .is_some_and(|last| matches!(last, syn::Stmt::Expr(syn::Expr::Return(_), _)));

            if needs_implicit_return {
                if let Some(ident) = (named_return_idents.len() == 1)
                    .then(|| named_return_idents.first())
                    .flatten()
                {
                    block.stmts.push(syn::Stmt::Expr(
                        syn::Expr::Return(syn::ExprReturn {
                            attrs: vec![],
                            return_token: <Token![return]>::default(),
                            expr: Some(Box::new(syn::parse_quote! { #ident })),
                        }),
                        None,
                    ));
                } else {
                    let idents = &named_return_idents;
                    block.stmts.push(syn::Stmt::Expr(
                        syn::Expr::Return(syn::ExprReturn {
                            attrs: vec![],
                            return_token: <Token![return]>::default(),
                            expr: Some(Box::new(syn::parse_quote! { (#(#idents),*) })),
                        }),
                        None,
                    ));
                }
            }
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

        Self::new(&import_rust_name(ident.name), Span::mixed_site())
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
                let call: syn::Expr = ast::Expr::CallExpr(s.call).into();
                let n = DEFER_COUNTER.with(|c| {
                    let mut val = c.borrow_mut();
                    let n = *val;
                    *val += 1;
                    n
                });
                let defer_ident = quote::format_ident!("_defer_{}", n);
                Ok(vec![syn::parse_quote! {
                    let #defer_ident = {
                        struct __Defer<F: FnOnce()>(Option<F>);
                        impl<F: FnOnce()> Drop for __Defer<F> {
                            fn drop(&mut self) { if let Some(f) = self.0.take() { f(); } }
                        }
                        __Defer(Some(move || { #call; }))
                    };
                }])
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
                    let block: syn::Block = func_lit.body.try_into()?;
                    let stmts = &block.stmts;
                    let clones = extract_idents_for_clone(&block);
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
                    let call: syn::ExprCall = call_expr.into();
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
            ast::Stmt::IncDecStmt(s) => Ok(s.into()),
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
                stmts.push(syn::Stmt::Expr(s.try_into()?, None));
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

impl From<ast::IncDecStmt<'_>> for Vec<syn::Stmt> {
    fn from(inc_dec_stmt: ast::IncDecStmt) -> Self {
        let x: syn::Expr = inc_dec_stmt.x.into();
        match inc_dec_stmt.tok {
            token::Token::INC => vec![syn::parse_quote! { #x += 1; }],
            token::Token::DEC => vec![syn::parse_quote! { #x -= 1; }],
            _ => vec![],
        }
    }
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
            // Rust doesn't have goto - would need restructuring
            GOTO => vec![],
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
        let stmt = *labeled_stmt.stmt;
        if let ast::Stmt::ForStmt(for_stmt) = stmt {
            let mut expr = syn::Expr::try_from(for_stmt)?;
            if label_outer_loop(&mut expr, label_ident.clone()) {
                return Ok(vec![syn::Stmt::Expr(expr, None)]);
            }

            let inner_stmts = vec![syn::Stmt::Expr(expr, None)];
            return Ok(vec![syn::Stmt::Expr(
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
                None,
            )]);
        }

        let inner_stmts: Vec<syn::Stmt> = stmt.try_into()?;

        Ok(inner_stmts)
    }
}

fn label_outer_loop(expr: &mut syn::Expr, label_ident: syn::Ident) -> bool {
    let label = syn::Label {
        name: syn::Lifetime {
            apostrophe: Span::call_site(),
            ident: label_ident,
        },
        colon_token: <Token![:]>::default(),
    };

    match expr {
        syn::Expr::While(while_expr) => {
            while_expr.label = Some(label);
            true
        }
        syn::Expr::Loop(loop_expr) => {
            loop_expr.label = Some(label);
            true
        }
        syn::Expr::ForLoop(for_loop) => {
            for_loop.label = Some(label);
            true
        }
        syn::Expr::Block(block_expr) => block_expr
            .block
            .stmts
            .last_mut()
            .and_then(stmt_expr_mut)
            .is_some_and(|expr| label_outer_loop(expr, label.name.ident)),
        _ => false,
    }
}

fn stmt_expr_mut(stmt: &mut syn::Stmt) -> Option<&mut syn::Expr> {
    match stmt {
        syn::Stmt::Expr(expr, _) => Some(expr),
        _ => None,
    }
}

impl TryFrom<ast::ForStmt<'_>> for syn::Expr {
    type Error = CompilerError;

    fn try_from(for_stmt: ast::ForStmt) -> Result<Self, Self::Error> {
        let mut stmts = vec![];

        if let Some(init) = for_stmt.init {
            stmts.extend(Vec::<syn::Stmt>::try_from(*init)?);
        }

        let mut body: syn::Block = for_stmt.body.try_into()?;

        if let Some(post) = for_stmt.post {
            let post_stmts = Vec::<syn::Stmt>::try_from(*post)?;

            if has_unlabeled_continue(&body.stmts) {
                // Go's `continue` executes the post statement before re-checking
                // the condition. Rust's `continue` skips to the condition directly.
                // Fix: wrap body in `'body: { ... }` and rewrite `continue` as
                // `break 'body` so control falls through to the post statements.
                rewrite_continue_as_break_body(&mut body.stmts);

                let labeled_body = syn::Stmt::Expr(
                    Self::Block(syn::ExprBlock {
                        attrs: vec![],
                        label: Some(syn::Label {
                            name: syn::Lifetime::new("'body", Span::mixed_site()),
                            colon_token: <Token![:]>::default(),
                        }),
                        block: body,
                    }),
                    Some(<Token![;]>::default()),
                );

                let mut loop_stmts = vec![labeled_body];
                loop_stmts.extend(post_stmts);

                body = syn::Block {
                    brace_token: syn::token::Brace::default(),
                    stmts: loop_stmts,
                };
            } else {
                body.stmts.extend(post_stmts);
            }
        }

        stmts.push(syn::Stmt::Expr(
            if let Some(cond) = for_stmt.cond {
                Self::While(syn::ExprWhile {
                    attrs: vec![],
                    label: None,
                    cond: Box::new(cond.into()),
                    body,
                    while_token: <Token![while]>::default(),
                })
            } else {
                Self::Loop(syn::ExprLoop {
                    attrs: vec![],
                    label: None,
                    body,
                    loop_token: <Token![loop]>::default(),
                })
            },
            None,
        ));

        Ok(Self::Block(syn::ExprBlock {
            attrs: vec![],
            label: None,
            block: syn::Block {
                stmts,
                brace_token: syn::token::Brace::default(),
            },
        }))
    }
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

        // Separate default from cases
        let mut cases: Vec<ast::CaseClause> = Vec::new();
        let mut default_body: Option<Vec<ast::Stmt>> = None;
        for clause in clauses {
            if clause.list.is_none() {
                default_body = Some(clause.body);
            } else {
                cases.push(clause);
            }
        }

        // Build the if/else chain from bottom up
        let else_block: Option<syn::Expr> = if let Some(body) = default_body {
            let mut stmts = vec![];
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

        let tag_syn: Option<syn::Expr> = switch_stmt.tag.map(Into::into);

        let mut result: Option<syn::Expr> = else_block;
        for case in cases.into_iter().rev() {
            let cond = build_case_condition(case.list, tag_syn.as_ref())?;
            let mut body_stmts = vec![];
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

        result.ok_or_else(|| {
            CompilerError::UnsupportedConstruct("empty switch statement".to_string())
        })
    }
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

fn has_unlabeled_continue(stmts: &[syn::Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        syn::Stmt::Expr(syn::Expr::Continue(cont), _) => cont.label.is_none(),
        syn::Stmt::Expr(expr, _) => has_unlabeled_continue_in_expr(expr),
        _ => false,
    })
}

fn has_unlabeled_continue_in_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::If(if_expr) => {
            has_unlabeled_continue(&if_expr.then_branch.stmts)
                || if_expr
                    .else_branch
                    .as_ref()
                    .is_some_and(|(_, e)| has_unlabeled_continue_in_expr(e))
        }
        syn::Expr::Block(block) => has_unlabeled_continue(&block.block.stmts),
        syn::Expr::While(_) | syn::Expr::Loop(_) | syn::Expr::ForLoop(_) => false,
        _ => false,
    }
}

/// Rewrite unlabeled `continue;` to `break 'body;` in a statement list.
/// Recurses into if/else and blocks but stops at nested loops (which have
/// their own continue targets).
fn rewrite_continue_as_break_body(stmts: &mut [syn::Stmt]) {
    for stmt in stmts.iter_mut() {
        match stmt {
            syn::Stmt::Expr(syn::Expr::Continue(cont), semi) if cont.label.is_none() => {
                *stmt = syn::Stmt::Expr(
                    syn::Expr::Break(syn::ExprBreak {
                        attrs: vec![],
                        break_token: <Token![break]>::default(),
                        label: Some(syn::Lifetime::new("'body", Span::mixed_site())),
                        expr: None,
                    }),
                    *semi,
                );
            }
            syn::Stmt::Expr(expr, _) => rewrite_continue_in_expr(expr),
            _ => {}
        }
    }
}

fn rewrite_continue_in_expr(expr: &mut syn::Expr) {
    match expr {
        syn::Expr::If(if_expr) => {
            rewrite_continue_as_break_body(&mut if_expr.then_branch.stmts);
            if let Some((_, else_expr)) = &mut if_expr.else_branch {
                rewrite_continue_in_expr(else_expr);
            }
        }
        syn::Expr::Block(block) => {
            rewrite_continue_as_break_body(&mut block.block.stmts);
        }
        // Don't recurse into loops — they have their own continue targets
        syn::Expr::While(_) | syn::Expr::Loop(_) | syn::Expr::ForLoop(_) => {}
        _ => {}
    }
}

impl From<ast::DeclStmt<'_>> for Vec<syn::Stmt> {
    fn from(decl_stmt: ast::DeclStmt) -> Self {
        let gen_decl = decl_stmt.decl;
        let mut stmts = vec![];

        for spec in gen_decl.specs {
            match spec {
                ast::Spec::ValueSpec(value_spec) => {
                    let names = value_spec.names;
                    let go_type = value_spec.type_.as_ref().map(typeinfer::GoType::from_expr);
                    let rust_type: Option<syn::Type> = value_spec.type_.map(syn::Type::from);
                    let mut values_iter = value_spec.values.unwrap_or_default().into_iter();

                    for name in names {
                        let ident: syn::Ident = name.into();
                        if let Some(ref go_type) = go_type {
                            TYPE_ENV.with(|env| {
                                env.borrow_mut()
                                    .set_var(&ident.to_string(), go_type.clone());
                            });
                        }
                        let init_expr: Option<syn::Expr> = values_iter.next().map(|v| v.into());

                        let init = init_expr
                            .unwrap_or_else(|| go_zero_value_from_type(rust_type.as_ref()));
                        if let Some(ref ty) = rust_type {
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

    if matches!(kind, CommaOkKind::TypeAssert) {
        if let Some(interface_name) = type_assert_interface_name(&rhs) {
            let fallback: syn::Expr = match interface_name.as_str() {
                "Formatter" | "Stringer" | "GoStringer" => {
                    syn::parse_quote! { __GorsNoopInterface::default() }
                }
                "error" => syn::parse_quote! { String::new() },
                _ => syn::parse_quote! { Default::default() },
            };
            if is_define {
                return Ok(vec![syn::parse_quote! {
                    let (#val_pat, #ok_pat) = (#fallback, false);
                }]);
            }

            let mut lhs_iter = lhs.into_iter();
            let val_lhs = lhs_iter.next().ok_or_else(|| {
                CompilerError::InvalidAssignment("missing comma-ok value lhs".to_string())
            })?;
            let ok_lhs = lhs_iter.next().ok_or_else(|| {
                CompilerError::InvalidAssignment("missing comma-ok ok lhs".to_string())
            })?;
            let val_e = comma_ok_lhs_expr(val_lhs);
            let ok_e = comma_ok_lhs_expr(ok_lhs);
            let rhs_expr: syn::Expr = syn::parse_quote! { (#fallback, false) };
            return Ok(comma_ok_assignment_stmts(vec![val_e, ok_e], rhs_expr));
        }
    }

    let rhs_expr: syn::Expr = match kind {
        CommaOkKind::MapIndex => {
            if let ast::Expr::IndexExpr(ie) = rhs {
                let map_e: syn::Expr = (*ie.x).into();
                let key_e: syn::Expr = (*ie.index).into();
                syn::parse_quote! {
                    match (#map_e).get(&#key_e) {
                        Some(__v) => (__v.clone(), true),
                        None => (Default::default(), false),
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
                let x_e: syn::Expr = (*ta.x).into();
                let Some(type_expr) = ta.type_ else {
                    return Err(CompilerError::InvalidAssignment(
                        "comma-ok type assertion without asserted type".to_string(),
                    ));
                };
                let ty: syn::Type = (*type_expr).into();
                syn::parse_quote! {
                    match (&*#x_e as &dyn std::any::Any).downcast_ref::<#ty>() {
                        Some(__v) => (__v.clone(), true),
                        None => (Default::default(), false),
                    }
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
        let lhs_exprs = lhs.into_iter().map(comma_ok_lhs_expr).collect();
        Ok(comma_ok_assignment_stmts(lhs_exprs, rhs_expr))
    }
}

fn comma_ok_lhs_expr(expr: ast::Expr) -> Option<syn::Expr> {
    if matches!(&expr, ast::Expr::Ident(id) if id.name == "_") {
        None
    } else {
        Some(expr.into())
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

fn type_assert_interface_name(rhs: &ast::Expr) -> Option<String> {
    let ast::Expr::TypeAssertExpr(ta) = rhs else {
        return None;
    };
    let type_expr = ta.type_.as_ref()?;
    match &**type_expr {
        ast::Expr::Ident(id) if id.name == "error" => Some(id.name.to_string()),
        ast::Expr::Ident(id) if is_type_interface(id.name) => Some(id.name.to_string()),
        ast::Expr::Ident(id) if TYPE_ENV.with(|env| env.borrow().is_interface(id.name)) => {
            Some(id.name.to_string())
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
                        let left: syn::Expr = lhs.into();
                        assignments.push(syn::parse_quote! { #left = #tmp; });
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
                    first_rhs.into()
                }
                _ => {
                    let mut elems = syn::punctuated::Punctuated::new();
                    for expr in assign_stmt.rhs {
                        elems.push(expr.into())
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
                let (lhs_ty, rhs_ty) = infer_assignment_types(&lhs_ast, &rhs_ast);
                let right_raw = compile_expr_with_expected(rhs_ast, Some(&lhs_ty));
                let right = coerce_assignment_expr(&lhs_ty, &rhs_ty, right_raw);
                let left: syn::Expr = lhs_ast.into();
                return Ok(vec![syn::parse_quote! { #left = #right; }]);
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
                let left: syn::Expr = lhs.into();
                out.push(syn::parse_quote! { #left = #right; });
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
            let left: syn::Expr = assign_stmt
                .lhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?
                .into();
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
            let left: syn::Expr = assign_stmt
                .lhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?
                .into();
            let right: syn::Expr = assign_stmt
                .rhs
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?
                .into();
            let op: syn::BinOp = assign_stmt.tok.into();
            return Ok(vec![syn::parse_quote! { #left #op #right; }]);
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

        let expr = match return_stmt.results.len() {
            0 => None,
            1 => Some(syn::Expr::from(
                return_stmt.results.into_iter().next().unwrap_or_else(|| {
                    ast::Expr::Ident(ast::Ident {
                        name_pos: token::Position::default(),
                        name: "__gors_missing_return",
                        obj: None,
                    })
                }),
            )),
            _ => {
                let mut elems = syn::punctuated::Punctuated::new();
                for result in return_stmt.results {
                    elems.push(result.into());
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
    use crate::backend_rust;
    use crate::parser::parse_file;
    use quote::quote;
    use syn::parse_quote as rust;

    fn test(go_input: &str, expected: syn::File) {
        let parsed = parse_file("test.go", go_input).unwrap();
        let compiled = compile(parsed).unwrap();
        let output = (quote! {#compiled}).to_string();
        let expected = (quote! {#expected}).to_string();
        assert_eq!(output, expected);
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
        let rust_source = backend_rust::generate(compiled).unwrap();

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
        let rust_source = backend_rust::generate(compiled).unwrap();

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
    fn compile_program_multi_hello_world() {
        let go_source =
            "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hello\")\n}\n";
        let ast = parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec!["fmt".to_string()],
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
        assert!(builtin_item_names.contains("go_fmt_println"));
        assert!(builtin_item_names.contains("go_fmt_sprintln"));
        assert!(!builtin_item_names.contains("append"));
    }

    #[test]
    fn compile_program_multi_generates_valid_rust() {
        let go_source =
            "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"test\")\n}\n";
        let ast = parse_file("main.go", go_source).unwrap();
        let program = crate::parser::ParsedProgram {
            main_package: crate::parser::ParsedPackage {
                name: "main".to_string(),
                import_path: String::new(),
                ast,
                files: vec![("main.go".to_string(), go_source.to_string())],
            },
            imports: vec![],
            stdlib_imports: vec!["fmt".to_string()],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        let output = backend_rust::generate_multi(compiled).unwrap();
        assert!(output.files.contains_key("main.rs"));
        assert!(output.files.contains_key("lib.rs"));
        assert!(output.files.contains_key("builtin.rs"));
        assert!(!output.files.contains_key("fmt.rs"));
        let main_rs = output.files.get("main.rs").unwrap();
        assert!(main_rs.contains("mod lib"));
        assert!(main_rs.contains("use lib::{"));
        assert!(main_rs.contains("builtin"));
        assert!(main_rs.contains("go_fmt_println"));
        let lib_rs = output.files.get("lib.rs").unwrap();
        assert!(lib_rs.contains("pub mod builtin"));
        assert!(!lib_rs.contains("pub mod fmt"));
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
                #[derive(Clone, Default)]
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
                #[derive(Clone, Default)]
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
                impl crate::builtin::GoLen for buffer {
                    fn go_len(&self) -> usize { self.0.len() }
                }
                impl crate::builtin::GoCap for buffer {
                    fn go_cap(&self) -> usize { self.0.capacity() }
                }
                impl crate::builtin::GoString for buffer {
                    fn go_string(self) -> String {
                        String::from_utf8(self.0).unwrap_or_default()
                    }
                }
                impl crate::builtin::GoString for &buffer {
                    fn go_string(self) -> String {
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
                impl crate::builtin::GoAppend<u8> for buffer {
                    fn go_append(mut self, elem: u8) -> Self {
                        self.0.push(elem);
                        self
                    }
                }
                impl crate::builtin::GoAppend<Vec<u8>> for buffer {
                    fn go_append(mut self, elem: Vec<u8>) -> Self {
                        self.0.extend(elem);
                        self
                    }
                }
                impl crate::builtin::GoAppend<buffer> for Vec<u8> {
                    fn go_append(mut self, elem: buffer) -> Self {
                        self.extend(elem.0);
                        self
                    }
                }
                impl crate::builtin::GoAppend<String> for buffer {
                    fn go_append(mut self, elem: String) -> Self {
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
                fn deref(mut p: Box<isize>) -> isize {
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
                #[derive(Clone, Default)]
                pub struct Node {
                    pub Value: isize,
                    pub Next: Box<Node>,
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
                #[derive(Clone, Default)]
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
                #[derive(Clone, Default)]
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
                #[derive(Clone, Default)]
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
                }
            "#,
            rust! {
                #[derive(Clone, Default)]
                pub struct Point {
                    pub X: isize,
                    pub Y: isize,
                }
                pub fn main() {
                    let mut p = Point { X: 1, Y: 2, .. Default::default() };
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
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                }
            },
        );
    }

    #[test]
    fn it_should_compile_nil() {
        test(
            r#"
                package main

                func main() {
                    x := nil
                }
            "#,
            rust! {
                pub fn main() {
                    let mut x = Default::default();
                }
            },
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
                    }
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    for (mut i, mut v) in (s).iter().cloned().enumerate().map(|(i, v)| (i as isize, v)) {
                        let mut x = i + v;
                    }
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
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    let mut t = (s[(1) as usize..(2) as usize]).to_vec();
                }
            },
        );
    }

    #[test]
    fn it_should_compile_address_of_as_box() {
        test(
            r#"
                package main

                func main() {
                    x := 42
                    p := &x
                }
            "#,
            rust! {
                pub fn main() {
                    let mut x = 42;
                    let mut p = Box::new(x);
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
                    f := func() { x := 1 }
                }
            "#,
            rust! {
                pub fn main() {
                    let mut f = move || { let mut x = 1; };
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
                }
            "#,
            rust! {
                pub fn main() {
                    let mut s = Vec::from([1, 2, 3]);
                    let mut n = (crate::builtin::len(&s) as isize);
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
                    s = crate::builtin::append(s, 3);
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
                    s = crate::builtin::append(crate::builtin::append(s, 3), 4);
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
                    crate::builtin::go_println_value("hello");
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
                    let _defer_0 = {
                        struct __Defer<F: FnOnce()>(Option<F>);
                        impl<F: FnOnce()> Drop for __Defer<F> {
                            fn drop(&mut self) {
                                if let Some(f) = self.0.take() {
                                    f();
                                }
                            }
                        }
                        __Defer(Some(move || { cleanup(); }))
                    };
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
                }
            "#,
            rust! {
                #[derive(Clone, Default)]
                pub struct Flags {
                    pub X: bool,
                }
                pub fn main() {
                    let mut f = Flags::default();
                }
            },
        );
    }

    #[test]
    fn compile_program_multi_prunes_lowered_stdlib_imports() {
        let go_source = r#"package main

import "fmt"
import "errors"
import "strconv"
import "sort"

func main() {
	fmt.Println("hello")
	e := errors.New("fail")
	s := strconv.Itoa(42)
	xs := []int{3, 1}
	sort.Ints(xs)
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
            stdlib_imports: vec![
                "errors".to_string(),
                "fmt".to_string(),
                "sort".to_string(),
                "strconv".to_string(),
            ],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        assert!(!compiled.modules.contains_key("fmt"));
        assert!(compiled.modules.contains_key("strconv"));
        assert!(!compiled.modules.contains_key("errors"));
        assert!(!compiled.modules.contains_key("sort"));
        let output = backend_rust::generate_multi(compiled).unwrap();
        assert!(!output.files.contains_key("fmt.rs"));
        assert!(output.files.contains_key("strconv.rs"));
        assert!(!output.files.contains_key("errors.rs"));
        assert!(!output.files.contains_key("sort.rs"));
        let lib_rs = output.files.get("lib.rs").unwrap();
        assert!(!lib_rs.contains("pub mod fmt"));
        assert!(lib_rs.contains("pub mod strconv"));
        assert!(!lib_rs.contains("pub mod errors"));
        assert!(!lib_rs.contains("pub mod sort"));
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
    fn it_should_support_blank_identifier_in_define() {
        test(
            r#"
                package main

                func main() {
                    _, b := 1, 2
                }
            "#,
            rust! {
                pub fn main() {
                    let (_, mut b) = (1, 2);
                }
            },
        )
    }

    // --- Concurrency tests (Agent 3) ---

    /// Helper that compiles Go source and generates Rust source code.
    fn go_to_rust(go_input: &str) -> String {
        let parsed = parse_file("test.go", go_input).unwrap();
        let compiled = compile(parsed).unwrap();
        backend_rust::generate(compiled).unwrap()
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
                    let mut x = 0;
                    let mut y = 0;
                    x = b;
                    y = a;
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
                fn newInt(mut x: isize) -> Box<isize> {
                    Box::new(x)
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
                    x
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
                pub fn Max<T: PartialOrd + Copy + std::fmt::Display>(mut a: T, mut b: T) -> T {
                    if a > b {
                        return a
                    }
                    b
                }
                pub fn main() {}
            },
        );
    }
}
