//! Runtime representation for Go interface values.

use std::sync::Arc;

use crate::{
    GoInt, GoPointerI64, GoPointerStructI64, GoSliceGoString, GoSliceI64, GoSliceInterface,
    GoString, go_pointer_i64_equal, go_pointer_struct_i64_equal, go_slice_i64_index,
    go_slice_i64_len, go_slice_interface_index, go_slice_interface_len, go_string_from_bytes,
    go_string_from_static,
};

const RUNTIME_ERROR_TYPE_IDENTITY: &[u8] = b"runtime:gors-error";

/// Opaque payload crossing the Rust unwind boundary into Go recovery semantics.
pub type GoPanicPayload = Box<dyn std::any::Any + Send + 'static>;

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
    F64(f64),
    GoString(GoString),
    StructI64(Arc<[GoInt]>),
    PointerI64(GoPointerI64),
    PointerStructI64(GoPointerStructI64),
    SliceGoString(GoSliceGoString),
    Aggregate(GoSliceInterface),
    ComparableAggregate(GoSliceInterface),
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

/// Copy a floating-point value into an interface with its exact dynamic type.
#[must_use]
pub fn go_interface_box_f64(type_identity: GoString, value: f64) -> GoInterface {
    boxed(type_identity, InterfacePayload::F64(value))
}

/// Copy a string header into an interface with its exact dynamic type.
#[must_use]
pub fn go_interface_box_go_string(type_identity: GoString, value: GoString) -> GoInterface {
    boxed(type_identity, InterfacePayload::GoString(value))
}

/// Copy a string-slice header into an interface while preserving its backing array.
#[must_use]
pub fn go_interface_box_go_slice_go_string(
    type_identity: GoString,
    value: GoSliceGoString,
) -> GoInterface {
    boxed(type_identity, InterfacePayload::SliceGoString(value))
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

/// Copy a pointer-to-integer header into an interface while preserving pointee identity.
#[must_use]
pub fn go_interface_box_pointer_i64(type_identity: GoString, value: GoPointerI64) -> GoInterface {
    boxed(type_identity, InterfacePayload::PointerI64(value))
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

/// Copy a comparable aggregate snapshot into an interface.
#[must_use]
pub fn go_interface_box_comparable_aggregate(
    type_identity: GoString,
    value: GoSliceInterface,
) -> GoInterface {
    boxed(type_identity, InterfacePayload::ComparableAggregate(value))
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

/// Report whether an interface holds an implementation-provided runtime error.
#[must_use]
pub fn go_interface_is_runtime_error(value: GoInterface) -> bool {
    value.0.is_some_and(|dynamic| {
        dynamic.type_identity == go_string_from_static(RUNTIME_ERROR_TYPE_IDENTITY)
    })
}

/// Convert one opaque Rust unwind payload into its exact recoverable Go value.
#[must_use]
pub fn go_panic_payload_to_interface(payload: GoPanicPayload) -> GoInterface {
    let payload = match payload.downcast::<GoInterface>() {
        Ok(value) => return *value,
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<bool>() {
        Ok(value) => {
            return go_interface_box_bool(go_string_from_static(b"builtin:bool"), *value);
        }
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<GoInt>() {
        Ok(value) => {
            return go_interface_box_i64(go_string_from_static(b"builtin:int"), *value);
        }
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<GoString>() {
        Ok(value) => {
            return go_interface_box_go_string(go_string_from_static(b"builtin:string"), *value);
        }
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<&'static str>() {
        Ok(message) => return runtime_error(go_string_from_static(message.as_bytes())),
        Err(payload) => payload,
    };
    if let Ok(message) = payload.downcast::<String>() {
        return runtime_error(go_string_from_bytes(message.as_bytes()));
    }
    runtime_error(go_string_from_static(
        b"runtime error: unknown panic payload",
    ))
}

/// Raise an explicit Go panic carrying an already boxed interface value.
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
pub fn panic_go_interface(value: GoInterface) {
    let value = if value.0.is_none() {
        runtime_error(go_string_from_static(b"panic called with nil argument"))
    } else {
        value
    };
    std::panic::resume_unwind(Box::new(value))
}

/// Compare two interface values with Go's dynamic-type and comparability rules.
#[must_use]
pub fn go_interface_equal(left: GoInterface, right: GoInterface) -> bool {
    let (Some(left), Some(right)) = (&left.0, &right.0) else {
        return left.0.is_none() && right.0.is_none();
    };
    if left.type_identity != right.type_identity {
        return false;
    }
    match (&left.payload, &right.payload) {
        (InterfacePayload::Bool(left), InterfacePayload::Bool(right)) => left == right,
        (InterfacePayload::I64(left), InterfacePayload::I64(right)) => left == right,
        (InterfacePayload::F64(left), InterfacePayload::F64(right)) => left == right,
        (InterfacePayload::GoString(left), InterfacePayload::GoString(right)) => left == right,
        (InterfacePayload::StructI64(left), InterfacePayload::StructI64(right)) => left == right,
        (InterfacePayload::PointerI64(left), InterfacePayload::PointerI64(right)) => {
            go_pointer_i64_equal(left.clone(), right.clone())
        }
        (InterfacePayload::PointerStructI64(left), InterfacePayload::PointerStructI64(right)) => {
            go_pointer_struct_i64_equal(left.clone(), right.clone())
        }
        (InterfacePayload::SliceGoString(_), InterfacePayload::SliceGoString(_)) => {
            uncomparable_interface_comparison()
        }
        (
            InterfacePayload::ComparableAggregate(left),
            InterfacePayload::ComparableAggregate(right),
        ) => comparable_aggregate_equal(left, right),
        (InterfacePayload::Aggregate(_), InterfacePayload::Aggregate(_)) => {
            uncomparable_interface_comparison()
        }
        _ => false,
    }
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

/// Extract a floating-point value after checking the exact dynamic type.
#[must_use]
pub fn go_interface_unbox_f64(value: GoInterface, type_identity: GoString) -> f64 {
    match checked_payload(value, type_identity) {
        InterfacePayload::F64(value) => value,
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

/// Extract a string-slice header after checking its exact dynamic type.
#[must_use]
pub fn go_interface_unbox_go_slice_go_string(
    value: GoInterface,
    type_identity: GoString,
) -> GoSliceGoString {
    match checked_payload(value, type_identity) {
        InterfacePayload::SliceGoString(value) => value,
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

/// Extract a pointer-to-integer after checking its exact dynamic type.
#[must_use]
pub fn go_interface_unbox_pointer_i64(value: GoInterface, type_identity: GoString) -> GoPointerI64 {
    match checked_payload(value, type_identity) {
        InterfacePayload::PointerI64(value) => value,
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
        InterfacePayload::Aggregate(value) | InterfacePayload::ComparableAggregate(value) => value,
        _ => type_assertion_failure(),
    }
}

fn comparable_aggregate_equal(left: &GoSliceInterface, right: &GoSliceInterface) -> bool {
    let length = go_slice_interface_len(left.clone());
    if length != go_slice_interface_len(right.clone()) {
        return false;
    }
    (0..length).all(|index| {
        go_interface_equal(
            go_slice_interface_index(left.clone(), index),
            go_slice_interface_index(right.clone(), index),
        )
    })
}

fn boxed(type_identity: GoString, payload: InterfacePayload) -> GoInterface {
    GoInterface(Some(DynamicValue {
        type_identity,
        payload,
    }))
}

fn runtime_error(message: GoString) -> GoInterface {
    boxed(
        go_string_from_static(RUNTIME_ERROR_TYPE_IDENTITY),
        InterfacePayload::GoString(message),
    )
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
#[allow(clippy::panic)] // This is Go's interface comparison panic boundary.
fn uncomparable_interface_comparison() -> ! {
    std::panic::resume_unwind(Box::new(
        "runtime error: comparing uncomparable interface dynamic type",
    ))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is a checked runtime interface-field boundary.
fn interface_struct_bounds() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: struct field index out of range"))
}
