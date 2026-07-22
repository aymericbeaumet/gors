//! Go string-literal decoding and canonical import-path validation.
//!
//! Import paths are source metadata. Decode them before classification or
//! filesystem resolution so escaped spellings cannot bypass path checks.

use std::fmt;

/// Why a parsed Go import literal cannot name a package.
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Decode one Go string literal and validate its package import-path value.
pub(super) fn decode_and_validate(literal: &str) -> Result<String, ImportPathIssue> {
    let bytes = decode_go_string_literal(literal)?;
    let path = String::from_utf8(bytes).map_err(|_| ImportPathIssue::InvalidUtf8)?;
    validate(&path)?;
    Ok(path)
}

/// Validate an already decoded Go package import path.
pub(super) fn validate(path: &str) -> Result<(), ImportPathIssue> {
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

fn decode_go_string_literal(literal: &str) -> Result<Vec<u8>, ImportPathIssue> {
    let bytes = literal.as_bytes();
    let Some((&delimiter, trailing)) = bytes.split_first() else {
        return Err(ImportPathIssue::MalformedLiteral);
    };
    let Some((&closing_delimiter, contents)) = trailing.split_last() else {
        return Err(ImportPathIssue::MalformedLiteral);
    };
    if delimiter != closing_delimiter {
        return Err(ImportPathIssue::MalformedLiteral);
    }

    match delimiter {
        b'`' => Ok(contents
            .iter()
            .copied()
            .filter(|byte| *byte != b'\r')
            .collect()),
        b'"' => decode_interpreted_string(bytes),
        _ => Err(ImportPathIssue::MalformedLiteral),
    }
}

fn decode_interpreted_string(literal: &[u8]) -> Result<Vec<u8>, ImportPathIssue> {
    let end = literal.len() - 1;
    let mut output = Vec::with_capacity(end.saturating_sub(1));
    let mut cursor = 1;
    while cursor < end {
        let byte = *literal
            .get(cursor)
            .ok_or(ImportPathIssue::MalformedLiteral)?;
        if byte == b'"' || byte == b'\n' || byte == b'\r' {
            return Err(ImportPathIssue::MalformedLiteral);
        }
        if byte != b'\\' {
            output.push(byte);
            cursor += 1;
            continue;
        }

        cursor += 1;
        let escape = *literal.get(cursor).ok_or(ImportPathIssue::InvalidEscape)?;
        cursor += 1;
        match escape {
            b'a' => output.push(0x07),
            b'b' => output.push(0x08),
            b'f' => output.push(0x0c),
            b'n' => output.push(b'\n'),
            b'r' => output.push(b'\r'),
            b't' => output.push(b'\t'),
            b'v' => output.push(0x0b),
            b'\\' => output.push(b'\\'),
            b'"' => output.push(b'"'),
            b'x' => {
                output.push(parse_digits(literal, &mut cursor, 2, 16)? as u8);
            }
            b'u' => append_unicode_escape(literal, &mut cursor, 4, &mut output)?,
            b'U' => append_unicode_escape(literal, &mut cursor, 8, &mut output)?,
            b'0'..=b'7' => {
                let mut value = u32::from(escape - b'0');
                for _ in 0..2 {
                    let digit = *literal.get(cursor).ok_or(ImportPathIssue::InvalidEscape)?;
                    if !(b'0'..=b'7').contains(&digit) {
                        return Err(ImportPathIssue::InvalidEscape);
                    }
                    value = value * 8 + u32::from(digit - b'0');
                    cursor += 1;
                }
                let value =
                    u8::try_from(value).map_err(|_| ImportPathIssue::OctalEscapeOutOfRange)?;
                output.push(value);
            }
            _ => return Err(ImportPathIssue::InvalidEscape),
        }
    }
    Ok(output)
}

fn append_unicode_escape(
    literal: &[u8],
    cursor: &mut usize,
    digits: usize,
    output: &mut Vec<u8>,
) -> Result<(), ImportPathIssue> {
    let value = parse_digits(literal, cursor, digits, 16)?;
    let character = char::from_u32(value).ok_or(ImportPathIssue::InvalidUnicodeEscape)?;
    let mut encoded = [0; 4];
    output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
    Ok(())
}

fn parse_digits(
    literal: &[u8],
    cursor: &mut usize,
    count: usize,
    radix: u32,
) -> Result<u32, ImportPathIssue> {
    let mut value = 0_u32;
    for _ in 0..count {
        let byte = *literal.get(*cursor).ok_or(ImportPathIssue::InvalidEscape)?;
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a') + 10,
            b'A'..=b'F' => u32::from(byte - b'A') + 10,
            _ => return Err(ImportPathIssue::InvalidEscape),
        };
        if digit >= radix {
            return Err(ImportPathIssue::InvalidEscape);
        }
        value = value * radix + digit;
        *cursor += 1;
    }
    Ok(value)
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
