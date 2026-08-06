//! Public function-header aggregate for one source file.

use std::sync::Arc;

use super::{FingerprintBuilder, FunctionSignature};
use crate::compiler::fingerprint::Fingerprint;
use crate::compiler::ids::FileId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicApi {
    file: FileId,
    signatures: Arc<[FunctionSignature]>,
    fingerprint: Fingerprint,
}

impl PublicApi {
    pub(in crate::compiler::db) fn new(file: FileId, signatures: Arc<[FunctionSignature]>) -> Self {
        let mut writer = FingerprintBuilder::new(b"file-public-api");
        writer.bytes(file.canonical_bytes());
        for signature in &*signatures {
            writer.bytes(signature.name().as_bytes());
            writer.bytes(signature.fingerprint().as_bytes());
        }
        Self {
            file,
            signatures,
            fingerprint: writer.finish(),
        }
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub fn signatures(&self) -> &[FunctionSignature] {
        &self.signatures
    }

    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.signatures.iter().fold(32_usize, |total, signature| {
            total.saturating_add(signature.retained_bytes())
        })
    }
}
