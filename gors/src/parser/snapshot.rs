//! Owned source inputs for high-level parser products.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::import_path::{ImportPathIssue, decode_and_validate};
use super::{Result, ast, parse_file};

/// Path-independent source bytes and indexing metadata for one Go file revision.
#[derive(Eq, PartialEq)]
pub struct SourceContent {
    source: Arc<str>,
    line_starts: Arc<[usize]>,
    content_digest: [u8; 32],
}

impl SourceContent {
    /// Own exact UTF-8 source bytes without parsing or validating them.
    pub fn from_source(source: impl Into<Arc<str>>) -> Self {
        let source = source.into();
        let mut line_starts = Vec::with_capacity(source.lines().count().saturating_add(1));
        line_starts.push(0);
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(offset, byte)| (byte == b'\n').then_some(offset + 1)),
        );
        Self {
            content_digest: Sha256::digest(source.as_bytes()).into(),
            source,
            line_starts: line_starts.into(),
        }
    }

    /// Complete immutable UTF-8 source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// SHA-256 of the exact source bytes.
    #[must_use]
    pub fn content_digest(&self) -> [u8; 32] {
        self.content_digest
    }

    /// Byte offset at which a one-based source line starts.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        line.checked_sub(1)
            .and_then(|index| self.line_starts.get(index).copied())
    }

    /// Number of source lines represented by the line index.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Approximate retained bytes for query-cache accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.source
            .len()
            .saturating_add(
                self.line_starts
                    .len()
                    .saturating_mul(std::mem::size_of::<usize>()),
            )
            .saturating_add(self.content_digest.len())
    }

    /// Parse this content under an explicit logical or diagnostic filename.
    ///
    /// The returned AST borrows both this content and `path`; callers must keep
    /// both alive for the complete AST inspection scope.
    pub fn parse<'content>(&'content self, path: &'content str) -> Result<ast::File<'content>> {
        parse_file(path, self.source())
    }
}

impl fmt::Debug for SourceContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceContent")
            .field("source_bytes", &self.source.len())
            .field("line_count", &self.line_starts.len())
            .field("content_digest", &self.content_digest)
            .finish()
    }
}

/// One diagnostic location paired with immutable source content.
///
/// A snapshot is reference counted by [`ParsedFile`]. It never borrows the
/// caller's input and can therefore be retained by a query result or released
/// independently when that file revision is evicted. Compiler semantic inputs
/// track [`SourceContent`] separately so moving a file does not invalidate it.
#[derive(Eq, PartialEq)]
pub struct SourceSnapshot {
    path: Arc<str>,
    content: Arc<SourceContent>,
}

impl SourceSnapshot {
    /// Own an input revision without parsing or validating it.
    ///
    /// Syntax failure is an output of the parse query, not a failure to create
    /// an immutable source input.
    pub fn from_source(path: impl Into<Arc<str>>, source: impl Into<Arc<str>>) -> Self {
        Self::from_content(path, Arc::new(SourceContent::from_source(source)))
    }

    /// Attach a diagnostic filename to already-owned source content.
    pub fn from_content(path: impl Into<Arc<str>>, content: Arc<SourceContent>) -> Self {
        Self {
            path: path.into(),
            content,
        }
    }

    /// User-visible physical path or URI for diagnostics and source maps.
    #[must_use]
    pub fn diagnostic_path(&self) -> &str {
        &self.path
    }

    /// Shared user-visible diagnostic path without copying its bytes.
    #[must_use]
    pub fn shared_diagnostic_path(&self) -> Arc<str> {
        Arc::clone(&self.path)
    }

    /// Shared path-independent source content.
    #[must_use]
    pub fn content(&self) -> Arc<SourceContent> {
        Arc::clone(&self.content)
    }

    /// Whether two snapshots contain exactly the same source bytes and index.
    #[must_use]
    pub fn has_same_content(&self, other: &Self) -> bool {
        self.content == other.content
    }

    /// Complete immutable UTF-8 source text.
    #[must_use]
    pub fn source(&self) -> &str {
        self.content.source()
    }

    /// SHA-256 of the exact source bytes.
    #[must_use]
    pub fn content_digest(&self) -> [u8; 32] {
        self.content.content_digest()
    }

    /// Byte offset at which a one-based source line starts.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.content.line_start(line)
    }

    /// Number of source lines represented by the line index.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.content.line_count()
    }

    /// Approximate retained bytes for query-cache accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.path
            .len()
            .saturating_add(self.content.retained_bytes())
    }

    /// Parse an ephemeral AST borrowing this snapshot.
    pub fn parse(&self) -> Result<ast::File<'_>> {
        self.content.parse(self.diagnostic_path())
    }
}

impl fmt::Debug for SourceSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceSnapshot")
            .field("path", &self.path)
            .field("content", &self.content)
            .finish()
    }
}

/// An owned high-level syntax failure tied to its exact source revision.
///
/// The low-level [`super::ParserError`] remains unchanged. This wrapper makes
/// path-based and query-facing parser errors self-contained without leaking or
/// re-reading source text.
#[derive(Clone, Debug)]
pub struct FileParseError {
    snapshot: Arc<SourceSnapshot>,
    error: super::ParserError,
}

impl FileParseError {
    fn new(snapshot: Arc<SourceSnapshot>, error: super::ParserError) -> Self {
        Self { snapshot, error }
    }

    /// Exact immutable input revision that failed to parse.
    #[must_use]
    pub fn snapshot(&self) -> Arc<SourceSnapshot> {
        Arc::clone(&self.snapshot)
    }

    /// User-facing path or URI of the failing source revision.
    #[must_use]
    pub fn path(&self) -> &str {
        self.snapshot.diagnostic_path()
    }

    /// Exact source text that produced this failure.
    #[must_use]
    pub fn source_text(&self) -> &str {
        self.snapshot.source()
    }

    /// Original low-level parser failure.
    #[must_use]
    pub const fn parser_error(&self) -> &super::ParserError {
        &self.error
    }

    /// Best available one-based line and column in this source revision.
    #[must_use]
    pub fn line_column(&self) -> Option<(usize, usize)> {
        if let Some((_, line, column)) = self.error.location() {
            return Some((line, column));
        }
        if matches!(self.error, super::ParserError::UnexpectedEndOfFile) {
            let line = self.snapshot.line_count();
            let start = self.snapshot.line_start(line)?;
            return Some((line, self.snapshot.source().len() - start + 1));
        }
        None
    }

    /// Human-readable parser message without duplicating source ownership.
    #[must_use]
    pub fn message(&self) -> String {
        self.error.message()
    }
}

impl Deref for FileParseError {
    type Target = super::ParserError;

    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl fmt::Display for FileParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((line, column)) = self.line_column() {
            write!(
                formatter,
                "{}:{line}:{column}: {}",
                self.path(),
                self.error.message()
            )
        } else {
            write!(formatter, "{}: {}", self.path(), self.error)
        }
    }
}

impl std::error::Error for FileParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// A decoded import literal that cannot be used as a canonical Go import path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidImportPathError {
    snapshot: Arc<SourceSnapshot>,
    literal: Arc<str>,
    line: usize,
    column: usize,
    issue: ImportPathIssue,
}

impl InvalidImportPathError {
    fn new(
        snapshot: Arc<SourceSnapshot>,
        literal: &str,
        line: usize,
        column: usize,
        issue: ImportPathIssue,
    ) -> Self {
        Self {
            snapshot,
            literal: Arc::from(literal),
            line,
            column,
            issue,
        }
    }

    /// Exact immutable input revision containing the invalid import.
    #[must_use]
    pub fn snapshot(&self) -> Arc<SourceSnapshot> {
        Arc::clone(&self.snapshot)
    }

    /// User-facing path or URI of the source file containing the invalid import.
    #[must_use]
    pub fn path(&self) -> &str {
        self.snapshot.diagnostic_path()
    }

    /// Original quoted Go literal.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }

    /// One-based location of the import literal.
    #[must_use]
    pub const fn line_column(&self) -> (usize, usize) {
        (self.line, self.column)
    }

    /// Structured validation failure.
    #[must_use]
    pub const fn issue(&self) -> &ImportPathIssue {
        &self.issue
    }
}

impl fmt::Display for InvalidImportPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}: invalid import path literal {}: {}",
            self.path(),
            self.line,
            self.column,
            self.literal,
            self.issue
        )
    }
}

impl std::error::Error for InvalidImportPathError {}

/// Failure while validating one independently owned parsed file.
#[derive(Clone, Debug)]
pub enum ParsedFileError {
    Parser(FileParseError),
    InvalidImportPath(InvalidImportPathError),
}

impl fmt::Display for ParsedFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parser(error) => error.fmt(formatter),
            Self::InvalidImportPath(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParsedFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parser(error) => Some(error),
            Self::InvalidImportPath(error) => Some(error),
        }
    }
}

/// Parser-validated, independently owned Go source file.
///
/// The AST continues to use efficient borrowed token text. Calling [`parse`](Self::parse)
/// creates that ephemeral AST with a lifetime tied to this file's immutable
/// snapshot; no self-reference, leaked allocation, or `'static` fiction is
/// required.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedFile {
    snapshot: Arc<SourceSnapshot>,
    package_name: Arc<str>,
    imports: Arc<[String]>,
}

impl ParsedFile {
    /// Own and validate one source revision.
    pub fn from_source(
        path: impl Into<Arc<str>>,
        source: impl Into<Arc<str>>,
    ) -> std::result::Result<Self, ParsedFileError> {
        Self::from_snapshot(Arc::new(SourceSnapshot::from_source(path, source)))
    }

    /// Validate and retain an existing immutable source input.
    pub fn from_snapshot(
        snapshot: Arc<SourceSnapshot>,
    ) -> std::result::Result<Self, ParsedFileError> {
        let (package_name, imports) = {
            let ast = snapshot.parse().map_err(|error| {
                ParsedFileError::Parser(FileParseError::new(Arc::clone(&snapshot), error))
            })?;
            let package_name: Arc<str> = Arc::from(ast.name.name);
            let imports: Arc<[String]> = ast
                .imports()
                .into_iter()
                .map(|spec| {
                    decode_and_validate(spec.path.value).map_err(|issue| {
                        ParsedFileError::InvalidImportPath(InvalidImportPathError::new(
                            Arc::clone(&snapshot),
                            spec.path.value,
                            spec.path.value_pos.line,
                            spec.path.value_pos.column,
                            issue,
                        ))
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?
                .into();
            (package_name, imports)
        };
        Ok(Self {
            snapshot,
            package_name,
            imports,
        })
    }

    /// Parse an ephemeral AST borrowing this immutable source snapshot.
    pub fn parse(&self) -> Result<ast::File<'_>> {
        self.snapshot.parse()
    }

    /// User-facing filename or URI supplied to the parser.
    #[must_use]
    pub fn path(&self) -> &str {
        self.snapshot.diagnostic_path()
    }

    /// Exact source text retained by this file revision.
    #[must_use]
    pub fn source(&self) -> &str {
        self.snapshot.source()
    }

    /// Package clause extracted when this file was validated.
    #[must_use]
    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    /// Go import paths extracted from this file in source order.
    #[must_use]
    pub fn imports(&self) -> &[String] {
        &self.imports
    }

    /// Shared immutable source snapshot retained by parsed-file consumers.
    #[must_use]
    pub fn snapshot(&self) -> Arc<SourceSnapshot> {
        Arc::clone(&self.snapshot)
    }
}
