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
mod passes;

use crate::mapping::SourceMapTracker;
use crate::{ast, token};
use proc_macro2::Span;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use syn::Token;

// Thread-local storage for source map tracker during compilation
thread_local! {
    static TRACKER: RefCell<SourceMapTracker> = RefCell::new(SourceMapTracker::new());
    static DEFER_COUNTER: RefCell<usize> = RefCell::new(0);
    static IMPORT_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
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
                if let Some(ch) = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    result.push(ch);
                } else {
                    result.push('\\');
                    result.push('u');
                    result.push_str(&hex);
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
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
                    let name = syn::Ident::new(ident.name, Span::mixed_site());
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
                token::Token::ADD => Some(lhs + rhs),
                token::Token::SUB => Some(lhs - rhs),
                token::Token::MUL => Some(lhs * rhs),
                token::Token::QUO => {
                    if rhs == 0 {
                        None
                    } else {
                        Some(lhs / rhs)
                    }
                }
                token::Token::REM => {
                    if rhs == 0 {
                        None
                    } else {
                        Some(lhs % rhs)
                    }
                }
                token::Token::SHL => Some(lhs << rhs),
                token::Token::SHR => Some(lhs >> rhs),
                token::Token::AND => Some(lhs & rhs),
                token::Token::OR => Some(lhs | rhs),
                token::Token::XOR => Some(lhs ^ rhs),
                _ => None,
            }
        }
        ast::Expr::ParenExpr(paren) => const_eval_expr(&paren.x, iota_value),
        ast::Expr::UnaryExpr(unary) => {
            let val = const_eval_expr(&unary.x, iota_value)?;
            match unary.op {
                token::Token::SUB => Some(-val),
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
        let value_spec = &value_specs[spec_idx];

        let has_own_values = value_spec.values.is_some();
        if has_own_values {
            last_valued_idx = Some(spec_idx);
        }

        let source_idx = if has_own_values {
            Some(spec_idx)
        } else {
            last_valued_idx
        };

        let source_values = source_idx.and_then(|idx| value_specs[idx].values.as_ref());

        // Determine the type name string (from this spec or inherited)
        let type_name_str: Option<&str> = if let Some(ref te) = value_spec.type_ {
            if let ast::Expr::Ident(id) = te {
                Some(id.name)
            } else {
                None
            }
        } else {
            source_idx.and_then(|idx| {
                value_specs[idx].type_.as_ref().and_then(|te| {
                    if let ast::Expr::Ident(id) = te {
                        Some(id.name)
                    } else {
                        None
                    }
                })
            })
        };

        let rust_type: syn::Type = if let Some(name) = type_name_str {
            let type_ident = syn::Ident::new(name, Span::mixed_site());
            syn::parse_quote! { #type_ident }
        } else {
            syn::parse_quote! { isize }
        };

        for (name_idx, name) in value_spec.names.iter().enumerate() {
            if name.name == "_" {
                continue;
            }

            let vis: syn::Visibility = name.into();
            let ident = syn::Ident::new(name.name, Span::mixed_site());

            let value_expr = source_values.and_then(|vals| vals.get(name_idx));

            let value: syn::Expr = if let Some(expr) = value_expr {
                if let Some(evaluated) = const_eval_expr(expr, iota as i64) {
                    let lit = syn::LitInt::new(&evaluated.to_string(), Span::mixed_site());
                    syn::parse_quote! { #lit }
                } else {
                    // If we can't evaluate it, try to convert it directly
                    // This won't work for complex expressions since ast::Expr doesn't impl Clone
                    // For const blocks, iota expressions should always be evaluable
                    syn::parse_quote! { 0 }
                }
            } else {
                syn::parse_quote! { 0 }
            };

            items.push(syn::parse_quote! {
                #vis const #ident: #rust_type = #value;
            });
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
            if named_return_idents.len() == 1 {
                let ident = &named_return_idents[0];
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
                        if path.path.segments.len() == 1 {
                            let name = path.path.segments[0].ident.to_string();
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
                syn::Expr::Path(path) => {
                    if path.path.segments.len() == 1 {
                        let name = path.path.segments[0].ident.to_string();
                        if !self.locals.contains(&name) {
                            self.idents.insert(name);
                        }
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
        "println", "print", "eprintln", "make_chan", "spawn", "true", "false",
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
fn compile_select_stmt(
    select_stmt: ast::SelectStmt,
) -> Result<Vec<syn::Stmt>, CompilerError> {
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
    fn extract_channel_recv(
        expr: ast::Expr,
    ) -> Option<syn::Expr> {
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
        let comm = *case.comm.unwrap();

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
                ast::Stmt::AssignStmt(assign) => {
                    // v := <-ch or v, ok := <-ch
                    if assign.rhs.len() == 1 && assign.lhs.len() >= 1 {
                        let lhs_pat = expr_to_pat(&assign.lhs[0]);
                        let rhs_expr = assign.rhs.into_iter().next().unwrap();
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
                    // Fallback: just compile normally
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
        let comm = *case.comm.unwrap();
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
            ast::Stmt::AssignStmt(assign) => {
                if assign.rhs.len() == 1 && assign.lhs.len() >= 1 {
                    let pat = expr_to_pat(&assign.lhs[0]);
                    let rhs_expr = assign.rhs.into_iter().next().unwrap();
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
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

/// Compile a parsed program (main package + imports) into a single Rust file.
///
/// Imported packages are emitted as `mod` blocks before the main package items.
pub fn compile_program(
    program: crate::parser::ParsedProgram,
) -> Result<syn::File, CompilerError> {
    let mut all_items: Vec<syn::Item> = Vec::new();

    for stdlib_path in &program.stdlib_imports {
        if let Some(stdlib_mod) = crate::stdlib::resolve_stdlib(stdlib_path) {
            all_items.push(syn::Item::Mod(stdlib_mod));
        }
    }

    let pkg_names: std::collections::HashSet<String> = program
        .imports
        .iter()
        .map(|p| p.name.clone())
        .chain(program.stdlib_imports.iter().map(|path| {
            path.rsplit('/').next().unwrap_or(path).to_string()
        }))
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
    format!("{:x}", hasher.finalize())
}

pub fn compile_program_multi(
    program: crate::parser::ParsedProgram,
) -> Result<CompiledProgram, CompilerError> {
    let mut modules = BTreeMap::new();

    let pkg_names: std::collections::HashSet<String> = program
        .imports
        .iter()
        .map(|p| p.name.clone())
        .chain(
            program
                .stdlib_imports
                .iter()
                .map(|path| path.rsplit('/').next().unwrap_or(path).to_string()),
        )
        .collect();

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

    for stdlib_path in &program.stdlib_imports {
        if let Some(stdlib_mod) = crate::stdlib::resolve_stdlib(stdlib_path) {
            let mod_name = stdlib_path
                .rsplit('/')
                .next()
                .unwrap_or(stdlib_path)
                .to_string();
            let items = match stdlib_mod.content {
                Some((_, items)) => items,
                None => vec![],
            };
            modules.insert(
                stdlib_path.clone(),
                CompiledModule {
                    mod_name: mod_name.clone(),
                    import_path: stdlib_path.clone(),
                    file: syn::File {
                        attrs: vec![],
                        items,
                        shebang: None,
                    },
                    filename: format!("{mod_name}.rs"),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: true,
                },
            );
        }
    }

    for pkg in program.imports {
        let content_hash = compute_content_hash(&pkg.files);
        let mut pkg_file = TryInto::<syn::File>::try_into(pkg.ast)?;
        passes::pass_for_imported_package(&mut pkg_file);
        prefix_sibling_paths(&mut pkg_file, &pkg_names);

        let filename = import_path_to_filename(&pkg.import_path);
        modules.insert(
            pkg.import_path.clone(),
            CompiledModule {
                mod_name: pkg.name.clone(),
                import_path: pkg.import_path.clone(),
                file: pkg_file,
                filename,
                content_hash,
                is_main: false,
                is_stdlib: false,
            },
        );
    }

    let has_main_fn = program.main_package.name == "main"
        && program.main_package.ast.decls.iter().any(|d| {
            matches!(d, ast::Decl::FuncDecl(f) if f.name.name == "main")
        });

    let main_hash = compute_content_hash(&program.main_package.files);
    let mut main_file: syn::File = program.main_package.ast.try_into()?;
    passes::pass(&mut main_file);

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

    Ok(CompiledProgram {
        modules,
        has_main: has_main_fn,
    })
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
                let first = path.segments[0].ident.to_string();
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

fn compile_type_spec(ts: ast::TypeSpec) -> Result<Vec<syn::Item>, CompilerError> {
    let name = ts.name.ok_or_else(|| {
        CompilerError::UnsupportedConstruct("type spec has no name".to_string())
    })?;
    let vis: syn::Visibility = (&name).into();
    let ident: syn::Ident = name.into();

    match ts.type_ {
        ast::Expr::StructType(struct_type) => {
            let mut fields = syn::punctuated::Punctuated::new();
            let mut embedded_types: Vec<(syn::Ident, syn::Type)> = vec![];
            if let Some(field_list) = struct_type.fields {
                for field in field_list.list {
                    let field_type = field.type_.ok_or_else(|| {
                        CompilerError::UnsupportedConstruct(
                            "struct field has no type".to_string(),
                        )
                    })?;

                    if let Some(names) = field.names {
                        let rust_type: syn::Type = field_type.into();
                        for field_name in names {
                            let field_vis: syn::Visibility = (&field_name).into();
                            let field_ident: syn::Ident = field_name.into();
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
                            let field_ident = syn::Ident::new(&name, Span::mixed_site());
                            let field_vis: syn::Visibility = if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                                syn::parse_quote! { pub }
                            } else {
                                syn::Visibility::Inherited
                            };
                            embedded_types.push((field_ident.clone(), rust_type.clone()));
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

            let struct_item = syn::Item::Struct(syn::ItemStruct {
                attrs: vec![],
                vis: vis.clone(),
                struct_token: <Token![struct]>::default(),
                ident: ident.clone(),
                generics: syn::Generics::default(),
                fields: syn::Fields::Named(syn::FieldsNamed {
                    brace_token: syn::token::Brace::default(),
                    named: fields,
                }),
                semi_token: None,
            });

            if embedded_types.len() == 1 {
                let (ref emb_field, ref emb_ty) = embedded_types[0];
                let deref_impl: syn::Item = syn::parse_quote! {
                    impl std::ops::Deref for #ident {
                        type Target = #emb_ty;
                        fn deref(&self) -> &#emb_ty {
                            &self.#emb_field
                        }
                    }
                };
                return Ok(vec![struct_item, deref_impl]);
            }

            Ok(vec![struct_item])
        }
        ast::Expr::InterfaceType(iface) => {
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
                                mutability: None,
                                self_token: <Token![self]>::default(),
                                colon_token: None,
                                ty: Box::new(syn::parse_quote! { &Self }),
                            }));

                            for (pname, ty) in &param_types {
                                let pident =
                                    syn::Ident::new(pname, Span::mixed_site());
                                inputs.push(syn::FnArg::Typed(syn::PatType {
                                    attrs: vec![],
                                    pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                                        attrs: vec![],
                                        by_ref: None,
                                        subpat: None,
                                        mutability: Some(<Token![mut]>::default()),
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
            let rust_type: syn::Type = other.into();
            Ok(vec![syn::Item::Type(syn::ItemType {
                attrs: vec![],
                vis,
                type_token: <Token![type]>::default(),
                ident,
                generics: syn::Generics::default(),
                eq_token: <Token![=]>::default(),
                ty: Box::new(rust_type),
                semi_token: <Token![;]>::default(),
            })])
        }
    }
}

fn compile_return_type(
    results: Option<ast::FieldList>,
) -> Result<syn::ReturnType, CompilerError> {
    let Some(results) = results else {
        return Ok(syn::ReturnType::Default);
    };
    let result_types: Vec<syn::Type> = results
        .list
        .into_iter()
        .filter_map(|f| f.type_.map(syn::Type::from))
        .collect();
    match result_types.len() {
        0 => Ok(syn::ReturnType::Default),
        1 => Ok(syn::ReturnType::Type(
            <Token![->]>::default(),
            Box::new(result_types.into_iter().next().unwrap()),
        )),
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

fn extract_receiver_type(expr: &ast::Expr) -> Result<(String, bool), CompilerError> {
    match expr {
        ast::Expr::StarExpr(star) => {
            if let ast::Expr::Ident(ident) = &*star.x {
                Ok((ident.name.to_string(), true))
            } else {
                Err(CompilerError::UnsupportedConstruct(
                    "complex receiver type".to_string(),
                ))
            }
        }
        ast::Expr::Ident(ident) => Ok((ident.name.to_string(), false)),
        _ => Err(CompilerError::UnsupportedConstruct(format!(
            "unsupported receiver type: {:?}",
            expr
        ))),
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
                    let first = expr_path.path.segments[0].ident.to_string();
                    if first == self.recv_name {
                        let field_ident = expr_path.path.segments[1].ident.clone();
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
                    let name = expr_path.path.segments[0].ident.to_string();
                    if name == self.recv_name {
                        expr_path.path = syn::parse_quote! { self };
                    }
                }
            }
        }
    }

    RewriteReceiver { recv_name }.visit_block_mut(block);
}

fn compile_method(func_decl: ast::FuncDecl) -> Result<(String, syn::ImplItemFn), CompilerError> {
    let recv = func_decl.recv.ok_or_else(|| {
        CompilerError::UnsupportedConstruct("method has no receiver".to_string())
    })?;

    let recv_field = recv.list.into_iter().next().ok_or_else(|| {
        CompilerError::UnsupportedConstruct("empty receiver list".to_string())
    })?;

    let recv_name = recv_field
        .names
        .as_ref()
        .and_then(|n| n.first())
        .map(|n| n.name.to_string())
        .unwrap_or_default();

    let recv_type = recv_field.type_.ok_or_else(|| {
        CompilerError::UnsupportedConstruct("receiver has no type".to_string())
    })?;

    let (type_name, is_pointer) = extract_receiver_type(&recv_type)?;

    let self_arg: syn::FnArg = if is_pointer {
        syn::parse_quote! { &mut self }
    } else {
        syn::parse_quote! { &self }
    };

    let mut inputs = syn::punctuated::Punctuated::new();
    inputs.push(self_arg);
    for param in func_decl.type_.params.list {
        inputs.push(param.try_into()?);
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
    "len", "cap", "append", "make", "new", "copy", "delete", "clear", "close", "panic",
    "println", "print", "max", "min", "complex", "real", "imag", "recover",
];

fn extract_type_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::StarExpr(star) => extract_type_name(&star.x),
        ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
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

fn compile_type_conversion(call_expr: ast::CallExpr, kind: &str) -> syn::Expr {
    let raw_arg = call_expr.args.unwrap().into_iter().next().unwrap();
    let is_int_arg = matches!(&raw_arg, ast::Expr::BasicLit(lit) if lit.kind == token::Token::INT);
    let arg: syn::Expr = raw_arg.into();
    match kind {
        "string" if is_int_arg => {
            syn::parse_quote! { char::from_u32(#arg as u32).map(String::from).unwrap_or_default() }
        }
        "string" => {
            syn::parse_quote! { String::from_utf8(#arg).unwrap() }
        }
        "[]byte" => syn::parse_quote! { (#arg).as_bytes().to_vec() },
        "[]rune" => syn::parse_quote! { (#arg).chars().collect::<Vec<char>>() },
        _ => unreachable!(),
    }
}

fn is_builtin_call(call_expr: &ast::CallExpr) -> bool {
    if let ast::Expr::Ident(ident) = &*call_expr.fun {
        BUILTINS.contains(&ident.name)
    } else {
        false
    }
}

fn is_fmt_call(call_expr: &ast::CallExpr) -> bool {
    let ast::Expr::SelectorExpr(sel) = &*call_expr.fun else {
        return false;
    };
    let ast::Expr::Ident(pkg) = &*sel.x else {
        return false;
    };
    pkg.name == "fmt"
        && matches!(
            sel.sel.name,
            "Println" | "Print" | "Printf" | "Sprintf" | "Errorf"
        )
}

fn compile_fmt_call(call_expr: ast::CallExpr) -> syn::Expr {
    let method = if let ast::Expr::SelectorExpr(ref sel) = *call_expr.fun {
        sel.sel.name
    } else {
        unreachable!()
    };

    // Handle Printf, Sprintf, Errorf with format string conversion
    match method {
        "Printf" | "Sprintf" | "Errorf" => {
            let args: Vec<syn::Expr> = call_expr
                .args
                .unwrap_or_default()
                .into_iter()
                .map(syn::Expr::from)
                .collect();

            if args.is_empty() {
                return match method {
                    "Printf" => syn::parse_quote! { ::std::print!("") },
                    "Sprintf" => syn::parse_quote! { ::std::format!("") },
                    "Errorf" => syn::parse_quote! { ::std::format!("") },
                    _ => unreachable!(),
                };
            }

            // Convert Go format string to Rust format string
            let format_str = &args[0];
            let rest = &args[1..];

            // If first arg is a string literal, convert Go format verbs to Rust
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = format_str
            {
                let go_fmt = lit_str.value();

                // Parse format string to detect %c verbs and wrap corresponding args
                let mut rust_fmt = String::new();
                let mut char_arg_indices = vec![];
                let mut arg_idx = 0usize;
                let mut chars = go_fmt.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        if let Some(&next) = chars.peek() {
                            match next {
                                '%' => { rust_fmt.push('%'); chars.next(); }
                                'c' => { rust_fmt.push_str("{}"); chars.next(); char_arg_indices.push(arg_idx); arg_idx += 1; }
                                's' | 'd' | 'v' | 'f' | 't' => { rust_fmt.push_str("{}"); chars.next(); arg_idx += 1; }
                                'p' => { rust_fmt.push_str("{:p}"); chars.next(); arg_idx += 1; }
                                'x' => { rust_fmt.push_str("{:x}"); chars.next(); arg_idx += 1; }
                                'o' => { rust_fmt.push_str("{:o}"); chars.next(); arg_idx += 1; }
                                'b' => { rust_fmt.push_str("{:b}"); chars.next(); arg_idx += 1; }
                                'e' => { rust_fmt.push_str("{:e}"); chars.next(); arg_idx += 1; }
                                'q' => { rust_fmt.push_str("{:?}"); chars.next(); arg_idx += 1; }
                                _ => { rust_fmt.push(c); }
                            }
                        } else {
                            rust_fmt.push(c);
                        }
                    } else {
                        rust_fmt.push(c);
                    }
                }

                let fmt_lit = syn::LitStr::new(&rust_fmt, Span::mixed_site());

                // Wrap %c args with char::from_u32
                let wrapped_rest: Vec<syn::Expr> = rest.iter().enumerate().map(|(i, e)| {
                    if char_arg_indices.contains(&i) {
                        syn::parse_quote! { char::from_u32(#e as u32).unwrap_or_default() }
                    } else {
                        e.clone()
                    }
                }).collect();

                return match method {
                    "Printf" => syn::parse_quote! { ::std::print!(#fmt_lit, #(#wrapped_rest),*) },
                    "Sprintf" => syn::parse_quote! { ::std::format!(#fmt_lit, #(#wrapped_rest),*) },
                    "Errorf" => syn::parse_quote! { ::std::format!(#fmt_lit, #(#wrapped_rest),*) },
                    _ => unreachable!(),
                };
            }

            // Non-literal format string: use format! with {}
            return match method {
                "Printf" => syn::parse_quote! { ::std::print!("{}", #format_str) },
                "Sprintf" => syn::parse_quote! { ::std::format!("{}", #format_str) },
                "Errorf" => syn::parse_quote! { ::std::format!("{}", #format_str) },
                _ => unreachable!(),
            };
        }
        _ => {}
    }

    let arg_count = call_expr.args.as_ref().map_or(0, |a| a.len());

    // fmt.Println() with no args → just print a newline
    if method == "Println" && arg_count == 0 {
        return syn::parse_quote! { ::std::println!() };
    }
    if method == "Print" && arg_count == 0 {
        return syn::parse_quote! { ::std::print!("") };
    }

    let suffix = match (method, arg_count) {
        ("Println", 1) => "Println",
        ("Println", 2) => "Println2",
        ("Println", 3) => "Println3",
        ("Println", 4) => "Println4",
        ("Print", 0) | ("Print", 1) => "Print",
        ("Print", 2) => "Print2",
        ("Print", 3) => "Print3",
        _ => {
            return syn::Expr::Call(call_expr.into());
        }
    };
    let func_ident = syn::Ident::new(suffix, Span::mixed_site());
    let fmt_ident = syn::Ident::new("fmt", Span::mixed_site());
    let func: syn::Expr = syn::parse_quote! { #fmt_ident::#func_ident };
    let args: Vec<syn::Expr> = call_expr
        .args
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            let e = syn::Expr::from(a);
            let needs_paren = !matches!(
                e,
                syn::Expr::Path(_) | syn::Expr::Lit(_) | syn::Expr::Call(_) | syn::Expr::Paren(_)
            );
            if needs_paren {
                syn::parse_quote! { &(#e) }
            } else {
                syn::parse_quote! { &#e }
            }
        })
        .collect();
    let mut punct_args = syn::punctuated::Punctuated::new();
    for arg in args {
        punct_args.push(arg);
    }
    syn::Expr::Call(syn::ExprCall {
        attrs: vec![],
        func: Box::new(func),
        paren_token: syn::token::Paren::default(),
        args: punct_args,
    })
}

fn compile_builtin(call_expr: ast::CallExpr) -> syn::Expr {
    let name = match *call_expr.fun {
        ast::Expr::Ident(ident) => ident.name.to_string(),
        _ => unreachable!(),
    };

    let raw_args: Vec<ast::Expr> = call_expr.args.unwrap_or_default().into_iter().collect();

    match name.as_str() {
        "make" => {
            let mut it = raw_args.into_iter();
            let type_arg = it.next().unwrap();
            let remaining: Vec<syn::Expr> = it.map(syn::Expr::from).collect();
            match type_arg {
                ast::Expr::ArrayType(arr) => {
                    let elem_type: syn::Type = (*arr.elt).into();
                    if remaining.is_empty() {
                        syn::parse_quote! { Vec::<#elem_type>::new() }
                    } else if remaining.len() == 1 {
                        let size = &remaining[0];
                        syn::parse_quote! { builtin::make_vec::<#elem_type>(#size) }
                    } else {
                        let size = &remaining[0];
                        let cap_arg = &remaining[1];
                        syn::parse_quote! { { let mut v = Vec::<#elem_type>::with_capacity(#cap_arg); v.resize_with(#size, Default::default); v } }
                    }
                }
                ast::Expr::MapType(map) => {
                    let key_type: syn::Type = (*map.key).into();
                    let val_type: syn::Type = (*map.value).into();
                    if remaining.is_empty() {
                        syn::parse_quote! { std::collections::HashMap::<#key_type, #val_type>::new() }
                    } else {
                        let cap_arg = &remaining[0];
                        syn::parse_quote! { std::collections::HashMap::<#key_type, #val_type>::with_capacity(#cap_arg) }
                    }
                }
                ast::Expr::ChanType(_) => {
                    if remaining.is_empty() {
                        syn::parse_quote! { builtin::make_chan(0) }
                    } else {
                        let cap_arg = &remaining[0];
                        syn::parse_quote! { builtin::make_chan(#cap_arg) }
                    }
                }
                _ => {
                    syn::parse_quote! { Default::default() }
                }
            }
        }
        "new" => {
            let type_arg: syn::Type = raw_args.into_iter().next().unwrap().into();
            syn::parse_quote! { Box::new(<#type_arg>::default()) }
        }
        _ => {
            let args: Vec<syn::Expr> = raw_args.into_iter().map(syn::Expr::from).collect();
            match name.as_str() {
                "len" => {
                    let x = &args[0];
                    syn::parse_quote! { builtin::len(&#x) }
                }
                "cap" => {
                    let x = &args[0];
                    syn::parse_quote! { builtin::cap(&#x) }
                }
                "append" => {
                    let slice = &args[0];
                    let elem = &args[1];
                    syn::parse_quote! { builtin::append(#slice, #elem) }
                }
                "copy" => {
                    let dst = &args[0];
                    let src = &args[1];
                    syn::parse_quote! { builtin::copy_slice(&mut #dst, &#src) }
                }
                "delete" => {
                    let map = &args[0];
                    let key = &args[1];
                    syn::parse_quote! { builtin::delete(&mut #map, &#key) }
                }
                "clear" => {
                    let x = &args[0];
                    syn::parse_quote! { builtin::clear(&mut #x) }
                }
                "close" => {
                    let ch = &args[0];
                    syn::parse_quote! { builtin::close(&#ch) }
                }
                "max" => {
                    if args.len() == 2 {
                        let a = &args[0];
                        let b = &args[1];
                        syn::parse_quote! { builtin::max(#a, #b) }
                    } else if args.len() == 3 {
                        let a = &args[0];
                        let b = &args[1];
                        let c = &args[2];
                        syn::parse_quote! { builtin::max3(#a, #b, #c) }
                    } else {
                        let a = &args[0];
                        let b = &args[1];
                        syn::parse_quote! { builtin::max(#a, #b) }
                    }
                }
                "min" => {
                    if args.len() == 2 {
                        let a = &args[0];
                        let b = &args[1];
                        syn::parse_quote! { builtin::min(#a, #b) }
                    } else if args.len() == 3 {
                        let a = &args[0];
                        let b = &args[1];
                        let c = &args[2];
                        syn::parse_quote! { builtin::min3(#a, #b, #c) }
                    } else {
                        let a = &args[0];
                        let b = &args[1];
                        syn::parse_quote! { builtin::min(#a, #b) }
                    }
                }
                "complex" => {
                    let re = &args[0];
                    let im = &args[1];
                    syn::parse_quote! { builtin::complex128(#re, #im) }
                }
                "real" => {
                    let c = &args[0];
                    syn::parse_quote! { builtin::real128(#c) }
                }
                "imag" => {
                    let c = &args[0];
                    syn::parse_quote! { builtin::imag128(#c) }
                }
                "recover" => {
                    syn::parse_quote! { None::<String> }
                }
                "panic" => {
                    if args.is_empty() {
                        syn::parse_quote! { panic!() }
                    } else {
                        let msg = &args[0];
                        syn::parse_quote! { panic!("{}", #msg) }
                    }
                }
                "println" => {
                    if args.is_empty() {
                        syn::parse_quote! { ::std::println!() }
                    } else {
                        let first = &args[0];
                        syn::parse_quote! { ::std::println!("{}", #first) }
                    }
                }
                "print" => {
                    if args.is_empty() {
                        syn::parse_quote! { ::std::print!() }
                    } else {
                        let first = &args[0];
                        syn::parse_quote! { ::std::print!("{}", #first) }
                    }
                }
                _ => unreachable!("not a builtin: {}", name),
            }
        }
    }
}

fn elts_to_field_values(elts: Vec<syn::Expr>) -> Vec<syn::FieldValue> {
    elts.into_iter()
        .map(|e| {
            if let syn::Expr::Tuple(ref tuple) = e {
                if tuple.elems.len() == 2 {
                    let mut iter = tuple.elems.clone().into_iter();
                    let key = iter.next().unwrap();
                    let value = iter.next().unwrap();
                    if let syn::Expr::Path(ref path) = key {
                        return syn::FieldValue {
                            attrs: vec![],
                            member: syn::Member::Named(
                                path.path.segments.last().unwrap().ident.clone(),
                            ),
                            colon_token: Some(<Token![:]>::default()),
                            expr: value,
                        };
                    }
                }
            }
            syn::FieldValue {
                attrs: vec![],
                member: syn::Member::Unnamed(syn::Index {
                    index: 0,
                    span: Span::mixed_site(),
                }),
                colon_token: None,
                expr: e,
            }
        })
        .collect()
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
                let field_values = elts_to_field_values(elts);
                let mut fields = syn::punctuated::Punctuated::new();
                for fv in field_values {
                    fields.push(fv);
                }
                syn::Expr::Struct(syn::ExprStruct {
                    attrs: vec![],
                    qself: None,
                    path: syn::parse_quote! { #type_ident },
                    brace_token: syn::token::Brace::default(),
                    fields,
                    dot2_token: None,
                    rest: None,
                })
            }
            ast::Expr::SelectorExpr(sel) => {
                let path: syn::ExprPath = sel.into();
                let field_values = elts_to_field_values(elts);
                let mut fields = syn::punctuated::Punctuated::new();
                for fv in field_values {
                    fields.push(fv);
                }
                syn::Expr::Struct(syn::ExprStruct {
                    attrs: vec![],
                    qself: None,
                    path: path.path,
                    brace_token: syn::token::Brace::default(),
                    fields,
                    dot2_token: None,
                    rest: None,
                })
            }
            ast::Expr::ArrayType(array_type) => {
                // Slice/array literal: []T{e1, e2, ...} → vec![e1, e2, ...]
                if array_type.len.is_none() {
                    syn::parse_quote! { vec![#(#elts),*] }
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
                syn::parse_quote! { vec![#(#elts),*] }
            }
        }
    } else {
        // No type — nested composite lit in an array/slice context
        syn::parse_quote! { vec![#(#elts),*] }
    }
}

fn compile_func_lit(func_lit: ast::FuncLit) -> syn::Expr {
    let mut params = syn::punctuated::Punctuated::<syn::Pat, Token![,]>::new();
    let mut param_types = Vec::new();

    for field in func_lit.type_.params.list {
        let ty: Option<syn::Type> = field.type_.map(syn::Type::from);
        if let Some(names) = field.names {
            for name in names {
                let ident: syn::Ident = name.into();
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
    let x: syn::Expr = (*slice_expr.x).into();
    let low = slice_expr.low.map(|l| syn::Expr::from(*l));
    let high = slice_expr.high.map(|h| syn::Expr::from(*h));

    match (low, high) {
        (None, None) => {
            // x[:] → x.clone() or x[..]
            syn::parse_quote! { #x[..] }
        }
        (Some(lo), None) => {
            // x[lo:] → x[lo..]
            syn::parse_quote! { #x[#lo..] }
        }
        (None, Some(hi)) => {
            // x[:hi] → x[..hi]
            syn::parse_quote! { #x[..#hi] }
        }
        (Some(lo), Some(hi)) => {
            // x[lo:hi] → x[lo..hi]
            syn::parse_quote! { #x[#lo..#hi] }
        }
    }
}

fn compile_type_switch_stmt(
    ts: ast::TypeSwitchStmt,
) -> Result<Vec<syn::Stmt>, CompilerError> {
    // type switch: switch x := val.(type) { case T: ... }
    // Compile to if/else chain with downcast checks
    let assign_expr = match *ts.assign {
        ast::Stmt::ExprStmt(s) => syn::Expr::from(s.x),
        ast::Stmt::AssignStmt(s) => {
            let rhs: syn::Expr = s
                .rhs
                .into_iter()
                .next()
                .map(syn::Expr::from)
                .unwrap_or_else(|| syn::parse_quote! { () });
            rhs
        }
        _ => syn::parse_quote! { () },
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
        let type_exprs: Vec<syn::Type> = case
            .list
            .unwrap_or_default()
            .into_iter()
            .map(syn::Type::from)
            .collect();

        let cond = if type_exprs.len() == 1 {
            let ty = &type_exprs[0];
            let val = &assign_expr;
            syn::parse_quote! { (#val as &dyn std::any::Any).is::<#ty>() }
        } else {
            let checks: Vec<syn::Expr> = type_exprs
                .iter()
                .map(|ty| {
                    let val = &assign_expr;
                    syn::parse_quote! { (#val as &dyn std::any::Any).is::<#ty>() }
                })
                .collect();
            checks
                .into_iter()
                .reduce(|acc, e| syn::parse_quote! { #acc || #e })
                .unwrap_or_else(|| syn::parse_quote! { true })
        };

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

    match result {
        Some(expr) => Ok(vec![syn::Stmt::Expr(expr, None)]),
        None => Ok(vec![]),
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

fn is_chan_type_expr(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::ChanType(_))
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
    let is_string = is_string_literal(&range_stmt.x);
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
                Ok(make_for_loop(pat, syn::parse_quote! { (#x).char_indices() }, body))
            } else {
                Ok(make_for_loop(pat, syn::parse_quote! { (#x).iter().enumerate() }, body))
            }
        }
        // for i := range x  OR  for v := range ch
        (Some(key_expr), None) => {
            let key_pat = expr_to_pat(&key_expr);
            if is_int {
                Ok(make_for_loop(key_pat, syn::parse_quote! { 0..((#x) as usize) }, body))
            } else {
                // Use into_iter() which works for channels (via IntoIterator) and
                // for slices/vecs (gives values). For index-only iteration over
                // slices, use `for i, _ := range s` instead.
                Ok(make_for_loop(key_pat, syn::parse_quote! { (#x).into_iter() }, body))
            }
        }
        // for range x
        (None, None) => {
            let pat: syn::Pat = syn::parse_quote! { _ };
            if is_int {
                Ok(make_for_loop(pat, syn::parse_quote! { 0..((#x) as usize) }, body))
            } else {
                Ok(make_for_loop(pat, x, body))
            }
        }
        _ => Err(CompilerError::UnsupportedConstruct(
            "range with value but no key".to_string(),
        )),
    }
}

fn expr_to_pat(expr: &ast::Expr) -> syn::Pat {
    match expr {
        ast::Expr::Ident(ident) if ident.name == "_" => syn::parse_quote! { _ },
        ast::Expr::Ident(ident) => {
            let name = syn::Ident::new(ident.name, Span::mixed_site());
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

fn compile_top_level_value_spec(
    vs: ast::ValueSpec,
    tok: token::Token,
) -> Result<Vec<syn::Item>, CompilerError> {
    let mut items = vec![];
    let mut values_iter = vs.values.unwrap_or_default().into_iter();

    for name in vs.names {
        let vis: syn::Visibility = (&name).into();
        let ident: syn::Ident = name.into();
        let init = values_iter.next().map(syn::Expr::from);

        if tok == token::Token::CONST {
            let value = init.unwrap_or_else(|| syn::parse_quote! { 0 });
            items.push(syn::parse_quote! {
                #vis const #ident: isize = #value;
            });
        } else {
            let value = init.unwrap_or_else(|| go_zero_value(vs.type_.as_ref()));
            items.push(syn::parse_quote! {
                static mut #ident: isize = #value;
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
                if raw.starts_with('`') {
                    // Raw string literal — no escape processing
                    let inner = &raw[1..raw.len() - 1];
                    Self::Str(syn::LitStr::new(inner, Span::mixed_site()))
                } else {
                    let inner = &raw[1..raw.len() - 1];
                    let interpreted = interpret_go_string_escapes(inner);
                    Self::Str(syn::LitStr::new(&interpreted, Span::mixed_site()))
                }
            }
            CHAR => {
                let raw = basic_lit.value;
                let inner = &raw[1..raw.len() - 1];
                let interpreted = interpret_go_string_escapes(inner);
                let ch = interpreted.chars().next().unwrap_or(' ');
                Self::Char(syn::LitChar::new(ch, Span::mixed_site()))
            }
            FLOAT => Self::Float(syn::LitFloat::new(basic_lit.value, Span::mixed_site())),
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

        let (x, op, y) = (
            syn::Expr::from(*binary_expr.x),
            syn::BinOp::from(binary_expr.op),
            syn::Expr::from(*binary_expr.y),
        );
        syn::parse_quote! { #x #op #y }
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

        let func = if let ast::Expr::Ident(ident) = *call_expr.fun {
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
        } else {
            (*call_expr.fun).into()
        };

        let mut args = syn::punctuated::Punctuated::new();
        if let Some(cargs) = call_expr.args {
            for arg in cargs {
                args.push(arg.into())
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
            ast::Expr::BinaryExpr(binary_expr) => Self::Binary(binary_expr.into()),
            ast::Expr::CallExpr(call_expr) => {
                if let Some(kind) = detect_type_conversion(&call_expr) {
                    return compile_type_conversion(call_expr, kind);
                }
                if is_builtin_call(&call_expr) {
                    return compile_builtin(call_expr);
                }
                if is_fmt_call(&call_expr) {
                    return compile_fmt_call(call_expr);
                }
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
                            for arg in cargs {
                                args.push(syn::Expr::from(arg));
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
            ast::Expr::Ident(ident) => Self::Path(ident.into()),
            ast::Expr::SelectorExpr(selector_expr) => {
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
                _ => Self::Unary(syn::ExprUnary {
                    attrs: vec![],
                    op: match unary_expr.op {
                        token::Token::SUB => syn::UnOp::Neg(<Token![-]>::default()),
                        token::Token::NOT => syn::UnOp::Not(<Token![!]>::default()),
                        token::Token::MUL => syn::UnOp::Deref(<Token![*]>::default()),
                        _ => unimplemented!("unary op: {:?}", unary_expr.op),
                    },
                    expr: Box::new((*unary_expr.x).into()),
                }),
            },
            ast::Expr::IndexExpr(index_expr) => Self::Index(syn::ExprIndex {
                attrs: vec![],
                expr: Box::new((*index_expr.x).into()),
                bracket_token: syn::token::Bracket::default(),
                index: Box::new((*index_expr.index).into()),
            }),
            ast::Expr::StarExpr(star_expr) => Self::Unary(syn::ExprUnary {
                attrs: vec![],
                op: syn::UnOp::Deref(<Token![*]>::default()),
                expr: Box::new((*star_expr.x).into()),
            }),
            ast::Expr::CompositeLit(comp_lit) => {
                compile_composite_lit(comp_lit)
            }
            ast::Expr::FuncLit(func_lit) => {
                compile_func_lit(func_lit)
            }
            ast::Expr::SliceExpr(slice_expr) => {
                compile_slice_expr(slice_expr)
            }
            ast::Expr::TypeAssertExpr(ta) => {
                // x.(T) → downcast or type check
                let x: syn::Expr = (*ta.x).into();
                if let Some(type_expr) = ta.type_ {
                    let ty: syn::Type = (*type_expr).into();
                    syn::parse_quote! {
                        *((#x) as Box<dyn std::any::Any>).downcast::<#ty>().unwrap()
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
            _ => unimplemented!("expr: {:?}", expr),
        }
    }
}

impl From<ast::Expr<'_>> for syn::Type {
    fn from(expr: ast::Expr) -> Self {
        match expr {
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
                if array_type.len.is_some() {
                    // Fixed-size array: [N]T → [T; N]
                    let len_expr: syn::Expr = (*array_type.len.unwrap()).into();
                    syn::parse_quote! { [#elem; #len_expr] }
                } else {
                    // Slice: []T → Vec<T>
                    syn::parse_quote! { Vec<#elem> }
                }
            }
            ast::Expr::SelectorExpr(selector_expr) => {
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
                    if results.list.len() == 1 {
                        let ret_type: syn::Type = results
                            .list
                            .into_iter()
                            .next()
                            .unwrap()
                            .type_
                            .unwrap()
                            .into();
                        syn::parse_quote! { fn(#param_types) -> #ret_type }
                    } else {
                        let mut ret_types =
                            syn::punctuated::Punctuated::<syn::Type, Token![,]>::new();
                        for field in results.list {
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
                // chan T → gors_channel::GoChan<T>
                let inner: syn::Type = (*chan_type.value).into();
                syn::parse_quote! { ::gors_channel::GoChan<#inner> }
            }
            ast::Expr::Ellipsis(ellipsis) => {
                if let Some(elt) = ellipsis.elt {
                    let inner: syn::Type = (*elt).into();
                    syn::parse_quote! { Vec<#inner> }
                } else {
                    syn::parse_quote! { Vec<Box<dyn std::any::Any>> }
                }
            }
            _ => unimplemented!("type expr: {:?}", expr),
        }
    }
}

impl TryFrom<ast::File<'_>> for syn::File {
    type Error = CompilerError;

    fn try_from(file: ast::File) -> Result<Self, Self::Error> {
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
                    if func_decl.name.name == "init" {
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
                                    struct_has_string_method
                                        .insert(type_name.clone());
                                }
                            }
                            struct_methods
                                .entry(type_name.clone())
                                .or_default()
                                .push(method_name);
                        }
                        let (type_name, method) = compile_method(func_decl)?;
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
                                // Package-level var: collect as stmts to inject into main
                                let names = vs.names;
                                let has_type = vs.type_.is_some();
                                let type_expr = vs.type_;
                                let mut values_iter =
                                    vs.values.unwrap_or_default().into_iter();

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
                                                        method_names
                                                            .push(n.name.to_string());
                                                    }
                                                }
                                            }
                                        }
                                        trait_methods
                                            .insert(trait_name, method_names);
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
            items.push(syn::Item::Impl(syn::ItemImpl {
                attrs: vec![],
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics: syn::Generics::default(),
                trait_: None,
                self_ty: Box::new(syn::parse_quote! { #type_ident }),
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

        // Stringer pattern: generate `impl Display` and `impl GoDisplay` for structs with String() string
        for struct_name in &struct_has_string_method {
            let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
            items.push(syn::parse_quote! {
                impl std::fmt::Display for #struct_ident {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f, "{}", self.String())
                    }
                }
            });
            items.push(syn::parse_quote! {
                impl fmt::GoDisplay for #struct_ident {
                    fn go_fmt(&self) -> String { self.String() }
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

impl TryFrom<ast::Field<'_>> for syn::FnArg {
    type Error = CompilerError;

    fn try_from(field: ast::Field) -> Result<Self, Self::Error> {
        let name = field
            .names
            .ok_or_else(|| {
                CompilerError::InvalidFunctionSignature("field has no names".to_string())
            })?
            .into_iter()
            .next()
            .ok_or_else(|| {
                CompilerError::InvalidFunctionSignature("field names is empty".to_string())
            })?;
        let type_ = field.type_.ok_or_else(|| {
            CompilerError::InvalidFunctionSignature("field has no type".to_string())
        })?;
        Ok(Self::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: Some(<Token![mut]>::default()),
                ident: name.into(),
            })),
            colon_token: <Token![:]>::default(),
            ty: Box::new(type_.into()),
        }))
    }
}

impl TryFrom<ast::FuncDecl<'_>> for syn::ItemFn {
    type Error = CompilerError;

    fn try_from(func_decl: ast::FuncDecl) -> Result<Self, Self::Error> {
        // Record mapping for the function keyword with Go name
        if let Some(ref func_pos) = func_decl.type_.func {
            record_mapping(func_pos, Some("func"));
        }

        // Convert doc comments to Rust doc attributes
        let attrs = comment_group_to_attrs(&func_decl.doc);

        let mut inputs = syn::punctuated::Punctuated::new();
        for param in func_decl.type_.params.list {
            inputs.push(param.try_into()?);
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
                            let ident = syn::Ident::new(name.name, Span::mixed_site());
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
                if named_return_idents.len() == 1 {
                    let ident = &named_return_idents[0];
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
        let generics = if let Some(type_params) = func_decl.type_.type_params {
            let mut params = syn::punctuated::Punctuated::new();
            for field in type_params.list {
                if let Some(names) = field.names {
                    for name in names {
                        let ident: syn::Ident = name.into();
                        let bounds = if let Some(ref constraint) = field.type_ {
                            go_constraint_to_rust_bounds(constraint)
                        } else {
                            syn::punctuated::Punctuated::new()
                        };
                        params.push(syn::GenericParam::Type(syn::TypeParam {
                            attrs: vec![],
                            ident,
                            colon_token: if bounds.is_empty() {
                                None
                            } else {
                                Some(<Token![:]>::default())
                            },
                            bounds,
                            eq_token: None,
                            default: None,
                        }));
                    }
                }
            }
            if params.is_empty() {
                syn::Generics {
                    params: syn::punctuated::Punctuated::new(),
                    lt_token: None,
                    gt_token: None,
                    where_clause: None,
                }
            } else {
                syn::Generics {
                    lt_token: Some(<Token![<]>::default()),
                    gt_token: Some(<Token![>]>::default()),
                    params,
                    where_clause: None,
                }
            }
        } else {
            syn::Generics {
                params: syn::punctuated::Punctuated::new(),
                lt_token: None,
                gt_token: None,
                where_clause: None,
            }
        };

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
                segments.push(syn::PathSegment {
                    ident: ident.into(),
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

        Self::new(ident.name, Span::mixed_site())
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
                                let name = syn::Ident::new(ident.name, Span::mixed_site());
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
                                let inner_stmts = Vec::<syn::Stmt>::try_from(ast::Stmt::IfStmt(if_stmt))?;
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
            ast::Stmt::SwitchStmt(s) => Ok(vec![syn::Stmt::Expr(s.try_into()?, None)]),
            ast::Stmt::TypeSwitchStmt(s) => compile_type_switch_stmt(s),
            ast::Stmt::SendStmt(send_stmt) => {
                // ch <- value  =>  ch.send(value);
                let chan: syn::Expr = send_stmt.chan.into();
                let value: syn::Expr = send_stmt.value.into();
                Ok(vec![syn::parse_quote! {
                    #chan.send(#value);
                }])
            }
            ast::Stmt::SelectStmt(select_stmt) => {
                compile_select_stmt(select_stmt)
            }
            ast::Stmt::CommClause(_) | ast::Stmt::CaseClause(_) => {
                // These are handled inline by their parent (SwitchStmt/SelectStmt)
                Ok(vec![])
            }
        }
    }
}

impl From<ast::IncDecStmt<'_>> for Vec<syn::Stmt> {
    fn from(inc_dec_stmt: ast::IncDecStmt) -> Self {
        let x: syn::Expr = inc_dec_stmt.x.into();
        match inc_dec_stmt.tok {
            token::Token::INC => vec![syn::parse_quote! { #x += 1; }],
            token::Token::DEC => vec![syn::parse_quote! { #x -= 1; }],
            _ => unreachable!("implementation error"),
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
            _ => unreachable!("invalid branch token"),
        }
    }
}

impl TryFrom<ast::LabeledStmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(labeled_stmt: ast::LabeledStmt) -> Result<Self, Self::Error> {
        // Convert to Rust labeled block/loop
        let label_ident: syn::Ident = labeled_stmt.label.into();
        let inner_stmts: Vec<syn::Stmt> = (*labeled_stmt.stmt).try_into()?;

        // Create a labeled block
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
            None,
        )])
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
            let cond = build_case_condition(case.list, tag_syn.as_ref());
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
                else_branch: result
                    .map(|e| (<Token![else]>::default(), Box::new(e))),
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
) -> syn::Expr {
    let exprs = list.unwrap_or_default();

    if let Some(tag) = tag {
        let mut conditions: Vec<syn::Expr> = exprs
            .into_iter()
            .map(|e| {
                let tag_expr = tag.clone();
                let val_expr: syn::Expr = e.into();
                syn::parse_quote! { #tag_expr == #val_expr }
            })
            .collect();

        if conditions.len() == 1 {
            conditions.remove(0)
        } else {
            conditions
                .into_iter()
                .reduce(|acc, e| syn::parse_quote! { #acc || #e })
                .unwrap_or_else(|| syn::parse_quote! { true })
        }
    } else {
        // Tagless switch: `switch { case cond: ... }` → conditions are already booleans
        if exprs.len() == 1 {
            exprs.into_iter().next().unwrap().into()
        } else {
            let conditions: Vec<syn::Expr> = exprs.into_iter().map(Into::into).collect();
            conditions
                .into_iter()
                .reduce(|acc, e| syn::parse_quote! { #acc || #e })
                .unwrap_or_else(|| syn::parse_quote! { true })
        }
    }
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
fn rewrite_continue_as_break_body(stmts: &mut Vec<syn::Stmt>) {
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
            if let ast::Spec::ValueSpec(value_spec) = spec {
                let names = value_spec.names;
                let rust_type: Option<syn::Type> = value_spec.type_.map(syn::Type::from);
                let mut values_iter = value_spec.values.unwrap_or_default().into_iter();

                for name in names {
                    let ident: syn::Ident = name.into();
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
        }
        stmts
    }
}

fn go_zero_value(type_expr: Option<&ast::Expr>) -> syn::Expr {
    if let Some(ast::Expr::Ident(ident)) = type_expr {
        match ident.name {
            "bool" => return syn::parse_quote! { false },
            "string" => return syn::parse_quote! { String::new() },
            "float32" | "float64" => return syn::parse_quote! { 0.0 },
            _ => {}
        }
    }
    syn::parse_quote! { 0 }
}

fn go_zero_value_from_type(ty: Option<&syn::Type>) -> syn::Expr {
    if let Some(syn::Type::Path(type_path)) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            let name = seg.ident.to_string();
            match name.as_str() {
                "bool" => return syn::parse_quote! { false },
                // Go type names (before map_type pass)
                "string" | "String" => return syn::parse_quote! { String::new() },
                "float32" | "float64" | "f32" | "f64" => return syn::parse_quote! { 0.0 },
                _ => {}
            }
        }
    }
    syn::parse_quote! { 0 }
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
    let lhs_idents: Vec<Option<syn::Ident>> = lhs
        .iter()
        .map(|e| match e {
            ast::Expr::Ident(id) if id.name == "_" => Ok(None),
            ast::Expr::Ident(id) => Ok(Some(syn::Ident::new(id.name, Span::mixed_site()))),
            _ => Err(CompilerError::InvalidAssignment(
                "expected identifier in comma-ok lhs".to_string(),
            )),
        })
        .collect::<Result<_, _>>()?;

    let val_pat: syn::Pat = match &lhs_idents[0] {
        None => syn::parse_quote! { _ },
        Some(id) => syn::parse_quote! { mut #id },
    };
    let ok_pat: syn::Pat = match &lhs_idents[1] {
        None => syn::parse_quote! { _ },
        Some(id) => syn::parse_quote! { mut #id },
    };

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
                unreachable!()
            }
        }
        CommaOkKind::ChanRecv => {
            if let ast::Expr::UnaryExpr(u) = rhs {
                let ch_e: syn::Expr = (*u.x).into();
                syn::parse_quote! { (#ch_e).recv_with_ok() }
            } else {
                unreachable!()
            }
        }
        CommaOkKind::TypeAssert => {
            if let ast::Expr::TypeAssertExpr(ta) = rhs {
                let x_e: syn::Expr = (*ta.x).into();
                let ty: syn::Type = (*ta.type_.unwrap()).into();
                syn::parse_quote! {
                    match (&(#x_e) as &dyn std::any::Any).downcast_ref::<#ty>() {
                        Some(__v) => (__v.clone(), true),
                        None => (Default::default(), false),
                    }
                }
            } else {
                unreachable!()
            }
        }
    };

    if is_define {
        Ok(vec![syn::parse_quote! {
            let (#val_pat, #ok_pat) = #rhs_expr;
        }])
    } else {
        let mut lhs_iter = lhs.into_iter();
        let val_e: syn::Expr = lhs_iter.next().unwrap().into();
        let ok_e: syn::Expr = lhs_iter.next().unwrap().into();
        Ok(vec![syn::parse_quote! {
            (#val_e, #ok_e) = #rhs_expr;
        }])
    }
}

impl TryFrom<ast::AssignStmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(assign_stmt: ast::AssignStmt) -> Result<Self, Self::Error> {
        // Comma-ok patterns: v, ok := m[k] / v, ok := <-ch / v, ok := x.(T)
        if assign_stmt.lhs.len() == 2 && assign_stmt.rhs.len() == 1 {
            if let Some(kind) = detect_comma_ok(&assign_stmt.rhs[0]) {
                let is_define = assign_stmt.tok == token::Token::DEFINE;
                let rhs = assign_stmt.rhs.into_iter().next().unwrap();
                return compile_comma_ok(assign_stmt.lhs, rhs, kind, is_define);
            }
        }

        // Multi-value return: x, y := f() or x, y = f()
        if assign_stmt.lhs.len() > 1 && assign_stmt.rhs.len() == 1 {
            let rhs_expr: syn::Expr = assign_stmt
                .rhs
                .into_iter()
                .next()
                .unwrap()
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
                let mut lhs_elems = syn::punctuated::Punctuated::new();
                for expr in assign_stmt.lhs {
                    lhs_elems.push(syn::Expr::from(expr));
                }
                let lhs_tuple = syn::Expr::Tuple(syn::ExprTuple {
                    attrs: vec![],
                    paren_token: syn::token::Paren::default(),
                    elems: lhs_elems,
                });
                return Ok(vec![syn::Stmt::Expr(
                    syn::Expr::Assign(syn::ExprAssign {
                        attrs: vec![],
                        left: Box::new(lhs_tuple),
                        eq_token: <Token![=]>::default(),
                        right: Box::new(rhs_expr),
                    }),
                    Some(<Token![;]>::default()),
                )]);
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
                return Ok(vec![syn::parse_quote! { #left = #right; }]);
            }

            let mut out = vec![];

            let mut idents: Vec<syn::Ident> = vec![];
            let mut values: Vec<syn::Expr> = vec![];
            for (lhs, rhs) in assign_stmt.lhs.iter().zip(assign_stmt.rhs) {
                if let ast::Expr::Ident(ident) = lhs {
                    idents.push(quote::format_ident!("{}__", &ident.name));
                    values.push(rhs.into());
                } else {
                    return Err(CompilerError::InvalidAssignment(
                        "expected identifier on lhs of assignment".to_string(),
                    ));
                }
            }
            out.push(syn::parse_quote! { let (#(#idents),*) = (#(#values),*); });

            for lhs in assign_stmt.lhs {
                if let ast::Expr::Ident(ident) = &lhs {
                    let right = quote::format_ident!("{}__", &ident.name);
                    let left: syn::Expr = lhs.into();
                    out.push(syn::parse_quote! { #left = #right; });
                } else {
                    return Err(CompilerError::InvalidAssignment(
                        "expected identifier on lhs of assignment".to_string(),
                    ));
                }
            }

            return Ok(out);
        }

        // e += 4
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
                return_stmt.results.into_iter().next().unwrap(),
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
            _ => unreachable!("unsupported binary op: {:?}", token),
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

        // The stdlib_call pass rewrites fmt::Println to a macro invocation, which
        // creates new idents without original spans. Verify the string literal is mapped.
        let has_hello =
            (0..parsed_sm.get_name_count()).any(|i| {
                parsed_sm.get_name(i).is_some_and(|n| n.contains("Hello"))
            });
        assert!(
            has_hello,
            "Expected string literal mapping in source map"
        );
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
        let filenames: Vec<String> = paths.iter().map(|p| super::import_path_to_filename(p)).collect();
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
        let go_source = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hello\")\n}\n";
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
        assert!(compiled.modules.contains_key("fmt"));
        let fmt_mod = &compiled.modules["fmt"];
        assert_eq!(fmt_mod.mod_name, "fmt");
        assert_eq!(fmt_mod.filename, "fmt.rs");
        assert!(fmt_mod.is_stdlib);
    }

    #[test]
    fn compile_program_multi_generates_valid_rust() {
        let go_source = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"test\")\n}\n";
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
        assert!(output.files.contains_key("fmt.rs"));
        assert!(output.files.contains_key("builtin.rs"));
        let main_rs = &output.files["main.rs"];
        assert!(main_rs.contains("mod lib"));
        assert!(main_rs.contains("use lib::*"));
        let lib_rs = &output.files["lib.rs"];
        assert!(lib_rs.contains("pub mod fmt"));
        assert!(lib_rs.contains("pub mod builtin"));
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
                pub type MyInt = isize;
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
                type buffer = Vec<u8>;
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
                pub type Dict = std::collections::HashMap<String, isize>;
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
                pub struct Point {
                    pub X: isize,
                    pub Y: isize,
                }
                pub fn main() {
                    let mut p = Point { X: 1, Y: 2 };
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
                    let mut s = vec![1, 2, 3];
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
                    let mut x = None;
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
                    let mut s = vec![1, 2, 3];
                    for (mut i, mut v) in (s).iter().enumerate() {
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
                    let mut s = vec![1, 2, 3];
                    let mut t = s[1..2];
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
                    let mut s = vec![1, 2, 3];
                    let mut n = builtin::len(&s);
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
                    let mut s = vec![1, 2];
                    s = builtin::append(s, 3);
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
                    panic!("{}", "oh no");
                }
            },
        );
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
                use ::std::println;
                pub fn main() {
                    println!("{}", "hello");
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
                pub struct Flags {
                    pub X: bool,
                }
                pub fn main() {
                    let mut f = Flags {};
                }
            },
        );
    }

    #[test]
    fn compile_program_multi_with_stdlib_chain() {
        let go_source = r#"package main

import "fmt"
import "errors"
import "strconv"

func main() {
	fmt.Println("hello")
	e := errors.New("fail")
	s := strconv.Itoa(42)
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
                "strconv".to_string(),
            ],
        };
        let compiled = super::compile_program_multi(program).unwrap();
        assert!(compiled.modules.contains_key("fmt"));
        assert!(compiled.modules.contains_key("errors"));
        assert!(compiled.modules.contains_key("strconv"));
        let output = backend_rust::generate_multi(compiled).unwrap();
        assert!(output.files.contains_key("fmt.rs"));
        assert!(output.files.contains_key("errors.rs"));
        assert!(output.files.contains_key("strconv.rs"));
        assert!(output.files.contains_key("builtin.rs"));
        let lib_rs = &output.files["lib.rs"];
        assert!(lib_rs.contains("pub mod fmt"));
        assert!(lib_rs.contains("pub mod errors"));
        assert!(lib_rs.contains("pub mod strconv"));
        assert!(lib_rs.contains("pub mod builtin"));
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
