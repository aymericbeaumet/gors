//! Exact identities for package named types and their instantiations.

use std::sync::Arc;

use super::{ChannelDir, ComplexTy, FloatTy, IntTy, Signature, StructField, Ty, UintTy, UntypedTy};
use crate::compiler::ids::QualifiedDefId;

const DYNAMIC_NAMED_IDENTITY_SCHEMA: &[u8] = b"gors-dynamic-named-type-v1";

/// Exact semantic identity of one package named type or instantiation.
///
/// The declaration identity and ordered type arguments define Go identity.
/// The instantiated underlying representation deliberately does not.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NamedTypeId {
    definition: QualifiedDefId,
    arguments: Arc<[Ty]>,
}

impl NamedTypeId {
    #[must_use]
    pub fn new(definition: QualifiedDefId, arguments: impl Into<Arc<[Ty]>>) -> Self {
        Self {
            definition,
            arguments: arguments.into(),
        }
    }

    #[must_use]
    pub const fn definition(&self) -> QualifiedDefId {
        self.definition
    }

    #[must_use]
    pub fn arguments(&self) -> &[Ty] {
        &self.arguments
    }

    /// Whether two named identities denote the same Go type.
    #[must_use]
    pub fn is_identical_to(&self, other: &Self) -> bool {
        named_identities_are_identical(self, other)
    }

    pub(super) fn dynamic_identity(&self) -> Option<Vec<u8>> {
        let mut output = Vec::new();
        blob(&mut output, DYNAMIC_NAMED_IDENTITY_SCHEMA);
        blob(&mut output, self.definition.package().canonical_bytes());
        blob(&mut output, self.definition.definition().canonical_bytes());
        usize_value(&mut output, self.arguments.len());
        for argument in self.arguments.iter() {
            let mut encoded = Vec::new();
            encode_type(&mut encoded, argument)?;
            blob(&mut output, &encoded);
        }
        Some(output)
    }
}

impl Ty {
    /// Whether two compiler types are identical under Go's type-identity rules.
    ///
    /// Product equality remains stricter: it also distinguishes a complete
    /// named type from a finite recursive reference and checks retained
    /// representation facts. Semantic checks must use this operation.
    #[must_use]
    pub fn is_identical_to(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Named { identity: left, .. } | Self::NamedRef { identity: left },
                Self::Named {
                    identity: right, ..
                }
                | Self::NamedRef { identity: right },
            ) => named_identities_are_identical(left, right),
            (
                Self::LocalNamed { identity: left, .. },
                Self::LocalNamed {
                    identity: right, ..
                },
            ) => left == right,
            (Self::Struct(left), Self::Struct(right)) => fields_are_identical(left, right),
            (Self::Interface(left), Self::Interface(right)) => {
                left.len() == right.len()
                    && left.iter().zip(right).all(|(left, right)| {
                        left.name == right.name
                            && signatures_are_identical(&left.signature, &right.signature)
                    })
            }
            (Self::Function(left), Self::Function(right)) => signatures_are_identical(left, right),
            (Self::Pointer(left), Self::Pointer(right))
            | (Self::Slice(left), Self::Slice(right)) => left.is_identical_to(right),
            (Self::Array(left_len, left), Self::Array(right_len, right)) => {
                left_len == right_len && left.is_identical_to(right)
            }
            (Self::Map(left_key, left_value), Self::Map(right_key, right_value)) => {
                left_key.is_identical_to(right_key) && left_value.is_identical_to(right_value)
            }
            (Self::Channel(left_dir, left), Self::Channel(right_dir, right)) => {
                left_dir == right_dir && left.is_identical_to(right)
            }
            (Self::Tuple(left), Self::Tuple(right)) => type_sequences_are_identical(left, right),
            _ => self == other,
        }
    }
}

impl Signature {
    /// Whether two function signatures are identical under Go type identity.
    #[must_use]
    pub fn is_identical_to(&self, other: &Self) -> bool {
        signatures_are_identical(self, other)
    }
}

fn named_identities_are_identical(left: &NamedTypeId, right: &NamedTypeId) -> bool {
    left.definition == right.definition
        && type_sequences_are_identical(&left.arguments, &right.arguments)
}

fn type_sequences_are_identical(left: &[Ty], right: &[Ty]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.is_identical_to(right))
}

fn signatures_are_identical(left: &Signature, right: &Signature) -> bool {
    left.variadic == right.variadic
        && type_sequences_are_identical(&left.params, &right.params)
        && type_sequences_are_identical(&left.results, &right.results)
}

fn fields_are_identical(left: &[StructField], right: &[StructField]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && left.embedded == right.embedded
                && left.tag == right.tag
                && left.ty.is_identical_to(&right.ty)
        })
}

fn encode_type(output: &mut Vec<u8>, ty: &Ty) -> Option<()> {
    match ty {
        Ty::Unit => tag(output, b"unit"),
        Ty::Bool => tag(output, b"bool"),
        Ty::Int(kind) => value(output, b"int", int_kind(*kind)),
        Ty::Uint(kind) => value(output, b"uint", uint_kind(*kind)),
        Ty::Float(kind) => value(output, b"float", float_kind(*kind)),
        Ty::Complex(kind) => value(output, b"complex", complex_kind(*kind)),
        Ty::Named { identity, .. } | Ty::NamedRef { identity } => {
            tag(output, b"named");
            blob(output, &identity.dynamic_identity()?);
        }
        // Anonymous aggregate identity needs package ownership for unexported
        // members. That structural identity belongs to the next representation
        // checkpoint; do not silently encode member spellings as global facts.
        Ty::LocalNamed { .. } | Ty::Struct(_) | Ty::Interface(_) => return None,
        Ty::Function(signature) => {
            tag(output, b"function");
            encode_signature(output, signature)?;
        }
        Ty::String => tag(output, b"string"),
        Ty::Pointer(element) => {
            unary_type(output, b"pointer", element)?;
        }
        Ty::Array(length, element) => {
            tag(output, b"array");
            output.extend_from_slice(&length.to_be_bytes());
            nested_type(output, element)?;
        }
        Ty::Slice(element) => {
            unary_type(output, b"slice", element)?;
        }
        Ty::Map(key, value) => {
            tag(output, b"map");
            nested_type(output, key)?;
            nested_type(output, value)?;
        }
        Ty::Channel(direction, element) => {
            value(output, b"channel", channel_direction(*direction));
            nested_type(output, element)?;
        }
        Ty::Tuple(elements) => {
            tag(output, b"tuple");
            usize_value(output, elements.len());
            for element in elements {
                nested_type(output, element)?;
            }
        }
        Ty::Untyped(kind) => value(output, b"untyped", untyped_kind(*kind)),
    }
    Some(())
}

fn encode_signature(output: &mut Vec<u8>, signature: &Signature) -> Option<()> {
    output.push(u8::from(signature.variadic));
    usize_value(output, signature.params.len());
    for parameter in &signature.params {
        nested_type(output, parameter)?;
    }
    usize_value(output, signature.results.len());
    for result in &signature.results {
        nested_type(output, result)?;
    }
    Some(())
}

fn unary_type(output: &mut Vec<u8>, name: &[u8], element: &Ty) -> Option<()> {
    tag(output, name);
    nested_type(output, element)
}

fn nested_type(output: &mut Vec<u8>, ty: &Ty) -> Option<()> {
    let mut encoded = Vec::new();
    encode_type(&mut encoded, ty)?;
    blob(output, &encoded);
    Some(())
}

fn tag(output: &mut Vec<u8>, name: &[u8]) {
    blob(output, name);
}

fn value(output: &mut Vec<u8>, name: &[u8], value: u8) {
    tag(output, name);
    output.push(value);
}

fn usize_value(output: &mut Vec<u8>, value: usize) {
    // Rust supports at most 64-bit address spaces, so this widening is
    // lossless on every target supported by the compiler.
    output.extend_from_slice(&(value as u64).to_be_bytes());
}

fn blob(output: &mut Vec<u8>, value: &[u8]) {
    usize_value(output, value.len());
    output.extend_from_slice(value);
}

const fn int_kind(kind: IntTy) -> u8 {
    match kind {
        IntTy::Int => 0,
        IntTy::Int8 => 1,
        IntTy::Int16 => 2,
        IntTy::Int32 => 3,
        IntTy::Int64 => 4,
    }
}

const fn uint_kind(kind: UintTy) -> u8 {
    match kind {
        UintTy::Uint => 0,
        UintTy::Uint8 => 1,
        UintTy::Uint16 => 2,
        UintTy::Uint32 => 3,
        UintTy::Uint64 => 4,
        UintTy::Uintptr => 5,
    }
}

const fn float_kind(kind: FloatTy) -> u8 {
    match kind {
        FloatTy::Float32 => 0,
        FloatTy::Float64 => 1,
    }
}

const fn complex_kind(kind: ComplexTy) -> u8 {
    match kind {
        ComplexTy::Complex64 => 0,
        ComplexTy::Complex128 => 1,
    }
}

const fn channel_direction(direction: ChannelDir) -> u8 {
    match direction {
        ChannelDir::SendReceive => 0,
        ChannelDir::SendOnly => 1,
        ChannelDir::ReceiveOnly => 2,
    }
}

const fn untyped_kind(kind: UntypedTy) -> u8 {
    match kind {
        UntypedTy::Bool => 0,
        UntypedTy::Int => 1,
        UntypedTy::Rune => 2,
        UntypedTy::Float => 3,
        UntypedTy::Complex => 4,
        UntypedTy::String => 5,
    }
}
