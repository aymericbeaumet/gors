use gors_runtime_abi::{AllocationEffect, ArgumentMutationEffect, GoPanicCondition, RuntimeOp};

pub fn allocation(operation: RuntimeOp) -> AllocationEffect {
    assert!(is_string_slice_operation(operation));
    match operation {
        RuntimeOp::GoSliceGoStringNil
        | RuntimeOp::GoSliceGoStringMake
        | RuntimeOp::GoSliceGoStringAppend => AllocationEffect::MayAllocate,
        _ => AllocationEffect::None,
    }
}

pub fn argument_mutation(operation: RuntimeOp) -> ArgumentMutationEffect {
    assert!(is_string_slice_operation(operation));
    match operation {
        RuntimeOp::GoSliceGoStringSet
        | RuntimeOp::GoSliceGoStringAppend
        | RuntimeOp::GoSliceGoStringCopy
        | RuntimeOp::GoSliceGoStringClear => ArgumentMutationEffect::MayMutateOwnedArgument,
        _ => ArgumentMutationEffect::None,
    }
}

pub fn panics(operation: RuntimeOp) -> &'static [GoPanicCondition] {
    assert!(is_string_slice_operation(operation));
    match operation {
        RuntimeOp::GoSliceGoStringIndex | RuntimeOp::GoSliceGoStringSet => {
            &[GoPanicCondition::IndexOutOfRange]
        }
        RuntimeOp::GoSliceGoStringRange | RuntimeOp::GoSliceGoStringMake => {
            &[GoPanicCondition::SliceBoundsOutOfRange]
        }
        RuntimeOp::GoInterfaceUnboxGoSliceGoString => &[GoPanicCondition::TypeAssertionFailure],
        _ => &[],
    }
}

pub fn is_string_slice_operation(operation: RuntimeOp) -> bool {
    (147..=159).contains(&operation.id().get())
}
