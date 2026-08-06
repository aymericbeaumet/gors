//! Small domain-separated fingerprint accumulator for database products.

use crate::compiler::fingerprint::{Fingerprint, fingerprint_parts};

pub(in crate::compiler::db) struct FingerprintBuilder {
    domain: Vec<u8>,
    parts: Vec<Vec<u8>>,
}

impl FingerprintBuilder {
    pub(in crate::compiler::db) fn new(domain: &[u8]) -> Self {
        Self {
            domain: domain.to_vec(),
            parts: Vec::new(),
        }
    }

    pub(in crate::compiler::db) fn bytes(&mut self, value: &[u8]) {
        self.parts.push(value.to_vec());
    }

    pub(in crate::compiler::db) fn usize(&mut self, value: usize) {
        self.bytes(&u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
    }

    pub(in crate::compiler::db) fn finish(self) -> Fingerprint {
        let parts = self.parts.iter().map(Vec::as_slice).collect::<Vec<_>>();
        fingerprint_parts(&self.domain, &parts)
    }
}
