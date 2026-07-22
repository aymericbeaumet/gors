//! Canonical invalidation inputs for one Rust-IR query root.

use std::collections::BTreeMap;

use super::Fingerprint;
use super::encoder::{Encoder, def_id, signature};
use super::hir::hir_function_semantics;
use crate::compiler::hir;
use crate::compiler::ids::DefId;
use crate::compiler::types::Signature;

pub(in crate::compiler) fn rust_ir_root_inputs(
    function: &hir::Function,
    signatures: &BTreeMap<DefId, Signature>,
    representation_key: Fingerprint,
    executable_package: bool,
) -> Fingerprint {
    let semantic_hir = hir_function_semantics(function);
    let mut encoder = Encoder::root(b"rust-ir-root-inputs");
    encoder.field(b"semantic-hir", |encoder| {
        encoder.blob(semantic_hir.as_bytes());
    });
    encoder.field(b"signatures", |encoder| {
        let signatures = signatures.iter().collect::<Vec<_>>();
        encoder.sequence(&signatures, |encoder, (definition, value)| {
            encoder.field(b"definition", |encoder| def_id(encoder, **definition));
            encoder.field(b"signature", |encoder| signature(encoder, value));
        });
    });
    encoder.field(b"representation", |encoder| {
        encoder.blob(representation_key.as_bytes());
    });
    encoder.field(b"executable-package", |encoder| {
        encoder.bool(executable_package);
    });
    encoder.finish()
}
