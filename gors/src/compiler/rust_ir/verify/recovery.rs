//! Verification for panic cleanup payload capture and `recover` values.

use super::super::{Function, Operand, PanicEdge, Place, RuntimeOp, RustType};
use super::{verify_effects, verify_same, verify_source_provenance};
use crate::compiler::Diagnostic;
use gors_runtime_abi::RuntimeType;

pub(super) fn verify_panic_cleanup(function: &Function) -> Result<(), Diagnostic> {
    if let Some(cleanup) = function.panic_cleanup {
        function.verify_target(cleanup.entry)?;
        verify_same(
            function.place_ty(Place {
                local: cleanup.active,
            })?,
            RustType::Bool,
            "panic cleanup state",
        )?;
        verify_same(
            function.place_ty(Place {
                local: cleanup.recovered,
            })?,
            RustType::GoInterface,
            "panic cleanup recovered value",
        )?;
        if cleanup.capture.operation != RuntimeOp::GoPanicPayloadToInterface
            || cleanup.capture.operation.signature().parameters() != [RuntimeType::GoPanicPayload]
            || cleanup.capture.operation.signature().result() != RuntimeType::GoInterface
        {
            return Err(Diagnostic::backend(
                "Rust IR panic cleanup has an invalid payload capture operation",
            ));
        }
        verify_effects(
            cleanup.capture.effects,
            super::super::runtime_effects(cleanup.capture.operation),
            "panic payload capture",
        )?;
        verify_source_provenance(
            &cleanup.capture.provenance,
            function.id,
            "panic payload capture",
        )?;
        if cleanup.rethrow.operation != RuntimeOp::PanicGoInterface
            || cleanup.rethrow.operation.signature().parameters() != [RuntimeType::GoInterface]
            || cleanup.rethrow.operation.signature().result() != RuntimeType::Unit
        {
            return Err(Diagnostic::backend(
                "Rust IR panic cleanup has an invalid payload rethrow operation",
            ));
        }
        verify_effects(
            cleanup.rethrow.effects,
            super::super::runtime_effects(cleanup.rethrow.operation),
            "panic payload rethrow",
        )?;
        verify_source_provenance(
            &cleanup.rethrow.provenance,
            function.id,
            "panic payload rethrow",
        )?;
    }
    for block in &function.blocks {
        for edge in block
            .statements
            .iter()
            .map(|statement| statement.value.panic)
            .chain(std::iter::once(block.terminator.panic))
        {
            if let PanicEdge::Cleanup(target) = edge
                && function.panic_cleanup.map(|cleanup| cleanup.entry) != Some(target)
            {
                return Err(Diagnostic::backend(
                    "Rust IR panic edge does not target the function cleanup entry",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn verify_recover(
    function: &Function,
    state: Place,
    value: &Operand,
    nil: RuntimeOp,
) -> Result<RustType, Diagnostic> {
    verify_same(
        function.place_ty(state)?,
        RustType::Bool,
        "panic recovery state",
    )?;
    verify_same(
        function.operand_ty(value)?,
        RustType::GoInterface,
        "panic recovery value",
    )?;
    if nil != RuntimeOp::GoInterfaceNil
        || !nil.signature().parameters().is_empty()
        || nil.signature().result() != RuntimeType::GoInterface
    {
        return Err(Diagnostic::backend(
            "Rust IR recover uses an invalid nil-interface operation",
        ));
    }
    Ok(RustType::GoInterface)
}
