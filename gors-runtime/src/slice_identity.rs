//! Nil identity operations for Go slice headers.

use std::sync::{Arc, RwLock};

use crate::{GoSlice, GoSliceBool, GoSliceGoString, GoSliceI64, GoSliceU8};

impl<T> GoSlice<T> {
    pub(crate) fn nil() -> Self {
        Self {
            storage: Arc::new(RwLock::new(Vec::new())),
            start: 0,
            len: 0,
            capacity: 0,
            nil: true,
        }
    }
}

/// Construct the nil `[]int` value.
#[must_use]
pub fn go_slice_i64_nil() -> GoSliceI64 {
    GoSliceI64::nil()
}

/// Report whether an integer slice is nil.
#[must_use]
pub fn go_slice_i64_is_nil(slice: GoSliceI64) -> bool {
    slice.nil
}

/// Construct the nil `[]byte` value.
#[must_use]
pub fn go_slice_u8_nil() -> GoSliceU8 {
    GoSliceU8::nil()
}

/// Report whether a byte slice is nil.
#[must_use]
pub fn go_slice_u8_is_nil(slice: GoSliceU8) -> bool {
    slice.nil
}

/// Construct the nil `[]bool` value.
#[must_use]
pub fn go_slice_bool_nil() -> GoSliceBool {
    GoSliceBool::nil()
}

/// Report whether a boolean slice is nil.
#[must_use]
pub fn go_slice_bool_is_nil(slice: GoSliceBool) -> bool {
    slice.nil
}

/// Construct the nil `[]string` value.
#[must_use]
pub fn go_slice_go_string_nil() -> GoSliceGoString {
    GoSliceGoString::nil()
}

/// Report whether a string slice is nil.
#[must_use]
pub fn go_slice_go_string_is_nil(slice: GoSliceGoString) -> bool {
    slice.nil
}
