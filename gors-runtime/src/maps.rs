//! Runtime representations for the compiler's concrete Go map shapes.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::{GoInt, GoSlice, GoSliceGoString, GoSliceI64, GoString, nil_map_assignment};

/// A nullable Go `map[string]int` header with shared mutable identity.
#[derive(Clone, Debug, Default)]
pub struct GoMapStringI64 {
    storage: Option<Arc<RwLock<BTreeMap<GoString, GoInt>>>>,
}

/// Construct the nil `map[string]int` value.
#[must_use]
pub fn go_map_string_i64_nil() -> GoMapStringI64 {
    GoMapStringI64::default()
}

/// Allocate an empty non-nil `map[string]int` value.
#[must_use]
pub fn go_map_string_i64_make() -> GoMapStringI64 {
    GoMapStringI64 {
        storage: Some(Arc::new(RwLock::new(BTreeMap::new()))),
    }
}

/// Return the number of entries in a map. A nil map has length zero.
#[must_use]
pub fn go_map_string_i64_len(map: GoMapStringI64) -> GoInt {
    map.storage.map_or(0, |storage| {
        let entries = storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        GoInt::try_from(entries.len()).unwrap_or_else(|_| map_index_out_of_range())
    })
}

/// Read a map entry, returning the element zero value when the key is absent.
#[must_use]
pub fn go_map_string_i64_get(map: GoMapStringI64, key: GoString) -> GoInt {
    map.storage.map_or(0, |storage| {
        let entries = storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.get(&key).copied().unwrap_or_default()
    })
}

/// Report whether a map contains a key. Nil maps contain no keys.
#[must_use]
pub fn go_map_string_i64_contains(map: GoMapStringI64, key: GoString) -> bool {
    map.storage.is_some_and(|storage| {
        let entries = storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.contains_key(&key)
    })
}

/// Assign a map entry while preserving shared map identity.
pub fn go_map_string_i64_set(map: GoMapStringI64, key: GoString, value: GoInt) {
    let Some(storage) = map.storage else {
        nil_map_assignment();
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, value);
}

/// Delete a map entry. Deleting from a nil map is a no-op.
pub fn go_map_string_i64_delete(map: GoMapStringI64, key: GoString) {
    let Some(storage) = map.storage else {
        return;
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&key);
}

/// Delete every map entry. Clearing a nil map is a no-op.
pub fn go_map_string_i64_clear(map: GoMapStringI64) {
    let Some(storage) = map.storage else {
        return;
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Report whether a map header is nil.
#[must_use]
pub fn go_map_string_i64_is_nil(map: GoMapStringI64) -> bool {
    map.storage.is_none()
}

/// Return the key at one deterministic iteration index.
///
/// This operation remains in the ABI for older generated artifacts. New
/// compiler output snapshots keys before beginning map iteration.
#[must_use]
pub fn go_map_string_i64_key_at(map: GoMapStringI64, index: GoInt) -> GoString {
    let Some(storage) = map.storage else {
        map_index_out_of_range();
    };
    let Ok(index) = usize::try_from(index) else {
        map_index_out_of_range();
    };
    storage
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .keys()
        .nth(index)
        .cloned()
        .unwrap_or_else(|| map_index_out_of_range())
}

/// Snapshot the keys that are candidates for one map range statement.
#[must_use]
pub fn go_map_string_i64_range_keys(map: GoMapStringI64) -> GoSliceGoString {
    let keys = map.storage.map_or_else(Vec::new, |storage| {
        storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    });
    GoSlice::from_values(keys)
}

/// A nullable Go `map[int]string` header with shared mutable identity.
#[derive(Clone, Debug, Default)]
pub struct GoMapI64GoString {
    storage: Option<Arc<RwLock<BTreeMap<GoInt, GoString>>>>,
}

/// Construct the nil `map[int]string` value.
#[must_use]
pub fn go_map_i64_go_string_nil() -> GoMapI64GoString {
    GoMapI64GoString::default()
}

/// Allocate an empty non-nil `map[int]string` value.
#[must_use]
pub fn go_map_i64_go_string_make() -> GoMapI64GoString {
    GoMapI64GoString {
        storage: Some(Arc::new(RwLock::new(BTreeMap::new()))),
    }
}

/// Return the number of entries in a map. A nil map has length zero.
#[must_use]
pub fn go_map_i64_go_string_len(map: GoMapI64GoString) -> GoInt {
    map.storage.map_or(0, |storage| {
        let entries = storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        GoInt::try_from(entries.len()).unwrap_or_else(|_| map_index_out_of_range())
    })
}

/// Read a map entry, returning the element zero value when the key is absent.
#[must_use]
pub fn go_map_i64_go_string_get(map: GoMapI64GoString, key: GoInt) -> GoString {
    map.storage.map_or_else(GoString::default, |storage| {
        storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .cloned()
            .unwrap_or_default()
    })
}

/// Report whether a map contains a key. Nil maps contain no keys.
#[must_use]
pub fn go_map_i64_go_string_contains(map: GoMapI64GoString, key: GoInt) -> bool {
    map.storage.is_some_and(|storage| {
        storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&key)
    })
}

/// Assign a map entry while preserving shared map identity.
pub fn go_map_i64_go_string_set(map: GoMapI64GoString, key: GoInt, value: GoString) {
    let Some(storage) = map.storage else {
        nil_map_assignment();
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(key, value);
}

/// Delete a map entry. Deleting from a nil map is a no-op.
pub fn go_map_i64_go_string_delete(map: GoMapI64GoString, key: GoInt) {
    let Some(storage) = map.storage else {
        return;
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&key);
}

/// Delete every map entry. Clearing a nil map is a no-op.
pub fn go_map_i64_go_string_clear(map: GoMapI64GoString) {
    let Some(storage) = map.storage else {
        return;
    };
    storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Report whether a map header is nil.
#[must_use]
pub fn go_map_i64_go_string_is_nil(map: GoMapI64GoString) -> bool {
    map.storage.is_none()
}

/// Snapshot the keys that are candidates for one map range statement.
#[must_use]
pub fn go_map_i64_go_string_range_keys(map: GoMapI64GoString) -> GoSliceI64 {
    let keys = map.storage.map_or_else(Vec::new, |storage| {
        storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    });
    GoSlice::from_values(keys)
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is a checked runtime iteration boundary.
fn map_index_out_of_range() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: map iteration index out of range"))
}
