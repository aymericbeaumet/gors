//! Go import string-literal decoding.
//!
//! Import paths are source metadata. Decode them before classification or
//! filesystem resolution so escaped spellings cannot bypass the shared
//! canonical path validator.

use crate::import_path::{ImportPathIssue, validate_decoded_import_path};

/// Decode one Go string literal and validate its package import-path value.
pub fn decode_and_validate(literal: &str) -> Result<String, ImportPathIssue> {
    let bytes = decode_go_string_literal(literal)?;
    let path = String::from_utf8(bytes).map_err(|_| ImportPathIssue::InvalidUtf8)?;
    validate_decoded_import_path(&path)?;
    Ok(path)
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
