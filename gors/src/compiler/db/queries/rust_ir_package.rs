//! Deterministic verified Rust-IR package assembly.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{
    Db, PackageInput, file_projection, package_analysis_product,
    rust_signature_dependencies_product, typed_constant_product, typed_variable_product,
    verified_rust_ir_product,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{
    CompilerStage, StageFailure, StageResult, VerifiedRustIrPackage,
};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::fingerprint::{Fingerprint, fingerprint_parts};
use crate::compiler::rust_ir;

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn rust_ir_package_product(
    db: &dyn Db,
    input: PackageInput,
) -> StageResult<VerifiedRustIrPackage> {
    db.query_telemetry().record_query(QueryKind::RustIrPackage);
    let analysis = package_analysis_product(db, input);
    let mut functions = Vec::new();
    let mut signatures = BTreeMap::new();
    let mut runtime_requirement = rust_ir::RuntimeRequirement::default();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let facts = file_projection(db, source);
        let mut constants = facts.constants(db);
        constants.sort_by_key(|constant| constant.id(db));
        for constant in constants {
            db.unwind_if_revision_cancelled();
            typed_constant_product(db, input, constant)?;
        }
        let mut variables = facts.variables(db);
        variables.sort_by_key(|variable| variable.id(db));
        for variable in variables {
            db.unwind_if_revision_cancelled();
            typed_variable_product(db, input, variable)?;
        }
        let mut projected = facts.functions(db);
        projected.sort_by_key(|function| function.id(db));
        for function in projected {
            db.unwind_if_revision_cancelled();
            let dependencies = rust_signature_dependencies_product(db, input, function)?;
            signatures.extend(
                dependencies
                    .signatures
                    .iter()
                    .map(|(definition, signature)| (*definition, signature.clone())),
            );
            let verified = verified_rust_ir_product(db, input, function)?;
            runtime_requirement = runtime_requirement.union(verified.runtime_requirement());
            functions.push(verified.function().clone());
        }
    }
    functions.sort_by_key(|function| function.id);
    let file = rust_ir::File {
        package_id: input.package(db),
        package: analysis.package_name().to_string(),
        functions,
    };
    let verified_runtime_requirement = rust_ir::verify_with_signatures(&file, &signatures)
        .map_err(|diagnostic| {
            Arc::new(StageFailure::one(
                CompilerStage::RustRepresentation,
                diagnostic,
            ))
        })?;
    if verified_runtime_requirement != runtime_requirement {
        return Err(Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            Diagnostic::backend(format!(
                "Rust IR package runtime requirement mismatch: function products derived {runtime_requirement:?}, package verification derived {verified_runtime_requirement:?}"
            )),
        )));
    }
    let representation_key = representation_key(db)?;
    Ok(Arc::new(VerifiedRustIrPackage::new(
        file,
        representation_key,
        runtime_requirement,
    )))
}

pub(super) fn representation_key(db: &dyn Db) -> Result<Fingerprint, Arc<StageFailure>> {
    let build = db.query_build_input().ok_or_else(|| {
        Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            Diagnostic::backend(
                "compiler build config is missing from Rust representation lowering",
            ),
        ))
    })?;
    let runtime_abi = build.runtime_abi(db);
    Ok(fingerprint_parts(
        b"rust-representation-config",
        &[runtime_abi.as_bytes()],
    ))
}
