//! Interface-backed aggregate containers used after typed representation lowering.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::{
    GoInt, GoInterface, GoSlice, GoString, nil_map_assignment, slice_bounds_out_of_range,
    slice_index,
};

/// A Go slice whose concrete element type is retained by each tagged value.
pub type GoSliceInterface = GoSlice<GoInterface>;

/// A nullable `map[string]T` representation for tagged aggregate values.
#[derive(Clone, Debug, Default)]
pub struct GoMapStringInterface {
    storage: Option<Arc<RwLock<BTreeMap<GoString, GoInterface>>>>,
}

/// Allocate an interface-backed slice with an explicit Go length and capacity.
#[must_use]
pub fn go_slice_interface_make(len: GoInt, capacity: GoInt) -> GoSliceInterface {
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
    GoSliceInterface {
        storage: Arc::new(RwLock::new(vec![GoInterface::default(); capacity])),
        start: 0,
        len,
        capacity,
        nil: false,
    }
}

/// Construct a nil interface-backed slice.
#[must_use]
pub fn go_slice_interface_nil() -> GoSliceInterface {
    GoSliceInterface::nil()
}

/// Report whether an interface-backed slice is nil.
#[must_use]
pub fn go_slice_interface_is_nil(slice: GoSliceInterface) -> bool {
    slice.nil
}

/// Return an interface-backed slice length.
#[must_use]
pub fn go_slice_interface_len(slice: GoSliceInterface) -> GoInt {
    GoInt::try_from(slice.len).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Read one tagged slice element through the shared backing array.
#[must_use]
pub fn go_slice_interface_index(slice: GoSliceInterface, index: GoInt) -> GoInterface {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    let storage = slice
        .storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    storage
        .get(absolute)
        .cloned()
        .unwrap_or_else(|| slice_bounds_out_of_range())
}

/// Assign one tagged slice element through the shared backing array.
pub fn go_slice_interface_set(slice: GoSliceInterface, index: GoInt, value: GoInterface) {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    *slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_mut(absolute)
        .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
}

/// Allocate an empty non-nil interface-backed map.
#[must_use]
pub fn go_map_string_interface_make() -> GoMapStringInterface {
    GoMapStringInterface {
        storage: Some(Arc::new(RwLock::new(BTreeMap::new()))),
    }
}

/// Return an interface-backed map length.
#[must_use]
pub fn go_map_string_interface_len(map: GoMapStringInterface) -> GoInt {
    let Some(storage) = map.storage else {
        return 0;
    };
    let entries = storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    GoInt::try_from(entries.len()).unwrap_or_else(|_| slice_bounds_out_of_range())
}

/// Read a tagged map entry, returning a nil tag when the key is absent.
#[must_use]
pub fn go_map_string_interface_get(map: GoMapStringInterface, key: GoString) -> GoInterface {
    let Some(storage) = map.storage else {
        return GoInterface::default();
    };
    let entries = storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.get(&key).cloned().unwrap_or_default()
}

/// Report whether an interface-backed map contains a key.
#[must_use]
pub fn go_map_string_interface_contains(map: GoMapStringInterface, key: GoString) -> bool {
    let Some(storage) = map.storage else {
        return false;
    };
    let entries = storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.contains_key(&key)
}

/// Assign a tagged map entry while preserving shared map identity.
pub fn go_map_string_interface_set(map: GoMapStringInterface, key: GoString, value: GoInterface) {
    let Some(storage) = map.storage else {
        nil_map_assignment();
    };
    let mut entries = storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.insert(key, value);
}
