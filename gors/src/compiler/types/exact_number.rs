//! Canonical arbitrary-precision rational numbers for Go constants.

use std::cmp::Ordering;
use std::fmt;

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

/// One exact real component of a Go numeric constant.
///
/// Values are always reduced, the denominator is always positive, and zero
/// has the sole representation `0/1`. Keeping these invariants in the value
/// itself lets HIR, MIR, and their fingerprints carry non-terminating
/// rationals without falling back to source spellings or premature IEEE
/// rounding.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExactNumber {
    pub(super) numerator: BigInt,
    pub(super) denominator: BigInt,
}

impl ExactNumber {
    /// Bounded exact decoding of a Go integer or floating-point constant
    /// spelling. `None` means the spelling is invalid or exceeds the explicit
    /// constant-scale bound; it never means that a valid rational merely has
    /// a repeating decimal expansion.
    pub(crate) fn from_spelling(spelling: &str) -> Option<Self> {
        const EXACT_CONSTANT_SCALE_LIMIT: u64 = 16 * 1024;
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
        if power.unsigned_abs() > EXACT_CONSTANT_SCALE_LIMIT {
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

    pub(crate) fn from_integer_spelling(spelling: &str) -> Option<Self> {
        BigInt::parse_bytes(spelling.as_bytes(), 10)
            .map(|value| Self::reduced(value, BigInt::from(1_u8)))
    }

    #[must_use]
    pub(crate) fn zero() -> Self {
        Self::reduced(BigInt::ZERO, BigInt::from(1_u8))
    }

    #[must_use]
    pub(crate) fn is_zero(&self) -> bool {
        self.numerator.is_zero()
    }

    #[must_use]
    pub(crate) fn is_integer(&self) -> bool {
        self.denominator == BigInt::from(1_u8)
    }

    #[must_use]
    pub(crate) fn integer_spelling(&self) -> Option<String> {
        self.is_integer().then(|| self.numerator.to_string())
    }

    #[must_use]
    pub(crate) fn negated(&self) -> Self {
        Self::reduced(-&self.numerator, self.denominator.clone())
    }

    #[must_use]
    pub(crate) fn add(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.denominator + &other.numerator * &self.denominator,
            &self.denominator * &other.denominator,
        )
    }

    #[must_use]
    pub(crate) fn sub(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.denominator - &other.numerator * &self.denominator,
            &self.denominator * &other.denominator,
        )
    }

    #[must_use]
    pub(crate) fn mul(&self, other: &Self) -> Self {
        Self::reduced(
            &self.numerator * &other.numerator,
            &self.denominator * &other.denominator,
        )
    }

    /// Exact division. `None` only when `other` is zero.
    #[must_use]
    pub(crate) fn div(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            return None;
        }
        Some(Self::reduced(
            &self.numerator * &other.denominator,
            &self.denominator * &other.numerator,
        ))
    }

    #[must_use]
    pub(crate) fn numerator_spelling(&self) -> String {
        self.numerator.to_string()
    }

    #[must_use]
    pub(crate) fn denominator_spelling(&self) -> String {
        self.denominator.to_string()
    }

    pub(super) fn reduced(mut numerator: BigInt, mut denominator: BigInt) -> Self {
        debug_assert!(!denominator.is_zero());
        if denominator.is_negative() {
            numerator = -numerator;
            denominator = -denominator;
        }
        if numerator.is_zero() {
            return Self {
                numerator: BigInt::ZERO,
                denominator: BigInt::from(1_u8),
            };
        }
        let divisor = big_gcd(&numerator, &denominator);
        Self {
            numerator: numerator / &divisor,
            denominator: denominator / divisor,
        }
    }
}

impl Ord for ExactNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.numerator * &other.denominator).cmp(&(&other.numerator * &self.denominator))
    }
}

impl PartialOrd for ExactNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ExactNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_integer() {
            fmt::Display::fmt(&self.numerator, formatter)
        } else {
            write!(formatter, "{}/{}", self.numerator, self.denominator)
        }
    }
}

fn big_gcd(left: &BigInt, right: &BigInt) -> BigInt {
    let mut a = left.abs();
    let mut b = right.abs();
    while !b.is_zero() {
        let remainder = &a % &b;
        a = b;
        b = remainder;
    }
    a
}
