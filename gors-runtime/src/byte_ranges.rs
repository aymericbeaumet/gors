//! Bounds-checked byte-slice and string operations.

use super::{
    GoInt, GoSliceU8, GoString, go_slice_range, optional_slice_bound, slice_bounds_out_of_range,
    slice_index,
};

/// Return a `[]byte` length as the compiler's fixed-width Go `int`.
#[must_use]
pub fn go_slice_u8_len(slice: GoSliceU8) -> GoInt {
    GoInt::try_from(slice.len).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Read one `[]byte` element with Go bounds checking.
#[must_use]
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_u8_index(slice: GoSliceU8, index: GoInt) -> GoInt {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let storage = slice
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    GoInt::from(storage[absolute])
}

/// Produce a two- or three-index byte subslice while retaining the backing array.
#[must_use]
pub fn go_slice_u8_range(slice: GoSliceU8, low: GoInt, high: GoInt, max: GoInt) -> GoSliceU8 {
    go_slice_range(slice, low, high, max)
}

/// Read one byte from a Go string with Go bounds checking.
#[must_use]
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_string_index(value: GoString, index: GoInt) -> GoInt {
    let index = slice_index(index, value.len);
    GoInt::from(value.as_bytes()[index])
}

/// Produce a substring that shares the immutable backing storage.
#[must_use]
pub fn go_string_range(value: GoString, low: GoInt, high: GoInt) -> GoString {
    let low = optional_slice_bound(low, 0);
    let high = optional_slice_bound(high, value.len);
    if low > high || high > value.len {
        slice_bounds_out_of_range();
    }
    GoString {
        storage: value.storage,
        start: value.start.saturating_add(low),
        len: high.saturating_sub(low),
    }
}
