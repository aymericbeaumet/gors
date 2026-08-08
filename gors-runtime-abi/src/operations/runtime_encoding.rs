//! Canonical encoding of complete runtime operation contracts.

use super::{IntegerKindConstraint, RuntimeOp, RuntimeType};
use crate::encoding::CanonicalEncoder;

impl RuntimeOp {
    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        self.encode_with_integer_constraints(
            encoder,
            |position| self.integer_parameter_constraint(position),
            |position| self.integer_result_constraint(position),
        );
    }

    pub(crate) fn encode_with_integer_constraints(
        self,
        encoder: &mut CanonicalEncoder,
        parameter_constraint: impl Fn(usize) -> IntegerKindConstraint,
        result_constraint: impl Fn(usize) -> IntegerKindConstraint,
    ) {
        encoder.u16(self.id().get());
        encoder.text(self.symbol());
        let signature = self.signature();
        signature.encode(encoder);
        encoder.count(signature.parameters().len());
        for (position, parameter) in signature.parameters().iter().enumerate() {
            if *parameter == RuntimeType::I64 {
                encoder.u8(1);
                parameter_constraint(position).encode(encoder);
            } else {
                encoder.u8(0);
            }
        }
        let result_components = runtime_result_components(signature.result());
        encoder.count(result_components.len());
        for (position, result) in result_components.iter().enumerate() {
            if *result == RuntimeType::I64 {
                encoder.u8(1);
                result_constraint(position).encode(encoder);
            } else {
                encoder.u8(0);
            }
        }
        self.effects().encode(encoder);
        let requirements = self.required_capabilities();
        encoder.count(requirements.len());
        for requirement in requirements {
            encoder.u16(requirement.canonical_tag());
        }
    }
}

fn runtime_result_components(result: RuntimeType) -> &'static [RuntimeType] {
    use RuntimeType::*;
    match result {
        Unit => &[],
        I64BoolTuple => &[I64, Bool],
        I64I64Tuple => &[I64, I64],
        GoStringBoolTuple => &[GoString, Bool],
        GoStringI64Tuple => &[GoString, I64],
        GoChannelI64BoolTuple => &[GoChannelI64, Bool],
        GoChannelI64I64Tuple => &[GoChannelI64, I64],
        Bool => &[Bool],
        I64 => &[I64],
        GoString => &[GoString],
        ByteSlice => &[ByteSlice],
        StaticByteSlice => &[StaticByteSlice],
        F64 => &[F64],
        Complex128 => &[Complex128],
        GoSliceI64 => &[GoSliceI64],
        StaticI64Slice => &[StaticI64Slice],
        GoSliceU8 => &[GoSliceU8],
        GoMapStringI64 => &[GoMapStringI64],
        GoPointerI64 => &[GoPointerI64],
        GoChannelI64 => &[GoChannelI64],
        GoPointerStructI64 => &[GoPointerStructI64],
        GoInterface => &[GoInterface],
        StaticBoolSlice => &[StaticBoolSlice],
        GoSliceBool => &[GoSliceBool],
        GoSliceInterface => &[GoSliceInterface],
        GoMapStringInterface => &[GoMapStringInterface],
        GoPanicPayload => &[GoPanicPayload],
        GoChannelGoString => &[GoChannelGoString],
        GoChannelGoChannelI64 => &[GoChannelGoChannelI64],
        GoSliceGoString => &[GoSliceGoString],
        GoMapI64GoString => &[GoMapI64GoString],
    }
}
