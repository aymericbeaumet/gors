//! Minimal runtime ABI for code emitted by gors.
//!
//! This crate owns Go value representations and language-level operations that
//! cannot be expressed faithfully with primitive Rust types. It deliberately
//! contains no Go standard-library replacements. The language built-ins
//! `print` and `println` write to standard error, matching the pinned Go oracle.

// Generated programs embed the complete runtime ABI even when one program uses
// only a subset of it.
#![allow(dead_code)]

use std::io::Write as _;

/// Version of the compiler/runtime ABI implemented by this crate.
///
/// Generated artifacts and incremental cache keys must include this value once
/// external runtime linking is introduced.
pub const GORS_RUNTIME_ABI_VERSION: u32 = 2;

/// The fixed-width representation of Go `int` for the bootstrap target.
///
/// The initial backend deliberately targets the 64-bit Go data model on every
/// Rust host, including wasm32, instead of inheriting Rust's pointer width.
pub type GoInt = i64;

/// An immutable Go string containing arbitrary bytes.
///
/// The byte vector preserves invalid UTF-8, embedded NUL bytes, and exact
/// bytewise comparison semantics without an encoding layer.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct GoString(Vec<u8>);

impl GoString {
    /// Return the exact bytes stored in this Go string.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Construct a Go string without interpreting its bytes as UTF-8.
#[must_use]
pub fn go_string_from_bytes(bytes: &[u8]) -> GoString {
    GoString(bytes.to_vec())
}

/// Concatenate two Go strings while preserving every byte.
#[must_use]
pub fn concat_go_strings(mut left: GoString, right: GoString) -> GoString {
    if left.0.is_empty() {
        return right;
    }
    if right.0.is_empty() {
        return left;
    }

    left.0.extend_from_slice(&right.0);
    left
}

/// Go `int` addition wraps modulo 2^64.
#[must_use]
pub fn int_add(left: GoInt, right: GoInt) -> GoInt {
    left.wrapping_add(right)
}

/// Go `int` subtraction wraps modulo 2^64.
#[must_use]
pub fn int_sub(left: GoInt, right: GoInt) -> GoInt {
    left.wrapping_sub(right)
}

/// Go `int` multiplication wraps modulo 2^64.
#[must_use]
pub fn int_mul(left: GoInt, right: GoInt) -> GoInt {
    left.wrapping_mul(right)
}

/// Go `int` negation wraps, including `-MIN == MIN`.
#[must_use]
pub fn int_neg(value: GoInt) -> GoInt {
    value.wrapping_neg()
}

/// Go signed division, including the specified `MIN / -1 == MIN` case.
///
/// Division by zero is a language-level runtime panic.
#[must_use]
pub fn int_div(left: GoInt, right: GoInt) -> GoInt {
    if right == 0 {
        integer_divide_by_zero();
    }
    if left == GoInt::MIN && right == -1 {
        GoInt::MIN
    } else {
        left / right
    }
}

/// Go signed remainder, including the specified `MIN % -1 == 0` case.
///
/// Division by zero is a language-level runtime panic.
#[must_use]
pub fn int_rem(left: GoInt, right: GoInt) -> GoInt {
    if right == 0 {
        integer_divide_by_zero();
    }
    if left == GoInt::MIN && right == -1 {
        0
    } else {
        left % right
    }
}

/// Go left shift for a signed 64-bit `int` value.
///
/// Shift counts are not masked. Counts of 64 or more discard every bit and a
/// negative dynamic count panics.
#[must_use]
pub fn int_shl(value: GoInt, shift: GoInt) -> GoInt {
    if shift < 0 {
        negative_shift_amount();
    }
    if shift >= GoInt::from(GoInt::BITS) {
        return 0;
    }
    let Ok(shift) = u32::try_from(shift) else {
        return 0;
    };
    value.wrapping_shl(shift)
}

/// Go arithmetic right shift for a signed 64-bit `int` value.
///
/// Counts of 64 or more retain only the sign extension and a negative dynamic
/// count panics.
#[must_use]
pub fn int_shr(value: GoInt, shift: GoInt) -> GoInt {
    if shift < 0 {
        negative_shift_amount();
    }
    if shift >= GoInt::from(GoInt::BITS) {
        return if value < 0 { -1 } else { 0 };
    }
    let Ok(shift) = u32::try_from(shift) else {
        return if value < 0 { -1 } else { 0 };
    };
    value >> shift
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn integer_divide_by_zero() -> ! {
    std::panic::panic_any("runtime error: integer divide by zero")
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn negative_shift_amount() -> ! {
    std::panic::panic_any("runtime error: negative shift amount")
}

/// Emit no bytes for `print()` with no arguments.
pub fn print_empty() {}

/// Print an exact Go boolean representation.
pub fn print_bool(value: bool) {
    write_stderr_bytes(if value { b"true" } else { b"false" });
}

/// Print an exact 64-bit bootstrap Go `int` representation.
pub fn print_i64(value: GoInt) {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    drop(write!(output, "{value}"));
}

/// Emit the separator used between arguments to Go `println`.
pub fn print_space() {
    write_stderr_bytes(b" ");
}

/// Emit the line terminator used by Go `println`.
pub fn print_newline() {
    write_stderr_bytes(b"\n");
}

/// Print the exact bytes of a Go string.
pub fn print_go_string(value: GoString) {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    drop(write_go_string_to(&mut output, &value));
}

fn write_stderr_bytes(bytes: &[u8]) {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    drop(output.write_all(bytes));
}

fn write_go_string_to(output: &mut impl std::io::Write, value: &GoString) -> std::io::Result<()> {
    output.write_all(value.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_preserve_arbitrary_bytes() {
        let bytes = [b'a', 0, 0xff, 0x80, b'z'];
        let value = go_string_from_bytes(&bytes);

        assert_eq!(value.as_bytes(), bytes);
        assert_eq!(value.clone(), value);
    }

    #[test]
    fn concatenation_is_byte_exact() {
        let left = go_string_from_bytes(&[0xff, b'a']);
        let right = go_string_from_bytes(&[0, b'b']);

        assert_eq!(
            concat_go_strings(left, right).as_bytes(),
            [0xff, b'a', 0, b'b']
        );
    }

    #[test]
    fn raw_output_does_not_require_utf8() {
        let value = go_string_from_bytes(&[b'x', 0xff]);
        let mut output = Vec::new();

        assert!(write_go_string_to(&mut output, &value).is_ok());

        assert_eq!(output, [b'x', 0xff]);
    }

    #[test]
    fn int_arithmetic_matches_go_overflow_rules() {
        assert_eq!(GORS_RUNTIME_ABI_VERSION, 2);
        assert_eq!(int_add(GoInt::MAX, 1), GoInt::MIN);
        assert_eq!(int_sub(GoInt::MIN, 1), GoInt::MAX);
        assert_eq!(int_mul(GoInt::MAX, 2), -2);
        assert_eq!(int_neg(GoInt::MIN), GoInt::MIN);
        assert_eq!(int_div(GoInt::MIN, -1), GoInt::MIN);
        assert_eq!(int_rem(GoInt::MIN, -1), 0);
    }

    #[test]
    fn int_shifts_do_not_use_rusts_masked_shift_count() {
        assert_eq!(int_shl(1, 63), GoInt::MIN);
        assert_eq!(int_shl(1, 64), 0);
        assert_eq!(int_shl(1, 10_000), 0);
        assert_eq!(int_shr(-2, 1), -1);
        assert_eq!(int_shr(-2, 64), -1);
        assert_eq!(int_shr(2, 64), 0);
    }

    #[test]
    fn invalid_integer_operations_panic() {
        assert!(std::panic::catch_unwind(|| int_div(1, 0)).is_err());
        assert!(std::panic::catch_unwind(|| int_rem(1, 0)).is_err());
        assert!(std::panic::catch_unwind(|| int_shl(1, -1)).is_err());
        assert!(std::panic::catch_unwind(|| int_shr(1, -1)).is_err());
    }
}
