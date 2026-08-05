//! Strongly typed SHA-256 identities for independent ABI domains.

use std::fmt::{Debug, Display, Formatter};

use crate::encoding::sha256;

/// SHA-256 identity of the target-neutral runtime contract manifest.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContractIdentity([u8; 32]);

impl ContractIdentity {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }
}

impl Debug for ContractIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ContractIdentity")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for ContractIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

/// SHA-256 identity of exact compiled runtime implementation bytes.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ImplementationHash([u8; 32]);

impl ImplementationHash {
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether these are the exact bytes named by this implementation hash.
    #[must_use]
    pub fn matches(self, bytes: &[u8]) -> bool {
        self == Self::sha256(bytes)
    }
}

impl Debug for ImplementationHash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ImplementationHash")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for ImplementationHash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

/// SHA-256 identity of one target-specific runtime artifact selection.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactIdentity([u8; 32]);

impl ArtifactIdentity {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }
}

impl Debug for ArtifactIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ArtifactIdentity")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for ArtifactIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

/// SHA-256 provenance identity of the compiler host and recipe that produced
/// an rlib. Consumers retain this for evidence but never use it for selection.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProducerIdentity([u8; 32]);

impl ProducerIdentity {
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for ProducerIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ProducerIdentity")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for ProducerIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

/// SHA-256 identity of the host-neutral compiler, target rustlib ABI, and
/// representation settings required to consume a Rust runtime rlib.
///
/// This is deliberately separate from producer provenance: a cross-produced
/// rlib is selected by the target machine's compatibility identity, while its
/// original compiler host remains immutable evidence.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompatibilityIdentity([u8; 32]);

impl CompatibilityIdentity {
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for CompatibilityIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("CompatibilityIdentity")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for CompatibilityIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

/// SHA-256 identity of one validated consumer-to-provider link plan.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LinkPlanIdentity([u8; 32]);

impl LinkPlanIdentity {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn sha256(bytes: &[u8]) -> Self {
        Self(sha256(bytes))
    }
}

impl Debug for LinkPlanIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LinkPlanIdentity")
            .field(&HexDigest(&self.0))
            .finish()
    }
}

impl Display for LinkPlanIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, &self.0)
    }
}

struct HexDigest<'a>(&'a [u8; 32]);

impl Debug for HexDigest<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write_digest(formatter, self.0)
    }
}

fn write_digest(formatter: &mut Formatter<'_>, digest: &[u8; 32]) -> std::fmt::Result {
    for byte in digest {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}
