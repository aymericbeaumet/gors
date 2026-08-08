//! Typed compiler identity for the target-neutral runtime contract.

use std::fmt;

use gors_runtime_abi::{ContractIdentity, RuntimeAbiManifest};

/// Compiler identity for the exact target-neutral runtime contract.
///
/// The runtime ABI crate owns canonical manifest encoding. Compiler queries
/// retain only its typed SHA-256 identity, never a label scraped from build
/// output or an ambient environment variable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeAbiId(ContractIdentity);

impl RuntimeAbiId {
    /// Construct an identity from a canonical runtime-contract digest.
    #[must_use]
    pub const fn from_contract_hash(bytes: [u8; 32]) -> Self {
        Self(ContractIdentity::from_bytes(bytes))
    }

    /// Identity of the current canonical runtime contract.
    #[must_use]
    pub fn current() -> Self {
        Self(RuntimeAbiManifest::current().identity())
    }

    /// Canonical contract identity used by artifact packaging.
    #[must_use]
    pub const fn contract_identity(self) -> ContractIdentity {
        self.0
    }

    /// Canonical digest bytes used by query and persistent-cache keys.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }
}

impl From<ContractIdentity> for RuntimeAbiId {
    fn from(identity: ContractIdentity) -> Self {
        Self(identity)
    }
}

impl fmt::Display for RuntimeAbiId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}
