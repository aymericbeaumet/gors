use gors_runtime_abi::{
    CURRENT_RUNTIME_DEPENDENCY_SCHEMA, Endianness, RuntimeAbiManifest, RuntimeArtifactFormat,
    RuntimeDependency, RuntimeLinkPlan, RuntimeRequirement,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

const LINK_DESCRIPTOR_SCHEMA: u32 = 2;

/// Current-schema wire record for one unconditional runtime dependency.
///
/// The descriptor deliberately stores stable operation IDs rather than Rust
/// enum discriminants. It has no compatibility decoder: any schema, contract,
/// or operation unknown to this CLI fails closed and must be recompiled.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDependencyDescriptor {
    schema_version: u32,
    contract: String,
    operation_ids: Vec<u16>,
}

/// Path-independent record of the exact provider selected for generated Rust.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLinkDescriptor {
    #[serde(rename = "link_descriptor_schema_version")]
    schema_version: u32,
    dependency: RuntimeDependencyDescriptor,
    target_triple: String,
    target_pointer_width: u16,
    target_endianness: String,
    format: String,
    producer_identity: String,
    compatibility_identity: String,
    implementation_hash: String,
    artifact_identity: String,
    link_plan_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeDescriptorError {
    UnsupportedDependencySchema(u32),
    UnsupportedLinkSchema(u32),
    ContractMismatch { cached: String, current: String },
    NonCanonicalOperations,
    InvalidOperation(u16),
}

impl Display for RuntimeDescriptorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedDependencySchema(schema) => write!(
                formatter,
                "runtime dependency descriptor schema {schema} is unsupported"
            ),
            Self::UnsupportedLinkSchema(schema) => {
                write!(
                    formatter,
                    "runtime link descriptor schema {schema} is unsupported"
                )
            }
            Self::ContractMismatch { cached, current } => write!(
                formatter,
                "cached runtime contract {cached} does not match current contract {current}"
            ),
            Self::NonCanonicalOperations => formatter
                .write_str("runtime operation IDs must be strictly increasing without duplicates"),
            Self::InvalidOperation(operation) => {
                write!(formatter, "runtime operation ID {operation} is unknown")
            }
        }
    }
}

impl std::error::Error for RuntimeDescriptorError {}

impl RuntimeDependencyDescriptor {
    #[must_use]
    pub fn from_dependency(dependency: &RuntimeDependency) -> Self {
        Self {
            schema_version: CURRENT_RUNTIME_DEPENDENCY_SCHEMA,
            contract: dependency.contract().to_string(),
            operation_ids: dependency.requirement().operation_ids().collect(),
        }
    }

    pub fn reconstruct(&self) -> Result<RuntimeDependency, RuntimeDescriptorError> {
        if self.schema_version != CURRENT_RUNTIME_DEPENDENCY_SCHEMA {
            return Err(RuntimeDescriptorError::UnsupportedDependencySchema(
                self.schema_version,
            ));
        }
        let contract = RuntimeAbiManifest::current();
        let current_identity = contract.identity().to_string();
        if self.contract != current_identity {
            return Err(RuntimeDescriptorError::ContractMismatch {
                cached: self.contract.clone(),
                current: current_identity,
            });
        }
        if !self
            .operation_ids
            .windows(2)
            .all(|pair| matches!(pair, [previous, current] if previous < current))
        {
            return Err(RuntimeDescriptorError::NonCanonicalOperations);
        }
        let requirement =
            RuntimeRequirement::from_operation_ids(self.operation_ids.iter().copied())
                .map_err(|error| RuntimeDescriptorError::InvalidOperation(error.get()))?;
        RuntimeDependency::new(&contract, requirement)
            .map_err(|error| RuntimeDescriptorError::InvalidOperation(error.operation().id().get()))
    }
}

impl RuntimeLinkDescriptor {
    #[must_use]
    pub fn from_plan(
        plan: &RuntimeLinkPlan,
        producer_identity: gors_runtime_abi::ProducerIdentity,
    ) -> Self {
        let target = plan.request().target();
        Self {
            schema_version: LINK_DESCRIPTOR_SCHEMA,
            dependency: RuntimeDependencyDescriptor::from_dependency(plan.request().dependency()),
            target_triple: target.triple().to_string(),
            target_pointer_width: target.pointer_width().bits(),
            target_endianness: match target.endianness() {
                Endianness::Little => "little",
                Endianness::Big => "big",
            }
            .to_string(),
            format: match plan.request().format() {
                RuntimeArtifactFormat::RustRlibV1 => "rust-rlib-v1",
            }
            .to_string(),
            producer_identity: producer_identity.to_string(),
            compatibility_identity: plan.request().compatibility().to_string(),
            implementation_hash: plan.implementation().to_string(),
            artifact_identity: plan.artifact().to_string(),
            link_plan_identity: plan.identity().to_string(),
        }
    }

    pub fn reconstruct_dependency(&self) -> Result<RuntimeDependency, RuntimeDescriptorError> {
        if self.schema_version != LINK_DESCRIPTOR_SCHEMA {
            return Err(RuntimeDescriptorError::UnsupportedLinkSchema(
                self.schema_version,
            ));
        }
        self.dependency.reconstruct()
    }

    #[must_use]
    pub fn link_plan_identity(&self) -> &str {
        &self.link_plan_identity
    }

    #[must_use]
    pub fn compatibility_identity(&self) -> &str {
        &self.compatibility_identity
    }

    #[must_use]
    pub fn implementation_hash(&self) -> &str {
        &self.implementation_hash
    }

    #[must_use]
    pub fn target_triple(&self) -> &str {
        &self.target_triple
    }

    #[must_use]
    pub fn is_canonical(&self) -> bool {
        self.reconstruct_dependency().is_ok()
            && !self.target_triple.is_empty()
            && matches!(self.target_pointer_width, 32 | 64)
            && matches!(self.target_endianness.as_str(), "little" | "big")
            && self.format == "rust-rlib-v1"
            && [
                &self.producer_identity,
                &self.compatibility_identity,
                &self.implementation_hash,
                &self.artifact_identity,
                &self.link_plan_identity,
            ]
            .into_iter()
            .all(|identity| is_sha256(identity))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[allow(clippy::panic)]
pub fn test_runtime_link_descriptor() -> RuntimeLinkDescriptor {
    test_runtime_link_descriptor_with_payload(b"test-runtime")
}

#[cfg(test)]
#[allow(clippy::panic)]
pub fn test_runtime_link_descriptor_with_payload(payload: &[u8]) -> RuntimeLinkDescriptor {
    use gors_runtime_abi::{
        CompatibilityIdentity, DataWidth, Endianness, ImplementationHash, ProducerIdentity,
        RuntimeArtifactFormat, RuntimeArtifactManifest, RuntimeLinkRequest, RuntimeOp,
        TargetCapabilities, TargetModel,
    };

    let contract = RuntimeAbiManifest::current();
    let Ok(dependency) = RuntimeDependency::new(
        &contract,
        RuntimeRequirement::new([RuntimeOp::PrintI64, RuntimeOp::PrintNewline]),
    ) else {
        panic!("test runtime dependency must satisfy the current ABI")
    };
    let Ok(target) = TargetModel::new("test-target", DataWidth::Bits64, Endianness::Little) else {
        panic!("test target must satisfy the target model")
    };
    let compatibility = CompatibilityIdentity::sha256(b"test-compatibility");
    let artifact = RuntimeArtifactManifest::new(
        contract.identity(),
        target.clone(),
        TargetCapabilities::new(dependency.requirement().required_capabilities().iter()),
        RuntimeArtifactFormat::RustRlibV1,
        compatibility,
        ImplementationHash::sha256(payload),
    );
    let request = RuntimeLinkRequest::new(
        dependency,
        target,
        RuntimeArtifactFormat::RustRlibV1,
        compatibility,
    );
    let Ok(plan) = artifact.select(request) else {
        panic!("test artifact must satisfy its matching link request")
    };
    RuntimeLinkDescriptor::from_plan(&plan, ProducerIdentity::sha256(b"test-producer"))
}

#[cfg(test)]
#[allow(clippy::panic)]
pub fn test_runtime_dependency() -> RuntimeDependency {
    let Ok(dependency) = test_runtime_link_descriptor().reconstruct_dependency() else {
        panic!("test link descriptor must reconstruct its dependency")
    };
    dependency
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;

    #[test]
    fn dependency_round_trip_uses_stable_operation_ids() {
        let descriptor = test_runtime_link_descriptor();
        let Ok(dependency) = descriptor.reconstruct_dependency() else {
            panic!("test descriptor must reconstruct its dependency")
        };
        assert_eq!(
            dependency.requirement().operation_ids().collect::<Vec<_>>(),
            vec![14, 16]
        );
    }

    #[test]
    fn unknown_schema_contract_and_operation_fail_closed() {
        let mut descriptor = test_runtime_link_descriptor();
        descriptor.schema_version += 1;
        assert!(matches!(
            descriptor.reconstruct_dependency(),
            Err(RuntimeDescriptorError::UnsupportedLinkSchema(_))
        ));

        let mut descriptor = test_runtime_link_descriptor();
        descriptor.dependency.contract = "stale-contract".to_string();
        assert!(matches!(
            descriptor.reconstruct_dependency(),
            Err(RuntimeDescriptorError::ContractMismatch { .. })
        ));

        let mut descriptor = test_runtime_link_descriptor();
        descriptor.dependency.operation_ids.push(u16::MAX);
        assert_eq!(
            descriptor.reconstruct_dependency(),
            Err(RuntimeDescriptorError::InvalidOperation(u16::MAX))
        );

        for operation_ids in [vec![16, 14], vec![14, 14]] {
            let mut descriptor = test_runtime_link_descriptor();
            descriptor.dependency.operation_ids = operation_ids;
            assert_eq!(
                descriptor.reconstruct_dependency(),
                Err(RuntimeDescriptorError::NonCanonicalOperations)
            );
        }
    }

    #[test]
    fn descriptor_json_rejects_unknown_fields() {
        let descriptor = test_runtime_link_descriptor();
        let Ok(mut value) = serde_json::to_value(&descriptor) else {
            panic!("test descriptor must serialize")
        };
        let Some(object) = value.as_object_mut() else {
            panic!("test descriptor must serialize as an object")
        };
        object.insert("future_field".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<RuntimeLinkDescriptor>(value).is_err());

        let Ok(mut value) = serde_json::to_value(&descriptor) else {
            panic!("test descriptor must serialize")
        };
        let Some(dependency) = value
            .get_mut("dependency")
            .and_then(serde_json::Value::as_object_mut)
        else {
            panic!("test descriptor must contain an object dependency")
        };
        dependency.insert("future_field".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<RuntimeLinkDescriptor>(value).is_err());
    }
}
