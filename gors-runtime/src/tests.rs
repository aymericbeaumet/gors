use super::*;

#[test]
fn strings_preserve_arbitrary_bytes() {
    let bytes = [b'a', 0, 0xff, 0x80, b'z'];
    let value = go_string_from_bytes(&bytes);

    assert_eq!(value.as_bytes(), bytes);
    assert_eq!(value.clone(), value);
}

#[test]
#[allow(clippy::panic)]
fn string_clones_share_backing_storage() {
    let value = go_string_from_bytes(b"shared");
    let clone = value.clone();

    let (StringStorage::Shared(value_storage), StringStorage::Shared(clone_storage)) =
        (&value.storage, &clone.storage)
    else {
        panic!("dynamic strings should use shared storage");
    };
    assert!(Arc::ptr_eq(value_storage, clone_storage));
}

#[test]
fn static_and_dynamic_strings_share_value_semantics() {
    use std::collections::hash_map::DefaultHasher;

    fn hash(value: &GoString) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    let static_value = go_string_from_static(&[0xff, 0, b'a']);
    let dynamic_value = go_string_from_bytes(&[0xff, 0, b'a']);
    let greater = go_string_from_static(&[0xff, 0, b'b']);

    assert_eq!(static_value, dynamic_value);
    assert_eq!(static_value.cmp(&dynamic_value), Ordering::Equal);
    assert!(dynamic_value < greater);
    assert_eq!(hash(&static_value), hash(&dynamic_value));
}

#[test]
fn strings_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoString>();
}

#[test]
fn integer_slices_share_backing_storage_across_reslices() {
    let values = go_slice_i64_from_static(&[1, 2, 3]);
    let alias = go_slice_i64_range(values.clone(), 1, -1, -1);

    go_slice_i64_set(alias.clone(), 0, 9);
    assert_eq!(go_slice_i64_index(values.clone(), 1), 9);

    go_slice_i64_set(values, 1, 7);
    assert_eq!(go_slice_i64_index(alias.clone(), 0), 7);
    assert_eq!(go_slice_i64_index(alias, 1), 3);
}

#[test]
fn integer_slices_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoSliceI64>();
}

#[test]
fn integer_slice_bounds_fail_at_the_runtime_boundary() {
    let values = go_slice_i64_from_static(&[1, 2, 3]);

    assert!(std::panic::catch_unwind(|| go_slice_i64_index(values.clone(), 3)).is_err());
    assert!(std::panic::catch_unwind(|| go_slice_i64_set(values.clone(), -1, 0)).is_err());
    assert!(std::panic::catch_unwind(|| go_slice_i64_range(values.clone(), 2, 1, -1)).is_err());
}

#[test]
fn static_strings_do_not_create_shared_heap_storage() {
    let value = go_string_from_static(b"literal");

    assert!(matches!(value.storage, StringStorage::Static(b"literal")));
    assert_eq!(value.as_bytes(), b"literal");
}

#[test]
fn concatenation_is_byte_exact() {
    let left = go_string_from_bytes(&[0xff, b'a']);
    let right = go_string_from_bytes(&[0, b'b']);

    assert_eq!(
        concat_go_strings(left, right).as_bytes(),
        [0xff, b'a', 0, b'b']
    );
}

#[test]
fn concatenation_reuses_a_unique_byte_buffer_when_capacity_permits() {
    let left = concat_go_strings(go_string_from_static(b"left"), go_string_from_static(b"-"));
    let before = left.as_bytes().as_ptr();

    let combined = concat_go_strings(left, go_string_from_static(b"x"));

    assert_eq!(combined.as_bytes().as_ptr(), before);
    assert_eq!(combined.as_bytes(), b"left-x");
}

#[test]
fn concatenation_does_not_mutate_a_shared_clone() {
    let left = go_string_from_bytes(b"left");
    let retained = left.clone();

    let combined = concat_go_strings(left, go_string_from_static(b"-right"));

    assert_eq!(retained.as_bytes(), b"left");
    assert_eq!(combined.as_bytes(), b"left-right");
}

#[test]
fn raw_output_does_not_require_utf8() {
    let value = go_string_from_bytes(&[b'x', 0xff]);
    let mut output = Vec::new();

    assert!(write_go_string_to(&mut output, &value).is_ok());

    assert_eq!(output, [b'x', 0xff]);
}

#[test]
fn runtime_integer_operations_match_go_edge_rules() {
    assert_eq!(int_div(GoInt::MIN, -1), GoInt::MIN);
    assert_eq!(int_rem(GoInt::MIN, -1), 0);
}

#[test]
fn int_shifts_do_not_use_rusts_masked_shift_count() {
    assert_eq!(int_shl(1, 63), GoInt::MIN);
    assert_eq!(int_shl(1, 64), 0);
    assert_eq!(int_shl(1, 10_000), 0);
    assert_eq!(int_shr(-2, 1), -1);
    assert_eq!(int_shr(-2, 64), -1);
    assert_eq!(int_shr(2, 64), 0);
}

#[test]
fn invalid_integer_operations_panic() {
    assert!(std::panic::catch_unwind(|| int_div(1, 0)).is_err());
    assert!(std::panic::catch_unwind(|| int_rem(1, 0)).is_err());
    assert!(std::panic::catch_unwind(|| int_shl(1, -1)).is_err());
    assert!(std::panic::catch_unwind(|| int_shr(1, -1)).is_err());
}

#[test]
fn explicit_panics_preserve_supported_payload_types() {
    let boolean = std::panic::catch_unwind(|| panic_bool(true));
    assert!(boolean.is_err());
    assert_eq!(
        boolean
            .err()
            .and_then(|value| value.downcast::<bool>().ok())
            .as_deref(),
        Some(&true)
    );

    let integer = std::panic::catch_unwind(|| panic_i64(42));
    assert!(integer.is_err());
    assert_eq!(
        integer
            .err()
            .and_then(|value| value.downcast::<GoInt>().ok())
            .as_deref(),
        Some(&42)
    );

    let string = std::panic::catch_unwind(|| panic_go_string(go_string_from_static(b"boom")));
    assert!(string.is_err());
    let payload = string
        .err()
        .and_then(|value| value.downcast::<GoString>().ok());
    assert_eq!(
        payload.as_deref().map(GoString::as_bytes),
        Some(b"boom".as_slice())
    );
}
