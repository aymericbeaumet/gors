//! Immutable source bytes and presentation snapshots.

use std::fmt;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::compiler::source::{
    PhysicalLineColumn, PhysicalLineColumnOverflow, TextRange, TextSize, TextSizeOverflow,
};

/// Path-independent source bytes and indexing metadata for one Go file revision.
///
/// This type deliberately exposes no parsing operation. Parsing belongs to the
/// compiler database's file-projection query, which creates one ephemeral AST
/// through [`crate::parser::parse_file`] and publishes owned semantic products.
#[derive(Eq, PartialEq)]
pub struct SourceContent {
    source: Arc<str>,
    text_len: TextSize,
    line_starts: Arc<[TextSize]>,
    content_digest: [u8; 32],
}

impl SourceContent {
    /// Own exact UTF-8 source bytes after enforcing the compiler coordinate
    /// width.
    pub fn from_source(source: impl Into<Arc<str>>) -> Result<Self, TextSizeOverflow> {
        let source = source.into();
        let text_len = TextSize::try_from(source.len())?;
        let line_starts = std::iter::once(Ok(TextSize::ZERO))
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter(|(_, byte)| *byte == b'\n')
                    .map(|(offset, _)| TextSize::try_from(offset.saturating_add(1))),
            )
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::from_checked_parts(
            source,
            text_len,
            line_starts.into(),
        ))
    }

    pub(crate) fn empty() -> Self {
        Self::from_checked_parts(Arc::from(""), TextSize::ZERO, Arc::from([TextSize::ZERO]))
    }

    fn from_checked_parts(
        source: Arc<str>,
        text_len: TextSize,
        line_starts: Arc<[TextSize]>,
    ) -> Self {
        Self {
            content_digest: Sha256::digest(source.as_bytes()).into(),
            source,
            text_len,
            line_starts,
        }
    }

    /// Fixed-width byte length of this source revision.
    #[must_use]
    pub const fn text_len(&self) -> TextSize {
        self.text_len
    }

    /// Complete half-open byte range of this source revision.
    #[must_use]
    pub const fn text_range(&self) -> TextRange {
        TextRange::up_to(self.text_len)
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

    /// Physical one-based line and UTF-8 byte column for a checked byte offset.
    ///
    /// `Ok(None)` means that the offset is past EOF. A coordinate overflow is
    /// reported separately instead of wrapping at the pathological end of a
    /// maximally sized source file.
    pub fn physical_line_column(
        &self,
        byte_offset: TextSize,
    ) -> Result<Option<PhysicalLineColumn>, PhysicalLineColumnOverflow> {
        if byte_offset > self.text_len {
            return Ok(None);
        }
        let line = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte_offset);
        let Some(line_start) = self.line_starts.get(line.saturating_sub(1)).copied() else {
            return Ok(None);
        };
        PhysicalLineColumn::try_from_usize(
            line,
            byte_offset
                .to_usize()
                .saturating_sub(line_start.to_usize())
                .saturating_add(1),
        )
        .map(Some)
    }

    /// Approximate retained bytes for query-cache accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.source
            .len()
            .saturating_add(
                self.line_starts
                    .len()
                    .saturating_mul(std::mem::size_of::<TextSize>()),
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
    /// Own an input revision after validating its fixed-width byte domain.
    ///
    /// This does not scan or parse Go syntax. Syntax failure remains an output
    /// of the parse query, not a failure to create an immutable source input.
    pub fn from_source(
        path: impl Into<Arc<str>>,
        source: impl Into<Arc<str>>,
    ) -> Result<Self, TextSizeOverflow> {
        SourceContent::from_source(source)
            .map(Arc::new)
            .map(|content| Self::from_content(path, content))
    }

    /// Attach a diagnostic filename to already-owned source content.
    #[must_use]
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
