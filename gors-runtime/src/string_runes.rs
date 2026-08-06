//! UTF-8 decoding and rune conversion at the Go string boundary.

use super::{GoInt, GoSliceI64, GoString, go_string_from_bytes, index_out_of_range};

const REPLACEMENT_RUNE: GoInt = 0xfffd;

/// Convert a rune slice to its Go UTF-8 string representation.
#[must_use]
pub fn go_string_from_slice_runes(runes: GoSliceI64) -> GoString {
    let storage = runes
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = runes.start.saturating_add(runes.len);
    let mut bytes = Vec::with_capacity(runes.len);
    let visible = storage
        .get(runes.start..end)
        .unwrap_or_else(|| std::process::abort());
    for rune in visible {
        let scalar = u32::try_from(*rune)
            .ok()
            .and_then(char::from_u32)
            .unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut encoded = [0_u8; 4];
        bytes.extend_from_slice(scalar.encode_utf8(&mut encoded).as_bytes());
    }
    drop(storage);
    go_string_from_bytes(&bytes)
}

/// Count the UTF-8 decoding steps performed by a Go string range loop.
#[must_use]
pub fn go_string_range_count(value: GoString) -> GoInt {
    let mut byte_index = 0;
    let mut count = 0_i64;
    while byte_index < value.len {
        let (_, width) = decode_rune(value.as_bytes(), byte_index);
        byte_index = byte_index.saturating_add(width);
        count = count
            .checked_add(1)
            .unwrap_or_else(|| std::process::abort());
    }
    count
}

/// Return the byte index for one UTF-8 decoding step of a Go string range.
#[must_use]
pub fn go_string_range_index_at(value: GoString, ordinal: GoInt) -> GoInt {
    let (byte_index, _) = range_entry(value.as_bytes(), ordinal);
    GoInt::try_from(byte_index).unwrap_or_else(|_| std::process::abort())
}

/// Return the decoded rune for one step of a Go string range.
#[must_use]
pub fn go_string_range_rune_at(value: GoString, ordinal: GoInt) -> GoInt {
    range_entry(value.as_bytes(), ordinal).1
}

fn range_entry(bytes: &[u8], ordinal: GoInt) -> (usize, GoInt) {
    let Ok(ordinal) = usize::try_from(ordinal) else {
        index_out_of_range();
    };
    let mut byte_index = 0;
    for current in 0..=ordinal {
        if byte_index >= bytes.len() {
            index_out_of_range();
        }
        let (rune, width) = decode_rune(bytes, byte_index);
        if current == ordinal {
            return (byte_index, rune);
        }
        byte_index = byte_index.saturating_add(width);
    }
    index_out_of_range()
}

fn decode_rune(bytes: &[u8], byte_index: usize) -> (GoInt, usize) {
    let remaining = bytes
        .get(byte_index..)
        .unwrap_or_else(|| std::process::abort());
    match std::str::from_utf8(remaining) {
        Ok(valid) => valid.chars().next().map_or((REPLACEMENT_RUNE, 1), |value| {
            (GoInt::from(u32::from(value)), value.len_utf8())
        }),
        Err(error) if error.valid_up_to() == 0 => (REPLACEMENT_RUNE, 1),
        Err(error) => {
            let valid = remaining
                .get(..error.valid_up_to())
                .unwrap_or_else(|| std::process::abort());
            let value = std::str::from_utf8(valid)
                .ok()
                .and_then(|valid| valid.chars().next())
                .unwrap_or(char::REPLACEMENT_CHARACTER);
            (GoInt::from(u32::from(value)), value.len_utf8())
        }
    }
}
