//! Allocation, mutation, and overlap-safe copying for byte slices.

use std::sync::{Arc, RwLock};

use super::{GoInt, GoSliceU8, slice_bounds_out_of_range, slice_index, slice_values};

/// Allocate a zero-initialized `[]byte` with an explicit Go length and capacity.
/// A capacity of `-1` selects the requested length for two-argument `make`.
#[must_use]
pub fn go_slice_u8_make(len: GoInt, capacity: GoInt) -> GoSliceU8 {
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
    GoSliceU8 {
        storage: Arc::new(RwLock::new(vec![0; capacity])),
        start: 0,
        len,
        capacity,
        nil: false,
    }
}

/// Assign one `[]byte` element through its shared backing array.
#[allow(clippy::indexing_slicing)] // The explicit Go bounds check validates this index.
pub fn go_slice_u8_set(slice: GoSliceU8, index: GoInt, value: GoInt) {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let Ok(value) = u8::try_from(value.rem_euclid(256)) else {
        slice_bounds_out_of_range();
    };
    let mut storage = slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    storage[absolute] = value;
}

/// Copy bytes between slices with Go's overlap-safe semantics.
pub fn go_slice_u8_copy(destination: GoSliceU8, source: GoSliceU8) -> GoInt {
    slice_values::copy(&destination, &source)
}
