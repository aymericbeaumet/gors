//! Exact semantic types for the initial compiler slice.
//!
//! Adding a supported Go construct extends this type algebra directly; it must
//! never be represented by an unknown sentinel plus a compensating side table.

mod exact_float;

use num_bigint::BigInt;

use super::ids::{DefId, LocalTypeId};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Ty {
    Unit,
    Bool,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Complex(ComplexTy),
    Named {
        definition: DefId,
        underlying: Box<Ty>,
    },
    /// A finite reference back to a package named type from within its own
    /// recursively guarded underlying representation.
    NamedRef {
        definition: DefId,
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
    /// The original exact Go spelling; parsing to `f64` is an emitter concern
    /// only after the semantic layer has selected float32/float64.
    Float(String),
    /// Exact real and imaginary component spellings.
    Complex {
        real: String,
        imag: String,
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
            Self::Named { definition, .. } | Self::NamedRef { definition } => {
                format!("named:{definition}")
            }
            Self::LocalNamed { identity, .. } => format!("local-named:{identity}"),
            Self::Pointer(element) => match element.as_ref() {
                Self::Named { definition, .. } => format!("pointer:named:{definition}"),
                Self::LocalNamed { identity, .. } => format!("pointer:local-named:{identity}"),
                _ => return None,
            },
            Self::Slice(element) => match element.as_ref() {
                Self::String => "slice:builtin:string".to_owned(),
                Self::Named { definition, .. } | Self::NamedRef { definition } => {
                    format!("slice:named:{definition}")
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
            Self::Pointer(_) => self.bootstrap_i64_struct_pointer_fields().is_some(),
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
                | Self::Complex(ComplexTy::Complex128)
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
            Self::Float(_) => Some(ConstValue::Int("0".into())),
            Self::Complex(_) => Some(ConstValue::Complex {
                real: "0".into(),
                imag: "0".into(),
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

impl ConstValue {
    /// Exact value converted to the canonical constant representation of `ty`.
    /// Typed floating-point and complex constants are rounded exactly once at
    /// every typing boundary, as required by Go's constant rules.
    #[must_use]
    pub fn normalized_for(&self, ty: &Ty) -> Self {
        match (self, ty.underlying()) {
            (
                Self::Float(spelling),
                Ty::Int(_) | Ty::Uint(_) | Ty::Untyped(UntypedTy::Int | UntypedTy::Rune),
            ) => exact_integer_from_number_spelling(spelling).unwrap_or_else(|| self.clone()),
            (Self::Int(_) | Self::Float(_), Ty::Float(float_ty)) => self
                .quantized_for_float(*float_ty)
                .unwrap_or_else(|| self.clone()),
            (Self::Int(_) | Self::Float(_), Ty::Complex(complex_ty)) => self
                .quantized_for_float(complex_ty.component_type())
                .unwrap_or_else(|| self.clone()),
            (Self::Complex { real, imag }, Ty::Complex(complex_ty)) => {
                let component_ty = complex_ty.component_type();
                let quantize = |spelling: &str| {
                    Self::Float(spelling.to_owned())
                        .quantized_for_float(component_ty)
                        .and_then(Self::into_number_spelling)
                };
                match (quantize(real), quantize(imag)) {
                    (Some(real), Some(imag)) => Self::Complex { real, imag },
                    _ => self.clone(),
                }
            }
            _ => self.clone(),
        }
    }

    fn into_number_spelling(self) -> Option<String> {
        match self {
            Self::Int(spelling) | Self::Float(spelling) => Some(spelling),
            Self::Bool(_) | Self::Complex { .. } | Self::String(_) => None,
        }
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
            (Self::Float(value), Ty::Untyped(UntypedTy::Float | UntypedTy::Complex)) => {
                ExactNumber::from_spelling(value).is_some()
            }
            (Self::Float(value), Ty::Int(_) | Ty::Uint(_)) => {
                exact_integer_from_number_spelling(value)
                    .is_some_and(|integer| integer.is_representable_as(ty))
            }
            (Self::Float(_), Ty::Float(float_ty)) => self.ieee_bits_for(*float_ty).is_some(),
            (Self::Float(_), Ty::Complex(complex_ty)) => {
                self.ieee_bits_for(complex_ty.component_type()).is_some()
            }
            (Self::Complex { real, imag }, Ty::Untyped(UntypedTy::Complex)) => {
                ExactNumber::from_spelling(real).is_some()
                    && ExactNumber::from_spelling(imag).is_some()
            }
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
            (Self::Struct(_) | Self::Array(_) | Self::Slice(_), _) => false,
        }
    }
}

/// Exact rational value of a Go numeric constant spelling.
///
/// The exact constant algebra keeps big-number precision through folding; the
/// denominator is always positive and the fraction is kept reduced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactNumber {
    numerator: BigInt,
    denominator: BigInt,
}

impl ExactNumber {
    /// Bounded exact decoding of a Go integer or floating-point constant
    /// spelling. `None` when the spelling is not an exact bounded number.
    pub(crate) fn from_spelling(spelling: &str) -> Option<Self> {
        const EXACT_CONSTANT_SCALE_LIMIT: i64 = 16 * 1024;
        let cleaned = spelling.replace('_', "");
        let (negative, unsigned) = match cleaned.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, cleaned.strip_prefix('+').unwrap_or(cleaned.as_str())),
        };
        let (value, power, base) = if let Some(hexadecimal) = unsigned
            .strip_prefix("0x")
            .or_else(|| unsigned.strip_prefix("0X"))
        {
            let (significand, exponent) = hexadecimal.split_once(['p', 'P'])?;
            let exponent = exponent.parse::<i64>().ok()?;
            let (integer, fraction) = significand.split_once('.').unwrap_or((significand, ""));
            if integer.is_empty() && fraction.is_empty() {
                return None;
            }
            let digits = format!("{integer}{fraction}");
            let value = BigInt::parse_bytes(digits.as_bytes(), 16)?;
            let power = exponent.checked_sub(4 * i64::try_from(fraction.len()).ok()?)?;
            (value, power, 2_u8)
        } else {
            let (significand, exponent) = match unsigned.split_once(['e', 'E']) {
                Some((significand, exponent)) => (significand, exponent.parse::<i64>().ok()?),
                None => (unsigned, 0),
            };
            let (integer, fraction) = significand.split_once('.').unwrap_or((significand, ""));
            if integer.is_empty() && fraction.is_empty() {
                return None;
            }
            let digits = format!("{integer}{fraction}");
            let value = BigInt::parse_bytes(digits.as_bytes(), 10)?;
            let power = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
            (value, power, 10)
        };
        if power.abs() > EXACT_CONSTANT_SCALE_LIMIT {
            return None;
        }
        let factor = BigInt::from(base).pow(u32::try_from(power.unsigned_abs()).ok()?);
        let (numerator, denominator) = if power >= 0 {
            (value * factor, BigInt::from(1_u8))
        } else {
            (value, factor)
        };
        let numerator = if negative { -numerator } else { numerator };
        Some(Self::reduced(numerator, denominator))
    }

    pub(crate) fn is_zero(&self) -> bool {
        use num_traits::Zero;
        self.numerator.is_zero()
    }

    pub(crate) fn add(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.denominator + &other.numerator * &self.denominator,
            &self.denominator * &other.denominator,
        )
    }

    pub(crate) fn sub(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.denominator - &other.numerator * &self.denominator,
            &self.denominator * &other.denominator,
        )
    }

    pub(crate) fn mul(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.numerator,
            &self.denominator * &other.denominator,
        )
    }

    /// Exact division. `None` when `other` is zero.
    pub(crate) fn div(&self, other: &Self) -> Option<Self> {
        use num_traits::Signed;
        if other.is_zero() {
            return None;
        }
        let mut numerator = &self.numerator * &other.denominator;
        let mut denominator = &self.denominator * &other.numerator;
        if denominator.is_negative() {
            numerator = -numerator;
            denominator = -denominator;
        }
        Some(Self::reduced(numerator, denominator))
    }

    /// The exact constant value: an integer constant when the value is
    /// integral, otherwise the exact finite decimal spelling. `None` when the
    /// value has no bounded finite decimal representation.
    pub(crate) fn to_const_value(&self) -> Option<ConstValue> {
        use num_traits::{Signed, Zero};
        const EXACT_DECIMAL_DIGIT_LIMIT: u32 = 64 * 1024;
        let one = BigInt::from(1_u8);
        if self.denominator == one {
            return Some(ConstValue::Int(self.numerator.to_string()));
        }
        let two = BigInt::from(2_u8);
        let five = BigInt::from(5_u8);
        let mut remaining = self.denominator.clone();
        let mut twos = 0_u32;
        let mut fives = 0_u32;
        while (&remaining % &two).is_zero() && twos < EXACT_DECIMAL_DIGIT_LIMIT {
            remaining /= &two;
            twos += 1;
        }
        while (&remaining % &five).is_zero() && fives < EXACT_DECIMAL_DIGIT_LIMIT {
            remaining /= &five;
            fives += 1;
        }
        if remaining != one {
            return None;
        }
        let fraction_digits = twos.max(fives);
        let scaled = &self.numerator * BigInt::from(10_u8).pow(fraction_digits) / &self.denominator;
        let sign = if scaled.is_negative() { "-" } else { "" };
        let magnitude = scaled.abs().to_string();
        let fraction_digits = fraction_digits as usize;
        let padded = format!("{magnitude:0>width$}", width = fraction_digits + 1);
        let split = padded.len() - fraction_digits;
        Some(ConstValue::Float(format!(
            "{sign}{}.{}",
            &padded[..split],
            &padded[split..]
        )))
    }

    fn reduced(numerator: BigInt, denominator: BigInt) -> Self {
        use num_traits::Zero;
        debug_assert!(!denominator.is_zero());
        let divisor = big_gcd(&numerator, &denominator);
        if divisor > BigInt::from(1_u8) {
            Self {
                numerator: numerator / &divisor,
                denominator: denominator / &divisor,
            }
        } else {
            Self {
                numerator,
                denominator,
            }
        }
    }
}

fn big_gcd(left: &BigInt, right: &BigInt) -> BigInt {
    use num_traits::{Signed, Zero};
    let mut a = left.abs();
    let mut b = right.abs();
    while !b.is_zero() {
        let remainder = &a % &b;
        a = b;
        b = remainder;
    }
    a
}

/// Exact integer value of a Go numeric constant spelling, or `None` when the
/// spelling does not represent an integer.
pub(crate) fn exact_integer_from_number_spelling(spelling: &str) -> Option<ConstValue> {
    let number = ExactNumber::from_spelling(spelling)?;
    (number.denominator == BigInt::from(1_u8))
        .then(|| ConstValue::Int(number.numerator.to_string()))
}

pub(crate) fn parse_go_float(spelling: &str) -> Option<f64> {
    let spelling = spelling.replace('_', "");
    let unsigned = spelling
        .strip_prefix('+')
        .or_else(|| spelling.strip_prefix('-'))
        .unwrap_or(&spelling);
    let sign = if spelling.starts_with('-') { -1.0 } else { 1.0 };
    if let Some(hexadecimal) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        let (significand, exponent) = hexadecimal.split_once(['p', 'P'])?;
        let exponent = exponent.parse::<i32>().ok()?;
        let (integer, fraction) = significand.split_once('.').unwrap_or((significand, ""));
        if integer.is_empty() && fraction.is_empty() {
            return None;
        }
        let mut value = 0.0;
        for digit in integer.chars() {
            value = value * 16.0 + f64::from(digit.to_digit(16)?);
        }
        let mut place = 1.0 / 16.0;
        for digit in fraction.chars() {
            value += f64::from(digit.to_digit(16)?) * place;
            place /= 16.0;
        }
        let value = sign * value * 2.0_f64.powi(exponent);
        value.is_finite().then_some(value)
    } else {
        let value = spelling.parse::<f64>().ok()?;
        value.is_finite().then_some(value)
    }
}

#[cfg(test)]
mod tests;
