use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, BlockingEffect, GoPanicCondition, HostIoEffect,
    RuntimeOp,
};

pub fn allocation(operation: RuntimeOp) -> AllocationEffect {
    assert!(
        is_extended_channel_operation(operation),
        "unexpected operation in extended channel allocation test: {operation:?}"
    );
    match operation {
        RuntimeOp::GoChannelGoStringMake | RuntimeOp::GoChannelGoChannelI64Make => {
            AllocationEffect::MayAllocate
        }
        _ => AllocationEffect::None,
    }
}

pub fn argument_mutation(operation: RuntimeOp) -> ArgumentMutationEffect {
    assert!(
        is_extended_channel_operation(operation),
        "unexpected operation in extended channel mutation test: {operation:?}"
    );
    match operation {
        RuntimeOp::GoChannelGoStringSend
        | RuntimeOp::GoChannelGoStringReceiveValue
        | RuntimeOp::GoChannelGoStringReceive
        | RuntimeOp::GoChannelGoStringClose
        | RuntimeOp::GoChannelGoStringTrySend
        | RuntimeOp::GoChannelGoStringTryReceive
        | RuntimeOp::GoChannelGoChannelI64Send
        | RuntimeOp::GoChannelGoChannelI64ReceiveValue
        | RuntimeOp::GoChannelGoChannelI64Receive
        | RuntimeOp::GoChannelGoChannelI64Close
        | RuntimeOp::GoChannelGoChannelI64TrySend
        | RuntimeOp::GoChannelGoChannelI64TryReceive => {
            ArgumentMutationEffect::MayMutateOwnedArgument
        }
        _ => ArgumentMutationEffect::None,
    }
}

pub fn blocking(operation: RuntimeOp) -> BlockingEffect {
    assert!(
        is_extended_channel_operation(operation),
        "unexpected operation in extended channel blocking test: {operation:?}"
    );
    match operation {
        RuntimeOp::GoChannelGoStringSend
        | RuntimeOp::GoChannelGoStringReceiveValue
        | RuntimeOp::GoChannelGoStringReceive
        | RuntimeOp::GoChannelGoChannelI64Send
        | RuntimeOp::GoChannelGoChannelI64ReceiveValue
        | RuntimeOp::GoChannelGoChannelI64Receive => BlockingEffect::MayBlock,
        _ => BlockingEffect::None,
    }
}

pub fn host_io(operation: RuntimeOp) -> HostIoEffect {
    assert!(
        is_extended_channel_operation(operation),
        "unexpected operation in extended channel host-I/O test: {operation:?}"
    );
    HostIoEffect::None
}

pub fn panics(operation: RuntimeOp) -> &'static [GoPanicCondition] {
    assert!(
        is_extended_channel_operation(operation),
        "unexpected operation in extended channel panic test: {operation:?}"
    );
    match operation {
        RuntimeOp::GoChannelGoStringMake | RuntimeOp::GoChannelGoChannelI64Make => {
            &[GoPanicCondition::NegativeChannelCapacity]
        }
        RuntimeOp::GoChannelGoStringSend
        | RuntimeOp::GoChannelGoStringTrySend
        | RuntimeOp::GoChannelGoChannelI64Send
        | RuntimeOp::GoChannelGoChannelI64TrySend => &[GoPanicCondition::SendOnClosedChannel],
        RuntimeOp::GoChannelGoStringClose | RuntimeOp::GoChannelGoChannelI64Close => &[
            GoPanicCondition::CloseOfNilChannel,
            GoPanicCondition::CloseOfClosedChannel,
        ],
        _ => &[],
    }
}

pub fn is_extended_channel_operation(operation: RuntimeOp) -> bool {
    (125..=146).contains(&operation.id().get())
}
