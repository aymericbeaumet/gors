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

mod byte_ranges;
mod byte_slices;
mod channels;
mod integer;
mod interface_containers;
mod interfaces;
mod maps;
mod printing;
mod slice_identity;
mod slice_values;
mod string_runes;
mod string_slices;

pub use byte_ranges::{
    go_slice_u8_index, go_slice_u8_len, go_slice_u8_range, go_string_index, go_string_range,
};
pub use byte_slices::{go_slice_u8_copy, go_slice_u8_make, go_slice_u8_set};
pub use channels::{
    GoChannelGoChannelI64, GoChannelGoString, GoChannelI64, go_channel_go_channel_i64_cap,
    go_channel_go_channel_i64_close, go_channel_go_channel_i64_is_nil,
    go_channel_go_channel_i64_len, go_channel_go_channel_i64_make, go_channel_go_channel_i64_nil,
    go_channel_go_channel_i64_receive, go_channel_go_channel_i64_receive_value,
    go_channel_go_channel_i64_send, go_channel_go_channel_i64_try_receive,
    go_channel_go_channel_i64_try_send, go_channel_go_string_cap, go_channel_go_string_close,
    go_channel_go_string_is_nil, go_channel_go_string_len, go_channel_go_string_make,
    go_channel_go_string_nil, go_channel_go_string_receive, go_channel_go_string_receive_value,
    go_channel_go_string_send, go_channel_go_string_try_receive, go_channel_go_string_try_send,
    go_channel_i64_cap, go_channel_i64_close, go_channel_i64_is_nil, go_channel_i64_len,
    go_channel_i64_make, go_channel_i64_nil, go_channel_i64_receive, go_channel_i64_receive_value,
    go_channel_i64_send, go_channel_i64_try_receive, go_channel_i64_try_send,
};
pub use integer::{
    int_div, int_div_i8, int_div_i16, int_div_i32, int_div_u8, int_div_u16, int_div_u32,
    int_div_u64, int_rem, int_rem_i8, int_rem_i16, int_rem_i32, int_rem_u8, int_rem_u16,
    int_rem_u32, int_rem_u64, int_shl, int_shl_signed_i8, int_shl_signed_i16, int_shl_signed_i32,
    int_shl_signed_u8, int_shl_signed_u16, int_shl_signed_u32, int_shl_signed_u64,
    int_shl_unsigned_i8, int_shl_unsigned_i16, int_shl_unsigned_i32, int_shl_unsigned_i64,
    int_shl_unsigned_u8, int_shl_unsigned_u16, int_shl_unsigned_u32, int_shl_unsigned_u64, int_shr,
    int_shr_signed_i8, int_shr_signed_i16, int_shr_signed_i32, int_shr_signed_u8,
    int_shr_signed_u16, int_shr_signed_u32, int_shr_signed_u64, int_shr_unsigned_i8,
    int_shr_unsigned_i16, int_shr_unsigned_i32, int_shr_unsigned_i64, int_shr_unsigned_u8,
    int_shr_unsigned_u16, int_shr_unsigned_u32, int_shr_unsigned_u64,
};
pub use interface_containers::{
    GoMapStringInterface, GoSliceInterface, go_map_string_interface_contains,
    go_map_string_interface_get, go_map_string_interface_len, go_map_string_interface_make,
    go_map_string_interface_set, go_slice_interface_index, go_slice_interface_is_nil,
    go_slice_interface_len, go_slice_interface_make, go_slice_interface_nil,
    go_slice_interface_set,
};
pub use interfaces::{
    GoInterface, GoPanicPayload, go_interface_box_aggregate, go_interface_box_bool,
    go_interface_box_comparable_aggregate, go_interface_box_f64,
    go_interface_box_go_slice_go_string, go_interface_box_go_string, go_interface_box_i64,
    go_interface_box_pointer_i64, go_interface_box_pointer_struct_i64, go_interface_box_struct_i64,
    go_interface_equal, go_interface_is_nil, go_interface_is_runtime_error, go_interface_is_type,
    go_interface_nil, go_interface_struct_i64_get, go_interface_unbox_aggregate,
    go_interface_unbox_bool, go_interface_unbox_f64, go_interface_unbox_go_slice_go_string,
    go_interface_unbox_go_string, go_interface_unbox_i64, go_interface_unbox_pointer_i64,
    go_interface_unbox_pointer_struct_i64, go_panic_payload_to_interface, panic_go_interface,
};
pub use maps::{
    GoMapI64GoString, GoMapStringI64, go_map_i64_go_string_clear, go_map_i64_go_string_contains,
    go_map_i64_go_string_delete, go_map_i64_go_string_get, go_map_i64_go_string_is_nil,
    go_map_i64_go_string_len, go_map_i64_go_string_make, go_map_i64_go_string_nil,
    go_map_i64_go_string_range_keys, go_map_i64_go_string_set, go_map_string_i64_clear,
    go_map_string_i64_contains, go_map_string_i64_delete, go_map_string_i64_get,
    go_map_string_i64_is_nil, go_map_string_i64_key_at, go_map_string_i64_len,
    go_map_string_i64_make, go_map_string_i64_nil, go_map_string_i64_range_keys,
    go_map_string_i64_set,
};
pub use printing::{
    print_bool, print_f64, print_go_string, print_i64, print_newline, print_space, print_u64,
};
pub use slice_identity::{
    go_slice_bool_is_nil, go_slice_bool_nil, go_slice_go_string_is_nil, go_slice_go_string_nil,
    go_slice_i64_is_nil, go_slice_i64_nil, go_slice_u8_is_nil, go_slice_u8_nil,
};
pub use string_runes::{
    go_string_from_rune, go_string_from_slice_runes, go_string_range_count,
    go_string_range_index_at, go_string_range_rune_at, go_string_to_slice_runes,
};
pub use string_slices::{
    GoSliceGoString, go_slice_go_string_append, go_slice_go_string_cap, go_slice_go_string_clear,
    go_slice_go_string_copy, go_slice_go_string_index, go_slice_go_string_len,
    go_slice_go_string_make, go_slice_go_string_range, go_slice_go_string_set,
};

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::sync::{Arc, RwLock};

/// The fixed-width representation of Go `int` for the selected data model.
///
/// The runtime uses the 64-bit Go data model on every
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
    nil: bool,
}

pub type GoSliceI64 = GoSlice<GoInt>;
pub type GoSliceU8 = GoSlice<u8>;
pub type GoSliceBool = GoSlice<bool>;

/// Construct a `[]bool` value from compiler-emitted literal elements.
#[must_use]
pub fn go_slice_bool_from_static(values: &'static [bool]) -> GoSliceBool {
    GoSliceBool {
        storage: Arc::new(RwLock::new(values.to_vec())),
        start: 0,
        len: values.len(),
        capacity: values.len(),
        nil: false,
    }
}

/// Construct a `[]int` value from compiler-emitted literal elements.
#[must_use]
pub fn go_slice_i64_from_static(values: &'static [GoInt]) -> GoSliceI64 {
    GoSliceI64 {
        storage: Arc::new(RwLock::new(values.to_vec())),
        start: 0,
        len: values.len(),
        capacity: values.len(),
        nil: false,
    }
}

/// Allocate a zero-initialized `[]int` with an explicit Go length and capacity.
/// A capacity of `-1` selects the requested length, matching two-argument
/// `make([]int, len)`.
#[must_use]
pub fn go_slice_i64_make(len: GoInt, capacity: GoInt) -> GoSliceI64 {
    slice_values::make(len, capacity)
}

/// Return a `[]int` length as the compiler's fixed-width Go `int`.
#[must_use]
pub fn go_slice_i64_len(slice: GoSliceI64) -> GoInt {
    slice_values::len(&slice)
}

/// Return a `[]int` capacity as the compiler's fixed-width Go `int`.
#[must_use]
pub fn go_slice_i64_cap(slice: GoSliceI64) -> GoInt {
    slice_values::cap(&slice)
}

/// Append one element, reusing the backing array exactly when capacity permits.
#[must_use]
pub fn go_slice_i64_append(slice: GoSliceI64, value: GoInt) -> GoSliceI64 {
    slice_values::append(slice, value)
}

/// Construct a `[]byte` value from compiler-emitted literal bytes.
#[must_use]
pub fn go_slice_u8_from_static(values: &'static [u8]) -> GoSliceU8 {
    GoSliceU8 {
        storage: Arc::new(RwLock::new(values.to_vec())),
        start: 0,
        len: values.len(),
        capacity: values.len(),
        nil: false,
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
    slice_values::copy(&destination, &source)
}

/// Assign the element zero value throughout an integer slice.
pub fn go_slice_i64_clear(slice: GoSliceI64) {
    slice_values::clear(&slice);
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
        nil: false,
    }
}

/// Read one `[]int` element with Go bounds checking.
#[must_use]
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_i64_index(slice: GoSliceI64, index: GoInt) -> GoInt {
    slice_values::index(&slice, index)
}

/// Produce a two- or three-index subslice while retaining the backing array.
///
/// `-1` denotes an omitted source bound; Go source indices are non-negative,
/// so the sentinel cannot collide with a valid bound.
#[must_use]
pub fn go_slice_i64_range(slice: GoSliceI64, low: GoInt, high: GoInt, max: GoInt) -> GoSliceI64 {
    go_slice_range(slice, low, high, max)
}

fn go_slice_range<T>(slice: GoSlice<T>, low: GoInt, high: GoInt, max: GoInt) -> GoSlice<T> {
    let low = optional_slice_bound(low, 0);
    let high = optional_slice_bound(high, slice.len);
    let max = optional_slice_bound(max, slice.capacity);
    if low > high || high > max || max > slice.capacity {
        slice_bounds_out_of_range();
    }
    GoSlice {
        storage: slice.storage,
        start: slice.start.saturating_add(low),
        len: high.saturating_sub(low),
        capacity: max.saturating_sub(low),
        nil: slice.nil,
    }
}

/// Assign one `[]int` element through its shared backing array.
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_i64_set(slice: GoSliceI64, index: GoInt, value: GoInt) {
    slice_values::set(&slice, index, value);
}

/// Read one `[]bool` element with Go bounds checking.
#[must_use]
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_bool_index(slice: GoSliceBool, index: GoInt) -> bool {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let storage = slice
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    storage[absolute]
}

/// Assign one `[]bool` element through its shared backing array.
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_bool_set(slice: GoSliceBool, index: GoInt, value: bool) {
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

/// Return a Go string's byte length.
#[must_use]
pub fn go_string_len(value: GoString) -> GoInt {
    match GoInt::try_from(value.len) {
        Ok(length) => length,
        Err(_) => std::process::abort(),
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

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn nil_map_assignment() -> ! {
    std::panic::resume_unwind(Box::new("assignment to entry in nil map"))
}

/// A nullable Go `*int` value with shared mutable pointee identity.
///
/// Cloning a non-nil pointer preserves the identity of its allocated storage.
#[derive(Clone, Debug, Default)]
pub struct GoPointerI64 {
    storage: Option<Arc<RwLock<GoInt>>>,
}

/// Construct the nil `*int` value.
#[must_use]
pub fn go_pointer_i64_nil() -> GoPointerI64 {
    GoPointerI64::default()
}

/// Allocate a zero-initialized Go `int` and return its address.
#[must_use]
pub fn go_pointer_i64_new() -> GoPointerI64 {
    GoPointerI64 {
        storage: Some(Arc::new(RwLock::new(0))),
    }
}

/// Dereference a Go `*int`, preserving the language's nil-pointer panic.
#[must_use]
pub fn go_pointer_i64_get(pointer: GoPointerI64) -> GoInt {
    let Some(storage) = pointer.storage else {
        nil_pointer_dereference();
    };
    let value = storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *value
}

/// Assign through a Go `*int`, preserving shared pointee identity.
pub fn go_pointer_i64_set(pointer: GoPointerI64, value: GoInt) {
    let Some(storage) = pointer.storage else {
        nil_pointer_dereference();
    };
    let mut target = storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *target = value;
}

/// Report whether a Go `*int` value is nil.
#[must_use]
pub fn go_pointer_i64_is_nil(pointer: GoPointerI64) -> bool {
    pointer.storage.is_none()
}

/// Compare two Go `*int` values by pointee identity.
#[must_use]
pub fn go_pointer_i64_equal(left: GoPointerI64, right: GoPointerI64) -> bool {
    match (left.storage, right.storage) {
        (None, None) => true,
        (Some(left), Some(right)) => Arc::ptr_eq(&left, &right),
        _ => false,
    }
}

/// A nullable pointer to an integer-field Go struct.
///
/// The field vector is private runtime storage. Cloning the pointer preserves
/// pointee identity, while compiler-generated struct values remain fixed Rust
/// arrays with Go value-copy semantics.
#[derive(Clone, Debug, Default)]
pub struct GoPointerStructI64 {
    storage: Option<Arc<RwLock<Box<[GoInt]>>>>,
}

/// Construct the nil pointer-to-integer-struct value.
#[must_use]
pub fn go_pointer_struct_i64_nil() -> GoPointerStructI64 {
    GoPointerStructI64::default()
}

/// Allocate a zero-initialized integer-field struct with `field_count` fields.
#[must_use]
pub fn go_pointer_struct_i64_new(field_count: GoInt) -> GoPointerStructI64 {
    let field_count = usize::try_from(field_count).unwrap_or_else(|_| pointer_struct_bounds());
    GoPointerStructI64 {
        storage: Some(Arc::new(RwLock::new(
            vec![0; field_count].into_boxed_slice(),
        ))),
    }
}

/// Read one field through a pointer-to-integer-struct value.
#[must_use]
pub fn go_pointer_struct_i64_get(pointer: GoPointerStructI64, field: GoInt) -> GoInt {
    let Some(storage) = pointer.storage else {
        nil_pointer_dereference();
    };
    let field = usize::try_from(field).unwrap_or_else(|_| pointer_struct_bounds());
    let fields = storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *fields.get(field).unwrap_or_else(|| pointer_struct_bounds())
}

/// Assign one field through a pointer-to-integer-struct value.
pub fn go_pointer_struct_i64_set(pointer: GoPointerStructI64, field: GoInt, value: GoInt) {
    let Some(storage) = pointer.storage else {
        nil_pointer_dereference();
    };
    let field = usize::try_from(field).unwrap_or_else(|_| pointer_struct_bounds());
    let mut fields = storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *fields
        .get_mut(field)
        .unwrap_or_else(|| pointer_struct_bounds()) = value;
}

/// Report whether a pointer-to-integer-struct value is nil.
#[must_use]
pub fn go_pointer_struct_i64_is_nil(pointer: GoPointerStructI64) -> bool {
    pointer.storage.is_none()
}

/// Compare pointer identity, including equality between two nil pointers.
#[must_use]
pub fn go_pointer_struct_i64_equal(left: GoPointerStructI64, right: GoPointerStructI64) -> bool {
    match (left.storage, right.storage) {
        (None, None) => true,
        (Some(left), Some(right)) => Arc::ptr_eq(&left, &right),
        (None, Some(_)) | (Some(_), None) => false,
    }
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is a checked runtime pointer-field boundary.
fn pointer_struct_bounds() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: struct field index out of range"))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn nil_pointer_dereference() -> ! {
    std::panic::resume_unwind(Box::new(
        "runtime error: invalid memory address or nil pointer dereference",
    ))
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
