use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, BlockingEffect, HostIoEffect, RuntimeOp, RuntimeType,
};

#[test]
fn whole_slice_append_operations_have_exact_abi_and_effects() {
    for (operation, id, symbol, slice_type) in [
        (
            RuntimeOp::GoSliceI64AppendSlice,
            220,
            "go_slice_i64_append_slice",
            RuntimeType::GoSliceI64,
        ),
        (
            RuntimeOp::GoSliceInterfaceAppend,
            221,
            "go_slice_interface_append",
            RuntimeType::GoSliceInterface,
        ),
    ] {
        assert_eq!(operation.id().get(), id);
        assert_eq!(RuntimeOp::try_from(id), Ok(operation));
        assert_eq!(operation.symbol(), symbol);
        assert_eq!(
            operation.signature().parameters(),
            &[slice_type, slice_type]
        );
        assert_eq!(operation.signature().result(), slice_type);
        assert!(operation.required_capabilities().is_empty());

        let effects = operation.effects();
        assert_eq!(effects.allocation(), AllocationEffect::MayAllocate);
        assert_eq!(
            effects.argument_mutation(),
            ArgumentMutationEffect::MayMutateOwnedArgument
        );
        assert_eq!(effects.blocking(), BlockingEffect::None);
        assert_eq!(effects.host_io(), HostIoEffect::None);
        assert!(effects.go_panics().is_empty());
    }
}
