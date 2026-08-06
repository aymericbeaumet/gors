//! Runtime representation for Go interface values.

use std::sync::Arc;

use crate::{
    GoInt, GoPointerStructI64, GoSliceI64, GoSliceInterface, GoString, go_slice_i64_index,
    go_slice_i64_len,
};

/// A Go interface pairs one concrete dynamic type with a copied dynamic value.
///
/// The representation is intentionally opaque. Struct payloads are immutable
/// snapshots, while pointer payloads retain their shared pointee identity.
#[derive(Clone, Debug, Default)]
pub struct GoInterface(Option<DynamicValue>);

#[derive(Clone, Debug)]
struct DynamicValue {
    type_identity: GoString,
    payload: InterfacePayload,
}

#[derive(Clone, Debug)]
enum InterfacePayload {
    Bool(bool),
    I64(GoInt),
    GoString(GoString),
    StructI64(Arc<[GoInt]>),
    PointerStructI64(GoPointerStructI64),
    Aggregate(GoSliceInterface),
}

/// Construct the nil interface value, which has no dynamic type.
#[must_use]
pub fn go_interface_nil() -> GoInterface {
    GoInterface::default()
}

/// Copy a boolean value into an interface with its exact dynamic type.
#[must_use]
pub fn go_interface_box_bool(type_identity: GoString, value: bool) -> GoInterface {
    boxed(type_identity, InterfacePayload::Bool(value))
}

/// Copy an integer value into an interface with its exact dynamic type.
#[must_use]
pub fn go_interface_box_i64(type_identity: GoString, value: GoInt) -> GoInterface {
    boxed(type_identity, InterfacePayload::I64(value))
}

/// Copy a string header into an interface with its exact dynamic type.
#[must_use]
pub fn go_interface_box_go_string(type_identity: GoString, value: GoString) -> GoInterface {
    boxed(type_identity, InterfacePayload::GoString(value))
}

/// Snapshot an integer-field struct into an interface.
#[must_use]
pub fn go_interface_box_struct_i64(type_identity: GoString, fields: GoSliceI64) -> GoInterface {
    let length = go_slice_i64_len(fields.clone());
    let length = usize::try_from(length).unwrap_or_else(|_| interface_struct_bounds());
    let values = (0..length)
        .map(|index| {
            let index = GoInt::try_from(index).unwrap_or_else(|_| interface_struct_bounds());
            go_slice_i64_index(fields.clone(), index)
        })
        .collect::<Vec<_>>();
    boxed(
        type_identity,
        InterfacePayload::StructI64(Arc::from(values)),
    )
}

/// Copy a pointer header into an interface while preserving pointee identity.
#[must_use]
pub fn go_interface_box_pointer_struct_i64(
    type_identity: GoString,
    value: GoPointerStructI64,
) -> GoInterface {
    boxed(type_identity, InterfacePayload::PointerStructI64(value))
}

/// Copy an interface-backed aggregate header into an interface.
///
/// Struct lowering supplies a fresh slice of tagged field snapshots. Slice
/// lowering supplies the original header so clones retain the shared backing
/// array and its Go alias identity.
#[must_use]
pub fn go_interface_box_aggregate(type_identity: GoString, value: GoSliceInterface) -> GoInterface {
    boxed(type_identity, InterfacePayload::Aggregate(value))
}

/// Report whether an interface has no dynamic type.
#[must_use]
pub fn go_interface_is_nil(value: GoInterface) -> bool {
    value.0.is_none()
}

/// Test an interface's exact dynamic type without inspecting its payload.
#[must_use]
pub fn go_interface_is_type(value: GoInterface, type_identity: GoString) -> bool {
    value
        .0
        .is_some_and(|dynamic| dynamic.type_identity == type_identity)
}

/// Extract a boolean after checking the exact dynamic type.
#[must_use]
pub fn go_interface_unbox_bool(value: GoInterface, type_identity: GoString) -> bool {
    match checked_payload(value, type_identity) {
        InterfacePayload::Bool(value) => value,
        _ => type_assertion_failure(),
    }
}

/// Extract an integer after checking the exact dynamic type.
#[must_use]
pub fn go_interface_unbox_i64(value: GoInterface, type_identity: GoString) -> GoInt {
    match checked_payload(value, type_identity) {
        InterfacePayload::I64(value) => value,
        _ => type_assertion_failure(),
    }
}

/// Extract a string after checking the exact dynamic type.
#[must_use]
pub fn go_interface_unbox_go_string(value: GoInterface, type_identity: GoString) -> GoString {
    match checked_payload(value, type_identity) {
        InterfacePayload::GoString(value) => value,
        _ => type_assertion_failure(),
    }
}

/// Read one field from an integer-field struct stored in an interface.
#[must_use]
pub fn go_interface_struct_i64_get(
    value: GoInterface,
    type_identity: GoString,
    field: GoInt,
) -> GoInt {
    let InterfacePayload::StructI64(fields) = checked_payload(value, type_identity) else {
        type_assertion_failure();
    };
    let field = usize::try_from(field).unwrap_or_else(|_| interface_struct_bounds());
    *fields
        .get(field)
        .unwrap_or_else(|| interface_struct_bounds())
}

/// Extract a pointer-to-integer-struct after checking its dynamic type.
#[must_use]
pub fn go_interface_unbox_pointer_struct_i64(
    value: GoInterface,
    type_identity: GoString,
) -> GoPointerStructI64 {
    match checked_payload(value, type_identity) {
        InterfacePayload::PointerStructI64(value) => value,
        _ => type_assertion_failure(),
    }
}

/// Extract an interface-backed aggregate after checking its exact dynamic type.
#[must_use]
pub fn go_interface_unbox_aggregate(
    value: GoInterface,
    type_identity: GoString,
) -> GoSliceInterface {
    match checked_payload(value, type_identity) {
        InterfacePayload::Aggregate(value) => value,
        _ => type_assertion_failure(),
    }
}

fn boxed(type_identity: GoString, payload: InterfacePayload) -> GoInterface {
    GoInterface(Some(DynamicValue {
        type_identity,
        payload,
    }))
}

fn checked_payload(value: GoInterface, type_identity: GoString) -> InterfacePayload {
    let Some(dynamic) = value.0 else {
        type_assertion_failure();
    };
    if dynamic.type_identity != type_identity {
        type_assertion_failure();
    }
    dynamic.payload
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary for a failed assertion.
fn type_assertion_failure() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: interface conversion failed"))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is a checked runtime interface-field boundary.
fn interface_struct_bounds() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: struct field index out of range"))
}
