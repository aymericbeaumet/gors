use std::sync::Arc;

use super::super::*;

#[test]
fn integer_slice_append_grows_before_writing_any_element() {
    let destination = go_slice_i64_make(1, 2);
    go_slice_i64_set(destination.clone(), 0, 1);
    let old_backing = go_slice_i64_range(destination.clone(), 0, 2, -1);
    go_slice_i64_set(old_backing.clone(), 1, 99);

    let source = go_slice_i64_from_static(&[2, 3]);
    let result = go_slice_i64_append_slice(destination, source);

    assert_eq!(go_slice_i64_len(result.clone()), 3);
    assert_eq!(go_slice_i64_index(result.clone(), 0), 1);
    assert_eq!(go_slice_i64_index(result.clone(), 1), 2);
    assert_eq!(go_slice_i64_index(result, 2), 3);
    assert_eq!(go_slice_i64_index(old_backing, 1), 99);
}

#[test]
fn integer_slice_append_snapshots_overlapping_source() {
    let backing = go_slice_i64_make(4, 8);
    for (index, value) in [1, 2, 3, 4].into_iter().enumerate() {
        go_slice_i64_set(backing.clone(), index as GoInt, value);
    }
    let destination = go_slice_i64_range(backing.clone(), 1, 3, -1);
    let source = go_slice_i64_range(backing, 0, 4, -1);
    let result = go_slice_i64_append_slice(destination, source);

    let values = (0..go_slice_i64_len(result.clone()))
        .map(|index| go_slice_i64_index(result.clone(), index))
        .collect::<Vec<_>>();
    assert_eq!(values, [2, 3, 1, 2, 3, 4]);
}

#[test]
fn empty_slice_append_preserves_the_exact_destination_header() {
    let backing = go_slice_i64_make(2, 5);
    let destination = go_slice_i64_range(backing, 1, 2, 4);
    let storage = destination.storage.clone();
    let start = destination.start;
    let capacity = destination.capacity;
    let result = go_slice_i64_append_slice(destination, go_slice_i64_nil());

    assert!(Arc::ptr_eq(&result.storage, &storage));
    assert_eq!(result.start, start);
    assert_eq!(result.capacity, capacity);
    assert!(!result.nil);

    let nil = go_slice_i64_append_slice(go_slice_i64_nil(), go_slice_i64_nil());
    assert!(go_slice_i64_is_nil(nil));
}

#[test]
fn interface_slice_append_preserves_mixed_tagged_values() {
    let source = go_slice_interface_make(3, 3);
    go_slice_interface_set(
        source.clone(),
        0,
        go_interface_box_i64(go_string_from_static(b"builtin:int"), 42),
    );
    go_slice_interface_set(
        source.clone(),
        1,
        go_interface_box_f64(go_string_from_static(b"builtin:float64"), 1.25),
    );
    go_slice_interface_set(
        source.clone(),
        2,
        go_interface_box_go_string(
            go_string_from_static(b"builtin:string"),
            go_string_from_static(b"foo"),
        ),
    );

    let result = go_slice_interface_append(go_slice_interface_nil(), source);
    assert_eq!(go_slice_interface_len(result.clone()), 3);
    assert_eq!(
        go_interface_unbox_i64(
            go_slice_interface_index(result.clone(), 0),
            go_string_from_static(b"builtin:int"),
        ),
        42,
    );
    assert_eq!(
        go_interface_unbox_f64(
            go_slice_interface_index(result.clone(), 1),
            go_string_from_static(b"builtin:float64"),
        ),
        1.25,
    );
    assert_eq!(
        go_interface_unbox_go_string(
            go_slice_interface_index(result, 2),
            go_string_from_static(b"builtin:string"),
        )
        .as_bytes(),
        b"foo",
    );
}
