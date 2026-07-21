use std::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

/// Exact integer used while evaluating Go constants.
///
/// Go requires integer constant operations to retain at least 256 bits of
/// intermediate precision. Keeping the arbitrary-precision representation
/// behind this compiler-only type prevents generated programs from depending
/// on `num-bigint` while avoiding a second set of fixed-width overflow rules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExactInt(BigInt);

impl ExactInt {
    pub(super) fn from_i128(value: i128) -> Self {
        Self(BigInt::from(value))
    }

    pub(super) fn from_u128(value: u128) -> Self {
        Self(BigInt::from(value))
    }

    pub(super) fn parse_go_literal(value: &str) -> Option<Self> {
        let cleaned = value.replace('_', "");
        let (radix, digits) = if let Some(rest) = cleaned
            .strip_prefix("0b")
            .or_else(|| cleaned.strip_prefix("0B"))
        {
            (2, rest)
        } else if let Some(rest) = cleaned
            .strip_prefix("0o")
            .or_else(|| cleaned.strip_prefix("0O"))
        {
            (8, rest)
        } else if let Some(rest) = cleaned
            .strip_prefix("0x")
            .or_else(|| cleaned.strip_prefix("0X"))
        {
            (16, rest)
        } else if cleaned.len() > 1 && cleaned.starts_with('0') {
            (8, cleaned.trim_start_matches('0'))
        } else {
            (10, cleaned.as_str())
        };
        let digits = if digits.is_empty() { "0" } else { digits };
        BigInt::parse_bytes(digits.as_bytes(), radix).map(Self)
    }

    pub(super) fn parse_decimal(value: &str) -> Option<Self> {
        BigInt::parse_bytes(value.as_bytes(), 10).map(Self)
    }

    pub(super) fn to_i128(&self) -> Option<i128> {
        self.0.to_i128()
    }

    pub(super) fn to_u128(&self) -> Option<u128> {
        self.0.to_u128()
    }

    pub(super) fn to_usize(&self) -> Option<usize> {
        self.0.to_usize()
    }

    pub(super) fn to_f64(&self) -> Option<f64> {
        self.0.to_f64()
    }

    pub(super) fn decimal_string(&self) -> String {
        self.0.to_string()
    }

    pub(super) fn is_negative(&self) -> bool {
        self.0.sign() == num_bigint::Sign::Minus
    }

    pub(super) fn neg(&self) -> Self {
        Self(-&self.0)
    }

    pub(super) fn bit_not(&self) -> Self {
        Self(!&self.0)
    }

    pub(super) fn add(&self, rhs: &Self) -> Self {
        Self(&self.0 + &rhs.0)
    }

    pub(super) fn sub(&self, rhs: &Self) -> Self {
        Self(&self.0 - &rhs.0)
    }

    pub(super) fn mul(&self, rhs: &Self) -> Self {
        Self(&self.0 * &rhs.0)
    }

    pub(super) fn div(&self, rhs: &Self) -> Option<Self> {
        (!rhs.0.is_zero()).then(|| Self(&self.0 / &rhs.0))
    }

    pub(super) fn rem(&self, rhs: &Self) -> Option<Self> {
        (!rhs.0.is_zero()).then(|| Self(&self.0 % &rhs.0))
    }

    pub(super) fn shl(&self, rhs: &Self) -> Option<Self> {
        rhs.to_usize().map(|shift| Self(&self.0 << shift))
    }

    pub(super) fn shr(&self, rhs: &Self) -> Option<Self> {
        rhs.to_usize().map(|shift| Self(&self.0 >> shift))
    }

    pub(super) fn bit_and(&self, rhs: &Self) -> Self {
        Self(&self.0 & &rhs.0)
    }

    pub(super) fn bit_and_not(&self, rhs: &Self) -> Self {
        Self(&self.0 & !&rhs.0)
    }

    pub(super) fn bit_or(&self, rhs: &Self) -> Self {
        Self(&self.0 | &rhs.0)
    }

    pub(super) fn bit_xor(&self, rhs: &Self) -> Self {
        Self(&self.0 ^ &rhs.0)
    }

    pub(super) fn cmp(&self, rhs: &Self) -> Ordering {
        self.0.cmp(&rhs.0)
    }

    pub(super) fn fits_signed_bits(&self, bits: u32) -> bool {
        if bits == 0 {
            return false;
        }
        let limit = BigInt::from(1u8) << (bits - 1);
        self.0 >= -&limit && self.0 < limit
    }

    pub(super) fn fits_unsigned_bits(&self, bits: u32) -> bool {
        if self.is_negative() {
            return false;
        }
        self.0 < (BigInt::from(1u8) << bits)
    }
}

#[cfg(test)]
mod tests {
    use super::ExactInt;

    #[test]
    fn preserves_values_beyond_the_spec_minimum_integer_precision() {
        let one = ExactInt::from_i128(1);
        let shift = ExactInt::from_i128(255);
        let high_bit = one.shl(&shift).unwrap_or_else(|| ExactInt::from_i128(0));

        assert!(high_bit.to_i128().is_none());
        assert_eq!(
            high_bit
                .shr(&ExactInt::from_i128(254))
                .and_then(|value| value.to_i128()),
            Some(2)
        );
    }

    #[test]
    fn folds_wide_bitwise_operations_without_truncation() {
        let high_bit = ExactInt::from_i128(1)
            .shl(&ExactInt::from_i128(255))
            .unwrap_or_else(|| ExactInt::from_i128(0));
        let low_mask = ExactInt::from_i128(0xffff);
        let all_lower_bits = high_bit.sub(&ExactInt::from_i128(1));

        assert_eq!(all_lower_bits.bit_and(&low_mask).to_i128(), Some(0xffff));
        assert_eq!(
            high_bit.bit_or(&low_mask).bit_and_not(&high_bit).to_i128(),
            Some(0xffff)
        );
        assert_eq!(
            low_mask.bit_xor(&ExactInt::from_i128(0xff00)).to_i128(),
            Some(0x00ff)
        );
    }

    #[test]
    fn signed_bitwise_not_matches_go_infinite_precision_rules() {
        assert_eq!(ExactInt::from_i128(0).bit_not().to_i128(), Some(-1));
        assert_eq!(ExactInt::from_i128(-1).bit_not().to_i128(), Some(0));
    }
}
