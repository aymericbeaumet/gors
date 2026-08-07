use gors_runtime::{
    GoChannelGoChannelI64, GoChannelGoString, GoChannelI64, GoInt, GoInterface, GoMapStringI64,
    GoMapStringInterface, GoPanicPayload, GoPointerI64, GoPointerStructI64, GoSliceBool,
    GoSliceGoString, GoSliceI64, GoSliceInterface, GoSliceU8, GoString, concat_go_strings,
    go_channel_go_channel_i64_cap, go_channel_go_channel_i64_close,
    go_channel_go_channel_i64_is_nil, go_channel_go_channel_i64_len,
    go_channel_go_channel_i64_make, go_channel_go_channel_i64_nil,
    go_channel_go_channel_i64_receive, go_channel_go_channel_i64_receive_value,
    go_channel_go_channel_i64_send, go_channel_go_channel_i64_try_receive,
    go_channel_go_channel_i64_try_send, go_channel_go_string_cap, go_channel_go_string_close,
    go_channel_go_string_is_nil, go_channel_go_string_len, go_channel_go_string_make,
    go_channel_go_string_nil, go_channel_go_string_receive, go_channel_go_string_receive_value,
    go_channel_go_string_send, go_channel_go_string_try_receive, go_channel_go_string_try_send,
    go_channel_i64_cap, go_channel_i64_close, go_channel_i64_is_nil, go_channel_i64_len,
    go_channel_i64_make, go_channel_i64_nil, go_channel_i64_receive, go_channel_i64_receive_value,
    go_channel_i64_send, go_channel_i64_try_receive, go_channel_i64_try_send,
    go_interface_box_aggregate, go_interface_box_bool, go_interface_box_comparable_aggregate,
    go_interface_box_f64, go_interface_box_go_slice_go_string, go_interface_box_go_string,
    go_interface_box_i64, go_interface_box_pointer_struct_i64, go_interface_box_struct_i64,
    go_interface_equal, go_interface_is_nil, go_interface_is_runtime_error, go_interface_is_type,
    go_interface_nil, go_interface_struct_i64_get, go_interface_unbox_aggregate,
    go_interface_unbox_bool, go_interface_unbox_f64, go_interface_unbox_go_slice_go_string,
    go_interface_unbox_go_string, go_interface_unbox_i64, go_interface_unbox_pointer_struct_i64,
    go_map_string_i64_clear, go_map_string_i64_contains, go_map_string_i64_delete,
    go_map_string_i64_get, go_map_string_i64_is_nil, go_map_string_i64_key_at,
    go_map_string_i64_len, go_map_string_i64_make, go_map_string_i64_nil, go_map_string_i64_set,
    go_map_string_interface_contains, go_map_string_interface_get, go_map_string_interface_len,
    go_map_string_interface_make, go_map_string_interface_set, go_panic_payload_to_interface,
    go_pointer_i64_get, go_pointer_i64_is_nil, go_pointer_i64_new, go_pointer_i64_nil,
    go_pointer_i64_set, go_pointer_struct_i64_equal, go_pointer_struct_i64_get,
    go_pointer_struct_i64_is_nil, go_pointer_struct_i64_new, go_pointer_struct_i64_nil,
    go_pointer_struct_i64_set, go_slice_bool_from_static, go_slice_bool_index,
    go_slice_bool_is_nil, go_slice_bool_nil, go_slice_bool_set, go_slice_go_string_append,
    go_slice_go_string_cap, go_slice_go_string_clear, go_slice_go_string_copy,
    go_slice_go_string_index, go_slice_go_string_is_nil, go_slice_go_string_len,
    go_slice_go_string_make, go_slice_go_string_nil, go_slice_go_string_range,
    go_slice_go_string_set, go_slice_i64_append, go_slice_i64_cap, go_slice_i64_clear,
    go_slice_i64_copy, go_slice_i64_from_static, go_slice_i64_index, go_slice_i64_is_nil,
    go_slice_i64_len, go_slice_i64_make, go_slice_i64_nil, go_slice_i64_range, go_slice_i64_set,
    go_slice_interface_index, go_slice_interface_is_nil, go_slice_interface_len,
    go_slice_interface_make, go_slice_interface_nil, go_slice_interface_set,
    go_slice_u8_append_slice, go_slice_u8_append_string, go_slice_u8_copy, go_slice_u8_copy_string,
    go_slice_u8_from_static, go_slice_u8_index, go_slice_u8_is_nil, go_slice_u8_len,
    go_slice_u8_make, go_slice_u8_nil, go_slice_u8_range, go_slice_u8_set, go_string_from_bytes,
    go_string_from_rune, go_string_from_slice_runes, go_string_from_slice_u8,
    go_string_from_static, go_string_index, go_string_len, go_string_range, go_string_range_count,
    go_string_range_index_at, go_string_range_rune_at, int_div, int_rem, int_shl, int_shr,
    panic_bool, panic_go_interface, panic_go_string, panic_i64, print_bool, print_f64,
    print_go_string, print_i64, print_newline, print_space,
};
use gors_runtime_abi::{RuntimeOp, RuntimeType};

struct RuntimeSurface {
    symbol: &'static str,
    parameters: &'static [RuntimeType],
    result: RuntimeType,
}

macro_rules! runtime_surface {
    ($function:ident, $function_type:ty, [$($parameter:expr),* $(,)?] -> $result:expr) => {{
        const IMPLEMENTATION: $function_type = $function;
        let _ = IMPLEMENTATION;
        RuntimeSurface {
            symbol: stringify!($function),
            parameters: &[$($parameter),*],
            result: $result,
        }
    }};
}

fn implementation_surface(operation: RuntimeOp) -> RuntimeSurface {
    match operation {
        RuntimeOp::GoStringFromBytes => runtime_surface!(
            go_string_from_bytes,
            fn(&[u8]) -> GoString,
            [RuntimeType::ByteSlice] -> RuntimeType::GoString
        ),
        RuntimeOp::GoStringFromRune => runtime_surface!(
            go_string_from_rune,
            fn(GoInt) -> GoString,
            [RuntimeType::I64] -> RuntimeType::GoString
        ),
        RuntimeOp::GoStringFromStatic => runtime_surface!(
            go_string_from_static,
            fn(&'static [u8]) -> GoString,
            [RuntimeType::StaticByteSlice] -> RuntimeType::GoString
        ),
        RuntimeOp::ConcatGoStrings => runtime_surface!(
            concat_go_strings,
            fn(GoString, GoString) -> GoString,
            [RuntimeType::GoString, RuntimeType::GoString] -> RuntimeType::GoString
        ),
        RuntimeOp::IntDiv => runtime_surface!(
            int_div,
            fn(GoInt, GoInt) -> GoInt,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::IntRem => runtime_surface!(
            int_rem,
            fn(GoInt, GoInt) -> GoInt,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::IntShl => runtime_surface!(
            int_shl,
            fn(GoInt, GoInt) -> GoInt,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::IntShr => runtime_surface!(
            int_shr,
            fn(GoInt, GoInt) -> GoInt,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::PrintBool => runtime_surface!(
            print_bool,
            fn(bool),
            [RuntimeType::Bool] -> RuntimeType::Unit
        ),
        RuntimeOp::PrintI64 => runtime_surface!(
            print_i64,
            fn(GoInt),
            [RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::PrintSpace => runtime_surface!(
            print_space,
            fn(),
            [] -> RuntimeType::Unit
        ),
        RuntimeOp::PrintNewline => runtime_surface!(
            print_newline,
            fn(),
            [] -> RuntimeType::Unit
        ),
        RuntimeOp::PrintGoString => runtime_surface!(
            print_go_string,
            fn(GoString),
            [RuntimeType::GoString] -> RuntimeType::Unit
        ),
        RuntimeOp::PanicBool => runtime_surface!(
            panic_bool,
            fn(bool),
            [RuntimeType::Bool] -> RuntimeType::Unit
        ),
        RuntimeOp::PanicI64 => runtime_surface!(
            panic_i64,
            fn(GoInt),
            [RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::PanicGoString => runtime_surface!(
            panic_go_string,
            fn(GoString),
            [RuntimeType::GoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceI64FromStatic => runtime_surface!(
            go_slice_i64_from_static,
            fn(&'static [GoInt]) -> GoSliceI64,
            [RuntimeType::StaticI64Slice] -> RuntimeType::GoSliceI64
        ),
        RuntimeOp::GoSliceI64Index => runtime_surface!(
            go_slice_i64_index,
            fn(GoSliceI64, GoInt) -> GoInt,
            [RuntimeType::GoSliceI64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceI64Range => runtime_surface!(
            go_slice_i64_range,
            fn(GoSliceI64, GoInt, GoInt, GoInt) -> GoSliceI64,
            [RuntimeType::GoSliceI64, RuntimeType::I64, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceI64
        ),
        RuntimeOp::GoSliceI64Set => runtime_surface!(
            go_slice_i64_set,
            fn(GoSliceI64, GoInt, GoInt),
            [RuntimeType::GoSliceI64, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceI64Make => runtime_surface!(
            go_slice_i64_make,
            fn(GoInt, GoInt) -> GoSliceI64,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceI64
        ),
        RuntimeOp::GoSliceI64Len => runtime_surface!(
            go_slice_i64_len,
            fn(GoSliceI64) -> GoInt,
            [RuntimeType::GoSliceI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceI64Cap => runtime_surface!(
            go_slice_i64_cap,
            fn(GoSliceI64) -> GoInt,
            [RuntimeType::GoSliceI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceBoolFromStatic => runtime_surface!(
            go_slice_bool_from_static,
            fn(&'static [bool]) -> GoSliceBool,
            [RuntimeType::StaticBoolSlice] -> RuntimeType::GoSliceBool
        ),
        RuntimeOp::GoSliceBoolIndex => runtime_surface!(
            go_slice_bool_index,
            fn(GoSliceBool, GoInt) -> bool,
            [RuntimeType::GoSliceBool, RuntimeType::I64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoSliceBoolSet => runtime_surface!(
            go_slice_bool_set,
            fn(GoSliceBool, GoInt, bool),
            [RuntimeType::GoSliceBool, RuntimeType::I64, RuntimeType::Bool] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceInterfaceMake => runtime_surface!(
            go_slice_interface_make,
            fn(GoInt, GoInt) -> GoSliceInterface,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceInterface
        ),
        RuntimeOp::GoSliceInterfaceLen => runtime_surface!(
            go_slice_interface_len,
            fn(GoSliceInterface) -> GoInt,
            [RuntimeType::GoSliceInterface] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceInterfaceIndex => runtime_surface!(
            go_slice_interface_index,
            fn(GoSliceInterface, GoInt) -> GoInterface,
            [RuntimeType::GoSliceInterface, RuntimeType::I64] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoSliceInterfaceSet => runtime_surface!(
            go_slice_interface_set,
            fn(GoSliceInterface, GoInt, GoInterface),
            [RuntimeType::GoSliceInterface, RuntimeType::I64, RuntimeType::GoInterface] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceI64Append => runtime_surface!(
            go_slice_i64_append,
            fn(GoSliceI64, GoInt) -> GoSliceI64,
            [RuntimeType::GoSliceI64, RuntimeType::I64] -> RuntimeType::GoSliceI64
        ),
        RuntimeOp::GoSliceU8FromStatic => runtime_surface!(
            go_slice_u8_from_static,
            fn(&'static [u8]) -> GoSliceU8,
            [RuntimeType::StaticByteSlice] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoSliceU8AppendSlice => runtime_surface!(
            go_slice_u8_append_slice,
            fn(GoSliceU8, GoSliceU8) -> GoSliceU8,
            [RuntimeType::GoSliceU8, RuntimeType::GoSliceU8] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoSliceU8AppendString => runtime_surface!(
            go_slice_u8_append_string,
            fn(GoSliceU8, GoString) -> GoSliceU8,
            [RuntimeType::GoSliceU8, RuntimeType::GoString] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoSliceU8CopyString => runtime_surface!(
            go_slice_u8_copy_string,
            fn(GoSliceU8, GoString) -> GoInt,
            [RuntimeType::GoSliceU8, RuntimeType::GoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceI64Clear => runtime_surface!(
            go_slice_i64_clear,
            fn(GoSliceI64),
            [RuntimeType::GoSliceI64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoStringFromSliceU8 => runtime_surface!(
            go_string_from_slice_u8,
            fn(GoSliceU8) -> GoString,
            [RuntimeType::GoSliceU8] -> RuntimeType::GoString
        ),
        RuntimeOp::GoSliceI64Copy => runtime_surface!(
            go_slice_i64_copy,
            fn(GoSliceI64, GoSliceI64) -> GoInt,
            [RuntimeType::GoSliceI64, RuntimeType::GoSliceI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoMapStringI64Nil => runtime_surface!(
            go_map_string_i64_nil,
            fn() -> GoMapStringI64,
            [] -> RuntimeType::GoMapStringI64
        ),
        RuntimeOp::GoMapStringI64Make => runtime_surface!(
            go_map_string_i64_make,
            fn() -> GoMapStringI64,
            [] -> RuntimeType::GoMapStringI64
        ),
        RuntimeOp::GoMapStringI64Len => runtime_surface!(
            go_map_string_i64_len,
            fn(GoMapStringI64) -> GoInt,
            [RuntimeType::GoMapStringI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoMapStringI64Get => runtime_surface!(
            go_map_string_i64_get,
            fn(GoMapStringI64, GoString) -> GoInt,
            [RuntimeType::GoMapStringI64, RuntimeType::GoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoMapStringI64Contains => runtime_surface!(
            go_map_string_i64_contains,
            fn(GoMapStringI64, GoString) -> bool,
            [RuntimeType::GoMapStringI64, RuntimeType::GoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoMapStringI64Set => runtime_surface!(
            go_map_string_i64_set,
            fn(GoMapStringI64, GoString, GoInt),
            [RuntimeType::GoMapStringI64, RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoMapStringI64Delete => runtime_surface!(
            go_map_string_i64_delete,
            fn(GoMapStringI64, GoString),
            [RuntimeType::GoMapStringI64, RuntimeType::GoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoMapStringI64Clear => runtime_surface!(
            go_map_string_i64_clear,
            fn(GoMapStringI64),
            [RuntimeType::GoMapStringI64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoMapStringI64IsNil => runtime_surface!(
            go_map_string_i64_is_nil,
            fn(GoMapStringI64) -> bool,
            [RuntimeType::GoMapStringI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoMapStringI64KeyAt => runtime_surface!(
            go_map_string_i64_key_at,
            fn(GoMapStringI64, GoInt) -> GoString,
            [RuntimeType::GoMapStringI64, RuntimeType::I64] -> RuntimeType::GoString
        ),
        RuntimeOp::GoMapStringInterfaceMake => runtime_surface!(
            go_map_string_interface_make,
            fn() -> GoMapStringInterface,
            [] -> RuntimeType::GoMapStringInterface
        ),
        RuntimeOp::GoMapStringInterfaceLen => runtime_surface!(
            go_map_string_interface_len,
            fn(GoMapStringInterface) -> GoInt,
            [RuntimeType::GoMapStringInterface] -> RuntimeType::I64
        ),
        RuntimeOp::GoMapStringInterfaceGet => runtime_surface!(
            go_map_string_interface_get,
            fn(GoMapStringInterface, GoString) -> GoInterface,
            [RuntimeType::GoMapStringInterface, RuntimeType::GoString] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoMapStringInterfaceContains => runtime_surface!(
            go_map_string_interface_contains,
            fn(GoMapStringInterface, GoString) -> bool,
            [RuntimeType::GoMapStringInterface, RuntimeType::GoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoMapStringInterfaceSet => runtime_surface!(
            go_map_string_interface_set,
            fn(GoMapStringInterface, GoString, GoInterface),
            [RuntimeType::GoMapStringInterface, RuntimeType::GoString, RuntimeType::GoInterface] -> RuntimeType::Unit
        ),
        RuntimeOp::GoPointerI64Nil => runtime_surface!(
            go_pointer_i64_nil,
            fn() -> GoPointerI64,
            [] -> RuntimeType::GoPointerI64
        ),
        RuntimeOp::GoPointerI64New => runtime_surface!(
            go_pointer_i64_new,
            fn() -> GoPointerI64,
            [] -> RuntimeType::GoPointerI64
        ),
        RuntimeOp::GoPointerI64Get => runtime_surface!(
            go_pointer_i64_get,
            fn(GoPointerI64) -> GoInt,
            [RuntimeType::GoPointerI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoPointerI64Set => runtime_surface!(
            go_pointer_i64_set,
            fn(GoPointerI64, GoInt),
            [RuntimeType::GoPointerI64, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoPointerI64IsNil => runtime_surface!(
            go_pointer_i64_is_nil,
            fn(GoPointerI64) -> bool,
            [RuntimeType::GoPointerI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoPointerStructI64Nil => runtime_surface!(
            go_pointer_struct_i64_nil,
            fn() -> GoPointerStructI64,
            [] -> RuntimeType::GoPointerStructI64
        ),
        RuntimeOp::GoPointerStructI64New => runtime_surface!(
            go_pointer_struct_i64_new,
            fn(GoInt) -> GoPointerStructI64,
            [RuntimeType::I64] -> RuntimeType::GoPointerStructI64
        ),
        RuntimeOp::GoPointerStructI64Get => runtime_surface!(
            go_pointer_struct_i64_get,
            fn(GoPointerStructI64, GoInt) -> GoInt,
            [RuntimeType::GoPointerStructI64, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoPointerStructI64Set => runtime_surface!(
            go_pointer_struct_i64_set,
            fn(GoPointerStructI64, GoInt, GoInt),
            [RuntimeType::GoPointerStructI64, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoPointerStructI64IsNil => runtime_surface!(
            go_pointer_struct_i64_is_nil,
            fn(GoPointerStructI64) -> bool,
            [RuntimeType::GoPointerStructI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoPointerStructI64Equal => runtime_surface!(
            go_pointer_struct_i64_equal,
            fn(GoPointerStructI64, GoPointerStructI64) -> bool,
            [RuntimeType::GoPointerStructI64, RuntimeType::GoPointerStructI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceNil => runtime_surface!(
            go_interface_nil,
            fn() -> GoInterface,
            [] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceBoxBool => runtime_surface!(
            go_interface_box_bool,
            fn(GoString, bool) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::Bool] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceBoxI64 => runtime_surface!(
            go_interface_box_i64,
            fn(GoString, GoInt) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceBoxGoString => runtime_surface!(
            go_interface_box_go_string,
            fn(GoString, GoString) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoString] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceBoxStructI64 => runtime_surface!(
            go_interface_box_struct_i64,
            fn(GoString, GoSliceI64) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoSliceI64] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceBoxPointerStructI64 => runtime_surface!(
            go_interface_box_pointer_struct_i64,
            fn(GoString, GoPointerStructI64) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoPointerStructI64] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceIsNil => runtime_surface!(
            go_interface_is_nil,
            fn(GoInterface) -> bool,
            [RuntimeType::GoInterface] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceIsType => runtime_surface!(
            go_interface_is_type,
            fn(GoInterface, GoString) -> bool,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceUnboxBool => runtime_surface!(
            go_interface_unbox_bool,
            fn(GoInterface, GoString) -> bool,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceUnboxI64 => runtime_surface!(
            go_interface_unbox_i64,
            fn(GoInterface, GoString) -> GoInt,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoInterfaceUnboxGoString => runtime_surface!(
            go_interface_unbox_go_string,
            fn(GoInterface, GoString) -> GoString,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::GoString
        ),
        RuntimeOp::GoInterfaceStructI64Get => runtime_surface!(
            go_interface_struct_i64_get,
            fn(GoInterface, GoString, GoInt) -> GoInt,
            [RuntimeType::GoInterface, RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoInterfaceUnboxPointerStructI64 => runtime_surface!(
            go_interface_unbox_pointer_struct_i64,
            fn(GoInterface, GoString) -> GoPointerStructI64,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::GoPointerStructI64
        ),
        RuntimeOp::GoChannelI64Nil => runtime_surface!(
            go_channel_i64_nil,
            fn() -> GoChannelI64,
            [] -> RuntimeType::GoChannelI64
        ),
        RuntimeOp::GoChannelI64Make => runtime_surface!(
            go_channel_i64_make,
            fn(GoInt) -> GoChannelI64,
            [RuntimeType::I64] -> RuntimeType::GoChannelI64
        ),
        RuntimeOp::GoChannelI64Len => runtime_surface!(
            go_channel_i64_len,
            fn(GoChannelI64) -> GoInt,
            [RuntimeType::GoChannelI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelI64Cap => runtime_surface!(
            go_channel_i64_cap,
            fn(GoChannelI64) -> GoInt,
            [RuntimeType::GoChannelI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelI64Send => runtime_surface!(
            go_channel_i64_send,
            fn(GoChannelI64, GoInt),
            [RuntimeType::GoChannelI64, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelI64ReceiveValue => runtime_surface!(
            go_channel_i64_receive_value,
            fn(GoChannelI64) -> GoInt,
            [RuntimeType::GoChannelI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelI64Receive => runtime_surface!(
            go_channel_i64_receive,
            fn(GoChannelI64) -> (GoInt, bool),
            [RuntimeType::GoChannelI64] -> RuntimeType::I64BoolTuple
        ),
        RuntimeOp::GoChannelI64Close => runtime_surface!(
            go_channel_i64_close,
            fn(GoChannelI64),
            [RuntimeType::GoChannelI64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelI64IsNil => runtime_surface!(
            go_channel_i64_is_nil,
            fn(GoChannelI64) -> bool,
            [RuntimeType::GoChannelI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoStringLen => runtime_surface!(
            go_string_len,
            fn(GoString) -> GoInt,
            [RuntimeType::GoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelI64TrySend => runtime_surface!(
            go_channel_i64_try_send,
            fn(GoChannelI64, GoInt) -> bool,
            [RuntimeType::GoChannelI64, RuntimeType::I64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoChannelI64TryReceive => runtime_surface!(
            go_channel_i64_try_receive,
            fn(GoChannelI64) -> (GoInt, GoInt),
            [RuntimeType::GoChannelI64] -> RuntimeType::I64I64Tuple
        ),
        RuntimeOp::GoChannelGoStringNil => runtime_surface!(
            go_channel_go_string_nil,
            fn() -> GoChannelGoString,
            [] -> RuntimeType::GoChannelGoString
        ),
        RuntimeOp::GoChannelGoStringMake => runtime_surface!(
            go_channel_go_string_make,
            fn(GoInt) -> GoChannelGoString,
            [RuntimeType::I64] -> RuntimeType::GoChannelGoString
        ),
        RuntimeOp::GoChannelGoStringLen => runtime_surface!(
            go_channel_go_string_len,
            fn(GoChannelGoString) -> GoInt,
            [RuntimeType::GoChannelGoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelGoStringCap => runtime_surface!(
            go_channel_go_string_cap,
            fn(GoChannelGoString) -> GoInt,
            [RuntimeType::GoChannelGoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelGoStringSend => runtime_surface!(
            go_channel_go_string_send,
            fn(GoChannelGoString, GoString),
            [RuntimeType::GoChannelGoString, RuntimeType::GoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelGoStringReceiveValue => runtime_surface!(
            go_channel_go_string_receive_value,
            fn(GoChannelGoString) -> GoString,
            [RuntimeType::GoChannelGoString] -> RuntimeType::GoString
        ),
        RuntimeOp::GoChannelGoStringReceive => runtime_surface!(
            go_channel_go_string_receive,
            fn(GoChannelGoString) -> (GoString, bool),
            [RuntimeType::GoChannelGoString] -> RuntimeType::GoStringBoolTuple
        ),
        RuntimeOp::GoChannelGoStringClose => runtime_surface!(
            go_channel_go_string_close,
            fn(GoChannelGoString),
            [RuntimeType::GoChannelGoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelGoStringIsNil => runtime_surface!(
            go_channel_go_string_is_nil,
            fn(GoChannelGoString) -> bool,
            [RuntimeType::GoChannelGoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoChannelGoStringTrySend => runtime_surface!(
            go_channel_go_string_try_send,
            fn(GoChannelGoString, GoString) -> bool,
            [RuntimeType::GoChannelGoString, RuntimeType::GoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoChannelGoStringTryReceive => runtime_surface!(
            go_channel_go_string_try_receive,
            fn(GoChannelGoString) -> (GoString, GoInt),
            [RuntimeType::GoChannelGoString] -> RuntimeType::GoStringI64Tuple
        ),
        RuntimeOp::GoChannelGoChannelI64Nil => runtime_surface!(
            go_channel_go_channel_i64_nil,
            fn() -> GoChannelGoChannelI64,
            [] -> RuntimeType::GoChannelGoChannelI64
        ),
        RuntimeOp::GoChannelGoChannelI64Make => runtime_surface!(
            go_channel_go_channel_i64_make,
            fn(GoInt) -> GoChannelGoChannelI64,
            [RuntimeType::I64] -> RuntimeType::GoChannelGoChannelI64
        ),
        RuntimeOp::GoChannelGoChannelI64Len => runtime_surface!(
            go_channel_go_channel_i64_len,
            fn(GoChannelGoChannelI64) -> GoInt,
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelGoChannelI64Cap => runtime_surface!(
            go_channel_go_channel_i64_cap,
            fn(GoChannelGoChannelI64) -> GoInt,
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::I64
        ),
        RuntimeOp::GoChannelGoChannelI64Send => runtime_surface!(
            go_channel_go_channel_i64_send,
            fn(GoChannelGoChannelI64, GoChannelI64),
            [RuntimeType::GoChannelGoChannelI64, RuntimeType::GoChannelI64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelGoChannelI64ReceiveValue => runtime_surface!(
            go_channel_go_channel_i64_receive_value,
            fn(GoChannelGoChannelI64) -> GoChannelI64,
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::GoChannelI64
        ),
        RuntimeOp::GoChannelGoChannelI64Receive => runtime_surface!(
            go_channel_go_channel_i64_receive,
            fn(GoChannelGoChannelI64) -> (GoChannelI64, bool),
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::GoChannelI64BoolTuple
        ),
        RuntimeOp::GoChannelGoChannelI64Close => runtime_surface!(
            go_channel_go_channel_i64_close,
            fn(GoChannelGoChannelI64),
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoChannelGoChannelI64IsNil => runtime_surface!(
            go_channel_go_channel_i64_is_nil,
            fn(GoChannelGoChannelI64) -> bool,
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoChannelGoChannelI64TrySend => runtime_surface!(
            go_channel_go_channel_i64_try_send,
            fn(GoChannelGoChannelI64, GoChannelI64) -> bool,
            [RuntimeType::GoChannelGoChannelI64, RuntimeType::GoChannelI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoChannelGoChannelI64TryReceive => runtime_surface!(
            go_channel_go_channel_i64_try_receive,
            fn(GoChannelGoChannelI64) -> (GoChannelI64, GoInt),
            [RuntimeType::GoChannelGoChannelI64] -> RuntimeType::GoChannelI64I64Tuple
        ),
        RuntimeOp::GoSliceU8Len => runtime_surface!(
            go_slice_u8_len,
            fn(GoSliceU8) -> GoInt,
            [RuntimeType::GoSliceU8] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceU8Index => runtime_surface!(
            go_slice_u8_index,
            fn(GoSliceU8, GoInt) -> GoInt,
            [RuntimeType::GoSliceU8, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceU8Range => runtime_surface!(
            go_slice_u8_range,
            fn(GoSliceU8, GoInt, GoInt, GoInt) -> GoSliceU8,
            [RuntimeType::GoSliceU8, RuntimeType::I64, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoStringIndex => runtime_surface!(
            go_string_index,
            fn(GoString, GoInt) -> GoInt,
            [RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoStringRange => runtime_surface!(
            go_string_range,
            fn(GoString, GoInt, GoInt) -> GoString,
            [RuntimeType::GoString, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoString
        ),
        RuntimeOp::GoStringFromSliceRunes => runtime_surface!(
            go_string_from_slice_runes,
            fn(GoSliceI64) -> GoString,
            [RuntimeType::GoSliceI64] -> RuntimeType::GoString
        ),
        RuntimeOp::GoStringRangeCount => runtime_surface!(
            go_string_range_count,
            fn(GoString) -> GoInt,
            [RuntimeType::GoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoStringRangeIndexAt => runtime_surface!(
            go_string_range_index_at,
            fn(GoString, GoInt) -> GoInt,
            [RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoStringRangeRuneAt => runtime_surface!(
            go_string_range_rune_at,
            fn(GoString, GoInt) -> GoInt,
            [RuntimeType::GoString, RuntimeType::I64] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceI64Nil => runtime_surface!(
            go_slice_i64_nil,
            fn() -> GoSliceI64,
            [] -> RuntimeType::GoSliceI64
        ),
        RuntimeOp::GoSliceI64IsNil => runtime_surface!(
            go_slice_i64_is_nil,
            fn(GoSliceI64) -> bool,
            [RuntimeType::GoSliceI64] -> RuntimeType::Bool
        ),
        RuntimeOp::GoSliceU8Nil => runtime_surface!(
            go_slice_u8_nil,
            fn() -> GoSliceU8,
            [] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoSliceU8IsNil => runtime_surface!(
            go_slice_u8_is_nil,
            fn(GoSliceU8) -> bool,
            [RuntimeType::GoSliceU8] -> RuntimeType::Bool
        ),
        RuntimeOp::GoSliceBoolNil => runtime_surface!(
            go_slice_bool_nil,
            fn() -> GoSliceBool,
            [] -> RuntimeType::GoSliceBool
        ),
        RuntimeOp::GoSliceBoolIsNil => runtime_surface!(
            go_slice_bool_is_nil,
            fn(GoSliceBool) -> bool,
            [RuntimeType::GoSliceBool] -> RuntimeType::Bool
        ),
        RuntimeOp::GoSliceInterfaceNil => runtime_surface!(
            go_slice_interface_nil,
            fn() -> GoSliceInterface,
            [] -> RuntimeType::GoSliceInterface
        ),
        RuntimeOp::GoSliceInterfaceIsNil => runtime_surface!(
            go_slice_interface_is_nil,
            fn(GoSliceInterface) -> bool,
            [RuntimeType::GoSliceInterface] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceBoxAggregate => runtime_surface!(
            go_interface_box_aggregate,
            fn(GoString, GoSliceInterface) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoSliceInterface] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceUnboxAggregate => runtime_surface!(
            go_interface_unbox_aggregate,
            fn(GoInterface, GoString) -> GoSliceInterface,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::GoSliceInterface
        ),
        RuntimeOp::GoSliceU8Make => runtime_surface!(
            go_slice_u8_make,
            fn(GoInt, GoInt) -> GoSliceU8,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceU8
        ),
        RuntimeOp::GoSliceU8Set => runtime_surface!(
            go_slice_u8_set,
            fn(GoSliceU8, GoInt, GoInt),
            [RuntimeType::GoSliceU8, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceU8Copy => runtime_surface!(
            go_slice_u8_copy,
            fn(GoSliceU8, GoSliceU8) -> GoInt,
            [RuntimeType::GoSliceU8, RuntimeType::GoSliceU8] -> RuntimeType::I64
        ),
        RuntimeOp::PrintF64 => runtime_surface!(
            print_f64,
            fn(f64),
            [RuntimeType::F64] -> RuntimeType::Unit
        ),
        RuntimeOp::GoInterfaceBoxF64 => runtime_surface!(
            go_interface_box_f64,
            fn(GoString, f64) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::F64] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceUnboxF64 => runtime_surface!(
            go_interface_unbox_f64,
            fn(GoInterface, GoString) -> f64,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::F64
        ),
        RuntimeOp::GoInterfaceEqual => runtime_surface!(
            go_interface_equal,
            fn(GoInterface, GoInterface) -> bool,
            [RuntimeType::GoInterface, RuntimeType::GoInterface] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceBoxComparableAggregate => runtime_surface!(
            go_interface_box_comparable_aggregate,
            fn(GoString, GoSliceInterface) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoSliceInterface] -> RuntimeType::GoInterface
        ),
        RuntimeOp::PanicGoInterface => runtime_surface!(
            panic_go_interface,
            fn(GoInterface),
            [RuntimeType::GoInterface] -> RuntimeType::Unit
        ),
        RuntimeOp::GoPanicPayloadToInterface => runtime_surface!(
            go_panic_payload_to_interface,
            fn(GoPanicPayload) -> GoInterface,
            [RuntimeType::GoPanicPayload] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceIsRuntimeError => runtime_surface!(
            go_interface_is_runtime_error,
            fn(GoInterface) -> bool,
            [RuntimeType::GoInterface] -> RuntimeType::Bool
        ),
        RuntimeOp::GoSliceGoStringNil => runtime_surface!(
            go_slice_go_string_nil,
            fn() -> GoSliceGoString,
            [] -> RuntimeType::GoSliceGoString
        ),
        RuntimeOp::GoSliceGoStringMake => runtime_surface!(
            go_slice_go_string_make,
            fn(GoInt, GoInt) -> GoSliceGoString,
            [RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceGoString
        ),
        RuntimeOp::GoSliceGoStringLen => runtime_surface!(
            go_slice_go_string_len,
            fn(GoSliceGoString) -> GoInt,
            [RuntimeType::GoSliceGoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceGoStringCap => runtime_surface!(
            go_slice_go_string_cap,
            fn(GoSliceGoString) -> GoInt,
            [RuntimeType::GoSliceGoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceGoStringIndex => runtime_surface!(
            go_slice_go_string_index,
            fn(GoSliceGoString, GoInt) -> GoString,
            [RuntimeType::GoSliceGoString, RuntimeType::I64] -> RuntimeType::GoString
        ),
        RuntimeOp::GoSliceGoStringRange => runtime_surface!(
            go_slice_go_string_range,
            fn(GoSliceGoString, GoInt, GoInt, GoInt) -> GoSliceGoString,
            [RuntimeType::GoSliceGoString, RuntimeType::I64, RuntimeType::I64, RuntimeType::I64] -> RuntimeType::GoSliceGoString
        ),
        RuntimeOp::GoSliceGoStringSet => runtime_surface!(
            go_slice_go_string_set,
            fn(GoSliceGoString, GoInt, GoString),
            [RuntimeType::GoSliceGoString, RuntimeType::I64, RuntimeType::GoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceGoStringAppend => runtime_surface!(
            go_slice_go_string_append,
            fn(GoSliceGoString, GoString) -> GoSliceGoString,
            [RuntimeType::GoSliceGoString, RuntimeType::GoString] -> RuntimeType::GoSliceGoString
        ),
        RuntimeOp::GoSliceGoStringCopy => runtime_surface!(
            go_slice_go_string_copy,
            fn(GoSliceGoString, GoSliceGoString) -> GoInt,
            [RuntimeType::GoSliceGoString, RuntimeType::GoSliceGoString] -> RuntimeType::I64
        ),
        RuntimeOp::GoSliceGoStringClear => runtime_surface!(
            go_slice_go_string_clear,
            fn(GoSliceGoString),
            [RuntimeType::GoSliceGoString] -> RuntimeType::Unit
        ),
        RuntimeOp::GoSliceGoStringIsNil => runtime_surface!(
            go_slice_go_string_is_nil,
            fn(GoSliceGoString) -> bool,
            [RuntimeType::GoSliceGoString] -> RuntimeType::Bool
        ),
        RuntimeOp::GoInterfaceBoxGoSliceGoString => runtime_surface!(
            go_interface_box_go_slice_go_string,
            fn(GoString, GoSliceGoString) -> GoInterface,
            [RuntimeType::GoString, RuntimeType::GoSliceGoString] -> RuntimeType::GoInterface
        ),
        RuntimeOp::GoInterfaceUnboxGoSliceGoString => runtime_surface!(
            go_interface_unbox_go_slice_go_string,
            fn(GoInterface, GoString) -> GoSliceGoString,
            [RuntimeType::GoInterface, RuntimeType::GoString] -> RuntimeType::GoSliceGoString
        ),
    }
}

#[test]
fn every_public_runtime_operation_has_the_declared_symbol_and_signature() {
    for operation in RuntimeOp::ALL {
        let implementation = implementation_surface(*operation);
        let signature = operation.signature();
        assert_eq!(implementation.symbol, operation.symbol(), "{operation:?}");
        assert_eq!(
            implementation.parameters,
            signature.parameters(),
            "{operation:?}"
        );
        assert_eq!(implementation.result, signature.result(), "{operation:?}");
    }
}
