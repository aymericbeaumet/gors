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
//! - Goroutines and channels
//! - Defer statements
//! - Goto statements
//! - Some complex type expressions

mod passes;

use crate::mapping::{GoSpan, MappingKind, SourceMap};
use crate::{ast, token};
use proc_macro2::Span;
use std::cell::RefCell;
use std::fmt;
use syn::Token;

// Thread-local storage for source map during compilation
thread_local! {
    static SOURCE_MAP: RefCell<Option<SourceMap>> = const { RefCell::new(None) };
}

/// Start tracking source mappings for the current compilation.
fn start_tracking() {
    SOURCE_MAP.with(|sm| {
        *sm.borrow_mut() = Some(SourceMap::new());
    });
}

/// Stop tracking and return the collected source map.
fn stop_tracking() -> Option<SourceMap> {
    SOURCE_MAP.with(|sm| sm.borrow_mut().take())
}

/// Record a span mapping if tracking is enabled.
fn record_mapping(go_span: GoSpan, kind: MappingKind) -> Option<u32> {
    SOURCE_MAP.with(|sm| {
        if let Some(ref mut map) = *sm.borrow_mut() {
            Some(map.add(go_span, kind))
        } else {
            None
        }
    })
}

/// Record a named span mapping if tracking is enabled.
fn record_named_mapping(go_span: GoSpan, kind: MappingKind, name: &str) -> Option<u32> {
    SOURCE_MAP.with(|sm| {
        if let Some(ref mut map) = *sm.borrow_mut() {
            Some(map.add_named(go_span, kind, name))
        } else {
            None
        }
    })
}

/// Helper to create a GoSpan from a Position.
fn go_span_from_position(pos: &token::Position, len: usize) -> GoSpan {
    GoSpan::new(
        pos.line as u32,
        pos.column as u32,
        pos.line as u32,
        (pos.column + len) as u32,
    )
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
    let mut out = TryInto::<syn::File>::try_into(file)?;
    passes::pass(&mut out);
    Ok(out)
}

/// Compile a Go AST into a Rust `syn` AST with source mapping.
///
/// This is like [`compile`], but also returns a [`SourceMap`] that tracks
/// the correspondence between Go source positions and Rust output positions.
///
/// # Arguments
///
/// * `file` - The Go AST to compile
///
/// # Returns
///
/// Returns `Ok((syn::File, SourceMap))` on success, or `Err(CompilerError)` if the
/// Go code contains constructs that cannot be translated to Rust.
///
/// # Example
///
/// ```
/// use gors::{parser, compiler};
///
/// let go_source = "package main\n\nfunc main() { x := 42 }";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let (rust_ast, source_map) = compiler::compile_with_source_map(go_ast).unwrap();
/// ```
pub fn compile_with_source_map(file: ast::File) -> Result<(syn::File, SourceMap), CompilerError> {
    start_tracking();
    let result = TryInto::<syn::File>::try_into(file);
    let source_map = stop_tracking().unwrap_or_default();

    match result {
        Ok(mut out) => {
            passes::pass(&mut out);
            Ok((out, source_map))
        }
        Err(e) => Err(e),
    }
}

impl From<ast::BasicLit<'_>> for syn::ExprLit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        // Record mapping for the literal
        let go_span = go_span_from_position(&basic_lit.value_pos, basic_lit.value.len());
        record_named_mapping(go_span, MappingKind::Literal, basic_lit.value);

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
                let mut value = basic_lit.value.chars();
                value.next();
                value.next_back();
                Self::Str(syn::LitStr::new(value.as_str(), Span::mixed_site()))
            }
            CHAR => {
                // Handle character literals
                let mut value = basic_lit.value.chars();
                value.next(); // skip opening '
                let ch = value.next().unwrap_or(' ');
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
        let go_span = go_span_from_position(&binary_expr.op_pos, op_str.len());
        record_named_mapping(go_span, MappingKind::Operator, op_str);

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
        // Record mapping for the call expression (from lparen to rparen)
        let go_span = GoSpan::new(
            call_expr.lparen.line as u32,
            call_expr.lparen.column as u32,
            call_expr.rparen.line as u32,
            (call_expr.rparen.column + 1) as u32,
        );
        record_mapping(go_span, MappingKind::Expression);

        // Check if this is a fmt.Println call and record a mapping for the transformed name
        // This enables sourcemap lookups after the inline_fmt pass transforms it to println!
        if let ast::Expr::SelectorExpr(ref selector) = *call_expr.fun {
            if let ast::Expr::Ident(ref x_ident) = *selector.x {
                if x_ident.name == "fmt" && selector.sel.name == "Println" {
                    // Record a mapping from the entire fmt.Println span to "println"
                    // This will match the transformed Rust output
                    let combined_span = GoSpan::new(
                        x_ident.name_pos.line as u32,
                        x_ident.name_pos.column as u32,
                        selector.sel.name_pos.line as u32,
                        (selector.sel.name_pos.column + selector.sel.name.len()) as u32,
                    );
                    record_named_mapping(combined_span, MappingKind::Identifier, "println");
                }
            }
        }

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
            ast::Expr::CallExpr(call_expr) => Self::Call(call_expr.into()),
            ast::Expr::Ident(ident) => Self::Path(ident.into()),
            ast::Expr::SelectorExpr(selector_expr) => Self::Path(selector_expr.into()),
            ast::Expr::ParenExpr(paren_expr) => Self::Paren(syn::ExprParen {
                attrs: vec![],
                paren_token: syn::token::Paren::default(),
                expr: Box::new((*paren_expr.x).into()),
            }),
            ast::Expr::UnaryExpr(unary_expr) => Self::Unary(syn::ExprUnary {
                attrs: vec![],
                op: match unary_expr.op {
                    token::Token::SUB => syn::UnOp::Neg(<Token![-]>::default()),
                    token::Token::NOT => syn::UnOp::Not(<Token![!]>::default()),
                    token::Token::MUL => syn::UnOp::Deref(<Token![*]>::default()),
                    _ => unimplemented!("unary op: {:?}", unary_expr.op),
                },
                expr: Box::new((*unary_expr.x).into()),
            }),
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
            _ => unimplemented!("{:?}", expr),
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
            _ => unimplemented!("{:?}", expr),
        }
    }
}

impl TryFrom<ast::File<'_>> for syn::File {
    type Error = CompilerError;

    fn try_from(file: ast::File) -> Result<Self, Self::Error> {
        let mut items = vec![];
        for decl in file.decls {
            if let ast::Decl::FuncDecl(func_decl) = decl {
                items.push(syn::Item::Fn(func_decl.try_into()?));
            }
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
            .ok_or_else(|| CompilerError::InvalidFunctionSignature("field has no names".to_string()))?
            .into_iter()
            .next()
            .ok_or_else(|| CompilerError::InvalidFunctionSignature("field names is empty".to_string()))?;
        let type_ = field
            .type_
            .ok_or_else(|| CompilerError::InvalidFunctionSignature("field has no type".to_string()))?;
        Ok(Self::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: None,
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
        // Record mapping for the function keyword if available
        // Use "fn" as the name since that's what "func" becomes in Rust output
        if let Some(ref func_pos) = func_decl.type_.func {
            let go_span = go_span_from_position(func_pos, 4); // "func" is 4 chars
            record_named_mapping(go_span, MappingKind::Function, "fn");
        }

        // Convert doc comments to Rust doc attributes
        let attrs = comment_group_to_attrs(&func_decl.doc);

        let mut inputs = syn::punctuated::Punctuated::new();
        for param in func_decl.type_.params.list {
            inputs.push(param.try_into()?);
        }

        let vis = (&func_decl.name).into();

        let block = Box::new(if let Some(body) = func_decl.body {
            body.try_into()?
        } else {
            syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: vec![],
            }
        });

        let output = if let Some(results) = func_decl.type_.results {
            let first_result = results
                .list
                .into_iter()
                .next()
                .ok_or_else(|| CompilerError::InvalidFunctionSignature("empty result list".to_string()))?;
            let result_type = first_result
                .type_
                .ok_or_else(|| CompilerError::InvalidFunctionSignature("result has no type".to_string()))?;
            syn::ReturnType::Type(<Token![->]>::default(), Box::new(result_type.into()))
        } else {
            syn::ReturnType::Default
        };

        let sig = syn::Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: <Token![fn]>::default(),
            ident: func_decl.name.into(),
            generics: syn::Generics {
                params: syn::punctuated::Punctuated::new(),
                lt_token: None,
                gt_token: None,
                where_clause: None,
            },
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
        let x = match *selector_expr.x {
            ast::Expr::Ident(ident) => ident,
            _ => unimplemented!(),
        };

        let mut segments = syn::punctuated::Punctuated::new();
        segments.push(syn::PathSegment {
            ident: x.into(),
            arguments: syn::PathArguments::None,
        });
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
        let go_span = go_span_from_position(&ident.name_pos, ident.name.len());
        record_named_mapping(go_span, MappingKind::Identifier, ident.name);

        Self::new(ident.name, Span::mixed_site())
    }
}

impl TryFrom<ast::IfStmt<'_>> for syn::ExprIf {
    type Error = CompilerError;

    fn try_from(if_stmt: ast::IfStmt) -> Result<Self, Self::Error> {
        let else_branch = if let Some(else_) = *if_stmt.else_ {
            Some((
                <Token![else]>::default(),
                Box::new(match else_ {
                    ast::Stmt::IfStmt(if_stmt) => syn::Expr::If(if_stmt.try_into()?),
                    ast::Stmt::BlockStmt(block_stmt) => syn::Expr::Block(block_stmt.try_into()?),
                    _ => {
                        return Err(CompilerError::UnsupportedConstruct(
                            "unsupported else branch type".to_string(),
                        ))
                    }
                }),
            ))
        } else {
            None
        };

        Ok(Self {
            attrs: vec![],
            cond: Box::new(if_stmt.cond.into()),
            if_token: <Token![if]>::default(),
            then_branch: if_stmt.body.try_into()?,
            else_branch,
        })
    }
}

impl TryFrom<ast::Stmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(stmt: ast::Stmt) -> Result<Self, Self::Error> {
        match stmt {
            ast::Stmt::AssignStmt(s) => s.try_into(),
            ast::Stmt::BlockStmt(s) => Ok(vec![syn::Stmt::Expr(syn::Expr::Block(s.try_into()?), None)]),
            ast::Stmt::BranchStmt(s) => Ok(s.into()),
            ast::Stmt::DeclStmt(s) => Ok(s.into()),
            ast::Stmt::DeferStmt(_) => {
                // Rust doesn't have defer, would need to use Drop/scope guards
                // For now, we skip it
                Ok(vec![])
            }
            ast::Stmt::EmptyStmt(_) => Ok(vec![]),
            ast::Stmt::ExprStmt(s) => {
                Ok(vec![syn::Stmt::Expr(s.x.into(), Some(<Token![;]>::default()))])
            }
            ast::Stmt::ForStmt(s) => Ok(vec![syn::Stmt::Expr(s.try_into()?, None)]),
            ast::Stmt::GoStmt(_) => {
                // Goroutines would need to be converted to threads/async
                // For now, we skip it
                Ok(vec![])
            }
            ast::Stmt::IfStmt(s) => Ok(vec![syn::Stmt::Expr(syn::Expr::If(s.try_into()?), None)]),
            ast::Stmt::IncDecStmt(s) => Ok(s.into()),
            ast::Stmt::LabeledStmt(s) => s.try_into(),
            ast::Stmt::ReturnStmt(s) => Ok(vec![syn::Stmt::Expr(syn::Expr::Return(s.into()), None)]),
            ast::Stmt::SendStmt(_) => {
                // Channel send would need different semantics
                Ok(vec![])
            }
            _ => Err(CompilerError::UnsupportedConstruct(format!(
                "unsupported statement type: {:?}",
                stmt
            ))),
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
            body.stmts.extend(Vec::<syn::Stmt>::try_from(*post)?);
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

impl From<ast::DeclStmt<'_>> for Vec<syn::Stmt> {
    fn from(decl_stmt: ast::DeclStmt) -> Self {
        let gen_decl = decl_stmt.decl;
        let mut stmts = vec![];

        for spec in gen_decl.specs {
            if let ast::Spec::ValueSpec(value_spec) = spec {
                // Convert to let statement
                let names = value_spec.names;
                let mut values_iter = value_spec.values.unwrap_or_default().into_iter();

                for name in names {
                    let ident: syn::Ident = name.into();

                    // Get the init value if available
                    let init_expr: Option<syn::Expr> = values_iter.next().map(|v| v.into());

                    // For type annotation, we'd need to pass type_ through properly
                    // For now, just use the init expression without type annotation
                    if let Some(init) = init_expr {
                        stmts.push(syn::parse_quote! {
                            let mut #ident = #init;
                        });
                    } else {
                        // Variable declared without initialization
                        // Would need default value or explicit type
                        stmts.push(syn::parse_quote! {
                            let mut #ident = Default::default();
                        });
                    }
                }
            }
            // Skip type specs and import specs in statement context
        }
        stmts
    }
}

impl TryFrom<ast::AssignStmt<'_>> for Vec<syn::Stmt> {
    type Error = CompilerError;

    fn try_from(assign_stmt: ast::AssignStmt) -> Result<Self, Self::Error> {
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
                    let first_lhs = assign_stmt
                        .lhs
                        .into_iter()
                        .next()
                        .ok_or_else(|| CompilerError::InvalidAssignment("empty lhs".to_string()))?;
                    if let ast::Expr::Ident(ident) = first_lhs {
                        syn::Pat::Ident(syn::PatIdent {
                            attrs: vec![],
                            ident: ident.into(),
                            by_ref: None,
                            subpat: None,
                            mutability: Some(<Token![mut]>::default()),
                        })
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
                            elems.push(syn::Pat::Ident(syn::PatIdent {
                                attrs: vec![],
                                ident: ident.into(),
                                by_ref: None,
                                subpat: None,
                                mutability: Some(<Token![mut]>::default()),
                            }))
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
                    let first_rhs = assign_stmt
                        .rhs
                        .into_iter()
                        .next()
                        .ok_or_else(|| CompilerError::InvalidAssignment("empty rhs".to_string()))?;
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
            for (lhs, rhs) in assign_stmt.lhs.iter().zip(assign_stmt.rhs.into_iter()) {
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
        // Record mapping for the return keyword
        let go_span = go_span_from_position(&return_stmt.return_, 6); // "return" is 6 chars
        record_mapping(go_span, MappingKind::Keyword);

        // Handle return statements: if there are results, convert the first one
        let expr = return_stmt.results.into_iter().next().map(Into::into);
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

    use super::{compile, compile_with_source_map};
    use crate::codegen::generate_with_positions;
    use crate::mapping::MappingKind;
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
        let go_source = r#"package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let (compiled, mut source_map) = compile_with_source_map(parsed).unwrap();

        // Check that we have a mapping with name "println" for the fmt.Println call
        let println_mappings: Vec<_> = source_map
            .mappings()
            .iter()
            .filter(|m| m.name.as_deref() == Some("println"))
            .collect();

        assert!(
            !println_mappings.is_empty(),
            "Expected a mapping named 'println' for fmt.Println call"
        );

        // The mapping should span from "fmt" to the end of "Println"
        // Line 6, column 2 (1-indexed): fmt.Println
        let mapping = &println_mappings[0];
        assert_eq!(mapping.go_span.start_line, 6);
        assert_eq!(mapping.go_span.start_column, 2);
        assert_eq!(mapping.kind, MappingKind::Identifier);

        // Generate the Rust code and update the source map
        let _rust_source = generate_with_positions(compiled, &mut source_map).unwrap();

        // After generation, the rust_span should be set to the position of "println" in the output
        let println_mapping = source_map
            .mappings()
            .iter()
            .find(|m| m.name.as_deref() == Some("println"))
            .unwrap();

        // The Rust span should now be set (non-zero)
        assert!(
            println_mapping.rust_span.start_line > 0,
            "Expected rust_span to be set for println mapping"
        );

        // Test the lookup: Go position should map to Rust position
        let rust_span = source_map.go_to_rust(6, 2); // Position of "fmt" in Go
        assert!(
            rust_span.is_some(),
            "Expected to find Rust span for Go position (6, 2)"
        );

        // Test reverse lookup: Rust position should map back to Go
        let go_span = source_map.rust_to_go(
            println_mapping.rust_span.start_line,
            println_mapping.rust_span.start_column,
        );
        assert!(
            go_span.is_some(),
            "Expected to find Go span for Rust println position"
        );
    }

    #[test]
    fn it_should_create_sourcemap_for_string_literal() {
        let go_source = r#"package main

import "fmt"

func main() {
	fmt.Println("Hello, 世界")
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let (compiled, mut source_map) = compile_with_source_map(parsed).unwrap();

        // Check that we have a mapping for the string literal
        let string_mappings: Vec<_> = source_map
            .mappings()
            .iter()
            .filter(|m| m.name.as_deref() == Some("\"Hello, 世界\""))
            .collect();

        assert!(
            !string_mappings.is_empty(),
            "Expected a mapping for the string literal"
        );

        let mapping = &string_mappings[0];
        assert_eq!(mapping.go_span.start_line, 6);
        assert_eq!(mapping.kind, MappingKind::Literal);

        // Generate the Rust code and update the source map
        let rust_source = generate_with_positions(compiled, &mut source_map).unwrap();

        // The generated Rust code should contain the same string literal
        assert!(
            rust_source.contains("\"Hello, 世界\""),
            "Expected Rust output to contain the string literal"
        );

        // After generation, the rust_span should be set
        let string_mapping = source_map
            .mappings()
            .iter()
            .find(|m| m.name.as_deref() == Some("\"Hello, 世界\""))
            .unwrap();

        assert!(
            string_mapping.rust_span.start_line > 0,
            "Expected rust_span to be set for string literal mapping"
        );

        // Test bidirectional lookup for the string literal
        let rust_span = source_map.go_to_rust(
            string_mapping.go_span.start_line,
            string_mapping.go_span.start_column,
        );
        assert!(
            rust_span.is_some(),
            "Expected to find Rust span for Go string literal position"
        );

        let go_span = source_map.rust_to_go(
            string_mapping.rust_span.start_line,
            string_mapping.rust_span.start_column,
        );
        assert!(
            go_span.is_some(),
            "Expected to find Go span for Rust string literal position"
        );
    }

    #[test]
    fn it_should_create_sourcemap_for_func_declaration() {
        let go_source = r#"package main

func main() {
}"#;
        let parsed = parse_file("test.go", go_source).unwrap();
        let (compiled, mut source_map) = compile_with_source_map(parsed).unwrap();

        // Check that we have mappings for both "fn" (keyword) and "main" (identifier)
        let fn_mappings: Vec<_> = source_map
            .mappings()
            .iter()
            .filter(|m| m.name.as_deref() == Some("fn"))
            .collect();

        let main_mappings: Vec<_> = source_map
            .mappings()
            .iter()
            .filter(|m| m.name.as_deref() == Some("main"))
            .collect();

        assert!(
            !fn_mappings.is_empty(),
            "Expected a mapping for 'fn' keyword (from 'func')"
        );
        assert!(
            !main_mappings.is_empty(),
            "Expected a mapping for 'main' identifier"
        );

        // The "func" keyword should be on line 3
        let fn_mapping = &fn_mappings[0];
        assert_eq!(fn_mapping.go_span.start_line, 3);
        assert_eq!(fn_mapping.kind, MappingKind::Function);

        // The "main" identifier should also be on line 3
        let main_mapping = &main_mappings[0];
        assert_eq!(main_mapping.go_span.start_line, 3);
        assert_eq!(main_mapping.kind, MappingKind::Identifier);

        // Generate the Rust code and update the source map
        let rust_source = generate_with_positions(compiled, &mut source_map).unwrap();

        // The generated Rust code should contain "fn main"
        assert!(
            rust_source.contains("fn main"),
            "Expected Rust output to contain 'fn main'"
        );

        // After generation, both rust_spans should be set
        let fn_mapping = source_map
            .mappings()
            .iter()
            .find(|m| m.name.as_deref() == Some("fn"))
            .unwrap();
        let main_mapping = source_map
            .mappings()
            .iter()
            .find(|m| m.name.as_deref() == Some("main"))
            .unwrap();

        assert!(
            fn_mapping.rust_span.start_line > 0,
            "Expected rust_span to be set for 'fn' keyword mapping"
        );
        assert!(
            main_mapping.rust_span.start_line > 0,
            "Expected rust_span to be set for 'main' identifier mapping"
        );

        // Test bidirectional lookup for "func" keyword -> "fn"
        let rust_span = source_map.go_to_rust(
            fn_mapping.go_span.start_line,
            fn_mapping.go_span.start_column,
        );
        assert!(
            rust_span.is_some(),
            "Expected to find Rust span for Go 'func' keyword position"
        );

        // Test reverse lookup for "fn"
        let go_span = source_map.rust_to_go(
            fn_mapping.rust_span.start_line,
            fn_mapping.rust_span.start_column,
        );
        assert!(
            go_span.is_some(),
            "Expected to find Go span for Rust 'fn' keyword position"
        );

        // Test bidirectional lookup for "main" identifier
        let rust_span = source_map.go_to_rust(
            main_mapping.go_span.start_line,
            main_mapping.go_span.start_column,
        );
        assert!(
            rust_span.is_some(),
            "Expected to find Rust span for Go 'main' identifier position"
        );

        let go_span = source_map.rust_to_go(
            main_mapping.rust_span.start_line,
            main_mapping.rust_span.start_column,
        );
        assert!(
            go_span.is_some(),
            "Expected to find Go span for Rust 'main' identifier position"
        );
    }
}
