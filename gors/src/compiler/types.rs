//! Exact semantic types for the initial compiler slice.
//!
//! Adding a supported Go construct extends this type algebra directly; it must
//! never be represented by an unknown sentinel plus a compensating side table.

mod exact_float;
mod exact_number;
mod named;

use num_bigint::BigInt;

use super::ids::LocalTypeId;

pub use exact_number::ExactNumber;
pub use named::NamedTypeId;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Ty {
    Unit,
    Bool,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Complex(ComplexTy),
    Named {
        identity: NamedTypeId,
        underlying: Box<Ty>,
    },
    /// A finite reference back to a package named type from within its own
    /// recursively guarded underlying representation.
    NamedRef {
        identity: NamedTypeId,
    },
    LocalNamed {
        identity: LocalTypeId,
        underlying: Box<Ty>,
    },
    Struct(Vec<StructField>),
    Interface(Vec<InterfaceMethod>),
    Function(Signature),
    String,
    Pointer(Box<Ty>),
    Array(u64, Box<Ty>),
    Slice(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Channel(ChannelDir, Box<Ty>),
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
pub enum ComplexTy {
    Complex64,
    Complex128,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChannelDir {
    SendReceive,
    SendOnly,
    ReceiveOnly,
}

impl ChannelDir {
    #[must_use]
    pub const fn can_send(self) -> bool {
        matches!(self, Self::SendReceive | Self::SendOnly)
    }

    #[must_use]
    pub const fn can_receive(self) -> bool {
        matches!(self, Self::SendReceive | Self::ReceiveOnly)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UntypedTy {
    Bool,
    Int,
    Rune,
    Float,
    Complex,
    String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstValue {
    Bool(bool),
    /// Canonical base-ten representation.  Constants remain exact until the
    /// semantic layer assigns a concrete destination type.
    Int(String),
    /// A canonical reduced rational. IEEE rounding happens only once the
    /// semantic layer selects a concrete floating-point type.
    Float(ExactNumber),
    /// Exact canonical real and imaginary rational components.
    Complex {
        real: ExactNumber,
        imag: ExactNumber,
    },
    /// Go strings are bytes, not Rust UTF-8 strings.
    String(Vec<u8>),
}

/// A fully evaluated immutable package initializer.
///
/// This is distinct from a Go constant: aggregate values are permitted here,
/// while every leaf still retains its exact constant representation until its
/// declared Go type is known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticValue {
    Constant(ConstValue),
    /// The zero value of a nil-capable package variable. Its exact runtime
    /// operation remains a MIR representation decision keyed by the Go type.
    Nil,
    Struct(Vec<StaticValue>),
    Array(Vec<StaticValue>),
    Slice(Vec<StaticValue>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Signature {
    pub params: Vec<Ty>,
    pub results: Vec<Ty>,
    pub variadic: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StructField {
    pub name: String,
    pub ty: Ty,
    pub embedded: bool,
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InterfaceMethod {
    pub name: String,
    pub signature: Signature,
}

impl Ty {
    /// A function shape whose proven capture-only representation can be
    /// lowered to one copied integer payload.
    #[must_use]
    pub fn snapshot_function_result(&self) -> Option<&Ty> {
        let Self::Function(signature) = self.underlying() else {
            return None;
        };
        if signature.variadic || !signature.params.is_empty() {
            return None;
        }
        let [result] = signature.results.as_slice() else {
            return None;
        };
        matches!(result.underlying(), Self::Int(IntTy::Int)).then_some(result)
    }

    /// The element type of a slice of capture-only functions.
    #[must_use]
    pub fn snapshot_function_slice_element(&self) -> Option<&Ty> {
        let Self::Slice(element) = self.underlying() else {
            return None;
        };
        element.snapshot_function_result().map(|_| element.as_ref())
    }

    #[must_use]
    pub fn underlying(&self) -> &Ty {
        match self {
            Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } => {
                underlying.underlying()
            }
            other => other,
        }
    }

    /// Canonical runtime identity for values stored behind a Go interface.
    #[must_use]
    pub fn dynamic_type_identity(&self) -> Option<Vec<u8>> {
        let identity = match self {
            Self::Bool => "builtin:bool".to_owned(),
            Self::Int(IntTy::Int) => "builtin:int".to_owned(),
            Self::Int(IntTy::Int8) => "builtin:int8".to_owned(),
            Self::Int(IntTy::Int16) => "builtin:int16".to_owned(),
            Self::Int(IntTy::Int32) => "builtin:int32".to_owned(),
            Self::Int(IntTy::Int64) => "builtin:int64".to_owned(),
            Self::Uint(UintTy::Uint) => "builtin:uint".to_owned(),
            Self::Uint(UintTy::Uint8) => "builtin:uint8".to_owned(),
            Self::Uint(UintTy::Uint16) => "builtin:uint16".to_owned(),
            Self::Uint(UintTy::Uint32) => "builtin:uint32".to_owned(),
            Self::Uint(UintTy::Uint64) => "builtin:uint64".to_owned(),
            Self::Uint(UintTy::Uintptr) => "builtin:uintptr".to_owned(),
            Self::Float(FloatTy::Float32) => "builtin:float32".to_owned(),
            Self::Float(FloatTy::Float64) => "builtin:float64".to_owned(),
            Self::String => "builtin:string".to_owned(),
            Self::Named { identity, .. } | Self::NamedRef { identity } => {
                return identity.dynamic_identity();
            }
            Self::LocalNamed { identity, .. } => format!("local-named:{identity}"),
            Self::Pointer(element) => match element.as_ref() {
                Self::Int(IntTy::Int) => "pointer:builtin:int".to_owned(),
                Self::Named { identity, .. } => {
                    return Some(container_dynamic_identity(
                        b"pointer",
                        &identity.dynamic_identity()?,
                    ));
                }
                Self::LocalNamed { identity, .. } => format!("pointer:local-named:{identity}"),
                _ => return None,
            },
            Self::Slice(element) => match element.as_ref() {
                Self::String => "slice:builtin:string".to_owned(),
                Self::Named { identity, .. } | Self::NamedRef { identity } => {
                    return Some(container_dynamic_identity(
                        b"slice",
                        &identity.dynamic_identity()?,
                    ));
                }
                Self::LocalNamed { identity, .. } => format!("slice:local-named:{identity}"),
                _ => return None,
            },
            _ => return None,
        };
        Some(identity.into_bytes())
    }

    pub fn default_typed(&self) -> Ty {
        match self {
            Self::Untyped(UntypedTy::Bool) => Self::Bool,
            Self::Untyped(UntypedTy::Int) => Self::Int(IntTy::Int),
            Self::Untyped(UntypedTy::Rune) => Self::Int(IntTy::Int32),
            Self::Untyped(UntypedTy::Float) => Self::Float(FloatTy::Float64),
            Self::Untyped(UntypedTy::Complex) => Self::Complex(ComplexTy::Complex128),
            Self::Untyped(UntypedTy::String) => Self::String,
            other => other.clone(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        if let Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } = self {
            return underlying.is_numeric();
        }
        matches!(
            self,
            Self::Int(_)
                | Self::Uint(_)
                | Self::Float(_)
                | Self::Complex(_)
                | Self::Untyped(
                    UntypedTy::Int | UntypedTy::Rune | UntypedTy::Float | UntypedTy::Complex
                )
        )
    }

    pub fn is_integer(&self) -> bool {
        if let Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } = self {
            return underlying.is_integer();
        }
        matches!(
            self,
            Self::Int(_) | Self::Uint(_) | Self::Untyped(UntypedTy::Int | UntypedTy::Rune)
        )
    }

    /// Fields of a struct whose executable representation is a fixed array of
    /// Go `int` values.
    #[must_use]
    pub fn bootstrap_i64_struct_fields(&self) -> Option<&[StructField]> {
        let Self::Struct(fields) = self.underlying() else {
            return None;
        };
        fields
            .iter()
            .all(|field| field.ty.underlying() == &Self::Int(IntTy::Int))
            .then_some(fields)
    }

    /// Fields of the integer struct referenced by an executable Go pointer.
    #[must_use]
    pub fn bootstrap_i64_struct_pointer_fields(&self) -> Option<&[StructField]> {
        let Self::Pointer(element) = self.underlying() else {
            return None;
        };
        element.bootstrap_i64_struct_fields()
    }

    /// Values stored in the canonical interface-backed aggregate containers.
    #[must_use]
    pub fn uses_interface_aggregate_representation(&self) -> bool {
        self.dynamic_type_identity().is_some()
            && (matches!(self, Self::String | Self::NamedRef { .. })
                || self.interface_aggregate_struct_fields().is_some())
    }

    /// Fields of a struct that can be copied to and reconstructed from the
    /// canonical tagged aggregate snapshot.
    #[must_use]
    pub fn interface_aggregate_struct_fields(&self) -> Option<&[StructField]> {
        let Self::Struct(fields) = self.underlying() else {
            return None;
        };
        fields
            .iter()
            .all(|field| {
                field.ty.dynamic_type_identity().is_some() && field.ty.supports_interface_payload()
            })
            .then_some(fields)
    }

    /// Whether this exact value has a complete tagged interface payload
    /// encoding and decoding path.
    #[must_use]
    pub fn supports_interface_payload(&self) -> bool {
        match self.underlying() {
            Self::Bool | Self::Int(_) | Self::Uint(_) | Self::Float(_) | Self::String => true,
            Self::Struct(_) => self.interface_aggregate_struct_fields().is_some(),
            Self::Slice(element) => {
                element.underlying() == &Self::String
                    || element.uses_interface_aggregate_representation()
            }
            Self::Pointer(element) => {
                element.underlying() == &Self::Int(IntTy::Int)
                    || self.bootstrap_i64_struct_pointer_fields().is_some()
            }
            _ => false,
        }
    }

    /// Struct values whose pointers retain an identity-bearing tagged aggregate
    /// snapshot.
    #[must_use]
    pub fn uses_interface_aggregate_pointer_representation(&self) -> bool {
        self.interface_aggregate_struct_fields().is_some()
            && Self::Pointer(Box::new(self.clone()))
                .dynamic_type_identity()
                .is_some()
    }

    /// Values supported by the current executable representation without
    /// relying on target-dependent or incomplete numeric semantics.
    pub fn is_bootstrap_value(&self) -> bool {
        if let Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } = self {
            return underlying.is_bootstrap_value();
        }
        if let Self::Slice(element) = self {
            return matches!(
                element.underlying(),
                Self::Bool
                    | Self::Int(IntTy::Int | IntTy::Int32)
                    | Self::Uint(UintTy::Uint8)
                    | Self::String
                    | Self::Interface(_)
            ) || element.uses_interface_aggregate_representation()
                || element.snapshot_function_result().is_some();
        }
        if let Self::Pointer(element) = self {
            return element.underlying() == &Self::Int(IntTy::Int)
                || element.bootstrap_i64_struct_fields().is_some()
                || element.uses_interface_aggregate_pointer_representation();
        }
        if let Self::Array(length, element) = self {
            if *length == 0 {
                return true;
            }
            return matches!(
                element.underlying(),
                Self::Bool
                    | Self::Int(IntTy::Int)
                    | Self::Uint(UintTy::Uint8)
                    | Self::Float(_)
                    | Self::String
            ) || element.bootstrap_i64_struct_pointer_fields().is_some();
        }
        if let Self::Map(key, value) = self {
            return (key.underlying() == &Self::String
                && (value.underlying() == &Self::Int(IntTy::Int)
                    || value.bootstrap_i64_struct_fields().is_some()))
                || (key.underlying() == &Self::Int(IntTy::Int)
                    && value.underlying() == &Self::String);
        }
        if let Self::Channel(_, element) = self {
            return matches!(element.underlying(), Self::Int(IntTy::Int) | Self::String)
                || matches!(
                    element.underlying(),
                    Self::Channel(_, nested) if nested.underlying() == &Self::Int(IntTy::Int)
                );
        }
        if let Self::Tuple(elements) = self {
            return elements.iter().all(Self::is_bootstrap_value);
        }
        if let Self::Interface(_) = self.underlying() {
            return true;
        }
        if let Self::Struct(fields) = self {
            return fields.iter().all(|field| field.ty.is_bootstrap_value());
        }
        if matches!(self, Self::NamedRef { .. }) {
            return true;
        }
        if matches!(self, Self::Function(_)) {
            return true;
        }
        matches!(
            self,
            Self::Bool
                | Self::Int(_)
                | Self::Uint(_)
                | Self::Float(_)
                | Self::Complex(_)
                | Self::String
        )
    }

    /// Aggregate values whose current executable representation preserves Go
    /// comparability through Rust's fixed-array equality.
    pub fn is_bootstrap_comparable_aggregate(&self) -> bool {
        match self.underlying() {
            Self::Array(_, element) => matches!(
                element.underlying(),
                Self::Int(IntTy::Int) | Self::Uint(UintTy::Uint8)
            ),
            Self::Struct(fields) => fields
                .iter()
                .all(|field| field.ty.underlying() == &Self::Int(IntTy::Int)),
            _ => false,
        }
    }

    /// Whether Go permits equality on values of this type.
    #[must_use]
    pub fn is_go_comparable(&self) -> bool {
        match self.underlying() {
            Self::Unit
            | Self::NamedRef { .. }
            | Self::Slice(_)
            | Self::Map(_, _)
            | Self::Function(_)
            | Self::Tuple(_) => false,
            Self::Array(_, element) => element.is_go_comparable(),
            Self::Struct(fields) => fields.iter().all(|field| field.ty.is_go_comparable()),
            Self::Bool
            | Self::Int(_)
            | Self::Uint(_)
            | Self::Float(_)
            | Self::Complex(_)
            | Self::String
            | Self::Pointer(_)
            | Self::Channel(_, _)
            | Self::Interface(_)
            | Self::Untyped(_) => true,
            Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } => {
                underlying.is_go_comparable()
            }
        }
    }

    pub fn zero(&self) -> Option<ConstValue> {
        if let Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } = self {
            return underlying.zero();
        }
        match self.default_typed() {
            Self::Bool => Some(ConstValue::Bool(false)),
            Self::Int(_) | Self::Uint(_) => Some(ConstValue::Int("0".into())),
            Self::Float(_) => Some(ConstValue::Float(ExactNumber::zero())),
            Self::Complex(_) => Some(ConstValue::Complex {
                real: ExactNumber::zero(),
                imag: ExactNumber::zero(),
            }),
            Self::String => Some(ConstValue::String(Vec::new())),
            Self::Struct(_) | Self::Interface(_) | Self::Function(_) => None,
            Self::Named { underlying, .. } | Self::LocalNamed { underlying, .. } => {
                underlying.zero()
            }
            Self::NamedRef { .. } => None,
            Self::Unit
            | Self::Pointer(_)
            | Self::Array(_, _)
            | Self::Slice(_)
            | Self::Map(_, _)
            | Self::Channel(_, _)
            | Self::Tuple(_)
            | Self::Untyped(_) => None,
        }
    }
}

fn container_dynamic_identity(kind: &[u8], element: &[u8]) -> Vec<u8> {
    let mut identity = Vec::with_capacity(kind.len() + element.len() + 16);
    // Rust supports at most 64-bit address spaces, so these widenings are
    // lossless on every target supported by the compiler.
    let kind_len = kind.len() as u64;
    let element_len = element.len() as u64;
    identity.extend_from_slice(&kind_len.to_be_bytes());
    identity.extend_from_slice(kind);
    identity.extend_from_slice(&element_len.to_be_bytes());
    identity.extend_from_slice(element);
    identity
}

impl ConstValue {
    /// Exact value converted to the canonical constant representation of `ty`.
    /// Typed floating-point and complex constants are rounded exactly once at
    /// every typing boundary, as required by Go's constant rules.
    #[must_use]
    pub fn normalized_for(&self, ty: &Ty) -> Self {
        match (self, ty.underlying()) {
            (
                Self::Float(_),
                Ty::Int(_) | Ty::Uint(_) | Ty::Untyped(UntypedTy::Int | UntypedTy::Rune),
            ) => self.exact_integer().unwrap_or_else(|| self.clone()),
            (Self::Int(_) | Self::Float(_), Ty::Float(float_ty)) => self
                .quantized_for_float(*float_ty)
                .map(Self::Float)
                .unwrap_or_else(|| self.clone()),
            (Self::Int(_) | Self::Float(_), Ty::Complex(complex_ty)) => self
                .quantized_for_float(complex_ty.component_type())
                .map(|real| Self::Complex {
                    real,
                    imag: ExactNumber::zero(),
                })
                .unwrap_or_else(|| self.clone()),
            (Self::Complex { real, imag }, Ty::Complex(complex_ty)) => {
                let component_ty = complex_ty.component_type();
                let quantize = |value: &ExactNumber| {
                    Self::Float(value.clone()).quantized_for_float(component_ty)
                };
                match (quantize(real), quantize(imag)) {
                    (Some(real), Some(imag)) => Self::Complex { real, imag },
                    _ => self.clone(),
                }
            }
            _ => self.clone(),
        }
    }

    /// The exact real value of an integer or floating-point constant.
    #[must_use]
    pub(crate) fn exact_number(&self) -> Option<ExactNumber> {
        match self {
            Self::Int(spelling) => ExactNumber::from_integer_spelling(spelling),
            Self::Float(value) => Some(value.clone()),
            Self::Bool(_) | Self::Complex { .. } | Self::String(_) => None,
        }
    }

    /// Convert a real-valued constant to the canonical exact integer payload.
    /// A complex value is accepted only when its imaginary component is zero.
    #[must_use]
    pub(crate) fn exact_integer(&self) -> Option<Self> {
        let value = match self {
            Self::Int(_) => return Some(self.clone()),
            Self::Float(value) => value,
            Self::Complex { real, imag } if imag.is_zero() => real,
            Self::Bool(_) | Self::Complex { .. } | Self::String(_) => return None,
        };
        value.integer_spelling().map(Self::Int)
    }

    /// Whether this exact Go constant can be materialized as `ty` by the
    /// current target representation.
    pub fn is_representable_as(&self, ty: &Ty) -> bool {
        if let Ty::Named { underlying, .. } | Ty::LocalNamed { underlying, .. } = ty {
            return self.is_representable_as(underlying);
        }
        match (self, ty) {
            (Self::Bool(_), Ty::Bool | Ty::Untyped(UntypedTy::Bool)) => true,
            (Self::String(_), Ty::String | Ty::Untyped(UntypedTy::String)) => true,
            (
                Self::Int(value),
                Ty::Untyped(
                    UntypedTy::Int | UntypedTy::Rune | UntypedTy::Float | UntypedTy::Complex,
                ),
            ) => BigInt::parse_bytes(value.as_bytes(), 10).is_some(),
            (Self::Int(value), Ty::Int(kind)) => {
                let Some(value) = BigInt::parse_bytes(value.as_bytes(), 10) else {
                    return false;
                };
                let (minimum, maximum) = match kind {
                    IntTy::Int | IntTy::Int64 => (BigInt::from(i64::MIN), BigInt::from(i64::MAX)),
                    IntTy::Int8 => (BigInt::from(i8::MIN), BigInt::from(i8::MAX)),
                    IntTy::Int16 => (BigInt::from(i16::MIN), BigInt::from(i16::MAX)),
                    IntTy::Int32 => (BigInt::from(i32::MIN), BigInt::from(i32::MAX)),
                };
                value >= minimum && value <= maximum
            }
            (Self::Int(value), Ty::Uint(kind)) => {
                let Some(value) = BigInt::parse_bytes(value.as_bytes(), 10) else {
                    return false;
                };
                let maximum = match kind {
                    UintTy::Uint | UintTy::Uint64 | UintTy::Uintptr => BigInt::from(u64::MAX),
                    UintTy::Uint8 => BigInt::from(u8::MAX),
                    UintTy::Uint16 => BigInt::from(u16::MAX),
                    UintTy::Uint32 => BigInt::from(u32::MAX),
                };
                value >= BigInt::from(0_u8) && value <= maximum
            }
            (Self::Int(_), Ty::Float(float_ty)) => self.ieee_bits_for(*float_ty).is_some(),
            (Self::Int(_), Ty::Complex(complex_ty)) => {
                self.ieee_bits_for(complex_ty.component_type()).is_some()
            }
            (Self::Float(_), Ty::Untyped(UntypedTy::Float | UntypedTy::Complex)) => true,
            (
                Self::Float(_),
                Ty::Int(_) | Ty::Uint(_) | Ty::Untyped(UntypedTy::Int | UntypedTy::Rune),
            ) => self
                .exact_integer()
                .is_some_and(|integer| integer.is_representable_as(ty)),
            (Self::Float(_), Ty::Float(float_ty)) => self.ieee_bits_for(*float_ty).is_some(),
            (Self::Float(_), Ty::Complex(complex_ty)) => {
                self.ieee_bits_for(complex_ty.component_type()).is_some()
            }
            (Self::Complex { .. }, Ty::Untyped(UntypedTy::Complex)) => true,
            (Self::Complex { real, imag }, Ty::Complex(complex_ty)) => {
                let component_ty = complex_ty.component_type();
                Self::Float(real.clone())
                    .ieee_bits_for(component_ty)
                    .is_some()
                    && Self::Float(imag.clone())
                        .ieee_bits_for(component_ty)
                        .is_some()
            }
            // Remaining source/target combinations are not representable.
            _ => false,
        }
    }
}

impl StaticValue {
    #[must_use]
    pub fn zero(ty: &Ty) -> Option<Self> {
        match ty.underlying() {
            Ty::Pointer(_) => Some(Self::Nil),
            Ty::Struct(fields) => fields
                .iter()
                .map(|field| Self::zero(&field.ty))
                .collect::<Option<Vec<_>>>()
                .map(Self::Struct),
            Ty::Array(length, element) => usize::try_from(*length)
                .ok()
                .and_then(|length| {
                    std::iter::repeat_with(|| Self::zero(element))
                        .take(length)
                        .collect::<Option<Vec<_>>>()
                })
                .map(Self::Array),
            underlying => underlying.zero().map(Self::Constant),
        }
    }

    #[must_use]
    pub fn is_representable_as(&self, ty: &Ty) -> bool {
        match (self, ty.underlying()) {
            (Self::Constant(value), ty) => value.is_representable_as(ty),
            (Self::Nil, Ty::Pointer(_)) => true,
            (Self::Struct(values), Ty::Struct(fields)) => {
                values.len() == fields.len()
                    && values
                        .iter()
                        .zip(fields)
                        .all(|(value, field)| value.is_representable_as(&field.ty))
            }
            (Self::Array(values), Ty::Array(length, element)) => {
                u64::try_from(values.len()).ok() == Some(*length)
                    && values
                        .iter()
                        .all(|value| value.is_representable_as(element))
            }
            (Self::Slice(values), Ty::Slice(element)) => values
                .iter()
                .all(|value| value.is_representable_as(element)),
            (Self::Nil | Self::Struct(_) | Self::Array(_) | Self::Slice(_), _) => false,
        }
    }
}

#[cfg(test)]
mod tests;
