use gors_runtime::{
    GoInt, GoString, print_bool, print_f32, print_f64, print_go_string, print_i64, print_newline,
    print_space, print_u64,
};
use gors_runtime_abi::RuntimeType;

use super::RuntimeSurface;

macro_rules! printing_surface {
    ($function:ident, $function_type:ty, [$($parameter:expr),* $(,)?]) => {{
        const IMPLEMENTATION: $function_type = $function;
        let _ = IMPLEMENTATION;
        RuntimeSurface {
            symbol: stringify!($function),
            parameters: &[$($parameter),*],
            result: RuntimeType::Unit,
        }
    }};
}

pub enum PrintOperation {
    Bool,
    I64,
    U64,
    F32,
    F64,
    Space,
    Newline,
    GoString,
}

pub fn implementation_surface(operation: PrintOperation) -> RuntimeSurface {
    match operation {
        PrintOperation::Bool => printing_surface!(print_bool, fn(bool), [RuntimeType::Bool]),
        PrintOperation::I64 => printing_surface!(print_i64, fn(GoInt), [RuntimeType::I64]),
        PrintOperation::U64 => printing_surface!(print_u64, fn(GoInt), [RuntimeType::I64]),
        PrintOperation::F32 => printing_surface!(print_f32, fn(f64), [RuntimeType::F64]),
        PrintOperation::F64 => printing_surface!(print_f64, fn(f64), [RuntimeType::F64]),
        PrintOperation::Space => printing_surface!(print_space, fn(), []),
        PrintOperation::Newline => printing_surface!(print_newline, fn(), []),
        PrintOperation::GoString => {
            printing_surface!(print_go_string, fn(GoString), [RuntimeType::GoString])
        }
    }
}
