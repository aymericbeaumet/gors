use std::collections::BTreeSet;

use gors_runtime_abi::{PrimitiveOp, RuntimeAbiManifest, RuntimeOp};

use super::manifest;

#[test]
fn current_operation_catalogs_are_complete_and_collision_free() {
    let current = RuntimeAbiManifest::current();
    assert_eq!(current.primitive_ops(), PrimitiveOp::ALL);
    assert_eq!(current.runtime_ops(), RuntimeOp::ALL);

    let symbols = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.symbol())
        .collect::<BTreeSet<_>>();
    assert_eq!(symbols.len(), RuntimeOp::ALL.len());

    let primitive_ids = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_ids.len(), PrimitiveOp::ALL.len());
    assert_eq!(PrimitiveOp::Int32WrappingAdd.id().get(), 51);
    assert_eq!(PrimitiveOp::Int32WrappingNeg.id().get(), 52);

    let primitive_names = PrimitiveOp::ALL
        .iter()
        .map(|operation| operation.name())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_names.len(), PrimitiveOp::ALL.len());

    let runtime_ids = RuntimeOp::ALL
        .iter()
        .map(|operation| operation.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(runtime_ids.len(), RuntimeOp::ALL.len());
    assert_eq!(RuntimeOp::GoChannelGoStringNil.id().get(), 125);
    assert_eq!(RuntimeOp::GoChannelGoStringTryReceive.id().get(), 135);
    assert_eq!(RuntimeOp::GoChannelGoChannelI64Nil.id().get(), 136);
    assert_eq!(RuntimeOp::GoChannelGoChannelI64TryReceive.id().get(), 146);

    let primitive_identities = PrimitiveOp::ALL
        .iter()
        .map(|operation| manifest([*operation], []).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(primitive_identities.len(), PrimitiveOp::ALL.len());

    let operation_identities = RuntimeOp::ALL
        .iter()
        .map(|operation| manifest([], [*operation]).identity())
        .collect::<BTreeSet<_>>();
    assert_eq!(operation_identities.len(), RuntimeOp::ALL.len());
}
