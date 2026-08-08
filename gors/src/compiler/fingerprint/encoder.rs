//! Length-delimited canonical binary encoding primitives.

use sha2::{Digest, Sha256};

use super::Fingerprint;
use crate::compiler::hir;
use crate::compiler::ids::{
    BasicBlockId, ClosureId, DefId, LocalId, LocalTypeId, NodeId, PackageId, QualifiedDefId,
};
use crate::compiler::provenance::{SourceRef, SourceRefKind};
use crate::compiler::types::{
    ChannelDir, ComplexTy, ConstValue, FloatTy, IntTy, InterfaceMethod, Signature, StructField, Ty,
    UintTy, UntypedTy,
};

const FORMAT_MAGIC: &[u8] = b"gors-stage-product";
const SCHEMA_VERSION: u32 = 9;

/// An encoder for one root product or one length-delimited nested field.
pub(super) struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    pub(super) fn root(domain: &[u8]) -> Self {
        let mut encoder = Self::nested();
        encoder.blob(FORMAT_MAGIC);
        encoder.u32(SCHEMA_VERSION);
        encoder.blob(domain);
        encoder
    }

    fn nested() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Add one named, independently length-delimited field.
    pub(super) fn field(&mut self, name: &[u8], encode: impl FnOnce(&mut Self)) {
        self.blob(name);
        let mut field = Self::nested();
        encode(&mut field);
        self.blob(&field.bytes);
    }

    /// Add an enum discriminant and its length-delimited payload.
    pub(super) fn variant(&mut self, name: &[u8], encode: impl FnOnce(&mut Self)) {
        self.blob(name);
        let mut payload = Self::nested();
        encode(&mut payload);
        self.blob(&payload.bytes);
    }

    /// Add a sequence with an explicit element count and delimited elements.
    pub(super) fn sequence<T>(&mut self, values: &[T], encode: impl Fn(&mut Self, &T)) {
        self.usize(values.len());
        for value in values {
            let mut element = Self::nested();
            encode(&mut element, value);
            self.blob(&element.bytes);
        }
    }

    pub(super) fn option<T>(&mut self, value: Option<&T>, encode: impl FnOnce(&mut Self, &T)) {
        match value {
            Some(value) => {
                self.u8(1);
                let mut payload = Self::nested();
                encode(&mut payload, value);
                self.blob(&payload.bytes);
            }
            None => self.u8(0),
        }
    }

    pub(super) fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub(super) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn usize(&mut self, value: usize) {
        // Rust supports at most 64-bit address spaces, so this widening is
        // lossless on every target supported by the compiler.
        self.u64(value as u64);
    }

    pub(super) fn string(&mut self, value: &str) {
        self.blob(value.as_bytes());
    }

    pub(super) fn blob(&mut self, value: &[u8]) {
        self.usize(value.len());
        self.bytes.extend_from_slice(value);
    }

    pub(super) fn finish(self) -> Fingerprint {
        Fingerprint::from_bytes(Sha256::digest(self.bytes).into())
    }
}

pub(super) fn def_id(encoder: &mut Encoder, value: DefId) {
    encoder.blob(value.canonical_bytes());
}

pub(super) fn package_id(encoder: &mut Encoder, value: PackageId) {
    encoder.blob(value.canonical_bytes());
}

pub(super) fn qualified_def_id(encoder: &mut Encoder, value: QualifiedDefId) {
    encoder.field(b"package", |encoder| package_id(encoder, value.package()));
    encoder.field(b"definition", |encoder| def_id(encoder, value.definition()));
}

pub(super) fn node_id(encoder: &mut Encoder, value: NodeId) {
    encoder.field(b"owner", |encoder| def_id(encoder, value.owner()));
    encoder.field(b"local", |encoder| encoder.u32(value.local_index()));
}

pub(super) fn local_id(encoder: &mut Encoder, value: LocalId) {
    encoder.u32(value.index());
}

pub(super) fn local_type_id(encoder: &mut Encoder, value: LocalTypeId) {
    encoder.field(b"owner", |encoder| def_id(encoder, value.owner()));
    encoder.field(b"local", |encoder| encoder.u32(value.local_index()));
}

pub(super) fn closure_id(encoder: &mut Encoder, value: ClosureId) {
    encoder.u32(value.index());
}

pub(super) fn block_id(encoder: &mut Encoder, value: BasicBlockId) {
    encoder.u32(value.index());
}

pub(super) fn source_ref(encoder: &mut Encoder, source: SourceRef) {
    match source.kind() {
        SourceRefKind::Definition => {
            encoder.variant(b"definition", |encoder| def_id(encoder, source.owner()));
        }
        SourceRefKind::Node(node) => {
            encoder.variant(b"node", |encoder| node_id(encoder, node));
        }
        SourceRefKind::Local(local) => encoder.variant(b"local", |encoder| {
            encoder.field(b"owner", |encoder| def_id(encoder, source.owner()));
            encoder.field(b"local", |encoder| local_id(encoder, local));
        }),
    }
}

pub(super) fn signature(encoder: &mut Encoder, value: &Signature) {
    encoder.field(b"params", |encoder| encoder.sequence(&value.params, ty));
    encoder.field(b"results", |encoder| encoder.sequence(&value.results, ty));
    encoder.field(b"variadic", |encoder| encoder.bool(value.variadic));
}

pub(super) fn ty(encoder: &mut Encoder, value: &Ty) {
    match value {
        Ty::Unit => encoder.variant(b"unit", |_| {}),
        Ty::Bool => encoder.variant(b"bool", |_| {}),
        Ty::Int(value) => encoder.variant(b"int", |encoder| int_ty(encoder, *value)),
        Ty::Uint(value) => encoder.variant(b"uint", |encoder| uint_ty(encoder, *value)),
        Ty::Float(value) => encoder.variant(b"float", |encoder| float_ty(encoder, *value)),
        Ty::Complex(value) => encoder.variant(b"complex", |encoder| complex_ty(encoder, *value)),
        Ty::Named {
            definition,
            underlying,
        } => encoder.variant(b"named", |encoder| {
            encoder.field(b"definition", |encoder| def_id(encoder, *definition));
            encoder.field(b"underlying", |encoder| ty(encoder, underlying));
        }),
        Ty::NamedRef { definition } => encoder.variant(b"named-ref", |encoder| {
            def_id(encoder, *definition);
        }),
        Ty::LocalNamed {
            identity,
            underlying,
        } => encoder.variant(b"local-named", |encoder| {
            encoder.field(b"identity", |encoder| local_type_id(encoder, *identity));
            encoder.field(b"underlying", |encoder| ty(encoder, underlying));
        }),
        Ty::Struct(fields) => {
            encoder.variant(b"struct", |encoder| encoder.sequence(fields, struct_field));
        }
        Ty::Interface(methods) => encoder.variant(b"interface", |encoder| {
            encoder.sequence(methods, interface_method);
        }),
        Ty::Function(value) => encoder.variant(b"function", |encoder| signature(encoder, value)),
        Ty::String => encoder.variant(b"string", |_| {}),
        Ty::Pointer(element) => encoder.variant(b"pointer", |encoder| ty(encoder, element)),
        Ty::Array(length, element) => encoder.variant(b"array", |encoder| {
            encoder.field(b"length", |encoder| encoder.u64(*length));
            encoder.field(b"element", |encoder| ty(encoder, element));
        }),
        Ty::Slice(element) => encoder.variant(b"slice", |encoder| ty(encoder, element)),
        Ty::Map(key, value) => encoder.variant(b"map", |encoder| {
            ty(encoder, key);
            ty(encoder, value);
        }),
        Ty::Channel(direction, element) => encoder.variant(b"channel", |encoder| {
            encoder.field(b"direction", |encoder| channel_dir(encoder, *direction));
            encoder.field(b"element", |encoder| ty(encoder, element));
        }),
        Ty::Tuple(values) => {
            encoder.variant(b"tuple", |encoder| encoder.sequence(values, ty));
        }
        Ty::Untyped(value) => {
            encoder.variant(b"untyped", |encoder| untyped_ty(encoder, *value));
        }
    }
}

fn struct_field(encoder: &mut Encoder, value: &StructField) {
    encoder.field(b"name", |encoder| encoder.string(&value.name));
    encoder.field(b"type", |encoder| ty(encoder, &value.ty));
    encoder.field(b"embedded", |encoder| encoder.bool(value.embedded));
    encoder.field(b"tag", |encoder| {
        encoder.option(value.tag.as_ref(), |encoder, value| encoder.string(value));
    });
}

fn interface_method(encoder: &mut Encoder, value: &InterfaceMethod) {
    encoder.field(b"name", |encoder| encoder.string(&value.name));
    encoder.field(b"signature", |encoder| signature(encoder, &value.signature));
}

fn channel_dir(encoder: &mut Encoder, direction: ChannelDir) {
    encoder.variant(
        match direction {
            ChannelDir::SendReceive => b"send-receive",
            ChannelDir::SendOnly => b"send-only",
            ChannelDir::ReceiveOnly => b"receive-only",
        },
        |_| {},
    );
}

fn int_ty(encoder: &mut Encoder, value: IntTy) {
    encoder.variant(
        match value {
            IntTy::Int => b"int",
            IntTy::Int8 => b"int8",
            IntTy::Int16 => b"int16",
            IntTy::Int32 => b"int32",
            IntTy::Int64 => b"int64",
        },
        |_| {},
    );
}

fn uint_ty(encoder: &mut Encoder, value: UintTy) {
    encoder.variant(
        match value {
            UintTy::Uint => b"uint",
            UintTy::Uint8 => b"uint8",
            UintTy::Uint16 => b"uint16",
            UintTy::Uint32 => b"uint32",
            UintTy::Uint64 => b"uint64",
            UintTy::Uintptr => b"uintptr",
        },
        |_| {},
    );
}

fn float_ty(encoder: &mut Encoder, value: FloatTy) {
    encoder.variant(
        match value {
            FloatTy::Float32 => b"float32",
            FloatTy::Float64 => b"float64",
        },
        |_| {},
    );
}

fn complex_ty(encoder: &mut Encoder, value: ComplexTy) {
    encoder.variant(
        match value {
            ComplexTy::Complex64 => b"complex64",
            ComplexTy::Complex128 => b"complex128",
        },
        |_| {},
    );
}

fn untyped_ty(encoder: &mut Encoder, value: UntypedTy) {
    encoder.variant(
        match value {
            UntypedTy::Bool => b"bool",
            UntypedTy::Int => b"int",
            UntypedTy::Rune => b"rune",
            UntypedTy::Float => b"float",
            UntypedTy::Complex => b"complex",
            UntypedTy::String => b"string",
        },
        |_| {},
    );
}

pub(super) fn const_value(encoder: &mut Encoder, value: &ConstValue) {
    match value {
        ConstValue::Bool(value) => {
            encoder.variant(b"bool", |encoder| encoder.bool(*value));
        }
        ConstValue::Int(value) => {
            encoder.variant(b"exact-int", |encoder| encoder.string(value));
        }
        ConstValue::Float(value) => {
            encoder.variant(b"exact-float", |encoder| exact_number(encoder, value));
        }
        ConstValue::Complex { real, imag } => encoder.variant(b"exact-complex", |encoder| {
            encoder.field(b"real", |encoder| exact_number(encoder, real));
            encoder.field(b"imag", |encoder| exact_number(encoder, imag));
        }),
        ConstValue::String(value) => {
            encoder.variant(b"go-string-bytes", |encoder| encoder.blob(value));
        }
    }
}

fn exact_number(encoder: &mut Encoder, value: &crate::compiler::types::ExactNumber) {
    encoder.field(b"numerator", |encoder| {
        encoder.string(&value.numerator_spelling())
    });
    encoder.field(b"denominator", |encoder| {
        encoder.string(&value.denominator_spelling())
    });
}

pub(super) fn hir_effects(encoder: &mut Encoder, effects: hir::Effects) {
    encoder.field(b"may-read", |encoder| encoder.bool(effects.may_read));
    encoder.field(b"may-call", |encoder| encoder.bool(effects.may_call));
    encoder.field(b"may-allocate", |encoder| {
        encoder.bool(effects.may_allocate)
    });
    encoder.field(b"may-block", |encoder| encoder.bool(effects.may_block));
    encoder.field(b"may-panic", |encoder| encoder.bool(effects.may_panic));
    encoder.field(b"may-write", |encoder| encoder.bool(effects.may_write));
}
