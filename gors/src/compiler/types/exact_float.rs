//! Exact conversion of Go constants to IEEE 754 binary formats.

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use super::{ComplexTy, ConstValue, ExactNumber, FloatTy};

impl ComplexTy {
    pub(crate) const fn component_type(self) -> FloatTy {
        match self {
            Self::Complex64 => FloatTy::Float32,
            Self::Complex128 => FloatTy::Float64,
        }
    }
}

#[derive(Clone, Copy)]
struct BinaryFormat {
    precision: i64,
    minimum_exponent: i64,
    maximum_exponent: i64,
    exponent_bias: i64,
    exponent_shift: u32,
    sign_shift: u32,
}

impl BinaryFormat {
    const fn for_type(ty: FloatTy) -> Self {
        match ty {
            FloatTy::Float32 => Self {
                precision: 24,
                minimum_exponent: -126,
                maximum_exponent: 127,
                exponent_bias: 127,
                exponent_shift: 23,
                sign_shift: 31,
            },
            FloatTy::Float64 => Self {
                precision: 53,
                minimum_exponent: -1022,
                maximum_exponent: 1023,
                exponent_bias: 1023,
                exponent_shift: 52,
                sign_shift: 63,
            },
        }
    }
}

struct RoundedFloat {
    value: ExactNumber,
    bits: u64,
}

impl ConstValue {
    pub(crate) fn ieee_bits_for(&self, ty: FloatTy) -> Option<u64> {
        let spelling = match self {
            Self::Int(spelling) | Self::Float(spelling) => spelling,
            Self::Bool(_) | Self::Complex { .. } | Self::String(_) => return None,
        };
        ExactNumber::from_spelling(spelling)
            .and_then(|value| value.round_to_ieee(ty))
            .map(|rounded| rounded.bits)
    }

    pub(super) fn quantized_for_float(&self, ty: FloatTy) -> Option<Self> {
        let spelling = match self {
            Self::Int(spelling) | Self::Float(spelling) => spelling,
            Self::Bool(_) | Self::Complex { .. } | Self::String(_) => return None,
        };
        ExactNumber::from_spelling(spelling)?
            .round_to_ieee(ty)?
            .value
            .to_const_value()
    }
}

impl ExactNumber {
    fn round_to_ieee(&self, ty: FloatTy) -> Option<RoundedFloat> {
        let format = BinaryFormat::for_type(ty);
        if self.numerator.is_zero() {
            return Some(RoundedFloat {
                value: Self::reduced(BigInt::ZERO, BigInt::from(1_u8)),
                bits: 0,
            });
        }

        let negative = self.numerator.is_negative();
        let magnitude = self.numerator.abs();
        let mut exponent = floor_binary_exponent(&magnitude, &self.denominator)?;
        let minimum_quantum = format.minimum_exponent - (format.precision - 1);
        let hidden_bit = BigInt::from(1_u8) << usize::try_from(format.precision - 1).ok()?;

        let (significand, value_exponent, biased_exponent) = if exponent < format.minimum_exponent {
            let significand =
                round_ratio_power_of_two(&magnitude, &self.denominator, -minimum_quantum)?;
            if significand.is_zero() {
                // Go constants have no negative zero. This also makes a
                // negative value that underflows indistinguishable from 0.
                return Some(RoundedFloat {
                    value: Self::reduced(BigInt::ZERO, BigInt::from(1_u8)),
                    bits: 0,
                });
            }
            let biased_exponent = i64::from(significand == hidden_bit);
            (significand, minimum_quantum, biased_exponent)
        } else {
            let mut significand = round_ratio_power_of_two(
                &magnitude,
                &self.denominator,
                format.precision - 1 - exponent,
            )?;
            let carry = &hidden_bit << 1_usize;
            if significand == carry {
                significand >>= 1_usize;
                exponent += 1;
            }
            if exponent > format.maximum_exponent {
                return None;
            }
            (
                significand,
                exponent - (format.precision - 1),
                exponent + format.exponent_bias,
            )
        };

        let fraction = if biased_exponent == 0 {
            significand.to_u64()?
        } else {
            (&significand - &hidden_bit).to_u64()?
        };
        let exponent_bits = u64::try_from(biased_exponent).ok()? << format.exponent_shift;
        let sign_bits = u64::from(negative) << format.sign_shift;
        let signed_significand = if negative { -significand } else { significand };
        let value = if value_exponent >= 0 {
            Self::reduced(
                signed_significand << usize::try_from(value_exponent).ok()?,
                BigInt::from(1_u8),
            )
        } else {
            Self::reduced(
                signed_significand,
                BigInt::from(1_u8) << usize::try_from(-value_exponent).ok()?,
            )
        };
        Some(RoundedFloat {
            value,
            bits: sign_bits | exponent_bits | fraction,
        })
    }
}

fn floor_binary_exponent(numerator: &BigInt, denominator: &BigInt) -> Option<i64> {
    debug_assert!(numerator > &BigInt::ZERO);
    debug_assert!(denominator > &BigInt::ZERO);
    let mut exponent =
        i64::try_from(numerator.bits()).ok()? - i64::try_from(denominator.bits()).ok()?;
    let below_candidate = if exponent >= 0 {
        numerator < &(denominator << usize::try_from(exponent).ok()?)
    } else {
        &(numerator << usize::try_from(-exponent).ok()?) < denominator
    };
    if below_candidate {
        exponent -= 1;
    }
    Some(exponent)
}

fn round_ratio_power_of_two(
    numerator: &BigInt,
    denominator: &BigInt,
    power: i64,
) -> Option<BigInt> {
    let (numerator, denominator) = if power >= 0 {
        (
            numerator << usize::try_from(power).ok()?,
            denominator.clone(),
        )
    } else {
        (
            numerator.clone(),
            denominator << usize::try_from(-power).ok()?,
        )
    };
    let quotient = &numerator / &denominator;
    let remainder = numerator % &denominator;
    let twice_remainder = &remainder << 1_usize;
    let quotient_is_odd = (&quotient % BigInt::from(2_u8)) == BigInt::from(1_u8);
    if twice_remainder > denominator || (twice_remainder == denominator && quotient_is_odd) {
        Some(quotient + 1_u8)
    } else {
        Some(quotient)
    }
}
