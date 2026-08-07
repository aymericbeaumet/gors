//! Shared value-slice mechanics for concrete runtime element representations.

use std::sync::{Arc, RwLock};

use crate::{GoInt, GoSlice, slice_bounds_out_of_range, slice_index};

pub fn make<T: Clone + Default>(len: GoInt, capacity: GoInt) -> GoSlice<T> {
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
    GoSlice {
        storage: Arc::new(RwLock::new(vec![T::default(); capacity])),
        start: 0,
        len,
        capacity,
        nil: false,
    }
}

pub fn len<T>(slice: &GoSlice<T>) -> GoInt {
    GoInt::try_from(slice.len).unwrap_or_else(|_| slice_bounds_out_of_range())
}

pub fn cap<T>(slice: &GoSlice<T>) -> GoInt {
    GoInt::try_from(slice.capacity).unwrap_or_else(|_| slice_bounds_out_of_range())
}

pub fn index<T: Clone>(slice: &GoSlice<T>, index: GoInt) -> T {
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

pub fn set<T>(slice: &GoSlice<T>, index: GoInt, value: T) {
    let index = slice_index(index, slice.len);
    let absolute = slice.start.saturating_add(index);
    *slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_mut(absolute)
        .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
}

pub fn append<T: Clone + Default>(mut slice: GoSlice<T>, value: T) -> GoSlice<T> {
    if slice.len < slice.capacity {
        let absolute = slice.start.saturating_add(slice.len);
        let mut storage = slice
            .storage
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *storage
            .get_mut(absolute)
            .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
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
        values.extend_from_slice(
            storage
                .get(slice.start..end)
                .unwrap_or_else(|| slice_bounds_out_of_range()),
        );
    }
    values.push(value);
    values.resize(capacity, T::default());
    GoSlice {
        storage: Arc::new(RwLock::new(values)),
        start: 0,
        len: required,
        capacity,
        nil: false,
    }
}

pub fn append_slice<T: Clone + Default>(
    mut destination: GoSlice<T>,
    source: &GoSlice<T>,
) -> GoSlice<T> {
    let appended = {
        let storage = source
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = source.start.saturating_add(source.len);
        storage
            .get(source.start..end)
            .unwrap_or_else(|| slice_bounds_out_of_range())
            .to_vec()
    };
    if appended.is_empty() {
        return destination;
    }

    let required = destination.len.saturating_add(appended.len());
    if required <= destination.capacity {
        let start = destination.start.saturating_add(destination.len);
        let end = start.saturating_add(appended.len());
        let mut storage = destination
            .storage
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let target = storage
            .get_mut(start..end)
            .unwrap_or_else(|| slice_bounds_out_of_range());
        target.clone_from_slice(&appended);
        drop(storage);
        destination.len = required;
        return destination;
    }

    let capacity = destination.capacity.saturating_mul(2).max(required).max(1);
    let mut values = Vec::with_capacity(capacity);
    {
        let storage = destination
            .storage
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let end = destination.start.saturating_add(destination.len);
        values.extend_from_slice(
            storage
                .get(destination.start..end)
                .unwrap_or_else(|| slice_bounds_out_of_range()),
        );
    }
    values.extend(appended);
    values.resize(capacity, T::default());
    GoSlice {
        storage: Arc::new(RwLock::new(values)),
        start: 0,
        len: required,
        capacity,
        nil: false,
    }
}

pub fn copy<T: Clone>(destination: &GoSlice<T>, source: &GoSlice<T>) -> GoInt {
    let count = destination.len.min(source.len);
    let source_end = source.start.saturating_add(count);
    let destination_end = destination.start.saturating_add(count);

    if Arc::ptr_eq(&destination.storage, &source.storage) {
        let mut storage = destination
            .storage
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if source_end > storage.len() || destination_end > storage.len() {
            slice_bounds_out_of_range();
        }
        if destination.start <= source.start {
            for offset in 0..count {
                let value = storage
                    .get(source.start.saturating_add(offset))
                    .cloned()
                    .unwrap_or_else(|| slice_bounds_out_of_range());
                *storage
                    .get_mut(destination.start.saturating_add(offset))
                    .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
            }
        } else {
            for offset in (0..count).rev() {
                let value = storage
                    .get(source.start.saturating_add(offset))
                    .cloned()
                    .unwrap_or_else(|| slice_bounds_out_of_range());
                *storage
                    .get_mut(destination.start.saturating_add(offset))
                    .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
            }
        }
    } else {
        {
            let storage = source
                .storage
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if source_end > storage.len() {
                slice_bounds_out_of_range();
            }
        }
        {
            let storage = destination
                .storage
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if destination_end > storage.len() {
                slice_bounds_out_of_range();
            }
        }
        for offset in 0..count {
            let value = source
                .storage
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(source.start.saturating_add(offset))
                .cloned()
                .unwrap_or_else(|| slice_bounds_out_of_range());
            *destination
                .storage
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_mut(destination.start.saturating_add(offset))
                .unwrap_or_else(|| slice_bounds_out_of_range()) = value;
        }
    }
    GoInt::try_from(count).unwrap_or_else(|_| slice_bounds_out_of_range())
}

pub fn clear<T: Clone + Default>(slice: &GoSlice<T>) {
    let mut storage = slice
        .storage
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = slice.start.saturating_add(slice.len);
    let values = storage
        .get_mut(slice.start..end)
        .unwrap_or_else(|| slice_bounds_out_of_range());
    values.fill(T::default());
    drop(storage);
}
