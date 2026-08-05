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
use std::sync::{Arc, RwLock};

/// The fixed-width representation of Go `int` for the bootstrap target.
///
/// The initial backend deliberately targets the 64-bit Go data model on every
/// Rust host, including wasm32, instead of inheriting Rust's pointer width.
pub type GoInt = i64;

/// A Go slice header backed by shared mutable array storage.
///
/// Cloning this value copies only the slice header. Indexing and reslicing
/// therefore preserve Go's backing-array aliasing rules.
#[derive(Clone, Debug)]
pub struct GoSlice<T> {
    storage: Arc<RwLock<Vec<T>>>,
    start: usize,
    len: usize,
    capacity: usize,
}

pub type GoSliceI64 = GoSlice<GoInt>;
pub type GoSliceU8 = GoSlice<u8>;

/// Construct a `[]int` value from compiler-emitted literal elements.
#[must_use]
pub fn go_slice_i64_from_static(values: &'static [GoInt]) -> GoSliceI64 {
    GoSliceI64 {
        storage: Arc::new(RwLock::new(values.to_vec())),
        start: 0,
        len: values.len(),
        capacity: values.len(),
    }
}

/// Allocate a zero-initialized `[]int` with an explicit Go length and capacity.
/// A capacity of `-1` selects the requested length, matching two-argument
/// `make([]int, len)`.
#[must_use]
pub fn go_slice_i64_make(len: GoInt, capacity: GoInt) -> GoSliceI64 {
    let Ok(len) = usize::try_from(len) else {
        slice_bounds_out_of_range();
    };
    let capacity = if capacity == -1 {
        len
    } else {
        usize::try_from(capacity).unwrap_or_else(|_| slice_bounds_out_of_range())
    };
    if len > capacity {
        slice_bounds_out_of_range();
    }
    GoSliceI64 {
        storage: Arc::new(RwLock::new(vec![0; capacity])),
        start: 0,
        len,
        capacity,
    }
}

/// Return a `[]int` length as the compiler's fixed-width Go `int`.
#[must_use]
pub fn go_slice_i64_len(slice: GoSliceI64) -> GoInt {
    GoInt::try_from(slice.len).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Return a `[]int` capacity as the compiler's fixed-width Go `int`.
#[must_use]
pub fn go_slice_i64_cap(slice: GoSliceI64) -> GoInt {
    GoInt::try_from(slice.capacity).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Append one element, reusing the backing array exactly when capacity permits.
#[must_use]
#[allow(clippy::indexing_slicing)] // The slice header invariants validate the write position.
pub fn go_slice_i64_append(mut slice: GoSliceI64, value: GoInt) -> GoSliceI64 {
    if slice.len < slice.capacity {
        let absolute = slice.start.saturating_add(slice.len);
        let mut storage = slice
            .storage
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        storage[absolute] = value;
        drop(storage);
        slice.len = slice.len.saturating_add(1);
        return slice;
    }

    let required = slice.len.saturating_add(1);
    let capacity = slice.capacity.saturating_mul(2).max(required).max(1);
    let mut values = Vec::with_capacity(capacity);
    {
        let storage = slice
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = slice.start.saturating_add(slice.len);
        values.extend_from_slice(&storage[slice.start..end]);
    }
    values.push(value);
    values.resize(capacity, 0);
    GoSliceI64 {
        storage: Arc::new(RwLock::new(values)),
        start: 0,
        len: required,
        capacity,
    }
}

/// Construct a `[]byte` value from compiler-emitted literal bytes.
#[must_use]
pub fn go_slice_u8_from_static(values: &'static [u8]) -> GoSliceU8 {
    GoSliceU8 {
        storage: Arc::new(RwLock::new(values.to_vec())),
        start: 0,
        len: values.len(),
        capacity: values.len(),
    }
}

/// Append every byte from another slice, retaining Go backing-array behavior.
#[must_use]
pub fn go_slice_u8_append_slice(slice: GoSliceU8, values: GoSliceU8) -> GoSliceU8 {
    let values = {
        let storage = values
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = values.start.saturating_add(values.len);
        storage
            .get(values.start..end)
            .unwrap_or_else(|| slice_bounds_out_of_range())
            .to_vec()
    };
    append_u8_values(slice, &values)
}

/// Append every byte from a Go string to a byte slice.
#[must_use]
pub fn go_slice_u8_append_string(slice: GoSliceU8, value: GoString) -> GoSliceU8 {
    append_u8_values(slice, value.as_bytes())
}

/// Copy bytes from a Go string into a destination byte slice.
pub fn go_slice_u8_copy_string(destination: GoSliceU8, source: GoString) -> GoInt {
    let count = destination.len.min(source.len);
    let mut storage = destination
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = destination.start.saturating_add(count);
    let Some(target) = storage.get_mut(destination.start..end) else {
        slice_bounds_out_of_range();
    };
    target.copy_from_slice(
        source
            .as_bytes()
            .get(..count)
            .unwrap_or_else(|| slice_bounds_out_of_range()),
    );
    drop(storage);
    GoInt::try_from(count).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Copy integer elements between slices with Go's overlap-safe semantics.
pub fn go_slice_i64_copy(destination: GoSliceI64, source: GoSliceI64) -> GoInt {
    let count = destination.len.min(source.len);
    let source_values = {
        let storage = source
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = source.start.saturating_add(count);
        storage
            .get(source.start..end)
            .unwrap_or_else(|| slice_bounds_out_of_range())
            .to_vec()
    };
    let mut storage = destination
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = destination.start.saturating_add(count);
    let target = storage
        .get_mut(destination.start..end)
        .unwrap_or_else(|| slice_bounds_out_of_range());
    target.copy_from_slice(&source_values);
    drop(storage);
    GoInt::try_from(count).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Assign the element zero value throughout an integer slice.
pub fn go_slice_i64_clear(slice: GoSliceI64) {
    let mut storage = slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = slice.start.saturating_add(slice.len);
    let Some(values) = storage.get_mut(slice.start..end) else {
        slice_bounds_out_of_range();
    };
    values.fill(0);
    drop(storage);
}

/// Convert the visible bytes of a byte slice into an immutable Go string.
#[must_use]
pub fn go_string_from_slice_u8(slice: GoSliceU8) -> GoString {
    let storage = slice
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = slice.start.saturating_add(slice.len);
    go_string_from_bytes(
        storage
            .get(slice.start..end)
            .unwrap_or_else(|| slice_bounds_out_of_range()),
    )
}

fn append_u8_values(mut slice: GoSliceU8, values: &[u8]) -> GoSliceU8 {
    let required = slice.len.saturating_add(values.len());
    if required <= slice.capacity {
        let start = slice.start.saturating_add(slice.len);
        let end = start.saturating_add(values.len());
        let mut storage = slice
            .storage
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(target) = storage.get_mut(start..end) else {
            slice_bounds_out_of_range();
        };
        target.copy_from_slice(values);
        drop(storage);
        slice.len = required;
        return slice;
    }

    let capacity = slice.capacity.saturating_mul(2).max(required).max(1);
    let mut combined = Vec::with_capacity(capacity);
    {
        let storage = slice
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = slice.start.saturating_add(slice.len);
        combined.extend_from_slice(
            storage
                .get(slice.start..end)
                .unwrap_or_else(|| slice_bounds_out_of_range()),
        );
    }
    combined.extend_from_slice(values);
    combined.resize(capacity, 0);
    GoSliceU8 {
        storage: Arc::new(RwLock::new(combined)),
        start: 0,
        len: required,
        capacity,
    }
}

/// Read one `[]int` element with Go bounds checking.
#[must_use]
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_i64_index(slice: GoSliceI64, index: GoInt) -> GoInt {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let storage = slice
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    storage[absolute]
}

/// Produce a two- or three-index subslice while retaining the backing array.
///
/// `-1` denotes an omitted source bound; Go source indices are non-negative,
/// so the sentinel cannot collide with a valid bound.
#[must_use]
pub fn go_slice_i64_range(slice: GoSliceI64, low: GoInt, high: GoInt, max: GoInt) -> GoSliceI64 {
    let low = optional_slice_bound(low, 0);
    let high = optional_slice_bound(high, slice.len);
    let max = optional_slice_bound(max, slice.capacity);
    if low > high || high > max || max > slice.capacity {
        slice_bounds_out_of_range();
    }
    GoSliceI64 {
        storage: slice.storage,
        start: slice.start.saturating_add(low),
        len: high.saturating_sub(low),
        capacity: max.saturating_sub(low),
    }
}

/// Assign one `[]int` element through its shared backing array.
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_i64_set(slice: GoSliceI64, index: GoInt, value: GoInt) {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let mut storage = slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    storage[absolute] = value;
}

fn slice_index(index: GoInt, len: usize) -> usize {
    let Ok(index) = usize::try_from(index) else {
        index_out_of_range();
    };
    if index >= len {
        index_out_of_range();
    }
    index
}

fn optional_slice_bound(bound: GoInt, default: usize) -> usize {
    if bound == -1 {
        return default;
    }
    usize::try_from(bound).unwrap_or_else(|_| slice_bounds_out_of_range())
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn index_out_of_range() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: index out of range"))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn slice_bounds_out_of_range() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: slice bounds out of range"))
}

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
    std::panic::resume_unwind(Box::new("runtime error: integer divide by zero"))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn negative_shift_amount() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: negative shift amount"))
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

/// Raise an explicit Go panic carrying a boolean value.
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
pub fn panic_bool(value: bool) {
    std::panic::resume_unwind(Box::new(value))
}

/// Raise an explicit Go panic carrying an `int` value.
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
pub fn panic_i64(value: GoInt) {
    std::panic::resume_unwind(Box::new(value))
}

/// Raise an explicit Go panic carrying a string value.
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
pub fn panic_go_string(value: GoString) {
    std::panic::resume_unwind(Box::new(value))
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
