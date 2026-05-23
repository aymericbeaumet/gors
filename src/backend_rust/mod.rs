//! Code generation backend.
//!
//! This module generates Rust source code from a `syn::File` AST.

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
/// use gors::{parser, compiler, backend_rust};
///
/// let go_source = "package main\n\nfunc main() {}";
/// let go_ast = parser::parse_file("example.go", go_source).unwrap();
/// let rust_ast = compiler::compile(go_ast).unwrap();
/// let rust_source = backend_rust::generate(rust_ast).unwrap();
/// ```
pub fn generate(mut file: syn::File) -> Result<String, Box<dyn std::error::Error>> {
    order_generated_file(&mut file);
    let mut output = Vec::new();
    fprint(&mut output, file)?;
    Ok(String::from_utf8(output)?)
}

const GENERATED_HEADER: &str = "//! Generated by gors. Do not edit.\n";
const GENERATED_LINTS: &str =
    "#![deny(dead_code, unused_imports, unused_macros, unsafe_code)]\n#![allow(nonstandard_style)]";

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

fn order_items(items: &mut Vec<syn::Item>) {
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

fn order_impl_items(items: &mut Vec<syn::ImplItem>) {
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

    let timer = ProfileTimer::start("backend.formatting");
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
            main_parts.push(format!(
                "#[path = \"lib.rs\"]\nmod lib;\nuse lib::{{{}}};",
                dependency_mods.join(", ")
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
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub const GORS_BUILTINS: &str = r#"use std::collections::HashMap;
use std::sync::{Arc, Mutex, Condvar, MutexGuard};
use std::fmt;

// ---------------------------------------------------------------------------
// len
// ---------------------------------------------------------------------------

pub trait GoLen {
    fn go_len(&self) -> usize;
}
impl<T> GoLen for Vec<T> {
    fn go_len(&self) -> usize { self.len() }
}
impl GoLen for String {
    fn go_len(&self) -> usize { self.len() }
}
impl GoLen for str {
    fn go_len(&self) -> usize { self.len() }
}
impl<T> GoLen for [T] {
    fn go_len(&self) -> usize { self.len() }
}
impl<K, V> GoLen for HashMap<K, V> {
    fn go_len(&self) -> usize { self.len() }
}
impl<T> GoLen for GoChan<T> {
    fn go_len(&self) -> usize { self.len() }
}
impl<T: GoLen + ?Sized> GoLen for &T {
    fn go_len(&self) -> usize { (**self).go_len() }
}

#[inline]
pub fn len<T: GoLen + ?Sized>(v: &T) -> usize {
    v.go_len()
}

// ---------------------------------------------------------------------------
// cap
// ---------------------------------------------------------------------------

pub trait GoCap {
    fn go_cap(&self) -> usize;
}
impl<T> GoCap for Vec<T> {
    fn go_cap(&self) -> usize { self.capacity() }
}
impl<T> GoCap for GoChan<T> {
    fn go_cap(&self) -> usize { self.cap() }
}
impl<T: GoCap + ?Sized> GoCap for &T {
    fn go_cap(&self) -> usize { (**self).go_cap() }
}
impl<T: GoCap + ?Sized> GoCap for &mut T {
    fn go_cap(&self) -> usize { (**self).go_cap() }
}

#[inline]
pub fn cap<T: GoCap + ?Sized>(v: &T) -> usize {
    v.go_cap()
}

// ---------------------------------------------------------------------------
// append
// ---------------------------------------------------------------------------

pub trait GoAppend<E> {
    fn go_append(self, elem: E) -> Self;
}

impl<T> GoAppend<T> for Vec<T> {
    fn go_append(mut self, elem: T) -> Self {
        self.push(elem);
        self
    }
}

impl<T> GoAppend<Vec<T>> for Vec<T> {
    fn go_append(mut self, elem: Vec<T>) -> Self {
        self.extend(elem);
        self
    }
}

impl GoAppend<String> for Vec<u8> {
    fn go_append(mut self, elem: String) -> Self {
        self.extend(elem.into_bytes());
        self
    }
}

#[inline]
pub fn append<C, E>(v: C, elem: E) -> C
where
    C: GoAppend<E>,
{
    v.go_append(elem)
}

#[inline]
pub fn append_slice<T: Clone>(mut v: Vec<T>, elems: &[T]) -> Vec<T> {
    v.extend_from_slice(elems);
    v
}

// ---------------------------------------------------------------------------
// go_string: Go's string() type conversion

pub trait GoString {
    fn go_string(self) -> String;
}
impl GoString for Vec<u8> {
    fn go_string(self) -> String { String::from_utf8(self).unwrap_or_default() }
}
impl GoString for &Vec<u8> {
    fn go_string(self) -> String { String::from_utf8(self.clone()).unwrap_or_default() }
}
impl GoString for String {
    fn go_string(self) -> String { self }
}
impl GoString for &String {
    fn go_string(self) -> String { self.clone() }
}
impl GoString for &str {
    fn go_string(self) -> String { self.to_string() }
}
impl GoString for &[u8] {
    fn go_string(self) -> String { String::from_utf8(self.to_vec()).unwrap_or_default() }
}

#[inline]
pub fn go_string<T: GoString>(v: T) -> String {
    v.go_string()
}

// ---------------------------------------------------------------------------
// copy
// ---------------------------------------------------------------------------

#[inline]
pub fn copy_slice<D, S, T>(dst: &mut D, src: &S) -> usize
where
    D: AsMut<[T]> + ?Sized,
    S: AsRef<[T]> + ?Sized,
    T: Clone,
{
    let dst = dst.as_mut();
    let src = src.as_ref();
    let n = dst.len().min(src.len());
    dst[..n].clone_from_slice(&src[..n]);
    n
}

// ---------------------------------------------------------------------------
// delete
// ---------------------------------------------------------------------------

#[inline]
pub fn delete<K: std::hash::Hash + Eq, V>(m: &mut HashMap<K, V>, key: &K) {
    m.remove(key);
}

// ---------------------------------------------------------------------------
// clear
// ---------------------------------------------------------------------------

pub trait GoClear {
    fn go_clear(&mut self);
}
impl<T: Default> GoClear for Vec<T> {
    fn go_clear(&mut self) {
        for elem in self.iter_mut() {
            *elem = T::default();
        }
    }
}
impl<K, V> GoClear for HashMap<K, V> {
    fn go_clear(&mut self) { self.clear(); }
}

#[inline]
pub fn clear<T: GoClear>(v: &mut T) {
    v.go_clear();
}

// ---------------------------------------------------------------------------
// new / make
// ---------------------------------------------------------------------------

#[inline]
pub fn new_box<T: Default>() -> Box<T> {
    Box::new(T::default())
}

#[inline]
pub fn make_vec<T: Default + Clone>(size: usize) -> Vec<T> {
    vec![T::default(); size]
}

#[inline]
pub fn make_vec_cap<T>(cap: usize) -> Vec<T> {
    Vec::with_capacity(cap)
}

#[inline]
pub fn make_map<K, V>() -> HashMap<K, V> {
    HashMap::new()
}

#[inline]
pub fn make_map_cap<K, V>(cap: usize) -> HashMap<K, V> {
    HashMap::with_capacity(cap)
}

// ---------------------------------------------------------------------------
// max / min
// ---------------------------------------------------------------------------

#[inline]
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a >= b { a } else { b }
}

#[inline]
pub fn max3<T: PartialOrd>(a: T, b: T, c: T) -> T {
    max(max(a, b), c)
}

#[inline]
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if a <= b { a } else { b }
}

#[inline]
pub fn min3<T: PartialOrd>(a: T, b: T, c: T) -> T {
    min(min(a, b), c)
}

// ---------------------------------------------------------------------------
// sort helpers
// ---------------------------------------------------------------------------

#[inline]
pub fn go_sort<T: Ord>(values: &mut [T]) {
    values.sort();
}

#[inline]
pub fn go_is_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

fn go_cmp_float64(left: f64, right: f64) -> std::cmp::Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

#[inline]
pub fn go_sort_float64s(values: &mut [f64]) {
    values.sort_by(|left, right| go_cmp_float64(*left, *right));
}

#[inline]
pub fn go_float64s_are_sorted(values: &[f64]) -> bool {
    values
        .windows(2)
        .all(|pair| go_cmp_float64(pair[0], pair[1]) != std::cmp::Ordering::Greater)
}

// ---------------------------------------------------------------------------
// fmt helpers
// ---------------------------------------------------------------------------

fn go_fmt_is_string(value: &dyn std::any::Any) -> bool {
    value.is::<String>() || value.is::<&str>()
}

fn go_fmt_slice_display<T: fmt::Display>(value: &[T]) -> String {
    let mut out = String::from("[");
    for (idx, item) in value.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&item.to_string());
    }
    out.push(']');
    out
}

fn go_fmt_default(value: &dyn std::any::Any) -> String {
    if let Some(value) = value.downcast_ref::<String>() {
        return value.clone();
    }
    if let Some(value) = value.downcast_ref::<&str>() {
        return (*value).to_string();
    }
    if let Some(value) = value.downcast_ref::<bool>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<isize>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<i8>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<i16>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<i32>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<i64>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<usize>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<u8>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<u16>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<u32>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<u64>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<f32>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<f64>() {
        return value.to_string();
    }
    if let Some(value) = value.downcast_ref::<Vec<isize>>() {
        return go_fmt_slice_display(value);
    }
    if let Some(value) = value.downcast_ref::<Vec<String>>() {
        return go_fmt_slice_display(value);
    }
    if let Some(value) = value.downcast_ref::<Vec<&str>>() {
        return go_fmt_slice_display(value);
    }
    if let Some(value) = value.downcast_ref::<Vec<f64>>() {
        return go_fmt_slice_display(value);
    }
    if let Some(value) = value.downcast_ref::<Vec<bool>>() {
        return go_fmt_slice_display(value);
    }
    String::new()
}

fn go_fmt_int(value: &dyn std::any::Any) -> Option<String> {
    if let Some(value) = value.downcast_ref::<isize>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<i8>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<i16>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<i32>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<i64>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<usize>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<u8>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<u16>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<u32>() {
        return Some(value.to_string());
    }
    if let Some(value) = value.downcast_ref::<u64>() {
        return Some(value.to_string());
    }
    None
}

fn go_fmt_float(value: &dyn std::any::Any, precision: Option<usize>) -> Option<String> {
    if let Some(value) = value.downcast_ref::<f32>() {
        return Some(match precision {
            Some(precision) => format!("{value:.precision$}"),
            None => value.to_string(),
        });
    }
    if let Some(value) = value.downcast_ref::<f64>() {
        return Some(match precision {
            Some(precision) => format!("{value:.precision$}"),
            None => value.to_string(),
        });
    }
    None
}

fn go_fmt_char(value: &dyn std::any::Any) -> Option<String> {
    let rune = if let Some(value) = value.downcast_ref::<isize>() {
        u32::try_from(*value).ok()
    } else if let Some(value) = value.downcast_ref::<i32>() {
        u32::try_from(*value).ok()
    } else if let Some(value) = value.downcast_ref::<i64>() {
        u32::try_from(*value).ok()
    } else if let Some(value) = value.downcast_ref::<usize>() {
        u32::try_from(*value).ok()
    } else if let Some(value) = value.downcast_ref::<u32>() {
        Some(*value)
    } else if let Some(value) = value.downcast_ref::<char>() {
        return Some(value.to_string());
    } else {
        None
    }?;
    char::from_u32(rune).map(|ch| ch.to_string())
}

fn go_fmt_format_arg(value: &dyn std::any::Any, verb: char, precision: Option<usize>) -> String {
    match verb {
        'c' => go_fmt_char(value).unwrap_or_else(|| go_fmt_default(value)),
        'd' => go_fmt_int(value).unwrap_or_else(|| go_fmt_default(value)),
        'f' => go_fmt_float(value, precision).unwrap_or_else(|| go_fmt_default(value)),
        'q' => format!("{:?}", go_fmt_default(value)),
        's' => go_fmt_default(value),
        't' => value
            .downcast_ref::<bool>()
            .map(bool::to_string)
            .unwrap_or_else(|| go_fmt_default(value)),
        'v' => go_fmt_default(value),
        _ => go_fmt_default(value),
    }
}

pub fn go_fmt_sprintf(format: &str, args: Vec<Box<dyn std::any::Any>>) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    let mut arg_idx = 0usize;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('%');
            break;
        }
        if chars[i] == '%' {
            out.push('%');
            i += 1;
            continue;
        }

        while i < chars.len() && matches!(chars[i], '#' | '+' | '-' | '0' | ' ') {
            i += 1;
        }
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let precision = if i < chars.len() && chars[i] == '.' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            chars[start..i]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .ok()
        } else {
            None
        };
        if i >= chars.len() {
            break;
        }

        let verb = chars[i];
        i += 1;
        if let Some(arg) = args.get(arg_idx) {
            out.push_str(&go_fmt_format_arg(&**arg, verb, precision));
        }
        arg_idx += 1;
    }

    out
}

pub fn go_fmt_sprint(args: Vec<Box<dyn std::any::Any>>) -> String {
    let mut out = String::new();
    let mut previous_was_string = false;
    for (idx, arg) in args.iter().enumerate() {
        let is_string = go_fmt_is_string(&**arg);
        if idx > 0 && !previous_was_string && !is_string {
            out.push(' ');
        }
        out.push_str(&go_fmt_default(&**arg));
        previous_was_string = is_string;
    }
    out
}

pub fn go_fmt_sprintln(args: Vec<Box<dyn std::any::Any>>) -> String {
    let mut out = String::new();
    for (idx, arg) in args.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&go_fmt_default(&**arg));
    }
    out.push('\n');
    out
}

pub fn go_fmt_print(args: Vec<Box<dyn std::any::Any>>) -> (isize, String) {
    let out = go_fmt_sprint(args);
    print!("{out}");
    (out.len() as isize, String::new())
}

pub fn go_fmt_println(args: Vec<Box<dyn std::any::Any>>) -> (isize, String) {
    let out = go_fmt_sprintln(args);
    print!("{out}");
    (out.len() as isize, String::new())
}

pub fn go_fmt_printf(format: &str, args: Vec<Box<dyn std::any::Any>>) -> (isize, String) {
    let out = go_fmt_sprintf(format, args);
    print!("{out}");
    (out.len() as isize, String::new())
}

pub fn go_fmt_append(mut buffer: Vec<u8>, args: Vec<Box<dyn std::any::Any>>) -> Vec<u8> {
    buffer.extend(go_fmt_sprint(args).into_bytes());
    buffer
}

pub fn go_fmt_appendln(mut buffer: Vec<u8>, args: Vec<Box<dyn std::any::Any>>) -> Vec<u8> {
    buffer.extend(go_fmt_sprintln(args).into_bytes());
    buffer
}

pub fn go_fmt_appendf(
    mut buffer: Vec<u8>,
    format: &str,
    args: Vec<Box<dyn std::any::Any>>,
) -> Vec<u8> {
    buffer.extend(go_fmt_sprintf(format, args).into_bytes());
    buffer
}

// ---------------------------------------------------------------------------
// complex / real / imag
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex64 {
    pub re: f32,
    pub im: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex128 {
    pub re: f64,
    pub im: f64,
}

impl fmt::Display for Complex64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}{:+}i)", self.re, self.im)
    }
}

impl fmt::Display for Complex128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}{:+}i)", self.re, self.im)
    }
}

impl std::ops::Add for Complex64 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self { re: self.re + rhs.re, im: self.im + rhs.im } }
}
impl std::ops::Sub for Complex64 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self { re: self.re - rhs.re, im: self.im - rhs.im } }
}
impl std::ops::Mul for Complex64 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}
impl std::ops::Div for Complex64 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        let denom = rhs.re * rhs.re + rhs.im * rhs.im;
        Self {
            re: (self.re * rhs.re + self.im * rhs.im) / denom,
            im: (self.im * rhs.re - self.re * rhs.im) / denom,
        }
    }
}
impl std::ops::Add for Complex128 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self { Self { re: self.re + rhs.re, im: self.im + rhs.im } }
}
impl std::ops::Sub for Complex128 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self { Self { re: self.re - rhs.re, im: self.im - rhs.im } }
}
impl std::ops::Mul for Complex128 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}
impl std::ops::Div for Complex128 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        let denom = rhs.re * rhs.re + rhs.im * rhs.im;
        Self {
            re: (self.re * rhs.re + self.im * rhs.im) / denom,
            im: (self.im * rhs.re - self.re * rhs.im) / denom,
        }
    }
}

#[inline]
pub fn complex64(re: f32, im: f32) -> Complex64 {
    Complex64 { re, im }
}

#[inline]
pub fn complex128(re: f64, im: f64) -> Complex128 {
    Complex128 { re, im }
}

pub trait GoComplex64 {
    fn go_complex64(self) -> Complex64;
}

pub trait GoComplex128 {
    fn go_complex128(self) -> Complex128;
}

impl GoComplex64 for Complex64 {
    fn go_complex64(self) -> Complex64 { self }
}

impl GoComplex64 for Complex128 {
    fn go_complex64(self) -> Complex64 {
        Complex64 { re: self.re as f32, im: self.im as f32 }
    }
}

impl GoComplex128 for Complex128 {
    fn go_complex128(self) -> Complex128 { self }
}

impl GoComplex128 for Complex64 {
    fn go_complex128(self) -> Complex128 {
        Complex128 { re: self.re as f64, im: self.im as f64 }
    }
}

macro_rules! impl_real_complex_conversions {
    ($($ty:ty),* $(,)?) => {
        $(
            impl GoComplex64 for $ty {
                fn go_complex64(self) -> Complex64 {
                    Complex64 { re: self as f32, im: 0.0 }
                }
            }

            impl GoComplex128 for $ty {
                fn go_complex128(self) -> Complex128 {
                    Complex128 { re: self as f64, im: 0.0 }
                }
            }
        )*
    };
}

impl_real_complex_conversions!(f32, f64, isize, i8, i16, i32, i64, usize, u8, u16, u32, u64);

#[inline]
pub fn to_complex64<T: GoComplex64>(v: T) -> Complex64 {
    v.go_complex64()
}

#[inline]
pub fn to_complex128<T: GoComplex128>(v: T) -> Complex128 {
    v.go_complex128()
}

#[inline]
pub fn real64(c: Complex64) -> f32 {
    c.re
}

#[inline]
pub fn real128(c: Complex128) -> f64 {
    c.re
}

#[inline]
pub fn imag64(c: Complex64) -> f32 {
    c.im
}

#[inline]
pub fn imag128(c: Complex128) -> f64 {
    c.im
}

// ---------------------------------------------------------------------------
// recover
// ---------------------------------------------------------------------------

#[inline]
pub fn recover_func<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> Option<String> {
    match std::panic::catch_unwind(f) {
        Ok(()) => None,
        Err(e) => {
            if let Some(s) = e.downcast_ref::<String>() {
                Some(s.clone())
            } else if let Some(s) = e.downcast_ref::<&str>() {
                Some(s.to_string())
            } else {
                Some("unknown panic".to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GoChan — Go-style channel using std::sync primitives
// ---------------------------------------------------------------------------

struct ChanInner<T> {
    buf: std::collections::VecDeque<T>,
    capacity: usize,
    closed: bool,
    senders_waiting: usize,
    receivers_waiting: usize,
}

pub struct GoChan<T> {
    inner: Arc<(Mutex<ChanInner<T>>, Condvar, Condvar)>,
}

fn lock_chan<T>(lock: &Mutex<ChanInner<T>>) -> MutexGuard<'_, ChanInner<T>> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_chan<'a, T>(cvar: &Condvar, guard: MutexGuard<'a, ChanInner<T>>) -> MutexGuard<'a, ChanInner<T>> {
    match cvar.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl<T> Clone for GoChan<T> {
    fn clone(&self) -> Self {
        GoChan { inner: Arc::clone(&self.inner) }
    }
}

impl<T> GoChan<T> {
    pub fn new(capacity: usize) -> Self {
        GoChan {
            inner: Arc::new((
                Mutex::new(ChanInner {
                    buf: std::collections::VecDeque::with_capacity(capacity),
                    capacity,
                    closed: false,
                    senders_waiting: 0,
                    receivers_waiting: 0,
                }),
                Condvar::new(), // notify receivers
                Condvar::new(), // notify senders
            )),
        }
    }

    pub fn send(&self, val: T) {
        let (lock, rx_cv, tx_cv) = &*self.inner;
        let mut inner = lock_chan(lock);
        if inner.closed {
            return;
        }
        while inner.buf.len() >= inner.capacity && inner.capacity > 0 {
            inner.senders_waiting += 1;
            inner = wait_chan(tx_cv, inner);
            inner.senders_waiting -= 1;
            if inner.closed {
                return;
            }
        }
        if inner.capacity == 0 {
            inner.buf.push_back(val);
            rx_cv.notify_one();
            inner.senders_waiting += 1;
            while !inner.buf.is_empty() && !inner.closed {
                inner = wait_chan(tx_cv, inner);
            }
            inner.senders_waiting -= 1;
        } else {
            inner.buf.push_back(val);
            rx_cv.notify_one();
        }
    }

    pub fn recv(&self) -> Option<T> {
        let (lock, rx_cv, tx_cv) = &*self.inner;
        let mut inner = lock_chan(lock);
        loop {
            if let Some(val) = inner.buf.pop_front() {
                tx_cv.notify_one();
                return Some(val);
            }
            if inner.closed {
                return None;
            }
            inner.receivers_waiting += 1;
            inner = wait_chan(rx_cv, inner);
            inner.receivers_waiting -= 1;
        }
    }

    pub fn recv_with_ok(&self) -> (T, bool) where T: Default {
        match self.recv() {
            Some(v) => (v, true),
            None => (T::default(), false),
        }
    }

    pub fn len(&self) -> usize {
        let (lock, _, _) = &*self.inner;
        lock_chan(lock).buf.len()
    }

    pub fn cap(&self) -> usize {
        let (lock, _, _) = &*self.inner;
        lock_chan(lock).capacity
    }
}

pub struct GoChanIter<T>(GoChan<T>);
impl<T: Default> Iterator for GoChanIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        let (val, ok) = self.0.recv_with_ok();
        if ok { Some(val) } else { None }
    }
}
impl<T: Default> IntoIterator for GoChan<T> {
    type Item = T;
    type IntoIter = GoChanIter<T>;
    fn into_iter(self) -> Self::IntoIter { GoChanIter(self) }
}

#[inline]
pub fn close<T>(c: &GoChan<T>) {
    let (lock, rx_cv, tx_cv) = &*c.inner;
    let mut inner = lock_chan(lock);
    if inner.closed {
        return;
    }
    inner.closed = true;
    rx_cv.notify_all();
    tx_cv.notify_all();
}

#[inline]
pub fn make_chan<T>(capacity: usize) -> GoChan<T> {
    GoChan::new(capacity)
}

#[inline]
pub fn go_print_empty() {
}

#[inline]
pub fn go_println_empty() {
    ::std::println!();
}

#[inline]
pub fn go_print_value<T: ::std::fmt::Display>(value: T) {
    ::std::print!("{}", value);
}

#[inline]
pub fn go_println_value<T: ::std::fmt::Display>(value: T) {
    ::std::println!("{}", value);
}

#[inline]
pub fn go_fmt_slice<T: ::std::fmt::Display>(value: &[T]) -> String {
    let mut out = String::from("[");
    for (idx, item) in value.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&item.to_string());
    }
    out.push(']');
    out
}

// ---------------------------------------------------------------------------
// print helpers (variadic via macros)
// ---------------------------------------------------------------------------

#[macro_export]
macro_rules! go_print {
    () => {};
    ($($arg:expr),+ $(,)?) => {{
        let mut _first = true;
        $(
            if !_first { eprint!(" "); }
            eprint!("{}", $arg);
            _first = false;
        )+
    }};
}

#[macro_export]
macro_rules! go_println {
    () => { eprintln!() };
    ($($arg:expr),+ $(,)?) => {{
        let mut _first = true;
        $(
            if !_first { eprint!(" "); }
            eprint!("{}", $arg);
            _first = false;
        )+
        eprintln!();
    }};
}
"#;

pub fn generate_with_sourcemap(
    file: syn::File,
    source_file: &str,
) -> Result<(String, SourceMap), CodegenError> {
    let output = generate(file).map_err(|e| CodegenError::generation(e.to_string()))?;

    // For now, create an empty source map
    // TODO: Integrate with the compiler's source map tracking
    let source_map = SourceMap::new(source_file);

    Ok((output, source_map))
}
