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
}

/// Record a mapping if tracking is enabled.
fn record_mapping(pos: &token::Position, name: Option<&str>) {
    TRACKER.with(|t| {
        t.borrow_mut()
            .record(pos.line as u32, pos.column as u32, name);
    });
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

fn compile_type_spec(ts: ast::TypeSpec) -> Result<syn::Item, CompilerError> {
    let name = ts.name.ok_or_else(|| {
        CompilerError::UnsupportedConstruct("type spec has no name".to_string())
    })?;
    let vis: syn::Visibility = (&name).into();
    let ident: syn::Ident = name.into();

    match ts.type_ {
        ast::Expr::StructType(struct_type) => {
            let mut fields = syn::punctuated::Punctuated::new();
            if let Some(field_list) = struct_type.fields {
                for field in field_list.list {
                    let field_type = field.type_.ok_or_else(|| {
                        CompilerError::UnsupportedConstruct(
                            "struct field has no type".to_string(),
                        )
                    })?;
                    let rust_type: syn::Type = field_type.into();

                    if let Some(names) = field.names {
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
                    }
                }
            }

            if !fields.empty_or_trailing() {
                fields.push_punct(<Token![,]>::default());
            }

            Ok(syn::Item::Struct(syn::ItemStruct {
                attrs: vec![],
                vis,
                struct_token: <Token![struct]>::default(),
                ident,
                generics: syn::Generics::default(),
                fields: syn::Fields::Named(syn::FieldsNamed {
                    brace_token: syn::token::Brace::default(),
                    named: fields,
                }),
                semi_token: None,
            }))
        }
        ast::Expr::InterfaceType(_) => {
            // interface{} / any → empty trait; named interfaces → trait with methods
            // For now, generate an empty trait
            Ok(syn::Item::Trait(syn::ItemTrait {
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
                items: vec![],
            }))
        }
        other => {
            let rust_type: syn::Type = other.into();
            Ok(syn::Item::Type(syn::ItemType {
                attrs: vec![],
                vis,
                type_token: <Token![type]>::default(),
                ident,
                generics: syn::Generics::default(),
                eq_token: <Token![=]>::default(),
                ty: Box::new(rust_type),
                semi_token: <Token![;]>::default(),
            }))
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
    "len", "cap", "append", "make", "new", "copy", "delete", "panic", "println", "print",
];

fn is_builtin_call(call_expr: &ast::CallExpr) -> bool {
    if let ast::Expr::Ident(ident) = &*call_expr.fun {
        BUILTINS.contains(&ident.name)
    } else {
        false
    }
}

fn compile_builtin(call_expr: ast::CallExpr) -> syn::Expr {
    let name = match *call_expr.fun {
        ast::Expr::Ident(ident) => ident.name.to_string(),
        _ => unreachable!(),
    };

    let args: Vec<syn::Expr> = call_expr
        .args
        .unwrap_or_default()
        .into_iter()
        .map(syn::Expr::from)
        .collect();

    match name.as_str() {
        "len" => {
            let x = &args[0];
            syn::parse_quote! { (#x).len() }
        }
        "cap" => {
            let x = &args[0];
            syn::parse_quote! { (#x).capacity() }
        }
        "append" => {
            let slice = &args[0];
            let elems = &args[1..];
            if elems.len() == 1 {
                let elem = &elems[0];
                syn::parse_quote! { { let mut __tmp = #slice; __tmp.push(#elem); __tmp } }
            } else {
                let pushes: Vec<proc_macro2::TokenStream> = elems
                    .iter()
                    .map(|e| quote::quote! { __tmp.push(#e); })
                    .collect();
                syn::parse_quote! { { let mut __tmp = #slice; #(#pushes)* __tmp } }
            }
        }
        "make" => {
            if args.len() <= 1 {
                syn::parse_quote! { Default::default() }
            } else if args.len() == 2 {
                let size = &args[1];
                syn::parse_quote! { vec![Default::default(); #size] }
            } else {
                let cap = &args[2];
                syn::parse_quote! { Vec::with_capacity(#cap) }
            }
        }
        "new" => {
            let type_arg = &args[0];
            syn::parse_quote! { Box::new(#type_arg::default()) }
        }
        "copy" => {
            let dst = &args[0];
            let src = &args[1];
            syn::parse_quote! { (#dst)[..].copy_from_slice(&(#src)) }
        }
        "delete" => {
            let map = &args[0];
            let key = &args[1];
            syn::parse_quote! { (#map).remove(&#key) }
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

fn compile_range_stmt(range_stmt: ast::RangeStmt) -> Result<Vec<syn::Stmt>, CompilerError> {
    let x: syn::Expr = range_stmt.x.into();
    let body: syn::Block = range_stmt.body.try_into()?;

    let is_define = range_stmt.tok == Some(token::Token::DEFINE);

    match (range_stmt.key, range_stmt.value) {
        (Some(key_expr), Some(val_expr)) => {
            let key_pat = expr_to_pat(&key_expr);
            let val_pat = expr_to_pat(&val_expr);
            let pat: syn::Pat = syn::parse_quote! { (#key_pat, #val_pat) };
            if is_define {
                Ok(vec![syn::Stmt::Expr(
                    syn::Expr::ForLoop(syn::ExprForLoop {
                        attrs: vec![],
                        label: None,
                        for_token: <Token![for]>::default(),
                        pat: Box::new(pat),
                        in_token: <Token![in]>::default(),
                        expr: Box::new(syn::parse_quote! { (#x).iter().enumerate() }),
                        body,
                    }),
                    None,
                )])
            } else {
                Ok(vec![syn::Stmt::Expr(
                    syn::Expr::ForLoop(syn::ExprForLoop {
                        attrs: vec![],
                        label: None,
                        for_token: <Token![for]>::default(),
                        pat: Box::new(pat),
                        in_token: <Token![in]>::default(),
                        expr: Box::new(syn::parse_quote! { (#x).iter().enumerate() }),
                        body,
                    }),
                    None,
                )])
            }
        }
        (Some(key_expr), None) => {
            let key_pat = expr_to_pat(&key_expr);
            Ok(vec![syn::Stmt::Expr(
                syn::Expr::ForLoop(syn::ExprForLoop {
                    attrs: vec![],
                    label: None,
                    for_token: <Token![for]>::default(),
                    pat: Box::new(key_pat),
                    in_token: <Token![in]>::default(),
                    expr: Box::new(syn::parse_quote! { 0..(#x).len() }),
                    body,
                }),
                None,
            )])
        }
        (None, None) => {
            // for range x { ... } → for _ in x { ... }
            let pat: syn::Pat = syn::parse_quote! { _ };
            Ok(vec![syn::Stmt::Expr(
                syn::Expr::ForLoop(syn::ExprForLoop {
                    attrs: vec![],
                    label: None,
                    for_token: <Token![for]>::default(),
                    pat: Box::new(pat),
                    in_token: <Token![in]>::default(),
                    expr: Box::new(x),
                    body,
                }),
                None,
            )])
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
                if is_builtin_call(&call_expr) {
                    return compile_builtin(call_expr);
                }
                Self::Call(call_expr.into())
            }
            ast::Expr::Ident(ident) if ident.name == "nil" => syn::parse_quote! { None },
            ast::Expr::Ident(ident) if ident.name == "true" => syn::parse_quote! { true },
            ast::Expr::Ident(ident) if ident.name == "false" => syn::parse_quote! { false },
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
                    token::Token::AND => {
                        // &x → Box::new(x)
                        let inner: syn::Expr = (*unary_expr.x).into();
                        return Self::Call(syn::ExprCall {
                            attrs: vec![],
                            func: Box::new(syn::parse_quote! { Box::new }),
                            paren_token: syn::token::Paren::default(),
                            args: {
                                let mut a = syn::punctuated::Punctuated::new();
                                a.push(inner);
                                a
                            },
                        });
                    }
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
        let mut items = vec![];
        let mut methods: BTreeMap<String, Vec<syn::ImplItemFn>> = BTreeMap::new();

        for decl in file.decls {
            match decl {
                ast::Decl::FuncDecl(func_decl) => {
                    if func_decl.recv.is_some() {
                        let (type_name, method) = compile_method(func_decl)?;
                        methods.entry(type_name).or_default().push(method);
                    } else {
                        items.push(syn::Item::Fn(func_decl.try_into()?));
                    }
                }
                ast::Decl::GenDecl(gen_decl) => {
                    if gen_decl.tok == token::Token::CONST || gen_decl.tok == token::Token::VAR {
                        for spec in gen_decl.specs {
                            if let ast::Spec::ValueSpec(vs) = spec {
                                items.extend(compile_top_level_value_spec(vs, gen_decl.tok)?);
                            }
                        }
                    } else if gen_decl.tok == token::Token::TYPE {
                        for spec in gen_decl.specs {
                            if let ast::Spec::TypeSpec(ts) = spec {
                                items.push(compile_type_spec(ts)?);
                            }
                        }
                    }
                }
            }
        }

        for (type_name, method_list) in methods {
            let type_ident = syn::Ident::new(&type_name, Span::mixed_site());
            items.push(syn::Item::Impl(syn::ItemImpl {
                attrs: vec![],
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics: syn::Generics::default(),
                trait_: None,
                self_ty: Box::new(syn::parse_quote! { #type_ident }),
                brace_token: syn::token::Brace::default(),
                items: method_list.into_iter().map(syn::ImplItem::Fn).collect(),
            }));
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
        // The Rust token ("fn") will be extracted dynamically from the output
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

        let block = Box::new(if let Some(body) = func_decl.body {
            body.try_into()?
        } else {
            syn::Block {
                brace_token: syn::token::Brace::default(),
                stmts: vec![],
            }
        });

        let output = compile_return_type(func_decl.type_.results)?;

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
        record_mapping(&ident.name_pos, Some(ident.name));

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
                        ));
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
            ast::Stmt::BlockStmt(s) => {
                Ok(vec![syn::Stmt::Expr(syn::Expr::Block(s.try_into()?), None)])
            }
            ast::Stmt::BranchStmt(s) => Ok(s.into()),
            ast::Stmt::DeclStmt(s) => Ok(s.into()),
            ast::Stmt::DeferStmt(s) => {
                // defer f() → scopeguard-style: let _defer = { let __f = move || { f(); }; ... };
                // Simplified: just wrap the call in a closure assigned to a _defer binding.
                // The deferred call will execute at scope exit via Drop.
                let call: syn::Expr = syn::Expr::Call(s.call.into());
                Ok(vec![syn::parse_quote! {
                    let _defer = {
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
            ast::Stmt::GoStmt(_) => {
                // Goroutines would need to be converted to threads/async
                // For now, we skip it
                Ok(vec![])
            }
            ast::Stmt::IfStmt(s) => Ok(vec![syn::Stmt::Expr(syn::Expr::If(s.try_into()?), None)]),
            ast::Stmt::IncDecStmt(s) => Ok(s.into()),
            ast::Stmt::LabeledStmt(s) => s.try_into(),
            ast::Stmt::ReturnStmt(s) => {
                Ok(vec![syn::Stmt::Expr(syn::Expr::Return(s.into()), None)])
            }
            ast::Stmt::RangeStmt(s) => compile_range_stmt(s),
            ast::Stmt::SwitchStmt(s) => Ok(vec![syn::Stmt::Expr(s.try_into()?, None)]),
            ast::Stmt::TypeSwitchStmt(s) => compile_type_switch_stmt(s),
            ast::Stmt::SelectStmt(_) | ast::Stmt::SendStmt(_) | ast::Stmt::CommClause(_) | ast::Stmt::CaseClause(_) => {
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
                let type_expr = value_spec.type_.as_ref();
                let mut values_iter = value_spec.values.unwrap_or_default().into_iter();

                for name in names {
                    let ident: syn::Ident = name.into();
                    let init_expr: Option<syn::Expr> = values_iter.next().map(|v| v.into());

                    let init = init_expr.unwrap_or_else(|| go_zero_value(type_expr));
                    stmts.push(syn::parse_quote! {
                        let mut #ident = #init;
                    });
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
                    let first_lhs =
                        assign_stmt.lhs.into_iter().next().ok_or_else(|| {
                            CompilerError::InvalidAssignment("empty lhs".to_string())
                        })?;
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
        let main_rs = &output.files["main.rs"];
        assert!(main_rs.contains("mod lib"));
        assert!(main_rs.contains("use lib::*"));
        let lib_rs = &output.files["lib.rs"];
        assert!(lib_rs.contains("pub mod fmt"));
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
                    let mut n = (s).len();
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
                    s = { let mut __tmp = s; __tmp.push(3); __tmp };
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
                    let _defer = {
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
}
