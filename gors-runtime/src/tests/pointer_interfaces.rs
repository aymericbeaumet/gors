use super::*;

#[test]
fn pointer_interfaces_preserve_typed_nil_and_exact_dynamic_identity() {
    let builtin = go_string_from_static(b"pointer:builtin:int");
    let named = go_string_from_static(b"pointer:named:test.Counter");
    let nil = go_pointer_i64_nil();
    let boxed = go_interface_box_pointer_i64(builtin.clone(), nil.clone());

    assert!(!go_interface_is_nil(boxed.clone()));
    assert!(go_interface_is_type(boxed.clone(), builtin.clone()));
    assert!(go_pointer_i64_is_nil(go_interface_unbox_pointer_i64(
        boxed.clone(),
        builtin.clone(),
    )));
    assert!(go_interface_equal(
        boxed.clone(),
        go_interface_box_pointer_i64(builtin, nil.clone()),
    ));
    assert!(!go_interface_equal(boxed.clone(), go_interface_nil()));
    assert!(!go_interface_equal(
        boxed,
        go_interface_box_pointer_i64(named, nil),
    ));
}

#[test]
fn pointer_interface_extraction_retains_alias_identity() {
    let identity = go_string_from_static(b"pointer:builtin:int");
    let pointer = go_pointer_i64_new();
    go_pointer_i64_set(pointer.clone(), 7);
    let boxed = go_interface_box_pointer_i64(identity.clone(), pointer.clone());
    let alias = go_interface_unbox_pointer_i64(boxed.clone(), identity.clone());

    assert!(go_interface_equal(
        boxed,
        go_interface_box_pointer_i64(identity.clone(), pointer.clone()),
    ));
    assert!(!go_interface_equal(
        go_interface_box_pointer_i64(identity.clone(), pointer.clone()),
        go_interface_box_pointer_i64(identity, go_pointer_i64_new()),
    ));
    go_pointer_i64_set(alias, 41);
    assert_eq!(go_pointer_i64_get(pointer), 41);
}

#[test]
fn pointer_interface_extraction_rejects_the_wrong_dynamic_type() {
    let builtin = go_string_from_static(b"pointer:builtin:int");
    let named = go_string_from_static(b"pointer:named:test.Counter");
    let boxed = go_interface_box_pointer_i64(builtin, go_pointer_i64_nil());

    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = go_interface_unbox_pointer_i64(boxed, named);
        }))
        .is_err()
    );
}

#[test]
fn pointer_interface_panics_round_trip_typed_nil() {
    let identity = go_string_from_static(b"pointer:builtin:int");
    let value = go_interface_box_pointer_i64(identity.clone(), go_pointer_i64_nil());
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic_go_interface(value);
    }));
    assert!(payload.is_err(), "a typed nil pointer panic must unwind");
    let Err(payload) = payload else {
        return;
    };
    let recovered = go_panic_payload_to_interface(payload);

    assert!(!go_interface_is_nil(recovered.clone()));
    assert!(go_pointer_i64_is_nil(go_interface_unbox_pointer_i64(
        recovered, identity,
    )));
}
