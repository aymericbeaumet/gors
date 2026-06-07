//! Rust source printer.
//!
//! This module prints Rust source code from a `syn::File` AST.

mod tracked;

use quote::ToTokens;
use sha2::{Digest, Sha256};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub use tracked::{
    BlankLineInfo, CommentToInsert, generate_with_comments, generate_with_comments_and_blanks,
};

#[derive(Clone)]
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

/// Source mapping between input and output positions.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Individual position mappings
    pub mappings: Vec<Mapping>,
    /// Source file name
    pub source_file: String,
    /// Optional source content
    pub source_content: Option<String>,
}

/// A single position mapping from input to output.
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Input line number (1-based)
    pub input_line: u32,
    /// Input column number (1-based)
    pub input_column: u32,
    /// Output line number (1-based)
    pub output_line: u32,
    /// Output column number (1-based)
    pub output_column: u32,
    /// Optional name/identifier at this position
    pub name: Option<String>,
}

impl SourceMap {
    /// Create a new empty source map.
    pub fn new(source_file: &str) -> Self {
        Self {
            mappings: Vec::new(),
            source_file: source_file.to_string(),
            source_content: None,
        }
    }

    /// Add a mapping.
    pub fn add_mapping(
        &mut self,
        input_line: u32,
        input_column: u32,
        output_line: u32,
        output_column: u32,
        name: Option<String>,
    ) {
        self.mappings.push(Mapping {
            input_line,
            input_column,
            output_line,
            output_column,
            name,
        });
    }

    /// Look up output position for a given input position.
    /// Returns (output_line, output_column, end_line, end_column) if found.
    pub fn input_to_output(&self, line: u32, column: u32) -> Option<(u32, u32, u32, u32)> {
        // Find the closest mapping at or before the given position
        let mut best: Option<&Mapping> = None;
        for mapping in &self.mappings {
            if mapping.input_line == line {
                if mapping.input_column <= column {
                    match best {
                        None => best = Some(mapping),
                        Some(b) if mapping.input_column > b.input_column => best = Some(mapping),
                        _ => {}
                    }
                }
            }
        }
        best.map(|m| {
            // Return a span (single token for now)
            (
                m.output_line,
                m.output_column,
                m.output_line,
                m.output_column + 1,
            )
        })
    }

    /// Look up input position for a given output position.
    /// Returns (input_line, input_column, end_line, end_column) if found.
    pub fn output_to_input(&self, line: u32, column: u32) -> Option<(u32, u32, u32, u32)> {
        // Find the closest mapping at or before the given position
        let mut best: Option<&Mapping> = None;
        for mapping in &self.mappings {
            if mapping.output_line == line {
                if mapping.output_column <= column {
                    match best {
                        None => best = Some(mapping),
                        Some(b) if mapping.output_column > b.output_column => best = Some(mapping),
                        _ => {}
                    }
                }
            }
        }
        best.map(|m| {
            // Return a span (single token for now)
            (
                m.input_line,
                m.input_column,
                m.input_line,
                m.input_column + 1,
            )
        })
    }
}

/// Error type for code generation.
#[derive(Debug, Clone)]
pub struct CodegenError {
    /// Error message
    pub message: String,
    /// Optional source location (line, column)
    pub location: Option<(u32, u32)>,
    /// Error kind
    pub kind: CodegenErrorKind,
}

/// Kind of code generation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodegenErrorKind {
    /// General generation error
    Generation,
    /// Unsupported construct
    Unsupported,
    /// Type inference error
    TypeInference,
    /// Validation error
    Validation,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some((line, col)) = self.location {
            write!(f, "{}:{}: {}", line, col, self.message)
        } else {
            write!(f, "{}", self.message)
        }
    }
}

impl std::error::Error for CodegenError {}

impl CodegenError {
    /// Create a new generation error.
    pub fn generation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::Generation,
        }
    }

    /// Create a new unsupported construct error.
    pub fn unsupported(message: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            message: message.into(),
            location: Some((line, column)),
            kind: CodegenErrorKind::Unsupported,
        }
    }

    /// Create a new type inference error.
    pub fn type_inference(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::TypeInference,
        }
    }

    /// Create a new validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            location: None,
            kind: CodegenErrorKind::Validation,
        }
    }

    /// Add location information.
    pub fn with_location(mut self, line: u32, column: u32) -> Self {
        self.location = Some((line, column));
        self
    }
}

/// Write formatted Rust source code to a writer.
///
/// # Arguments
///
/// * `w` - The writer to output the formatted code to
/// * `file` - The Rust AST to format
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if writing fails.
pub fn fprint<W: std::io::Write>(
    mut w: W,
    file: syn::File,
) -> Result<(), Box<dyn std::error::Error>> {
    let formatted = prettyplease::unparse(&file);

    for (i, line) in formatted.lines().enumerate() {
        if i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")) {
            w.write_all(b"\n")?;
        }
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
    }

    Ok(())
}

/// Generate formatted Rust source code as a String.
///
/// # Arguments
///
/// * `file` - The Rust AST to format
///
/// # Returns
///
/// Returns `Ok(String)` containing the formatted source code, or an error
/// if formatting fails.
///
/// # Example
///
/// ```
/// use gors::{parser, compiler, printer};
///
/// let go_source = "package main\n\nfunc main() {}";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let rust_ast = compiler::compile(go_ast).unwrap();
/// let rust_source = printer::generate(rust_ast).unwrap();
/// ```
pub fn generate(mut file: syn::File) -> Result<String, Box<dyn std::error::Error>> {
    order_generated_file(&mut file);
    let mut output = Vec::new();
    fprint(&mut output, file)?;
    Ok(String::from_utf8(output)?)
}

const GENERATED_HEADER: &str = "//! Generated by gors. Do not edit.\n";
const GENERATED_LINTS: &str = "#![deny(dead_code, unused_imports, unused_macros, unsafe_code)]\n#![allow(nonstandard_style, private_interfaces, unreachable_code, unused_assignments, unused_mut, unused_parens, unused_variables)]";

fn generated_source(source: String) -> String {
    format!("{GENERATED_HEADER}{GENERATED_LINTS}\n\n{source}")
}

fn ordered_dependency_modules(
    program: &crate::compiler::CompiledProgram,
) -> Vec<&crate::compiler::CompiledModule> {
    let mut modules: Vec<_> = program
        .modules
        .values()
        .filter(|module| !module.is_main)
        .collect();
    modules.sort_by(|a, b| {
        a.mod_name
            .cmp(&b.mod_name)
            .then_with(|| a.filename.cmp(&b.filename))
    });
    modules
}

fn order_generated_file(file: &mut syn::File) {
    order_items(&mut file.items);
}

fn order_items(items: &mut [syn::Item]) {
    for item in items.iter_mut() {
        match item {
            syn::Item::Impl(item_impl) => order_impl_items(&mut item_impl.items),
            syn::Item::Mod(item_mod) => {
                if let Some((_, items)) = &mut item_mod.content {
                    order_items(items);
                }
            }
            _ => {}
        }
    }
    items.sort_by_key(item_order_key);
}

fn order_impl_items(items: &mut [syn::ImplItem]) {
    items.sort_by_key(impl_item_order_key);
}

fn item_order_key(item: &syn::Item) -> (u8, u8, String) {
    match item {
        syn::Item::Use(item_use) => (0, 0, item_use.tree.to_token_stream().to_string()),
        syn::Item::Fn(item_fn) => (
            1,
            visibility_rank(&item_fn.vis),
            item_fn.sig.ident.to_string(),
        ),
        syn::Item::Const(item_const) => (
            2,
            visibility_rank(&item_const.vis),
            item_const.ident.to_string(),
        ),
        syn::Item::Static(item_static) => (
            3,
            visibility_rank(&item_static.vis),
            item_static.ident.to_string(),
        ),
        syn::Item::Struct(item_struct) => (
            4,
            visibility_rank(&item_struct.vis),
            item_struct.ident.to_string(),
        ),
        syn::Item::Enum(item_enum) => (
            5,
            visibility_rank(&item_enum.vis),
            item_enum.ident.to_string(),
        ),
        syn::Item::Trait(item_trait) => (
            6,
            visibility_rank(&item_trait.vis),
            item_trait.ident.to_string(),
        ),
        syn::Item::Impl(item_impl) => (7, 0, impl_self_type_name(item_impl)),
        syn::Item::Mod(item_mod) => (
            8,
            visibility_rank(&item_mod.vis),
            item_mod.ident.to_string(),
        ),
        syn::Item::Type(item_type) => (
            9,
            visibility_rank(&item_type.vis),
            item_type.ident.to_string(),
        ),
        syn::Item::Macro(item_macro) => {
            if item_macro.ident.is_some() && item_macro.mac.path.is_ident("macro_rules") {
                (
                    10,
                    0,
                    item_macro
                        .ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                )
            } else {
                (10, 1, item_macro.mac.path.to_token_stream().to_string())
            }
        }
        other => (11, 0, other.to_token_stream().to_string()),
    }
}

fn impl_item_order_key(item: &syn::ImplItem) -> (u8, u8, String) {
    match item {
        syn::ImplItem::Fn(item_fn) => (
            0,
            visibility_rank(&item_fn.vis),
            item_fn.sig.ident.to_string(),
        ),
        syn::ImplItem::Const(item_const) => (
            1,
            visibility_rank(&item_const.vis),
            item_const.ident.to_string(),
        ),
        syn::ImplItem::Type(item_type) => (
            2,
            visibility_rank(&item_type.vis),
            item_type.ident.to_string(),
        ),
        syn::ImplItem::Macro(item_macro) => {
            (3, 0, item_macro.mac.path.to_token_stream().to_string())
        }
        other => (4, 0, other.to_token_stream().to_string()),
    }
}

fn visibility_rank(vis: &syn::Visibility) -> u8 {
    match vis {
        syn::Visibility::Public(_) | syn::Visibility::Restricted(_) => 0,
        syn::Visibility::Inherited => 1,
    }
}

fn impl_self_type_name(item_impl: &syn::ItemImpl) -> String {
    item_impl.self_ty.to_token_stream().to_string()
}

/// Generate Rust source code with source map tracking.
///
/// # Arguments
///
/// * `file` - The Rust AST to format
/// * `source_file` - Name of the source file for the source map
///
/// # Returns
///
/// Returns `Ok((String, SourceMap))` containing the formatted source code and source map.
#[derive(Clone)]
pub struct GeneratedOutput {
    pub files: std::collections::BTreeMap<String, String>,
}

static MULTI_CODEGEN_CACHE: OnceLock<Mutex<std::collections::BTreeMap<String, GeneratedOutput>>> =
    OnceLock::new();

fn main_wrapper_module_name(dependency_mods: &[&str]) -> String {
    let mut name = "__gors_lib".to_string();
    while dependency_mods.contains(&name.as_str()) {
        name.push('_');
    }
    name
}

pub fn generate_multi(
    program: crate::compiler::CompiledProgram,
) -> Result<GeneratedOutput, Box<dyn std::error::Error>> {
    let cache_key = multi_codegen_cache_key(&program);
    if let Ok(cache) = MULTI_CODEGEN_CACHE
        .get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        && let Some(output) = cache.get(&cache_key)
    {
        return Ok(output.clone());
    }

    let timer = ProfileTimer::start("printer.formatting");
    let mut files = std::collections::BTreeMap::new();
    let mut mod_decls = Vec::new();

    let dependency_modules = ordered_dependency_modules(&program);
    for module in &dependency_modules {
        let source = generated_source(generate(module.file.clone())?);
        files.insert(module.filename.clone(), source);

        if module.filename == format!("{}.rs", module.mod_name) {
            mod_decls.push(format!("pub mod {};", module.mod_name));
        } else {
            mod_decls.push(format!(
                "#[path = \"{}\"]\npub mod {};",
                module.filename, module.mod_name
            ));
        }
    }

    files.insert(
        "lib.rs".to_string(),
        generated_source(mod_decls.join("\n") + "\n"),
    );

    if let Some(main_module) = program.modules.get("__main__") {
        let mut main_parts = Vec::new();

        let dependency_mods: Vec<_> = dependency_modules
            .iter()
            .map(|module| module.mod_name.as_str())
            .collect();
        if !dependency_mods.is_empty() {
            let wrapper_mod = main_wrapper_module_name(&dependency_mods);
            main_parts.push(format!(
                "#[path = \"lib.rs\"]\nmod {wrapper_mod};\nuse {wrapper_mod}::*;",
            ));
        }

        let main_body = generate(main_module.file.clone())?;
        main_parts.push(main_body);

        files.insert(
            "main.rs".to_string(),
            generated_source(main_parts.join("\n\n") + "\n"),
        );
    }

    drop(timer);

    let output = GeneratedOutput { files };
    if let Ok(mut cache) = MULTI_CODEGEN_CACHE
        .get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
        .lock()
    {
        cache.insert(cache_key, output.clone());
    }
    Ok(output)
}

fn multi_codegen_cache_key(program: &crate::compiler::CompiledProgram) -> String {
    let mut hasher = Sha256::new();
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0multi\0");
    hasher.update([u8::from(program.has_main)]);
    for (key, module) in &program.modules {
        hasher.update(key.as_bytes());
        hasher.update(b"\0");
        hasher.update(module.mod_name.as_bytes());
        hasher.update(b"\0");
        hasher.update(module.import_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(module.filename.as_bytes());
        hasher.update(b"\0");
        hasher.update(module.content_hash.as_bytes());
        hasher.update(b"\0");
        hasher.update([u8::from(module.is_main), u8::from(module.is_stdlib)]);
        for item in &module.file.items {
            hasher.update(item.to_token_stream().to_string().as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(b"\x1f");
    }
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Generate a single Rust source file from a compiled program.
///
/// All modules (builtins, stdlib, local imports) are emitted as inline `mod`
/// blocks, followed by the main module items. Modules are ordered by import
/// discovery: main items first, then dependencies in the order they were
/// encountered during parsing.
pub fn generate_single(
    program: crate::compiler::CompiledProgram,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut parts: Vec<String> = Vec::new();

    // Main module items first
    if let Some(main_module) = program.modules.get("__main__") {
        let main_body = generate(main_module.file.clone())?;
        parts.push(main_body);
    }

    // Then all dependency modules as inline `mod` blocks, in BTreeMap order
    // (builtin first due to alphabetical ordering, then stdlib/local imports)
    for module in ordered_dependency_modules(&program) {
        let body = generate(module.file.clone())?;
        parts.push(format!(
            "mod {} {{\n{}}}",
            module.mod_name,
            indent_block(&body),
        ));
    }

    Ok(generated_source(parts.join("\n\n") + "\n"))
}

fn indent_block(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.lines().count() * 4);
    for line in source.lines() {
        if !line.is_empty() {
            out.push_str("    ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

pub const GORS_BUILTINS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../gors-builtin/src/lib.rs"
));

pub fn generate_with_sourcemap(
    file: syn::File,
    source_file: &str,
) -> Result<(String, sourcemap::SourceMap), CodegenError> {
    let output = generate(file).map_err(|e| CodegenError::generation(e.to_string()))?;
    let source_map = if crate::compiler::source_map_tracker_is_active() {
        crate::compiler::build_source_map(&output)
    } else {
        empty_sourcemap(source_file)
    };

    Ok((output, source_map))
}

fn empty_sourcemap(source_file: &str) -> sourcemap::SourceMap {
    let mut builder = sourcemap::SourceMapBuilder::new(None);
    builder.add_source(source_file);
    builder.into_sourcemap()
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    fn compiled_module(
        mod_name: &str,
        filename: &str,
        file: syn::File,
        is_main: bool,
    ) -> crate::compiler::CompiledModule {
        crate::compiler::CompiledModule {
            mod_name: mod_name.to_string(),
            import_path: mod_name.to_string(),
            file,
            filename: filename.to_string(),
            content_hash: String::new(),
            is_main,
            is_stdlib: false,
        }
    }

    #[test]
    fn generate_with_sourcemap_uses_active_compiler_tracker() {
        crate::compiler::clear_source_map_tracker();
        let go_source = "package main\n\nfunc main() {}\n";
        let parsed = crate::parser::parse_file("test.go", go_source).unwrap();
        let compiled =
            crate::compiler::compile_with_source_map(parsed, "test.go", go_source).unwrap();

        let (rust_source, source_map) =
            super::generate_with_sourcemap(compiled, "output.rs").unwrap();

        assert!(rust_source.contains("fn main"), "{rust_source}");
        assert!(source_map.get_token_count() > 0);
        assert_eq!(source_map.get_source(0), Some("test.go"));
        let names = (0..source_map.get_name_count())
            .filter_map(|idx| source_map.get_name(idx))
            .collect::<Vec<_>>();
        assert!(names.contains(&"func"), "{names:?}");
        assert!(names.contains(&"main"), "{names:?}");
        crate::compiler::clear_source_map_tracker();
    }

    #[test]
    fn generated_output_orders_macro_rules_before_invocations() {
        let file: syn::File = syn::parse_quote! {
            impl_real_complex_conversions!(i32);

            macro_rules! impl_real_complex_conversions {
                ($($ty:ty),*) => {};
            }
        };
        let source = super::generate(file).unwrap();
        let definition = source
            .find("macro_rules! impl_real_complex_conversions")
            .unwrap();
        let invocation = source.find("impl_real_complex_conversions!(i32);").unwrap();

        assert!(definition < invocation, "{source}");
    }

    #[test]
    fn generated_modules_and_methods_are_ordered() {
        let mut modules = BTreeMap::new();
        modules.insert(
            "__main__".to_string(),
            compiled_module(
                "main",
                "main.rs",
                syn::parse_quote! {
                    pub fn main() {}
                },
                true,
            ),
        );
        modules.insert(
            "z".to_string(),
            compiled_module(
                "zeta",
                "zeta.rs",
                syn::parse_quote! {
                    fn private_z() {}
                },
                false,
            ),
        );
        modules.insert(
            "a".to_string(),
            compiled_module(
                "alpha",
                "alpha.rs",
                syn::parse_quote! {
                    fn private_alpha() {}
                    pub fn PublicAlpha() {}
                },
                false,
            ),
        );
        let program = crate::compiler::CompiledProgram {
            modules,
            has_main: true,
        };

        let multi = super::generate_multi(program.clone()).unwrap();
        let lib_rs = multi.files.get("lib.rs").unwrap();
        assert!(lib_rs.find("pub mod alpha;").unwrap() < lib_rs.find("pub mod zeta;").unwrap());
        let alpha_rs = multi.files.get("alpha.rs").unwrap();
        assert!(
            alpha_rs.find("pub fn PublicAlpha").unwrap()
                < alpha_rs.find("fn private_alpha").unwrap()
        );

        let single = super::generate_single(program).unwrap();
        assert!(single.find("mod alpha").unwrap() < single.find("mod zeta").unwrap());
    }

    #[test]
    fn generated_multi_files_have_header_lints_and_blank_line() {
        let mut modules = BTreeMap::new();
        modules.insert(
            "__main__".to_string(),
            compiled_module(
                "main",
                "main.rs",
                syn::parse_quote! {
                    pub fn main() {}
                },
                true,
            ),
        );
        modules.insert(
            "dependency".to_string(),
            compiled_module(
                "dependency",
                "dependency.rs",
                syn::parse_quote! {
                    pub fn Used() {}
                },
                false,
            ),
        );
        let program = crate::compiler::CompiledProgram {
            modules,
            has_main: true,
        };
        let output = super::generate_multi(program).unwrap();
        let expected_prefix = format!("{}{}\n\n", super::GENERATED_HEADER, super::GENERATED_LINTS);

        for (filename, source) in &output.files {
            assert!(
                source.starts_with(&expected_prefix),
                "{filename} should start with the generated header and lint prelude"
            );
            assert!(
                !source.contains("allow(dead_code)"),
                "{filename} should keep dead-code denial enabled"
            );
        }
    }
}
