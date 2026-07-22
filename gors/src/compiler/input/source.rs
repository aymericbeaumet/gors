//! Immutable source bytes and presentation snapshots.

use std::fmt;
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// Path-independent source bytes and indexing metadata for one Go file revision.
///
/// This type deliberately exposes no parsing operation. Parsing belongs to the
/// compiler database's file-projection query, which creates one ephemeral AST
/// through [`crate::parser::parse_file`] and publishes owned semantic products.
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

    /// Physical one-based line and byte column for a source byte offset.
    ///
    /// This deliberately ignores virtual `//line` coordinates. Consumers
    /// that place source text, such as browser comment reinsertion, need the
    /// exact physical content position instead.
    #[must_use]
    pub fn line_column(&self, byte_offset: usize) -> Option<(usize, usize)> {
        if byte_offset > self.source.len() {
            return None;
        }
        let line = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte_offset);
        let line_start = self.line_starts.get(line.saturating_sub(1)).copied()?;
        Some((
            line,
            byte_offset.saturating_sub(line_start).saturating_add(1),
        ))
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

/// One presentation path paired with immutable, path-independent source content.
///
/// The snapshot never borrows caller input and can be retained or released
/// independently per file revision. Compiler semantic inputs track
/// [`SourceContent`] separately so moving a file does not invalidate queries.
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
