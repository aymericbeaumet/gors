use gors_runtime::{
    GoInt, GoString, concat_go_strings, go_string_from_bytes, go_string_from_static, int_div,
    int_rem, int_shl, int_shr, print_bool, print_go_string, print_i64, print_newline, print_space,
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
