//! Exact semantic types for the initial compiler slice.
//!
//! Adding a supported Go construct extends this type algebra directly; it must
//! never be represented by an unknown sentinel plus a compensating side table.

use num_bigint::BigInt;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Ty {
    Unit,
    Bool,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    String,
    Tuple(Vec<Ty>),
    Untyped(UntypedTy),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntTy {
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UintTy {
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uintptr,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatTy {
    Float32,
    Float64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntypedTy {
    Bool,
    Int,
    Float,
    String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstValue {
    Bool(bool),
    /// Canonical base-ten representation.  Constants remain exact until the
    /// semantic layer assigns a concrete destination type.
    Int(String),
    /// The original exact Go spelling; parsing to `f64` is an emitter concern
    /// only after the semantic layer has selected float32/float64.
    Float(String),
    /// Go strings are bytes, not Rust UTF-8 strings.
    String(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub params: Vec<Ty>,
    pub results: Vec<Ty>,
}

impl Ty {
    pub fn default_typed(&self) -> Ty {
        match self {
            Self::Untyped(UntypedTy::Bool) => Self::Bool,
            Self::Untyped(UntypedTy::Int) => Self::Int(IntTy::Int),
            Self::Untyped(UntypedTy::Float) => Self::Float(FloatTy::Float64),
            Self::Untyped(UntypedTy::String) => Self::String,
            other => other.clone(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Int(_)
                | Self::Uint(_)
                | Self::Float(_)
                | Self::Untyped(UntypedTy::Int | UntypedTy::Float)
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::Int(_) | Self::Uint(_) | Self::Untyped(UntypedTy::Int)
        )
    }

    /// Values the bootstrap backend can currently execute without relying on
    /// target-dependent or incomplete numeric semantics.
    pub fn is_bootstrap_value(&self) -> bool {
        matches!(self, Self::Bool | Self::Int(IntTy::Int) | Self::String)
    }

    pub fn zero(&self) -> Option<ConstValue> {
        match self.default_typed() {
            Self::Bool => Some(ConstValue::Bool(false)),
            Self::Int(_) | Self::Uint(_) => Some(ConstValue::Int("0".into())),
            Self::Float(_) => Some(ConstValue::Float("0.0".into())),
            Self::String => Some(ConstValue::String(Vec::new())),
            Self::Unit | Self::Tuple(_) | Self::Untyped(_) => None,
        }
    }
}

impl ConstValue {
    /// Whether this exact Go constant can be materialized as `ty` by the
    /// current target's bootstrap representation.
    pub fn is_representable_as(&self, ty: &Ty) -> bool {
        match (self, ty) {
            (Self::Bool(_), Ty::Bool | Ty::Untyped(UntypedTy::Bool)) => true,
            (Self::String(_), Ty::String | Ty::Untyped(UntypedTy::String)) => true,
            (Self::Int(value), Ty::Untyped(UntypedTy::Int)) => {
                BigInt::parse_bytes(value.as_bytes(), 10).is_some()
            }
            (Self::Int(value), Ty::Int(IntTy::Int)) => {
                let Some(value) = BigInt::parse_bytes(value.as_bytes(), 10) else {
                    return false;
                };
                value >= BigInt::from(i64::MIN) && value <= BigInt::from(i64::MAX)
            }
            // Narrow integers, unsigned integers, and floating-point values
            // remain in the semantic algebra for the next frontier, but are
            // deliberately not executable yet.
            _ => false,
        }
    }
}
