use super::*;

#[test]
fn string_map_range_keys_are_a_fixed_candidate_snapshot() {
    let map = go_map_string_i64_make();
    go_map_string_i64_set(map.clone(), go_string_from_static(b"b"), 2);
    go_map_string_i64_set(map.clone(), go_string_from_static(b"a"), 1);

    let keys = go_map_string_i64_range_keys(map.clone());
    go_map_string_i64_delete(map.clone(), go_string_from_static(b"a"));
    go_map_string_i64_set(map.clone(), go_string_from_static(b"c"), 3);

    assert_eq!(go_slice_go_string_len(keys.clone()), 2);
    assert_eq!(go_slice_go_string_index(keys.clone(), 0).as_bytes(), b"a");
    assert_eq!(go_slice_go_string_index(keys, 1).as_bytes(), b"b");
    assert!(!go_map_string_i64_contains(
        map.clone(),
        go_string_from_static(b"a")
    ));
    assert!(go_map_string_i64_contains(map, go_string_from_static(b"c")));
}

#[test]
fn int_string_maps_preserve_nil_identity_and_mutation() {
    let nil_map = go_map_i64_go_string_nil();
    assert!(go_map_i64_go_string_is_nil(nil_map.clone()));
    assert_eq!(go_map_i64_go_string_len(nil_map.clone()), 0);
    assert_eq!(go_map_i64_go_string_get(nil_map.clone(), 1).as_bytes(), b"");
    assert!(!go_map_i64_go_string_contains(nil_map.clone(), 1));
    go_map_i64_go_string_delete(nil_map.clone(), 1);
    go_map_i64_go_string_clear(nil_map.clone());
    assert!(
        std::panic::catch_unwind(|| {
            go_map_i64_go_string_set(nil_map, 1, go_string_from_static(b"x"));
        })
        .is_err()
    );

    let map = go_map_i64_go_string_make();
    let alias = map.clone();
    go_map_i64_go_string_set(alias, 1, go_string_from_static(b"x"));
    assert_eq!(go_map_i64_go_string_get(map.clone(), 1).as_bytes(), b"x");
    assert!(go_map_i64_go_string_contains(map.clone(), 1));
    assert_eq!(go_map_i64_go_string_len(map.clone()), 1);
    go_map_i64_go_string_delete(map.clone(), 1);
    assert!(!go_map_i64_go_string_contains(map.clone(), 1));
    go_map_i64_go_string_set(map.clone(), 2, go_string_from_static(b"y"));
    go_map_i64_go_string_clear(map.clone());
    assert_eq!(go_map_i64_go_string_len(map), 0);
}

#[test]
fn int_string_map_snapshot_defers_live_membership_and_value_reads() {
    let map = go_map_i64_go_string_make();
    go_map_i64_go_string_set(map.clone(), 1, go_string_from_static(b"old"));
    go_map_i64_go_string_set(map.clone(), 2, go_string_from_static(b"deleted"));

    let keys = go_map_i64_go_string_range_keys(map.clone());
    go_map_i64_go_string_set(map.clone(), 1, go_string_from_static(b"updated"));
    go_map_i64_go_string_delete(map.clone(), 2);
    go_map_i64_go_string_set(map.clone(), 3, go_string_from_static(b"new"));

    assert_eq!(go_slice_i64_len(keys.clone()), 2);
    assert_eq!(go_slice_i64_index(keys.clone(), 0), 1);
    assert_eq!(go_slice_i64_index(keys, 1), 2);
    assert_eq!(
        go_map_i64_go_string_get(map.clone(), 1).as_bytes(),
        b"updated"
    );
    assert!(!go_map_i64_go_string_contains(map.clone(), 2));
    assert!(go_map_i64_go_string_contains(map, 3));
}

#[test]
fn concrete_maps_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<GoMapStringI64>();
    assert_send_sync::<GoMapI64GoString>();
}
