//! Canonical Go import-path identities shared by frontend and workspace code.
//!
//! This module validates already-decoded paths. Go string-literal decoding
//! remains a parser concern, but every producer uses this validator and every
//! durable package identity is a [`CanonicalImportPath`].

use std::fmt;
use std::sync::Arc;

/// Why text cannot be used as a canonical Go import path.
///
/// The literal-decoding variants are retained here because parser diagnostics
/// expose one stable error vocabulary for both decoding and path validation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ImportPathIssue {
    MalformedLiteral,
    InvalidEscape,
    OctalEscapeOutOfRange,
    InvalidUnicodeEscape,
    InvalidUtf8,
    ContainsNul,
    Empty,
    Absolute,
    Backslash,
    EmptyElement,
    DotElement(String),
    LeadingDash,
    TrailingDot(String),
    ConsecutiveDots(String),
    InvalidCharacter(char),
    ReservedWindowsName(String),
    WindowsShortName(String),
}

impl fmt::Display for ImportPathIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLiteral => formatter.write_str("malformed Go string literal"),
            Self::InvalidEscape => formatter.write_str("invalid Go string escape"),
            Self::OctalEscapeOutOfRange => formatter.write_str("octal escape exceeds one byte"),
            Self::InvalidUnicodeEscape => {
                formatter.write_str("escape is not a valid Unicode scalar value")
            }
            Self::InvalidUtf8 => formatter.write_str("decoded import path is not valid UTF-8"),
            Self::ContainsNul => formatter.write_str("decoded import path contains a NUL byte"),
            Self::Empty => formatter.write_str("import path is empty"),
            Self::Absolute => formatter.write_str("absolute import paths are not allowed"),
            Self::Backslash => formatter.write_str("backslashes are not allowed in import paths"),
            Self::EmptyElement => formatter.write_str("import path contains an empty element"),
            Self::DotElement(element) => {
                write!(formatter, "invalid dot path element {element:?}")
            }
            Self::LeadingDash => formatter.write_str("import path may not begin with '-'"),
            Self::TrailingDot(element) => {
                write!(formatter, "import path element {element:?} ends with '.'")
            }
            Self::ConsecutiveDots(element) => write!(
                formatter,
                "import path element {element:?} contains consecutive dots"
            ),
            Self::InvalidCharacter(character) => {
                write!(formatter, "invalid import path character {character:?}")
            }
            Self::ReservedWindowsName(element) => write!(
                formatter,
                "import path element {element:?} is a reserved Windows name"
            ),
            Self::WindowsShortName(element) => write!(
                formatter,
                "import path element {element:?} resembles a Windows short name"
            ),
        }
    }
}

impl std::error::Error for ImportPathIssue {}

/// Validated, slash-normalized package identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalImportPath(Arc<str>);

impl CanonicalImportPath {
    /// Validate decoded import-path text and retain it immutably.
    pub fn new(path: impl Into<Arc<str>>) -> Result<Self, ImportPathIssue> {
        let path = path.into();
        validate_decoded_import_path(&path)?;
        Ok(Self(path))
    }

    /// Exact canonical spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Share the exact canonical spelling without copying it.
    #[must_use]
    pub fn shared(&self) -> Arc<str> {
        Arc::clone(&self.0)
    }
}

impl AsRef<str> for CanonicalImportPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for CanonicalImportPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for CanonicalImportPath {
    type Err = ImportPathIssue;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        Self::new(path)
    }
}

pub(crate) fn validate_decoded_import_path(path: &str) -> Result<(), ImportPathIssue> {
    if path.as_bytes().contains(&0) {
        return Err(ImportPathIssue::ContainsNul);
    }
    if path.is_empty() {
        return Err(ImportPathIssue::Empty);
    }
    if path.starts_with('/') || is_windows_absolute(path) {
        return Err(ImportPathIssue::Absolute);
    }
    if path.contains('\u{5c}') {
        return Err(ImportPathIssue::Backslash);
    }
    if path.starts_with('-') {
        return Err(ImportPathIssue::LeadingDash);
    }

    for element in path.split('/') {
        validate_element(element)?;
    }
    Ok(())
}

fn validate_element(element: &str) -> Result<(), ImportPathIssue> {
    if element.is_empty() {
        return Err(ImportPathIssue::EmptyElement);
    }
    if element.bytes().all(|byte| byte == b'.') {
        return Err(ImportPathIssue::DotElement(element.to_string()));
    }
    if element.ends_with('.') {
        return Err(ImportPathIssue::TrailingDot(element.to_string()));
    }
    if element.contains("..") {
        return Err(ImportPathIssue::ConsecutiveDots(element.to_string()));
    }
    for character in element.chars() {
        let allowed =
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_' | '~' | '+');
        if !allowed {
            return Err(ImportPathIssue::InvalidCharacter(character));
        }
    }

    let short = element.split('.').next().unwrap_or(element);
    if is_reserved_windows_name(short) {
        return Err(ImportPathIssue::ReservedWindowsName(short.to_string()));
    }
    if resembles_windows_short_name(short) {
        return Err(ImportPathIssue::WindowsShortName(short.to_string()));
    }
    Ok(())
}

fn is_windows_absolute(path: &str) -> bool {
    let [drive, b':', separator, ..] = path.as_bytes() else {
        return false;
    };
    drive.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
}

fn is_reserved_windows_name(element: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    RESERVED
        .iter()
        .any(|reserved| element.eq_ignore_ascii_case(reserved))
}

fn resembles_windows_short_name(element: &str) -> bool {
    let Some((_, suffix)) = element.rsplit_once('~') else {
        return false;
    };
    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "import_path/tests.rs"]
mod tests;
