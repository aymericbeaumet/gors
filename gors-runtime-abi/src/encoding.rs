//! Minimal canonical binary encoding and digest primitives.

use sha2::{Digest as _, Sha256};

const CONTRACT_DOMAIN: &[u8] = b"gors.runtime-abi.contract\0";
const ARTIFACT_DOMAIN: &[u8] = b"gors.runtime-abi.artifact\0";
const LINK_PLAN_DOMAIN: &[u8] = b"gors.runtime-abi.link-plan\0";

pub struct CanonicalEncoder {
    bytes: Vec<u8>,
}

impl CanonicalEncoder {
    pub(crate) fn contract() -> Self {
        Self::new(CONTRACT_DOMAIN)
    }

    pub(crate) fn artifact() -> Self {
        Self::new(ARTIFACT_DOMAIN)
    }

    pub(crate) fn link_plan() -> Self {
        Self::new(LINK_PLAN_DOMAIN)
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn count(&mut self, value: usize) {
        self.usize_varint(value);
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    pub(crate) fn bytes(&mut self, value: &[u8]) {
        self.usize_varint(value.len());
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn fixed_bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn new(domain: &[u8]) -> Self {
        Self {
            bytes: domain.to_vec(),
        }
    }

    fn usize_varint(&mut self, mut value: usize) {
        loop {
            let low = u8::try_from(value & 0x7f).unwrap_or_default();
            value >>= 7;
            if value == 0 {
                self.bytes.push(low);
                return;
            }
            self.bytes.push(low | 0x80);
        }
    }
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
