//! Runtime value categories and shared operation metadata constants.

use crate::effects::GoPanicCondition;
use crate::target::{TargetCapability, TargetCapability::StandardIo};

/// Value categories supported at the typed runtime call boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeType {
    Unit,
    Bool,
    I64,
    GoString,
    ByteSlice,
    StaticByteSlice,
    F64,
    Complex128,
    GoSliceI64,
    StaticI64Slice,
    GoSliceU8,
    GoMapStringI64,
    GoPointerI64,
    GoChannelI64,
    /// ABI-only aggregate returned by comma-ok integer channel receive.
    I64BoolTuple,
    /// ABI-only aggregate returned by nonblocking integer channel receive.
    I64I64Tuple,
    GoPointerStructI64,
    GoInterface,
    StaticBoolSlice,
    GoSliceBool,
    GoSliceInterface,
    GoMapStringInterface,
    /// ABI-only opaque payload produced by Rust's unwind boundary.
    GoPanicPayload,
    GoChannelGoString,
    /// ABI-only aggregate returned by comma-ok string channel receive.
    GoStringBoolTuple,
    /// ABI-only aggregate returned by nonblocking string channel receive.
    GoStringI64Tuple,
    GoChannelGoChannelI64,
    /// ABI-only aggregate returned by comma-ok nested channel receive.
    GoChannelI64BoolTuple,
    /// ABI-only aggregate returned by nonblocking nested channel receive.
    GoChannelI64I64Tuple,
    GoSliceGoString,
}

/// Complete function signature for one runtime operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeSignature {
    pub(super) parameters: &'static [RuntimeType],
    pub(super) result: RuntimeType,
}

pub(super) const NO_CAPABILITIES: &[TargetCapability] = &[];
pub(super) const STANDARD_IO_CAPABILITY: &[TargetCapability] = &[StandardIo];
pub(super) const NO_GO_PANICS: &[GoPanicCondition] = &[];
pub(super) const INTEGER_DIVIDE_BY_ZERO: &[GoPanicCondition] =
    &[GoPanicCondition::IntegerDivideByZero];
pub(super) const NEGATIVE_SHIFT_AMOUNT: &[GoPanicCondition] =
    &[GoPanicCondition::NegativeShiftAmount];
pub(super) const EXPLICIT_PANIC: &[GoPanicCondition] = &[GoPanicCondition::ExplicitPanic];
pub(super) const INDEX_OUT_OF_RANGE: &[GoPanicCondition] = &[GoPanicCondition::IndexOutOfRange];
pub(super) const SLICE_BOUNDS_OUT_OF_RANGE: &[GoPanicCondition] =
    &[GoPanicCondition::SliceBoundsOutOfRange];
pub(super) const NIL_MAP_ASSIGNMENT: &[GoPanicCondition] = &[GoPanicCondition::NilMapAssignment];
pub(super) const NIL_POINTER_DEREFERENCE: &[GoPanicCondition] =
    &[GoPanicCondition::NilPointerDereference];
pub(super) const NIL_POINTER_OR_INDEX_OUT_OF_RANGE: &[GoPanicCondition] = &[
    GoPanicCondition::NilPointerDereference,
    GoPanicCondition::IndexOutOfRange,
];
pub(super) const NEGATIVE_CHANNEL_CAPACITY: &[GoPanicCondition] =
    &[GoPanicCondition::NegativeChannelCapacity];
pub(super) const SEND_ON_CLOSED_CHANNEL: &[GoPanicCondition] =
    &[GoPanicCondition::SendOnClosedChannel];
pub(super) const CLOSE_CHANNEL_PANICS: &[GoPanicCondition] = &[
    GoPanicCondition::CloseOfNilChannel,
    GoPanicCondition::CloseOfClosedChannel,
];
pub(super) const TYPE_ASSERTION_FAILURE: &[GoPanicCondition] =
    &[GoPanicCondition::TypeAssertionFailure];
pub(super) const TYPE_ASSERTION_OR_INDEX_OUT_OF_RANGE: &[GoPanicCondition] = &[
    GoPanicCondition::IndexOutOfRange,
    GoPanicCondition::TypeAssertionFailure,
];
pub(super) const UNCOMPARABLE_INTERFACE_COMPARISON: &[GoPanicCondition] =
    &[GoPanicCondition::UncomparableInterfaceComparison];
