use gors_runtime::{
    GoChannelI64, GoInt, GoMapStringI64, GoPointerI64, GoSliceI64, GoSliceU8, GoString,
    concat_go_strings, go_channel_i64_cap, go_channel_i64_close, go_channel_i64_is_nil,
    go_channel_i64_len, go_channel_i64_make, go_channel_i64_nil, go_channel_i64_receive,
    go_channel_i64_receive_value, go_channel_i64_send, go_channel_i64_try_receive,
    go_channel_i64_try_send, go_map_string_i64_clear, go_map_string_i64_contains,
    go_map_string_i64_delete, go_map_string_i64_get, go_map_string_i64_is_nil,
    go_map_string_i64_key_at, go_map_string_i64_len, go_map_string_i64_make, go_map_string_i64_nil,
    go_map_string_i64_set, go_pointer_i64_get, go_pointer_i64_is_nil, go_pointer_i64_new,
    go_pointer_i64_nil, go_pointer_i64_set, go_slice_i64_append, go_slice_i64_cap,
    go_slice_i64_clear, go_slice_i64_copy, go_slice_i64_from_static, go_slice_i64_index,
    go_slice_i64_len, go_slice_i64_make, go_slice_i64_range, go_slice_i64_set,
    go_slice_u8_append_slice, go_slice_u8_append_string, go_slice_u8_copy_string,
    go_slice_u8_from_static, go_string_from_bytes, go_string_from_slice_u8, go_string_from_static,
    go_string_len, int_div, int_rem, int_shl, int_shr, panic_bool, panic_go_string, panic_i64,
    print_bool, print_go_string, print_i64, print_newline, print_space,
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
