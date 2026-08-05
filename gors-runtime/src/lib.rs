//! Minimal runtime ABI for code emitted by gors.
//!
//! This crate owns Go value representations and language-level operations that
//! cannot be expressed faithfully with primitive Rust types. It deliberately
//! contains no Go standard-library replacements. The language built-ins
//! `print` and `println` write to standard error, matching the pinned Go oracle.

// Native distributions precompile this complete runtime once and generated
// programs link it as `__gors_runtime`, even when one program uses only a
// subset of its operations. Generated source never embeds or recompiles it.
#![allow(dead_code)]

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::sync::Arc;

/// The fixed-width representation of Go `int` for the bootstrap target.
///
/// The initial backend deliberately targets the 64-bit Go data model on every
/// Rust host, including wasm32, instead of inheriting Rust's pointer width.
pub type GoInt = i64;

/// An immutable Go string containing arbitrary bytes.
///
/// Clones share backing storage, matching Go's cheap immutable string-header
/// copies. A range is retained separately so future slice lowering can share
/// the same allocation instead of copying bytes. Compiler-emitted literals use
/// static backing and allocate nothing.
#[derive(Clone)]
pub struct GoString {
    storage: StringStorage,
    start: usize,
    len: usize,
}

#[derive(Clone)]
enum StringStorage {
    Static(&'static [u8]),
    Shared(Arc<Vec<u8>>),
}

impl GoString {
    /// Return the exact bytes stored in this Go string.
    #[must_use]
    #[allow(clippy::indexing_slicing)] // Private constructors prove the stored range is in bounds.
    pub fn as_bytes(&self) -> &[u8] {
        let end = self.start.saturating_add(self.len);
        match &self.storage {
            StringStorage::Static(bytes) => &bytes[self.start..end],
            StringStorage::Shared(bytes) => &bytes[self.start..end],
        }
    }
}

impl Default for GoString {
    fn default() -> Self {
        Self {
            storage: StringStorage::Static(&[]),
            start: 0,
            len: 0,
        }
    }
}

impl std::fmt::Debug for GoString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("GoString")
            .field(&self.as_bytes())
            .finish()
    }
}

impl PartialEq for GoString {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for GoString {}

impl PartialOrd for GoString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GoString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl Hash for GoString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

/// Construct a Go string without interpreting its bytes as UTF-8.
#[must_use]
pub fn go_string_from_bytes(bytes: &[u8]) -> GoString {
    GoString {
        storage: StringStorage::Shared(Arc::new(bytes.to_vec())),
        start: 0,
        len: bytes.len(),
    }
}

/// Construct a Go string backed directly by compiler-emitted static bytes.
#[must_use]
pub const fn go_string_from_static(bytes: &'static [u8]) -> GoString {
    GoString {
        storage: StringStorage::Static(bytes),
        start: 0,
        len: bytes.len(),
    }
}

/// Concatenate two Go strings while preserving every byte.
#[must_use]
pub fn concat_go_strings(mut left: GoString, right: GoString) -> GoString {
    if left.len == 0 {
        return right;
    }
    if right.len == 0 {
        return left;
    }

    let right_bytes = right.as_bytes();
    if left.start == 0
        && let StringStorage::Shared(storage) = &mut left.storage
        && left.len == storage.len()
        && let Some(bytes) = Arc::get_mut(storage)
    {
        bytes.extend_from_slice(right_bytes);
        left.len = bytes.len();
        return left;
    }

    let required = left.len.saturating_add(right.len);
    let mut bytes = Vec::with_capacity(concat_growth_capacity(required));
    bytes.extend_from_slice(left.as_bytes());
    bytes.extend_from_slice(right_bytes);
    let len = bytes.len();
    GoString {
        storage: StringStorage::Shared(Arc::new(bytes)),
        start: 0,
        len,
    }
}

fn concat_growth_capacity(required: usize) -> usize {
    required
        .saturating_add(required / 2)
        .saturating_add(usize::from(required != 0))
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
mod tests;
