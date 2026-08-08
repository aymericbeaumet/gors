//! Canonical Rust-IR constant representation checks.

use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::{Constant, RustType};
use gors_runtime_abi::{FloatKind, RuntimeType};

pub(super) fn constant_type(constant: &Constant) -> Result<RustType, Diagnostic> {
    match constant {
        Constant::Bool(_) => Ok(RustType::Bool),
        Constant::Integer { kind, bits } => {
            if !kind.is_canonical_carrier(*bits) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR integer constant has a non-canonical {kind:?} carrier"
                )));
            }
            Ok(RustType::Integer(*kind))
        }
        Constant::Float { kind, bits } => {
            verify_float_carrier(*kind, *bits, "float constant")?;
            Ok(RustType::Float(*kind))
        }
        Constant::Complex { kind, real, imag } => {
            verify_float_carrier(*kind, *real, "complex real component")?;
            verify_float_carrier(*kind, *imag, "complex imaginary component")?;
            Ok(RustType::Complex(*kind))
        }
        Constant::StaticIntegerArray { kind, values } => {
            if values
                .iter()
                .any(|value| !kind.is_canonical_carrier(*value))
            {
                return Err(Diagnostic::backend(format!(
                    "Rust IR static integer array has a non-canonical {kind:?} element"
                )));
            }
            Ok(RustType::ArrayInteger {
                length: u64::try_from(values.len())
                    .map_err(|_| Diagnostic::backend("Rust IR array length does not fit u64"))?,
                element: *kind,
            })
        }
        Constant::RuntimeStaticBytes { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticByteSlice]
                && signature.result() == RuntimeType::GoString
            {
                Ok(RustType::GoString)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static bytes use runtime operation {op:?} with incompatible signature {:?} -> {:?}",
                    signature.parameters(),
                    signature.result()
                )))
            }
        }
        Constant::RuntimeStaticI64s { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticI64Slice]
                && signature.result() == RuntimeType::GoSliceI64
            {
                Ok(RustType::GoSliceI64)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static int slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
        Constant::RuntimeStaticBools { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticBoolSlice]
                && signature.result() == RuntimeType::GoSliceBool
            {
                Ok(RustType::GoSliceBool)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static bool slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
        Constant::RuntimeStaticU8s { op, .. } => {
            let signature = op.signature();
            if signature.parameters() == [RuntimeType::StaticByteSlice]
                && signature.result() == RuntimeType::GoSliceU8
            {
                Ok(RustType::GoSliceU8)
            } else {
                Err(Diagnostic::backend(format!(
                    "Rust IR static byte slice uses runtime operation {op:?} with an incompatible signature"
                )))
            }
        }
    }
}

fn verify_float_carrier(kind: FloatKind, bits: u64, context: &str) -> Result<(), Diagnostic> {
    if kind == FloatKind::F32 {
        let value = f64::from_bits(bits);
        let canonical = f64::from(value as f32).to_bits();
        if canonical != bits {
            return Err(Diagnostic::backend(format!(
                "Rust IR {context} has a non-canonical float32 carrier"
            )));
        }
    }
    Ok(())
}
