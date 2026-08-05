//! Target-neutral runtime dependency carried across the browser worker boundary.

use std::fmt::{Display, Formatter};

/// Current browser wire schema for a target-neutral runtime dependency.
///
/// This is deliberately independent of artifact and link-plan schemas. The
/// browser compiler does not select a target, toolchain, implementation, or
/// artifact format.
pub(crate) const RUNTIME_DEPENDENCY_SCHEMA_VERSION: u32 =
    gors_runtime_abi::CURRENT_RUNTIME_DEPENDENCY_SCHEMA;

/// Browser wire facts required by the later VM-side artifact selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeDependencyProtocol {
    schema_version: u32,
    contract_identity: String,
    operation_ids: Vec<u16>,
}

impl RuntimeDependencyProtocol {
    /// Admit one compiler-owned dependency into the browser protocol.
    ///
    /// The generated-output type keeps the ABI crate out of this Wasm
    /// boundary while still exposing its typed dependency. A stale contract or
    /// non-canonical operation sequence fails closed before publication.
    pub(crate) fn from_generated_output(
        output: &gors::printer::GeneratedOutput,
    ) -> Result<Self, RuntimeDependencyProtocolError> {
        let contract_identity = output.runtime.contract().to_string();
        let current_contract = gors::compiler::db::RuntimeAbiId::current().to_string();
        if contract_identity != current_contract {
            return Err(RuntimeDependencyProtocolError::ContractMismatch {
                generated: contract_identity,
                current: current_contract,
            });
        }

        let operation_ids = output
            .runtime
            .requirement()
            .operation_ids()
            .collect::<Vec<_>>();
        if operation_ids.windows(2).any(|pair| {
            pair.first()
                .zip(pair.get(1))
                .is_some_and(|(left, right)| left >= right)
        }) {
            return Err(RuntimeDependencyProtocolError::NonCanonicalOperations);
        }

        Ok(Self {
            schema_version: RUNTIME_DEPENDENCY_SCHEMA_VERSION,
            contract_identity,
            operation_ids,
        })
    }

    pub(crate) const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub(crate) fn contract_identity(&self) -> &str {
        &self.contract_identity
    }

    pub(crate) fn operation_ids(&self) -> &[u16] {
        &self.operation_ids
    }
}

/// A compiler result cannot be represented by the current browser protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeDependencyProtocolError {
    ContractMismatch { generated: String, current: String },
    NonCanonicalOperations,
}

impl Display for RuntimeDependencyProtocolError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContractMismatch { generated, current } => write!(
                formatter,
                "generated runtime contract {generated} does not match browser compiler contract {current}"
            ),
            Self::NonCanonicalOperations => formatter.write_str(
                "generated runtime operation IDs are not in canonical strictly increasing order",
            ),
        }
    }
}

impl std::error::Error for RuntimeDependencyProtocolError {}
