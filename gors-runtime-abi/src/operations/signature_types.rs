//! Shared parameter lists for canonical primitive and runtime signatures.

use super::RuntimeType;

pub(super) const NO_PARAMETERS: &[RuntimeType] = &[];
pub(super) const BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::ByteSlice];
pub(super) const STATIC_BYTES_PARAMETER: &[RuntimeType] = &[RuntimeType::StaticByteSlice];
pub(super) const BOOL_PARAMETER: &[RuntimeType] = &[RuntimeType::Bool];
pub(super) const TWO_BOOL_PARAMETERS: &[RuntimeType] = &[RuntimeType::Bool, RuntimeType::Bool];
pub(super) const I64_PARAMETER: &[RuntimeType] = &[RuntimeType::I64];
pub(super) const TWO_I64_PARAMETERS: &[RuntimeType] = &[RuntimeType::I64, RuntimeType::I64];
pub(super) const F64_PARAMETER: &[RuntimeType] = &[RuntimeType::F64];
pub(super) const TWO_F64_PARAMETERS: &[RuntimeType] = &[RuntimeType::F64, RuntimeType::F64];
pub(super) const COMPLEX128_PARAMETER: &[RuntimeType] = &[RuntimeType::Complex128];
pub(super) const TWO_COMPLEX128_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::Complex128, RuntimeType::Complex128];
pub(super) const GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoString];
pub(super) const TWO_GO_STRING_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoString];
pub(super) const STATIC_I64_SLICE_PARAMETER: &[RuntimeType] = &[RuntimeType::StaticI64Slice];
pub(super) const GO_SLICE_I64_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoSliceI64, RuntimeType::I64];
pub(super) const GO_SLICE_I64_RANGE: &[RuntimeType] = &[
    RuntimeType::GoSliceI64,
    RuntimeType::I64,
    RuntimeType::I64,
    RuntimeType::I64,
];
pub(super) const GO_SLICE_I64_SET: &[RuntimeType] =
    &[RuntimeType::GoSliceI64, RuntimeType::I64, RuntimeType::I64];
pub(super) const GO_SLICE_I64_PARAMETER: &[RuntimeType] = &[RuntimeType::GoSliceI64];
pub(super) const TWO_GO_SLICE_I64_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoSliceI64, RuntimeType::GoSliceI64];
pub(super) const GO_SLICE_U8_PARAMETER: &[RuntimeType] = &[RuntimeType::GoSliceU8];
pub(super) const GO_SLICE_BOOL_PARAMETER: &[RuntimeType] = &[RuntimeType::GoSliceBool];
pub(super) const GO_SLICE_U8_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoSliceU8, RuntimeType::I64];
pub(super) const GO_SLICE_U8_RANGE: &[RuntimeType] = &[
    RuntimeType::GoSliceU8,
    RuntimeType::I64,
    RuntimeType::I64,
    RuntimeType::I64,
];
pub(super) const GO_SLICE_U8_SET: &[RuntimeType] =
    &[RuntimeType::GoSliceU8, RuntimeType::I64, RuntimeType::I64];
pub(super) const TWO_GO_SLICE_U8_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoSliceU8, RuntimeType::GoSliceU8];
pub(super) const GO_SLICE_U8_AND_STRING: &[RuntimeType] =
    &[RuntimeType::GoSliceU8, RuntimeType::GoString];
pub(super) const GO_STRING_AND_INDEX: &[RuntimeType] = &[RuntimeType::GoString, RuntimeType::I64];
pub(super) const GO_STRING_RANGE: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::I64, RuntimeType::I64];
pub(super) const GO_MAP_STRING_I64_PARAMETER: &[RuntimeType] = &[RuntimeType::GoMapStringI64];
pub(super) const GO_MAP_STRING_I64_AND_KEY: &[RuntimeType] =
    &[RuntimeType::GoMapStringI64, RuntimeType::GoString];
pub(super) const GO_MAP_STRING_I64_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoMapStringI64, RuntimeType::I64];
pub(super) const GO_MAP_STRING_I64_SET: &[RuntimeType] = &[
    RuntimeType::GoMapStringI64,
    RuntimeType::GoString,
    RuntimeType::I64,
];
pub(super) const GO_MAP_I64_GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoMapI64GoString];
pub(super) const GO_MAP_I64_GO_STRING_AND_KEY: &[RuntimeType] =
    &[RuntimeType::GoMapI64GoString, RuntimeType::I64];
pub(super) const GO_MAP_I64_GO_STRING_SET: &[RuntimeType] = &[
    RuntimeType::GoMapI64GoString,
    RuntimeType::I64,
    RuntimeType::GoString,
];
pub(super) const GO_POINTER_I64_PARAMETER: &[RuntimeType] = &[RuntimeType::GoPointerI64];
pub(super) const TWO_GO_POINTER_I64_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoPointerI64, RuntimeType::GoPointerI64];
pub(super) const GO_POINTER_I64_SET: &[RuntimeType] =
    &[RuntimeType::GoPointerI64, RuntimeType::I64];
pub(super) const GO_POINTER_STRUCT_I64_PARAMETER: &[RuntimeType] =
    &[RuntimeType::GoPointerStructI64];
pub(super) const TWO_GO_POINTER_STRUCT_I64_PARAMETERS: &[RuntimeType] = &[
    RuntimeType::GoPointerStructI64,
    RuntimeType::GoPointerStructI64,
];
pub(super) const GO_POINTER_STRUCT_I64_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoPointerStructI64, RuntimeType::I64];
pub(super) const GO_POINTER_STRUCT_I64_SET: &[RuntimeType] = &[
    RuntimeType::GoPointerStructI64,
    RuntimeType::I64,
    RuntimeType::I64,
];
pub(super) const GO_INTERFACE_PARAMETER: &[RuntimeType] = &[RuntimeType::GoInterface];
pub(super) const GO_PANIC_PAYLOAD_PARAMETER: &[RuntimeType] = &[RuntimeType::GoPanicPayload];
pub(super) const TWO_GO_INTERFACE_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoInterface, RuntimeType::GoInterface];
pub(super) const GO_INTERFACE_AND_TYPE: &[RuntimeType] =
    &[RuntimeType::GoInterface, RuntimeType::GoString];
pub(super) const GO_INTERFACE_AND_TYPE_AND_INDEX: &[RuntimeType] = &[
    RuntimeType::GoInterface,
    RuntimeType::GoString,
    RuntimeType::I64,
];
pub(super) const GO_INTERFACE_BOX_BOOL: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::Bool];
pub(super) const GO_INTERFACE_BOX_I64: &[RuntimeType] = &[RuntimeType::GoString, RuntimeType::I64];
pub(super) const GO_INTERFACE_BOX_F64: &[RuntimeType] = &[RuntimeType::GoString, RuntimeType::F64];
pub(super) const GO_INTERFACE_BOX_STRING: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoString];
pub(super) const GO_INTERFACE_BOX_STRUCT_I64: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoSliceI64];
pub(super) const GO_INTERFACE_BOX_POINTER_STRUCT_I64: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoPointerStructI64];
pub(super) const GO_INTERFACE_BOX_POINTER_I64: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoPointerI64];
pub(super) const GO_INTERFACE_BOX_AGGREGATE: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoSliceInterface];
pub(super) const STATIC_BOOL_SLICE_PARAMETER: &[RuntimeType] = &[RuntimeType::StaticBoolSlice];
pub(super) const GO_SLICE_BOOL_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoSliceBool, RuntimeType::I64];
pub(super) const GO_SLICE_BOOL_SET: &[RuntimeType] = &[
    RuntimeType::GoSliceBool,
    RuntimeType::I64,
    RuntimeType::Bool,
];
pub(super) const GO_SLICE_INTERFACE_PARAMETER: &[RuntimeType] = &[RuntimeType::GoSliceInterface];
pub(super) const TWO_GO_SLICE_INTERFACE_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoSliceInterface, RuntimeType::GoSliceInterface];
pub(super) const GO_SLICE_INTERFACE_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoSliceInterface, RuntimeType::I64];
pub(super) const GO_SLICE_INTERFACE_SET: &[RuntimeType] = &[
    RuntimeType::GoSliceInterface,
    RuntimeType::I64,
    RuntimeType::GoInterface,
];
pub(super) const GO_SLICE_GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoSliceGoString];
pub(super) const TWO_GO_SLICE_GO_STRING_PARAMETERS: &[RuntimeType] =
    &[RuntimeType::GoSliceGoString, RuntimeType::GoSliceGoString];
pub(super) const GO_SLICE_GO_STRING_AND_INDEX: &[RuntimeType] =
    &[RuntimeType::GoSliceGoString, RuntimeType::I64];
pub(super) const GO_SLICE_GO_STRING_RANGE: &[RuntimeType] = &[
    RuntimeType::GoSliceGoString,
    RuntimeType::I64,
    RuntimeType::I64,
    RuntimeType::I64,
];
pub(super) const GO_SLICE_GO_STRING_SET: &[RuntimeType] = &[
    RuntimeType::GoSliceGoString,
    RuntimeType::I64,
    RuntimeType::GoString,
];
pub(super) const GO_SLICE_GO_STRING_AND_VALUE: &[RuntimeType] =
    &[RuntimeType::GoSliceGoString, RuntimeType::GoString];
pub(super) const GO_INTERFACE_BOX_GO_SLICE_GO_STRING: &[RuntimeType] =
    &[RuntimeType::GoString, RuntimeType::GoSliceGoString];
pub(super) const GO_MAP_STRING_INTERFACE_PARAMETER: &[RuntimeType] =
    &[RuntimeType::GoMapStringInterface];
pub(super) const GO_MAP_STRING_INTERFACE_AND_KEY: &[RuntimeType] =
    &[RuntimeType::GoMapStringInterface, RuntimeType::GoString];
pub(super) const GO_MAP_STRING_INTERFACE_SET: &[RuntimeType] = &[
    RuntimeType::GoMapStringInterface,
    RuntimeType::GoString,
    RuntimeType::GoInterface,
];
pub(super) const GO_CHANNEL_I64_PARAMETER: &[RuntimeType] = &[RuntimeType::GoChannelI64];
pub(super) const GO_CHANNEL_I64_SEND: &[RuntimeType] =
    &[RuntimeType::GoChannelI64, RuntimeType::I64];
pub(super) const GO_CHANNEL_GO_STRING_PARAMETER: &[RuntimeType] = &[RuntimeType::GoChannelGoString];
pub(super) const GO_CHANNEL_GO_STRING_SEND: &[RuntimeType] =
    &[RuntimeType::GoChannelGoString, RuntimeType::GoString];
pub(super) const GO_CHANNEL_GO_CHANNEL_I64_PARAMETER: &[RuntimeType] =
    &[RuntimeType::GoChannelGoChannelI64];
pub(super) const GO_CHANNEL_GO_CHANNEL_I64_SEND: &[RuntimeType] = &[
    RuntimeType::GoChannelGoChannelI64,
    RuntimeType::GoChannelI64,
];
