//! Exact semantic types for the initial compiler slice.
//!
//! Adding a supported Go construct extends this type algebra directly; it must
//! never be represented by an unknown sentinel plus a compensating side table.

use num_bigint::BigInt;

use super::ids::DefId;

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
    String,
    Slice(Box<Ty>),
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
pub enum UntypedTy {
    Bool,
    Int,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub params: Vec<Ty>,
    pub results: Vec<Ty>,
    pub variadic: bool,
}

impl Ty {
    #[must_use]
    pub fn underlying(&self) -> &Ty {
        match self {
            Self::Named { underlying, .. } => underlying.underlying(),
            other => other,
        }
    }

    pub fn default_typed(&self) -> Ty {
        match self {
            Self::Untyped(UntypedTy::Bool) => Self::Bool,
            Self::Untyped(UntypedTy::Int) => Self::Int(IntTy::Int),
            Self::Untyped(UntypedTy::Float) => Self::Float(FloatTy::Float64),
            Self::Untyped(UntypedTy::Complex) => Self::Complex(ComplexTy::Complex128),
            Self::Untyped(UntypedTy::String) => Self::String,
            other => other.clone(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        if let Self::Named { underlying, .. } = self {
            return underlying.is_numeric();
        }
        matches!(
            self,
            Self::Int(_)
                | Self::Uint(_)
                | Self::Float(_)
                | Self::Complex(_)
                | Self::Untyped(UntypedTy::Int | UntypedTy::Float | UntypedTy::Complex)
        )
    }

    pub fn is_integer(&self) -> bool {
        if let Self::Named { underlying, .. } = self {
            return underlying.is_integer();
        }
        matches!(
            self,
            Self::Int(_) | Self::Uint(_) | Self::Untyped(UntypedTy::Int)
        )
    }

    /// Values the bootstrap backend can currently execute without relying on
    /// target-dependent or incomplete numeric semantics.
    pub fn is_bootstrap_value(&self) -> bool {
        if let Self::Named { underlying, .. } = self {
            return underlying.is_bootstrap_value();
        }
        if let Self::Slice(element) = self {
            return matches!(
                element.underlying(),
                Self::Int(IntTy::Int) | Self::Uint(UintTy::Uint8)
            );
        }
        matches!(
            self,
            Self::Bool
                | Self::Int(IntTy::Int)
                | Self::Float(FloatTy::Float64)
                | Self::Complex(ComplexTy::Complex128)
                | Self::String
        )
    }

    pub fn zero(&self) -> Option<ConstValue> {
        if let Self::Named { underlying, .. } = self {
            return underlying.zero();
        }
        match self.default_typed() {
            Self::Bool => Some(ConstValue::Bool(false)),
            Self::Int(_) | Self::Uint(_) => Some(ConstValue::Int("0".into())),
            Self::Float(_) => Some(ConstValue::Float("0.0".into())),
            Self::Complex(_) => Some(ConstValue::Complex {
                real: "0.0".into(),
                imag: "0.0".into(),
            }),
            Self::String => Some(ConstValue::String(Vec::new())),
            Self::Named { underlying, .. } => underlying.zero(),
            Self::Unit | Self::Slice(_) | Self::Tuple(_) | Self::Untyped(_) => None,
        }
    }
}

impl ConstValue {
    /// Whether this exact Go constant can be materialized as `ty` by the
    /// current target's bootstrap representation.
    pub fn is_representable_as(&self, ty: &Ty) -> bool {
        if let Ty::Named { underlying, .. } = ty {
            return self.is_representable_as(underlying);
        }
        match (self, ty) {
            (Self::Bool(_), Ty::Bool | Ty::Untyped(UntypedTy::Bool)) => true,
            (Self::String(_), Ty::String | Ty::Untyped(UntypedTy::String)) => true,
            (
                Self::Int(value),
                Ty::Untyped(UntypedTy::Int | UntypedTy::Float | UntypedTy::Complex),
            ) => BigInt::parse_bytes(value.as_bytes(), 10).is_some(),
            (Self::Int(value), Ty::Int(IntTy::Int)) => {
                let Some(value) = BigInt::parse_bytes(value.as_bytes(), 10) else {
                    return false;
                };
                value >= BigInt::from(i64::MIN) && value <= BigInt::from(i64::MAX)
            }
            (Self::Int(value), Ty::Uint(UintTy::Uint8)) => {
                let Some(value) = BigInt::parse_bytes(value.as_bytes(), 10) else {
                    return false;
                };
                value >= BigInt::from(u8::MIN) && value <= BigInt::from(u8::MAX)
            }
            (Self::Int(value), Ty::Float(FloatTy::Float64)) => {
                BigInt::parse_bytes(value.as_bytes(), 10)
                    .and_then(|value| value.to_string().parse::<f64>().ok())
                    .is_some_and(f64::is_finite)
            }
            (Self::Int(value), Ty::Complex(ComplexTy::Complex128)) => {
                BigInt::parse_bytes(value.as_bytes(), 10).is_some()
            }
            (Self::Float(value), Ty::Untyped(UntypedTy::Float | UntypedTy::Complex)) => {
                parse_go_float(value).is_some()
            }
            (Self::Float(value), Ty::Float(FloatTy::Float64)) => parse_go_float(value).is_some(),
            (
                Self::Complex { real, imag },
                Ty::Untyped(UntypedTy::Complex) | Ty::Complex(ComplexTy::Complex128),
            ) => parse_go_float(real).is_some() && parse_go_float(imag).is_some(),
            // Narrow integers, unsigned integers, and floating-point values
            // remain in the semantic algebra for the next frontier, but are
            // deliberately not executable yet.
            _ => false,
        }
    }
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
mod tests {
    use super::parse_go_float;

    #[test]
    fn parses_decimal_and_hexadecimal_go_float_literals() {
        for (spelling, expected) in [
            ("1.25", 1.25),
            ("1.5e1", 15.0),
            ("0x1p-2", 0.25),
            ("0x1.Fp+0", 1.9375),
            ("0X.8P+0", 0.5),
            ("-0x1p+2", -4.0),
        ] {
            assert_eq!(parse_go_float(spelling), Some(expected), "{spelling}");
        }
    }
}
