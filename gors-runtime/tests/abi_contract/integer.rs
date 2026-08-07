use super::RuntimeSurface;
use gors_runtime::{GoInt, *};
use gors_runtime_abi::{IntegerKind, IntegerRuntimeOp, RuntimeType};

macro_rules! runtime_surface {
    ($function:ident) => {{
        const IMPLEMENTATION: fn(GoInt, GoInt) -> GoInt = $function;
        let _ = IMPLEMENTATION;
        RuntimeSurface {
            symbol: stringify!($function),
            parameters: &[RuntimeType::I64, RuntimeType::I64],
            result: RuntimeType::I64,
        }
    }};
}

pub fn implementation_surface(operation: IntegerRuntimeOp, kind: IntegerKind) -> RuntimeSurface {
    match (operation, kind) {
        (IntegerRuntimeOp::Div, IntegerKind::I8) => runtime_surface!(int_div_i8),
        (IntegerRuntimeOp::Div, IntegerKind::I16) => runtime_surface!(int_div_i16),
        (IntegerRuntimeOp::Div, IntegerKind::I32) => runtime_surface!(int_div_i32),
        (IntegerRuntimeOp::Div, IntegerKind::I64) => runtime_surface!(int_div),
        (IntegerRuntimeOp::Div, IntegerKind::U8) => runtime_surface!(int_div_u8),
        (IntegerRuntimeOp::Div, IntegerKind::U16) => runtime_surface!(int_div_u16),
        (IntegerRuntimeOp::Div, IntegerKind::U32) => runtime_surface!(int_div_u32),
        (IntegerRuntimeOp::Div, IntegerKind::U64) => runtime_surface!(int_div_u64),
        (IntegerRuntimeOp::Rem, IntegerKind::I8) => runtime_surface!(int_rem_i8),
        (IntegerRuntimeOp::Rem, IntegerKind::I16) => runtime_surface!(int_rem_i16),
        (IntegerRuntimeOp::Rem, IntegerKind::I32) => runtime_surface!(int_rem_i32),
        (IntegerRuntimeOp::Rem, IntegerKind::I64) => runtime_surface!(int_rem),
        (IntegerRuntimeOp::Rem, IntegerKind::U8) => runtime_surface!(int_rem_u8),
        (IntegerRuntimeOp::Rem, IntegerKind::U16) => runtime_surface!(int_rem_u16),
        (IntegerRuntimeOp::Rem, IntegerKind::U32) => runtime_surface!(int_rem_u32),
        (IntegerRuntimeOp::Rem, IntegerKind::U64) => runtime_surface!(int_rem_u64),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::I8) => runtime_surface!(int_shl_signed_i8),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::I16) => runtime_surface!(int_shl_signed_i16),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::I32) => runtime_surface!(int_shl_signed_i32),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::I64) => runtime_surface!(int_shl),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::U8) => runtime_surface!(int_shl_signed_u8),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::U16) => runtime_surface!(int_shl_signed_u16),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::U32) => runtime_surface!(int_shl_signed_u32),
        (IntegerRuntimeOp::ShlSigned, IntegerKind::U64) => runtime_surface!(int_shl_signed_u64),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::I8) => runtime_surface!(int_shr_signed_i8),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::I16) => runtime_surface!(int_shr_signed_i16),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::I32) => runtime_surface!(int_shr_signed_i32),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::I64) => runtime_surface!(int_shr),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::U8) => runtime_surface!(int_shr_signed_u8),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::U16) => runtime_surface!(int_shr_signed_u16),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::U32) => runtime_surface!(int_shr_signed_u32),
        (IntegerRuntimeOp::ShrSigned, IntegerKind::U64) => runtime_surface!(int_shr_signed_u64),
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::I8) => runtime_surface!(int_shl_unsigned_i8),
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::I16) => {
            runtime_surface!(int_shl_unsigned_i16)
        }
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::I32) => {
            runtime_surface!(int_shl_unsigned_i32)
        }
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::I64) => {
            runtime_surface!(int_shl_unsigned_i64)
        }
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::U8) => runtime_surface!(int_shl_unsigned_u8),
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::U16) => {
            runtime_surface!(int_shl_unsigned_u16)
        }
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::U32) => {
            runtime_surface!(int_shl_unsigned_u32)
        }
        (IntegerRuntimeOp::ShlUnsigned, IntegerKind::U64) => {
            runtime_surface!(int_shl_unsigned_u64)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::I8) => runtime_surface!(int_shr_unsigned_i8),
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::I16) => {
            runtime_surface!(int_shr_unsigned_i16)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::I32) => {
            runtime_surface!(int_shr_unsigned_i32)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::I64) => {
            runtime_surface!(int_shr_unsigned_i64)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::U8) => runtime_surface!(int_shr_unsigned_u8),
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::U16) => {
            runtime_surface!(int_shr_unsigned_u16)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::U32) => {
            runtime_surface!(int_shr_unsigned_u32)
        }
        (IntegerRuntimeOp::ShrUnsigned, IntegerKind::U64) => {
            runtime_surface!(int_shr_unsigned_u64)
        }
    }
}
