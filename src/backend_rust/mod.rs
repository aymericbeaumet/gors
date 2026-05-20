//! Code generation backend.
//!
//! This module generates Rust source code from a `syn::File` AST.

mod tracked;

pub use tracked::{
    BlankLineInfo, CommentToInsert, generate_with_comments, generate_with_comments_and_blanks,
};

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
pub fn generate(file: syn::File) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    fprint(&mut output, file)?;
    Ok(String::from_utf8(output)?)
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
pub struct GeneratedOutput {
    pub files: std::collections::BTreeMap<String, String>,
}

pub fn generate_multi(
    program: crate::compiler::CompiledProgram,
) -> Result<GeneratedOutput, Box<dyn std::error::Error>> {
    let mut files = std::collections::BTreeMap::new();
    let mut mod_decls = Vec::new();

    for module in program.modules.values() {
        if module.is_main {
            continue;
        }

        let source = generate(module.file.clone())?;
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

    files.insert("lib.rs".to_string(), mod_decls.join("\n") + "\n");

    if let Some(main_module) = program.modules.get("__main__") {
        let mut main_parts = Vec::new();

        if program.has_main {
            main_parts.push("#[path = \"lib.rs\"]\nmod lib;\nuse lib::*;".to_string());
        }

        let main_body = generate(main_module.file.clone())?;
        main_parts.push(main_body);

        files.insert("main.rs".to_string(), main_parts.join("\n\n") + "\n");
    }

    Ok(GeneratedOutput { files })
}

pub const GORS_BUILTINS: &str = r#"#![allow(dead_code, non_snake_case, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Condvar};
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

#[inline]
pub fn cap<T: GoCap + ?Sized>(v: &T) -> usize {
    v.go_cap()
}

// ---------------------------------------------------------------------------
// append
// ---------------------------------------------------------------------------

#[inline]
pub fn append<T>(mut v: Vec<T>, elem: T) -> Vec<T> {
    v.push(elem);
    v
}

#[inline]
pub fn append_slice<T: Clone>(mut v: Vec<T>, elems: &[T]) -> Vec<T> {
    v.extend_from_slice(elems);
    v
}

// ---------------------------------------------------------------------------
// copy
// ---------------------------------------------------------------------------

#[inline]
pub fn copy_slice<T: Clone>(dst: &mut [T], src: &[T]) -> usize {
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
        let mut inner = lock.lock().unwrap();
        if inner.closed {
            panic!("send on closed channel");
        }
        while inner.buf.len() >= inner.capacity && inner.capacity > 0 {
            inner.senders_waiting += 1;
            inner = tx_cv.wait(inner).unwrap();
            inner.senders_waiting -= 1;
            if inner.closed {
                panic!("send on closed channel");
            }
        }
        if inner.capacity == 0 {
            inner.buf.push_back(val);
            rx_cv.notify_one();
            inner.senders_waiting += 1;
            while !inner.buf.is_empty() && !inner.closed {
                inner = tx_cv.wait(inner).unwrap();
            }
            inner.senders_waiting -= 1;
        } else {
            inner.buf.push_back(val);
            rx_cv.notify_one();
        }
    }

    pub fn recv(&self) -> Option<T> {
        let (lock, rx_cv, tx_cv) = &*self.inner;
        let mut inner = lock.lock().unwrap();
        loop {
            if let Some(val) = inner.buf.pop_front() {
                tx_cv.notify_one();
                return Some(val);
            }
            if inner.closed {
                return None;
            }
            inner.receivers_waiting += 1;
            inner = rx_cv.wait(inner).unwrap();
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
        lock.lock().unwrap().buf.len()
    }

    pub fn cap(&self) -> usize {
        let (lock, _, _) = &*self.inner;
        lock.lock().unwrap().capacity
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
    let mut inner = lock.lock().unwrap();
    if inner.closed {
        panic!("close of closed channel");
    }
    inner.closed = true;
    rx_cv.notify_all();
    tx_cv.notify_all();
}

#[inline]
pub fn make_chan<T>(capacity: usize) -> GoChan<T> {
    GoChan::new(capacity)
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
