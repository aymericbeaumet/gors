//! Owned, path-independent source metadata projected from one file parse.

use std::sync::Arc;

use crate::compiler::provenance::FileRange;
use crate::import_path::{CanonicalImportPath, ImportPathIssue};

use super::super::fingerprint::Fingerprint;
use super::super::ids::FileId;
use super::model::FingerprintBuilder;

/// Binding syntax attached to one Go import occurrence.
///
/// The range anchors the token that selects the binding. For a default import,
/// there is no separate name token, so the import-path literal is the anchor.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ImportBinding {
    Default { source: FileRange },
    Named { name: Arc<str>, source: FileRange },
    Blank { source: FileRange },
    Dot { source: FileRange },
}

impl ImportBinding {
    /// Physical source token that selected this binding.
    #[must_use]
    pub const fn source(&self) -> FileRange {
        match self {
            Self::Default { source }
            | Self::Named { source, .. }
            | Self::Blank { source }
            | Self::Dot { source } => *source,
        }
    }

    /// Explicit local package name, excluding the blank and dot forms.
    #[must_use]
    pub fn explicit_name(&self) -> Option<&str> {
        match self {
            Self::Named { name, .. } => Some(name),
            Self::Default { .. } | Self::Blank { .. } | Self::Dot { .. } => None,
        }
    }
}

/// Shared physical and presentation facts for one import occurrence.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct ImportOccurrenceLocation {
    file: FileId,
    source: FileRange,
    byte_offset: usize,
    line: usize,
    column: usize,
    virtual_file: Option<Arc<str>>,
}

impl ImportOccurrenceLocation {
    pub(super) fn new(
        file: FileId,
        source: FileRange,
        byte_offset: usize,
        line: usize,
        column: usize,
        virtual_file: Option<Arc<str>>,
    ) -> Self {
        Self {
            file,
            source,
            byte_offset,
            line,
            column,
            virtual_file,
        }
    }
}

/// One canonical direct-import occurrence in an independently parsed file.
///
/// The occurrence owns the original literal and portable source coordinates.
/// It deliberately carries the stable logical [`FileId`] instead of a
/// checkout path or browser URI.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DirectImport {
    path: CanonicalImportPath,
    binding: ImportBinding,
    literal: Arc<str>,
    location: ImportOccurrenceLocation,
}

impl DirectImport {
    pub(super) fn new(
        path: CanonicalImportPath,
        binding: ImportBinding,
        literal: Arc<str>,
        location: ImportOccurrenceLocation,
    ) -> Self {
        Self {
            path,
            binding,
            literal,
            location,
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.location.file
    }

    /// Decoded and validated canonical Go import path.
    #[must_use]
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    /// Validated package identity carried by this occurrence.
    #[must_use]
    pub const fn canonical_path(&self) -> &CanonicalImportPath {
        &self.path
    }

    /// File-scoped Go binding selected by this import declaration.
    #[must_use]
    pub const fn binding(&self) -> &ImportBinding {
        &self.binding
    }

    /// Exact physical range of the import-path literal.
    #[must_use]
    pub const fn source(&self) -> FileRange {
        self.location.source
    }

    /// Original quoted Go string literal.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }

    /// Zero-based byte offset of the import literal in the logical file.
    #[must_use]
    pub const fn byte_offset(&self) -> usize {
        self.location.byte_offset
    }

    /// One-based logical source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.location.line
    }

    /// One-based logical source column.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.location.column
    }

    /// Explicit virtual filename installed by `//line`, when present.
    #[must_use]
    pub fn virtual_file(&self) -> Option<&str> {
        self.location.virtual_file.as_deref()
    }

    fn retained_bytes(&self) -> usize {
        self.path
            .as_str()
            .len()
            .saturating_add(self.literal.len())
            .saturating_add(
                self.location
                    .virtual_file
                    .as_ref()
                    .map_or(0, |file| file.len()),
            )
    }
}

/// One parsed import occurrence whose literal is not a valid Go import path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InvalidImport {
    literal: Arc<str>,
    issue: ImportPathIssue,
    location: ImportOccurrenceLocation,
}

impl InvalidImport {
    pub(super) fn new(
        literal: Arc<str>,
        issue: ImportPathIssue,
        location: ImportOccurrenceLocation,
    ) -> Self {
        Self {
            literal,
            issue,
            location,
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.location.file
    }

    /// Exact physical range of the invalid import-path literal.
    #[must_use]
    pub const fn source(&self) -> FileRange {
        self.location.source
    }

    /// Original quoted Go string literal.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }

    /// Zero-based byte offset of the import literal in the logical file.
    #[must_use]
    pub const fn byte_offset(&self) -> usize {
        self.location.byte_offset
    }

    /// One-based logical source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.location.line
    }

    /// One-based logical source column.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.location.column
    }

    /// Explicit virtual filename installed by `//line`, when present.
    #[must_use]
    pub fn virtual_file(&self) -> Option<&str> {
        self.location.virtual_file.as_deref()
    }

    /// Structured parser-owned validation failure.
    #[must_use]
    pub const fn issue(&self) -> &ImportPathIssue {
        &self.issue
    }

    fn retained_bytes(&self) -> usize {
        self.literal
            .len()
            .saturating_add(
                self.location
                    .virtual_file
                    .as_ref()
                    .map_or(0, |file| file.len()),
            )
            .saturating_add(import_issue_bytes(&self.issue))
    }
}

/// Immutable import projection from one file parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileImports {
    file: FileId,
    direct: Arc<[DirectImport]>,
    invalid: Arc<[InvalidImport]>,
    fingerprint: Fingerprint,
}

impl FileImports {
    pub(super) fn new(
        file: FileId,
        direct: Arc<[DirectImport]>,
        invalid: Arc<[InvalidImport]>,
    ) -> Self {
        let mut writer = FingerprintBuilder::new(b"file-imports");
        writer.bytes(file.canonical_bytes());
        for import in &*direct {
            writer.bytes(b"direct");
            write_direct_import(&mut writer, import);
        }
        for import in &*invalid {
            writer.bytes(b"invalid");
            write_invalid_import(&mut writer, import);
        }
        Self {
            file,
            direct,
            invalid,
            fingerprint: writer.finish(),
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Valid direct-import occurrences in source order.
    #[must_use]
    pub fn direct(&self) -> &[DirectImport] {
        &self.direct
    }

    /// Invalid direct-import occurrences in source order.
    #[must_use]
    pub fn invalid(&self) -> &[InvalidImport] {
        &self.invalid
    }

    /// Canonical fingerprint of every import occurrence and source anchor.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.direct
            .iter()
            .fold(32_usize, |total, import| {
                total.saturating_add(import.retained_bytes())
            })
            .saturating_add(self.invalid.iter().fold(0_usize, |total, import| {
                total.saturating_add(import.retained_bytes())
            }))
    }
}

/// One owned source comment with checkout-independent provenance.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceComment {
    file: FileId,
    text: Arc<str>,
    byte_start: usize,
    byte_end: usize,
    line: usize,
    column: usize,
    is_doc: bool,
}

impl SourceComment {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        file: FileId,
        text: Arc<str>,
        byte_start: usize,
        byte_end: usize,
        line: usize,
        column: usize,
        is_doc: bool,
    ) -> Self {
        Self {
            file,
            text,
            byte_start,
            byte_end,
            line,
            column,
            is_doc,
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Exact comment text, including its delimiters.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Zero-based starting byte offset in the logical source content.
    #[must_use]
    pub const fn byte_start(&self) -> usize {
        self.byte_start
    }

    /// Exclusive zero-based ending byte offset in the logical source content.
    #[must_use]
    pub const fn byte_end(&self) -> usize {
        self.byte_end
    }

    /// One-based logical source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// One-based logical source column.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// Whether the parser associated this comment with a function declaration.
    #[must_use]
    pub const fn is_doc(&self) -> bool {
        self.is_doc
    }
}

/// Immutable owned comment projection from one file parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileComments {
    file: FileId,
    comments: Arc<[SourceComment]>,
    fingerprint: Fingerprint,
}

impl FileComments {
    pub(super) fn new(file: FileId, comments: Arc<[SourceComment]>) -> Self {
        let mut writer = FingerprintBuilder::new(b"file-comments");
        writer.bytes(file.canonical_bytes());
        for comment in &*comments {
            writer.bytes(comment.text.as_bytes());
            writer.usize(comment.byte_start);
            writer.usize(comment.byte_end);
            writer.usize(comment.line);
            writer.usize(comment.column);
            writer.bytes(if comment.is_doc { b"doc" } else { b"ordinary" });
        }
        Self {
            file,
            comments,
            fingerprint: writer.finish(),
        }
    }

    /// Stable logical source-file identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Comments in source order, flattened from parser comment groups.
    #[must_use]
    pub fn comments(&self) -> &[SourceComment] {
        &self.comments
    }

    /// Canonical fingerprint of comment text and logical source anchors.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Approximate retained bytes for memory-budget accounting.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.comments.iter().fold(32_usize, |total, comment| {
            total.saturating_add(comment.text.len())
        })
    }
}

fn write_direct_import(writer: &mut FingerprintBuilder, import: &DirectImport) {
    writer.bytes(import.file().canonical_bytes());
    writer.bytes(import.path.as_str().as_bytes());
    write_import_binding(writer, &import.binding);
    write_file_range(writer, import.source());
    writer.bytes(import.literal.as_bytes());
    writer.usize(import.byte_offset());
    writer.usize(import.line());
    writer.usize(import.column());
    write_optional_text(writer, import.virtual_file());
}

fn write_invalid_import(writer: &mut FingerprintBuilder, import: &InvalidImport) {
    writer.bytes(import.file().canonical_bytes());
    write_file_range(writer, import.source());
    writer.bytes(import.literal.as_bytes());
    writer.usize(import.byte_offset());
    writer.usize(import.line());
    writer.usize(import.column());
    write_optional_text(writer, import.virtual_file());
    write_import_issue(writer, &import.issue);
}

fn write_import_binding(writer: &mut FingerprintBuilder, binding: &ImportBinding) {
    match binding {
        ImportBinding::Default { source } => {
            writer.bytes(b"default");
            write_file_range(writer, *source);
        }
        ImportBinding::Named { name, source } => {
            writer.bytes(b"named");
            writer.bytes(name.as_bytes());
            write_file_range(writer, *source);
        }
        ImportBinding::Blank { source } => {
            writer.bytes(b"blank");
            write_file_range(writer, *source);
        }
        ImportBinding::Dot { source } => {
            writer.bytes(b"dot");
            write_file_range(writer, *source);
        }
    }
}

fn write_file_range(writer: &mut FingerprintBuilder, source: FileRange) {
    writer.bytes(source.file().canonical_bytes());
    writer.usize(source.range().start().to_usize());
    writer.usize(source.range().end().to_usize());
}

fn write_optional_text(writer: &mut FingerprintBuilder, value: Option<&str>) {
    match value {
        Some(value) => {
            writer.bytes(b"some");
            writer.bytes(value.as_bytes());
        }
        None => writer.bytes(b"none"),
    }
}

pub(super) fn write_import_issue(writer: &mut FingerprintBuilder, issue: &ImportPathIssue) {
    match issue {
        ImportPathIssue::MalformedLiteral => writer.bytes(b"malformed-literal"),
        ImportPathIssue::InvalidEscape => writer.bytes(b"invalid-escape"),
        ImportPathIssue::OctalEscapeOutOfRange => writer.bytes(b"octal-escape-out-of-range"),
        ImportPathIssue::InvalidUnicodeEscape => writer.bytes(b"invalid-unicode-escape"),
        ImportPathIssue::InvalidUtf8 => writer.bytes(b"invalid-utf8"),
        ImportPathIssue::ContainsNul => writer.bytes(b"contains-nul"),
        ImportPathIssue::Empty => writer.bytes(b"empty"),
        ImportPathIssue::Absolute => writer.bytes(b"absolute"),
        ImportPathIssue::Backslash => writer.bytes(b"backslash"),
        ImportPathIssue::EmptyElement => writer.bytes(b"empty-element"),
        ImportPathIssue::DotElement(element) => {
            writer.bytes(b"dot-element");
            writer.bytes(element.as_bytes());
        }
        ImportPathIssue::LeadingDash => writer.bytes(b"leading-dash"),
        ImportPathIssue::TrailingDot(element) => {
            writer.bytes(b"trailing-dot");
            writer.bytes(element.as_bytes());
        }
        ImportPathIssue::ConsecutiveDots(element) => {
            writer.bytes(b"consecutive-dots");
            writer.bytes(element.as_bytes());
        }
        ImportPathIssue::InvalidCharacter(character) => {
            writer.bytes(b"invalid-character");
            writer.bytes(&u32::from(*character).to_be_bytes());
        }
        ImportPathIssue::ReservedWindowsName(element) => {
            writer.bytes(b"reserved-windows-name");
            writer.bytes(element.as_bytes());
        }
        ImportPathIssue::WindowsShortName(element) => {
            writer.bytes(b"windows-short-name");
            writer.bytes(element.as_bytes());
        }
    }
}

fn import_issue_bytes(issue: &ImportPathIssue) -> usize {
    match issue {
        ImportPathIssue::DotElement(element)
        | ImportPathIssue::TrailingDot(element)
        | ImportPathIssue::ConsecutiveDots(element)
        | ImportPathIssue::ReservedWindowsName(element)
        | ImportPathIssue::WindowsShortName(element) => element.len(),
        ImportPathIssue::MalformedLiteral
        | ImportPathIssue::InvalidEscape
        | ImportPathIssue::OctalEscapeOutOfRange
        | ImportPathIssue::InvalidUnicodeEscape
        | ImportPathIssue::InvalidUtf8
        | ImportPathIssue::ContainsNul
        | ImportPathIssue::Empty
        | ImportPathIssue::Absolute
        | ImportPathIssue::Backslash
        | ImportPathIssue::EmptyElement
        | ImportPathIssue::LeadingDash
        | ImportPathIssue::InvalidCharacter(_) => 0,
    }
}

#[cfg(test)]
#[path = "source_metadata/tests.rs"]
mod tests;
