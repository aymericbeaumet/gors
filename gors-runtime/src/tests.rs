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
fn int_arithmetic_matches_go_overflow_rules() {
    assert_eq!(int_add(GoInt::MAX, 1), GoInt::MIN);
    assert_eq!(int_sub(GoInt::MIN, 1), GoInt::MAX);
    assert_eq!(int_mul(GoInt::MAX, 2), -2);
    assert_eq!(int_neg(GoInt::MIN), GoInt::MIN);
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
