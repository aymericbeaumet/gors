use super::*;

#[test]
fn artifact_and_link_plan_identities_cover_every_selection_dimension() -> Result<(), Box<dyn Error>>
{
    let contract = manifest([], [RuntimeOp::IntDiv, RuntimeOp::PrintI64]);
    let target = target_model("x86_64-unknown-linux-gnu")?;
    let toolchain = compatibility_identity(b"rustc");
    let baseline = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        toolchain,
        b"runtime",
    );
    let changed_capabilities = artifact(&contract, target.clone(), [], toolchain, b"runtime");
    let changed_toolchain = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        compatibility_identity(b"other rustc"),
        b"runtime",
    );
    let changed_implementation = artifact(
        &contract,
        target.clone(),
        [TargetCapability::StandardIo],
        toolchain,
        b"other runtime",
    );
    assert_ne!(baseline.identity(), changed_capabilities.identity());
    assert_ne!(baseline.identity(), changed_toolchain.identity());
    assert_ne!(baseline.identity(), changed_implementation.identity());

    let div = baseline.select(request(
        &contract,
        [RuntimeOp::IntDiv],
        target.clone(),
        toolchain,
    )?)?;
    let reordered = baseline.select(request(
        &contract,
        [RuntimeOp::IntDiv, RuntimeOp::IntDiv],
        target.clone(),
        toolchain,
    )?)?;
    let print = baseline.select(request(
        &contract,
        [RuntimeOp::PrintI64],
        target,
        toolchain,
    )?)?;
    assert_eq!(div.identity(), reordered.identity());
    assert_ne!(div.identity(), print.identity());
    Ok(())
}
