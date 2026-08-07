use super::*;

mod integer;
mod maps;

#[test]
fn strings_preserve_arbitrary_bytes() {
    let bytes = [b'a', 0, 0xff, 0x80, b'z'];
    let value = go_string_from_bytes(&bytes);

    assert_eq!(value.as_bytes(), bytes);
    assert_eq!(go_string_len(value.clone()), 5);
    assert_eq!(value.clone(), value);
}

#[test]
fn byte_slices_and_strings_preserve_bounds_and_ranges() {
    let bytes = go_slice_u8_from_static(&[0xff, b'a', b'b']);
    assert_eq!(go_slice_u8_len(bytes.clone()), 3);
    assert_eq!(go_slice_u8_index(bytes.clone(), 0), 0xff);
    let tail = go_slice_u8_range(bytes, 1, -1, -1);
    assert_eq!(go_slice_u8_len(tail.clone()), 2);
    assert_eq!(go_slice_u8_index(tail, 0), GoInt::from(b'a'));

    let string = go_string_from_static(&[0xff, b'a', b'b']);
    assert_eq!(go_string_index(string.clone(), 0), 0xff);
    assert_eq!(go_string_range(string, 1, -1).as_bytes(), b"ab");
}

#[test]
fn byte_slices_and_strings_reject_invalid_bounds() {
    let bytes = go_slice_u8_from_static(&[1]);
    assert!(std::panic::catch_unwind(|| go_slice_u8_index(bytes.clone(), 1)).is_err());
    assert!(std::panic::catch_unwind(|| go_slice_u8_range(bytes, 0, 2, -1)).is_err());

    let string = go_string_from_static(b"x");
    assert!(std::panic::catch_unwind(|| go_string_index(string.clone(), 1)).is_err());
    assert!(std::panic::catch_unwind(|| go_string_range(string, 1, 0)).is_err());
}

#[test]
fn rune_slices_encode_and_string_ranges_decode_go_utf8() {
    const RUNES: &[GoInt] = &[65, 233, -1, 0xd800, 0x110000];
    let runes = go_slice_i64_from_static(RUNES);
    assert_eq!(
        go_string_from_slice_runes(runes).as_bytes(),
        "Aé���".as_bytes()
    );

    let value = go_string_from_static(&[0xff, b'a', 0xc3, 0xbf]);
    assert_eq!(go_string_range_count(value.clone()), 3);
    assert_eq!(go_string_range_index_at(value.clone(), 0), 0);
    assert_eq!(go_string_range_index_at(value.clone(), 1), 1);
    assert_eq!(go_string_range_index_at(value.clone(), 2), 2);
    assert_eq!(go_string_range_rune_at(value.clone(), 0), 0xfffd);
    assert_eq!(
        go_string_range_rune_at(value.clone(), 1),
        GoInt::from(u32::from('a'))
    );
    assert_eq!(
        go_string_range_rune_at(value, 2),
        GoInt::from(u32::from('ÿ'))
    );
}

#[test]
fn individual_runes_encode_as_go_strings() {
    assert_eq!(go_string_from_rune(255).as_bytes(), "ÿ".as_bytes());
    assert_eq!(go_string_from_rune(-1).as_bytes(), "�".as_bytes());
    assert_eq!(go_string_from_rune(0xd800).as_bytes(), "�".as_bytes());
    assert_eq!(
        go_string_from_rune(0x10_ffff).as_bytes(),
        "\u{10ffff}".as_bytes()
    );
    assert_eq!(go_string_from_rune(0x11_0000).as_bytes(), "�".as_bytes());
}

#[test]
fn string_range_access_rejects_invalid_ordinals() {
    let value = go_string_from_static(b"x");
    assert!(std::panic::catch_unwind(|| go_string_range_index_at(value.clone(), -1)).is_err());
    assert!(std::panic::catch_unwind(|| go_string_range_rune_at(value, 1)).is_err());
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
    assert_send_sync::<GoSliceGoString>();
}

#[test]
fn string_slices_preserve_zero_values_aliasing_ranges_and_overlap() {
    let slice = go_slice_go_string_make(2, 4);
    assert_eq!(go_slice_go_string_len(slice.clone()), 2);
    assert_eq!(go_slice_go_string_cap(slice.clone()), 4);
    assert_eq!(go_slice_go_string_index(slice.clone(), 0).as_bytes(), b"");

    let alias = slice.clone();
    go_slice_go_string_set(slice.clone(), 0, go_string_from_static(b"a"));
    assert_eq!(go_slice_go_string_index(alias, 0).as_bytes(), b"a");

    let appended = go_slice_go_string_append(slice.clone(), go_string_from_static(b"b"));
    assert_eq!(go_slice_go_string_len(appended.clone()), 3);
    assert_eq!(
        go_slice_go_string_index(appended.clone(), 2).as_bytes(),
        b"b"
    );

    let tail = go_slice_go_string_range(appended.clone(), 1, 3, -1);
    go_slice_go_string_set(tail, 0, go_string_from_static(b"c"));
    assert_eq!(
        go_slice_go_string_index(appended.clone(), 1).as_bytes(),
        b"c"
    );

    assert_eq!(
        go_slice_go_string_copy(appended.clone(), appended.clone()),
        3
    );
    let shifted = go_slice_go_string_range(appended.clone(), 1, 3, -1);
    assert_eq!(go_slice_go_string_copy(shifted, appended.clone()), 2);
    assert_eq!(
        go_slice_go_string_index(appended.clone(), 1).as_bytes(),
        b"a"
    );
    assert_eq!(
        go_slice_go_string_index(appended.clone(), 2).as_bytes(),
        b"c"
    );

    go_slice_go_string_clear(appended.clone());
    for index in 0..go_slice_go_string_len(appended.clone()) {
        assert_eq!(
            go_slice_go_string_index(appended.clone(), index).as_bytes(),
            b""
        );
    }
    assert!(go_slice_go_string_is_nil(go_slice_go_string_nil()));
    assert!(!go_slice_go_string_is_nil(slice));
}

#[test]
fn string_slice_interfaces_preserve_headers_and_reject_comparison() {
    let identity = go_string_from_static(b"slice:builtin:string");
    let slice = go_slice_go_string_make(1, 1);
    go_slice_go_string_set(slice.clone(), 0, go_string_from_static(b"value"));
    let boxed = go_interface_box_go_slice_go_string(identity.clone(), slice.clone());
    let unboxed = go_interface_unbox_go_slice_go_string(boxed.clone(), identity);
    go_slice_go_string_set(unboxed, 0, go_string_from_static(b"updated"));
    assert_eq!(go_slice_go_string_index(slice, 0).as_bytes(), b"updated");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = go_interface_equal(boxed.clone(), boxed);
        }))
        .is_err()
    );
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
fn struct_pointers_preserve_identity_fields_and_nil_panics() {
    let nil_pointer = go_pointer_struct_i64_nil();
    assert!(go_pointer_struct_i64_is_nil(nil_pointer.clone()));
    assert!(
        std::panic::catch_unwind(|| { go_pointer_struct_i64_get(nil_pointer.clone(), 0) }).is_err()
    );

    let pointer = go_pointer_struct_i64_new(2);
    let alias = pointer.clone();
    let distinct = go_pointer_struct_i64_new(2);
    assert!(go_pointer_struct_i64_equal(pointer.clone(), alias.clone()));
    assert!(!go_pointer_struct_i64_equal(pointer.clone(), distinct));
    go_pointer_struct_i64_set(alias, 1, 42);
    assert_eq!(go_pointer_struct_i64_get(pointer, 1), 42);
}

#[test]
fn pointers_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoPointerI64>();
    assert_send_sync::<GoPointerStructI64>();
}

#[test]
fn interfaces_preserve_dynamic_types_and_value_copying() {
    let int_type = go_string_from_static(b"builtin:int");
    let string_type = go_string_from_static(b"builtin:string");
    let boxed_int = go_interface_box_i64(int_type.clone(), 42);
    assert!(!go_interface_is_nil(boxed_int.clone()));
    assert!(go_interface_is_type(boxed_int.clone(), int_type.clone()));
    assert!(!go_interface_is_type(
        boxed_int.clone(),
        string_type.clone()
    ));
    assert_eq!(go_interface_unbox_i64(boxed_int, int_type), 42);

    let boxed_string =
        go_interface_box_go_string(string_type.clone(), go_string_from_static(b"interface"));
    assert_eq!(
        go_interface_unbox_go_string(boxed_string, string_type).as_bytes(),
        b"interface"
    );

    let nil = go_interface_nil();
    assert!(go_interface_is_nil(nil.clone()));
    assert!(!go_interface_is_type(
        nil,
        go_string_from_static(b"builtin:int")
    ));
}

#[test]
fn float_interfaces_preserve_ieee_equality_and_checked_extraction() {
    let float_type = go_string_from_static(b"builtin:float64");
    let nan = go_interface_box_f64(float_type.clone(), f64::NAN);
    assert!(!go_interface_equal(nan.clone(), nan));

    let positive_zero = go_interface_box_f64(float_type.clone(), 0.0);
    let negative_zero = go_interface_box_f64(float_type.clone(), -0.0);
    assert!(go_interface_equal(
        positive_zero.clone(),
        negative_zero.clone()
    ));
    assert_eq!(
        go_interface_unbox_f64(negative_zero, float_type.clone()).to_bits(),
        (-0.0_f64).to_bits()
    );

    let integer = go_interface_box_i64(go_string_from_static(b"builtin:int"), 0);
    assert!(!go_interface_equal(positive_zero, integer));
    assert!(
        std::panic::catch_unwind(|| {
            let _ = go_interface_unbox_f64(
                go_interface_box_i64(go_string_from_static(b"builtin:int"), 1),
                float_type,
            );
        })
        .is_err()
    );
}

#[test]
fn interfaces_snapshot_structs_and_preserve_pointer_identity() {
    let struct_type = go_string_from_static(b"named:counter");
    let fields = go_slice_i64_from_static(&[1, 2]);
    let boxed_struct = go_interface_box_struct_i64(struct_type.clone(), fields.clone());
    go_slice_i64_set(fields, 1, 99);
    assert_eq!(
        go_interface_struct_i64_get(boxed_struct, struct_type.clone(), 1),
        2
    );

    let pointer = go_pointer_struct_i64_new(1);
    let boxed_pointer = go_interface_box_pointer_struct_i64(struct_type.clone(), pointer.clone());
    let unboxed = go_interface_unbox_pointer_struct_i64(boxed_pointer, struct_type);
    go_pointer_struct_i64_set(unboxed, 0, 7);
    assert_eq!(go_pointer_struct_i64_get(pointer, 0), 7);
}

#[test]
fn interfaces_preserve_interface_backed_aggregate_headers() {
    let aggregate_type = go_string_from_static(b"slice:named:node");
    let int_type = go_string_from_static(b"builtin:int");
    let aggregate = go_slice_interface_make(1, 1);
    go_slice_interface_set(
        aggregate.clone(),
        0,
        go_interface_box_i64(int_type.clone(), 2),
    );

    let boxed = go_interface_box_aggregate(aggregate_type.clone(), aggregate.clone());
    let unboxed = go_interface_unbox_aggregate(boxed.clone(), aggregate_type);
    go_slice_interface_set(unboxed, 0, go_interface_box_i64(int_type.clone(), 7));

    assert_eq!(
        go_interface_unbox_i64(go_slice_interface_index(aggregate, 0), int_type),
        7
    );
    assert!(
        std::panic::catch_unwind(|| {
            go_interface_unbox_aggregate(boxed, go_string_from_static(b"slice:named:other"))
        })
        .is_err()
    );
}

#[test]
fn interface_unboxing_checks_the_exact_dynamic_type() {
    let value = go_interface_box_bool(go_string_from_static(b"builtin:bool"), true);
    assert!(go_interface_unbox_bool(
        value.clone(),
        go_string_from_static(b"builtin:bool")
    ));
    assert!(
        std::panic::catch_unwind(|| {
            let _ = go_interface_unbox_bool(value, go_string_from_static(b"named:bool"));
        })
        .is_err()
    );
}

#[test]
fn interfaces_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoInterface>();
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
fn string_channels_preserve_fifo_close_and_zero_value_semantics() {
    let channel = go_channel_go_string_make(3);
    go_channel_go_string_send(channel.clone(), go_string_from_static(b"a"));
    go_channel_go_string_send(channel.clone(), go_string_from_static(b"b"));
    go_channel_go_string_send(channel.clone(), go_string_from_static(b"c"));
    assert_eq!(go_channel_go_string_len(channel.clone()), 3);
    assert_eq!(go_channel_go_string_cap(channel.clone()), 3);

    go_channel_go_string_close(channel.clone());
    assert_eq!(
        go_channel_go_string_receive_value(channel.clone()).as_bytes(),
        b"a"
    );
    let (second, second_ok) = go_channel_go_string_receive(channel.clone());
    let (third, third_ok) = go_channel_go_string_receive(channel.clone());
    let (zero, zero_ok) = go_channel_go_string_receive(channel);
    assert_eq!(second.as_bytes(), b"b");
    assert!(second_ok);
    assert_eq!(third.as_bytes(), b"c");
    assert!(third_ok);
    assert!(zero.as_bytes().is_empty());
    assert!(!zero_ok);
}

#[test]
fn nested_channels_preserve_shared_inner_identity_and_nil_zero_value() {
    let inner = go_channel_i64_make(1);
    go_channel_i64_send(inner.clone(), 42);
    let outer = go_channel_go_channel_i64_make(1);
    go_channel_go_channel_i64_send(outer.clone(), inner);

    let received = go_channel_go_channel_i64_receive_value(outer.clone());
    assert_eq!(go_channel_i64_receive_value(received), 42);

    go_channel_go_channel_i64_close(outer.clone());
    let (zero, open) = go_channel_go_channel_i64_receive(outer);
    assert!(!open);
    assert!(go_channel_i64_is_nil(zero));
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
    assert_send_sync::<GoChannelGoString>();
    assert_send_sync::<GoChannelGoChannelI64>();
}

#[test]
fn channel_select_operations_report_readiness_without_blocking() {
    let nil = go_channel_i64_nil();
    assert!(!go_channel_i64_try_send(nil.clone(), 1));
    assert_eq!(go_channel_i64_try_receive(nil), (0, 0));

    let channel = go_channel_i64_make(1);
    assert_eq!(go_channel_i64_try_receive(channel.clone()), (0, 0));
    assert!(go_channel_i64_try_send(channel.clone(), 7));
    assert!(!go_channel_i64_try_send(channel.clone(), 8));
    assert_eq!(go_channel_i64_try_receive(channel.clone()), (7, 2));
    go_channel_i64_close(channel.clone());
    assert_eq!(go_channel_i64_try_receive(channel.clone()), (0, 1));
    assert!(std::panic::catch_unwind(|| go_channel_i64_try_send(channel, 9)).is_err());
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
fn boolean_slices_preserve_shared_mutable_storage() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoSliceBool>();
    let values = go_slice_bool_from_static(&[false, true]);
    let alias = values.clone();
    go_slice_bool_set(alias, 0, true);
    assert!(go_slice_bool_index(values.clone(), 0));
    assert!(go_slice_bool_index(values, 1));
}

#[test]
fn interface_backed_containers_preserve_tagged_values_and_identity() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoSliceInterface>();
    assert_send_sync::<GoMapStringInterface>();

    let identity = go_string_from_static(b"named:point");
    let value = go_interface_box_i64(identity.clone(), 7);
    let slice = go_slice_interface_make(1, 1);
    go_slice_interface_set(slice.clone(), 0, value.clone());
    assert_eq!(go_slice_interface_len(slice.clone()), 1);
    assert_eq!(
        go_interface_unbox_i64(go_slice_interface_index(slice, 0), identity.clone()),
        7
    );

    let map = go_map_string_interface_make();
    let key = go_string_from_static(b"point");
    go_map_string_interface_set(map.clone(), key.clone(), value);
    assert_eq!(go_map_string_interface_len(map.clone()), 1);
    assert!(go_map_string_interface_contains(map.clone(), key.clone()));
    assert_eq!(
        go_interface_unbox_i64(go_map_string_interface_get(map, key), identity),
        7
    );
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
fn slice_headers_preserve_nil_identity_until_allocation() {
    let integers = go_slice_i64_nil();
    assert!(go_slice_i64_is_nil(integers.clone()));
    assert!(go_slice_i64_is_nil(go_slice_i64_range(
        integers.clone(),
        0,
        0,
        -1,
    )));
    assert!(!go_slice_i64_is_nil(go_slice_i64_append(integers, 1)));
    assert!(!go_slice_i64_is_nil(go_slice_i64_make(0, 0)));

    assert!(go_slice_u8_is_nil(go_slice_u8_nil()));
    assert!(!go_slice_u8_is_nil(go_slice_u8_from_static(b"")));
    assert!(go_slice_bool_is_nil(go_slice_bool_nil()));
    assert!(!go_slice_bool_is_nil(go_slice_bool_from_static(&[])));
    assert!(go_slice_interface_is_nil(go_slice_interface_nil()));
    assert!(!go_slice_interface_is_nil(go_slice_interface_make(0, 0)));
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
fn byte_slice_make_set_and_copy_preserve_capacity_aliasing_and_overlap() {
    let values = go_slice_u8_make(4, 6);
    assert_eq!(go_slice_u8_len(values.clone()), 4);
    go_slice_u8_set(values.clone(), 0, GoInt::from(b'a'));
    go_slice_u8_set(values.clone(), 1, GoInt::from(b'b'));
    go_slice_u8_set(values.clone(), 2, GoInt::from(b'c'));
    go_slice_u8_set(values.clone(), 3, GoInt::from(b'd'));

    let destination = go_slice_u8_range(values.clone(), 1, 4, -1);
    let source = go_slice_u8_range(values.clone(), 0, 3, -1);
    assert_eq!(go_slice_u8_copy(destination, source), 3);
    assert_eq!(go_string_from_slice_u8(values.clone()).as_bytes(), b"aabc");

    assert_eq!(go_slice_u8_copy(go_slice_u8_nil(), values.clone()), 0);
    assert_eq!(go_slice_u8_copy(values, go_slice_u8_nil()), 0);
    assert!(!go_slice_u8_is_nil(go_slice_u8_make(0, 0)));
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

#[test]
fn interface_panics_round_trip_through_the_recovery_payload_boundary() {
    let identity = go_string_from_static(b"builtin:int");
    let value = go_interface_box_i64(identity.clone(), 42);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic_go_interface(value);
    }))
    .err();
    assert!(payload.is_some(), "an explicit interface panic must unwind");
    let Some(payload) = payload else {
        return;
    };
    let recovered = go_panic_payload_to_interface(payload);

    assert!(!go_interface_is_nil(recovered.clone()));
    assert_eq!(go_interface_unbox_i64(recovered, identity), 42);
}

#[test]
fn scalar_panics_become_exact_recoverable_interface_values() {
    let boolean = std::panic::catch_unwind(|| panic_bool(true)).err();
    assert!(boolean.is_some(), "an explicit bool panic must unwind");
    if let Some(boolean) = boolean {
        assert!(go_interface_unbox_bool(
            go_panic_payload_to_interface(boolean),
            go_string_from_static(b"builtin:bool"),
        ));
    }

    let integer = std::panic::catch_unwind(|| panic_i64(42)).err();
    assert!(integer.is_some(), "an explicit int panic must unwind");
    if let Some(integer) = integer {
        assert_eq!(
            go_interface_unbox_i64(
                go_panic_payload_to_interface(integer),
                go_string_from_static(b"builtin:int"),
            ),
            42,
        );
    }

    let string = std::panic::catch_unwind(|| panic_go_string(go_string_from_static(b"boom"))).err();
    assert!(string.is_some(), "an explicit string panic must unwind");
    if let Some(string) = string {
        assert_eq!(
            go_interface_unbox_go_string(
                go_panic_payload_to_interface(string),
                go_string_from_static(b"builtin:string"),
            )
            .as_bytes(),
            b"boom",
        );
    }
}

#[test]
fn nil_and_implicit_panics_become_non_nil_runtime_errors() {
    let nil_payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic_go_interface(go_interface_nil());
    }))
    .err();
    assert!(
        nil_payload.is_some(),
        "panic(nil) must unwind with a replacement value"
    );
    let Some(nil_payload) = nil_payload else {
        return;
    };
    let recovered_nil = go_panic_payload_to_interface(nil_payload);
    assert!(!go_interface_is_nil(recovered_nil.clone()));
    assert!(go_interface_is_runtime_error(recovered_nil));

    let implicit =
        go_panic_payload_to_interface(Box::new("implicit runtime panic") as GoPanicPayload);
    assert!(!go_interface_is_nil(implicit.clone()));
    assert!(go_interface_is_runtime_error(implicit));
}
