//! Concrete runtime ABI for Go slices whose element representation is `GoString`.

use crate::{GoInt, GoSlice, GoString, go_slice_range, slice_values};

pub type GoSliceGoString = GoSlice<GoString>;

#[must_use]
pub fn go_slice_go_string_make(len: GoInt, capacity: GoInt) -> GoSliceGoString {
    slice_values::make(len, capacity)
}

#[must_use]
pub fn go_slice_go_string_len(slice: GoSliceGoString) -> GoInt {
    slice_values::len(&slice)
}

#[must_use]
pub fn go_slice_go_string_cap(slice: GoSliceGoString) -> GoInt {
    slice_values::cap(&slice)
}

#[must_use]
pub fn go_slice_go_string_index(slice: GoSliceGoString, index: GoInt) -> GoString {
    slice_values::index(&slice, index)
}

#[must_use]
pub fn go_slice_go_string_range(
    slice: GoSliceGoString,
    low: GoInt,
    high: GoInt,
    max: GoInt,
) -> GoSliceGoString {
    go_slice_range(slice, low, high, max)
}

pub fn go_slice_go_string_set(slice: GoSliceGoString, index: GoInt, value: GoString) {
    slice_values::set(&slice, index, value);
}

#[must_use]
pub fn go_slice_go_string_append(slice: GoSliceGoString, value: GoString) -> GoSliceGoString {
    slice_values::append(slice, value)
}

pub fn go_slice_go_string_copy(destination: GoSliceGoString, source: GoSliceGoString) -> GoInt {
    slice_values::copy(&destination, &source)
}

pub fn go_slice_go_string_clear(slice: GoSliceGoString) {
    slice_values::clear(&slice);
}
