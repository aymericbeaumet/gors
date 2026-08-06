use super::*;

#[test]
fn strings_preserve_arbitrary_bytes() {
    let bytes = [b'a', 0, 0xff, 0x80, b'z'];
    let value = go_string_from_bytes(&bytes);

    assert_eq!(value.as_bytes(), bytes);
    assert_eq!(go_string_len(value.clone()), 5);
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
fn maps_preserve_nil_and_shared_reference_semantics() {
    let nil_map = go_map_string_i64_nil();
    let missing = go_string_from_static(b"missing");
    assert!(go_map_string_i64_is_nil(nil_map.clone()));
    assert_eq!(go_map_string_i64_len(nil_map.clone()), 0);
    assert_eq!(go_map_string_i64_get(nil_map.clone(), missing.clone()), 0);
    assert!(!go_map_string_i64_contains(
        nil_map.clone(),
        missing.clone()
    ));
    go_map_string_i64_delete(nil_map.clone(), missing);
    go_map_string_i64_clear(nil_map.clone());
    assert!(
        std::panic::catch_unwind(|| {
            go_map_string_i64_set(nil_map, go_string_from_static(b"key"), 1);
        })
        .is_err()
    );

    let original = go_map_string_i64_make();
    let alias = original.clone();
    go_map_string_i64_set(alias, go_string_from_static(b"value"), 42);
    assert_eq!(
        go_map_string_i64_get(original.clone(), go_string_from_static(b"value")),
        42
    );
    assert_eq!(go_map_string_i64_len(original.clone()), 1);
    assert_eq!(go_map_string_i64_key_at(original, 0).as_bytes(), b"value");
}

#[test]
fn maps_delete_clear_and_order_keys_by_bytes() {
    let map = go_map_string_i64_make();
    go_map_string_i64_set(map.clone(), go_string_from_static(b"b"), 2);
    go_map_string_i64_set(map.clone(), go_string_from_static(b"a"), 1);
    assert_eq!(go_map_string_i64_key_at(map.clone(), 0).as_bytes(), b"a");
    assert_eq!(go_map_string_i64_key_at(map.clone(), 1).as_bytes(), b"b");

    go_map_string_i64_delete(map.clone(), go_string_from_static(b"a"));
    assert!(!go_map_string_i64_contains(
        map.clone(),
        go_string_from_static(b"a")
    ));
    go_map_string_i64_clear(map.clone());
    assert_eq!(go_map_string_i64_len(map), 0);
}

#[test]
fn maps_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoMapStringI64>();
}

#[test]
fn pointers_preserve_nil_and_shared_pointee_semantics() {
    let nil_pointer = go_pointer_i64_nil();
    assert!(go_pointer_i64_is_nil(nil_pointer.clone()));
    assert!(std::panic::catch_unwind(|| go_pointer_i64_get(nil_pointer.clone())).is_err());
    assert!(std::panic::catch_unwind(|| go_pointer_i64_set(nil_pointer, 1)).is_err());

    let pointer = go_pointer_i64_new();
    let alias = pointer.clone();
    assert!(!go_pointer_i64_is_nil(pointer.clone()));
    assert_eq!(go_pointer_i64_get(pointer.clone()), 0);
    go_pointer_i64_set(alias, 42);
    assert_eq!(go_pointer_i64_get(pointer), 42);
}

#[test]
fn pointers_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoPointerI64>();
}

#[test]
fn channels_preserve_buffer_close_and_comma_ok_semantics() {
    let nil = go_channel_i64_nil();
    assert!(go_channel_i64_is_nil(nil.clone()));
    assert_eq!(go_channel_i64_len(nil.clone()), 0);
    assert_eq!(go_channel_i64_cap(nil), 0);

    let channel = go_channel_i64_make(2);
    assert!(!go_channel_i64_is_nil(channel.clone()));
    assert_eq!(go_channel_i64_cap(channel.clone()), 2);
    go_channel_i64_send(channel.clone(), 11);
    go_channel_i64_send(channel.clone(), 22);
    assert_eq!(go_channel_i64_len(channel.clone()), 2);

    go_channel_i64_close(channel.clone());
    assert_eq!(go_channel_i64_receive(channel.clone()), (11, true));
    assert_eq!(go_channel_i64_receive_value(channel.clone()), 22);
    assert_eq!(go_channel_i64_receive(channel.clone()), (0, false));
    assert!(std::panic::catch_unwind(|| go_channel_i64_send(channel.clone(), 33)).is_err());
    assert!(std::panic::catch_unwind(|| go_channel_i64_close(channel)).is_err());
}

#[test]
fn unbuffered_channels_rendezvous_between_threads() {
    let channel = go_channel_i64_make(0);
    let sender = channel.clone();
    let task = std::thread::spawn(move || go_channel_i64_send(sender, 42));

    assert_eq!(go_channel_i64_receive(channel), (42, true));
    assert!(task.join().is_ok());
}

#[test]
fn closing_channels_wakes_waiting_receivers() {
    let channel = go_channel_i64_make(0);
    let receiver = channel.clone();
    let task = std::thread::spawn(move || go_channel_i64_receive(receiver));

    go_channel_i64_close(channel);
    assert!(matches!(task.join(), Ok((0, false))));
}

#[test]
fn channel_creation_and_close_validate_panics() {
    assert!(std::panic::catch_unwind(|| go_channel_i64_make(-1)).is_err());
    assert!(std::panic::catch_unwind(|| go_channel_i64_close(go_channel_i64_nil())).is_err());
}

#[test]
fn channels_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoChannelI64>();
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
fn integer_slice_append_reuses_or_detaches_by_capacity() {
    let base = go_slice_i64_make(2, 4);
    go_slice_i64_set(base.clone(), 0, 1);
    go_slice_i64_set(base.clone(), 1, 2);

    let shared = go_slice_i64_append(go_slice_i64_range(base.clone(), 0, 1, -1), 9);
    assert_eq!(go_slice_i64_len(shared.clone()), 2);
    assert_eq!(go_slice_i64_cap(shared), 4);
    assert_eq!(go_slice_i64_index(base.clone(), 1), 9);

    let limited = go_slice_i64_append(go_slice_i64_range(base.clone(), 0, 1, 1), 7);
    go_slice_i64_set(limited.clone(), 0, 8);
    assert!(go_slice_i64_cap(limited.clone()) >= 2);
    assert_eq!(go_slice_i64_index(limited, 1), 7);
    assert_eq!(go_slice_i64_index(base, 0), 1);
}

#[test]
fn byte_slice_spreads_and_string_conversion_preserve_bytes() {
    let bytes = go_slice_u8_from_static(b"go");
    let bytes = go_slice_u8_append_string(bytes, go_string_from_static(b"rs"));
    let bytes = go_slice_u8_append_slice(bytes, go_slice_u8_from_static(b"!?"));

    assert_eq!(go_string_from_slice_u8(bytes).as_bytes(), b"gors!?");
}

#[test]
fn byte_slice_copy_uses_the_shorter_visible_length() {
    let destination = go_slice_u8_from_static(b"\0\0\0\0\0");

    assert_eq!(
        go_slice_u8_copy_string(destination.clone(), go_string_from_static(b"hello!")),
        5
    );
    assert_eq!(go_string_from_slice_u8(destination).as_bytes(), b"hello");
}

#[test]
fn integer_slice_copy_is_overlap_safe() {
    let values = go_slice_i64_from_static(&[1, 2, 3, 4]);
    let destination = go_slice_i64_range(values.clone(), 1, 4, -1);
    let source = go_slice_i64_range(values.clone(), 0, 3, -1);

    assert_eq!(go_slice_i64_copy(destination, source), 3);
    assert_eq!(go_slice_i64_index(values.clone(), 0), 1);
    assert_eq!(go_slice_i64_index(values.clone(), 1), 1);
    assert_eq!(go_slice_i64_index(values.clone(), 2), 2);
    assert_eq!(go_slice_i64_index(values, 3), 3);
}

#[test]
fn clear_updates_only_the_visible_integer_slice() {
    let values = go_slice_i64_from_static(&[1, 2, 3, 4]);
    go_slice_i64_clear(go_slice_i64_range(values.clone(), 1, 3, -1));

    assert_eq!(go_slice_i64_index(values.clone(), 0), 1);
    assert_eq!(go_slice_i64_index(values.clone(), 1), 0);
    assert_eq!(go_slice_i64_index(values.clone(), 2), 0);
    assert_eq!(go_slice_i64_index(values, 3), 4);
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
