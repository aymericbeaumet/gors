//! Exact-width Go integer operations exposed through the runtime ABI.

use crate::GoInt;

fn mask(bits: u32) -> u64 {
    if bits == u64::BITS {
        u64::MAX
    } else {
        (1_u64 << bits) - 1
    }
}

fn canonical_signed(value: GoInt, bits: u32) -> GoInt {
    if bits == GoInt::BITS {
        value
    } else {
        let shift = GoInt::BITS - bits;
        value.wrapping_shl(shift) >> shift
    }
}

fn canonical_unsigned(value: GoInt, bits: u32) -> GoInt {
    ((value as u64) & mask(bits)) as GoInt
}

fn signed_div(left: GoInt, right: GoInt, bits: u32) -> GoInt {
    let left = canonical_signed(left, bits);
    let right = canonical_signed(right, bits);
    if right == 0 {
        integer_divide_by_zero();
    }
    let minimum = canonical_signed(1_i64.wrapping_shl(bits - 1), bits);
    if left == minimum && right == -1 {
        minimum
    } else {
        canonical_signed(left / right, bits)
    }
}

fn unsigned_div(left: GoInt, right: GoInt, bits: u32) -> GoInt {
    let left = (left as u64) & mask(bits);
    let right = (right as u64) & mask(bits);
    if right == 0 {
        integer_divide_by_zero();
    }
    canonical_unsigned((left / right) as GoInt, bits)
}

fn signed_rem(left: GoInt, right: GoInt, bits: u32) -> GoInt {
    let left = canonical_signed(left, bits);
    let right = canonical_signed(right, bits);
    if right == 0 {
        integer_divide_by_zero();
    }
    let minimum = canonical_signed(1_i64.wrapping_shl(bits - 1), bits);
    if left == minimum && right == -1 {
        0
    } else {
        canonical_signed(left % right, bits)
    }
}

fn unsigned_rem(left: GoInt, right: GoInt, bits: u32) -> GoInt {
    let left = (left as u64) & mask(bits);
    let right = (right as u64) & mask(bits);
    if right == 0 {
        integer_divide_by_zero();
    }
    canonical_unsigned((left % right) as GoInt, bits)
}

fn signed_shift_count(shift: GoInt) -> u64 {
    if shift < 0 {
        negative_shift_amount();
    }
    shift as u64
}

fn shift_left(value: GoInt, shift: u64, bits: u32, signed: bool) -> GoInt {
    if shift >= u64::from(bits) {
        return 0;
    }
    let shifted = ((value as u64) & mask(bits)) << (shift as u32);
    if signed {
        canonical_signed(shifted as GoInt, bits)
    } else {
        canonical_unsigned(shifted as GoInt, bits)
    }
}

fn shift_right(value: GoInt, shift: u64, bits: u32, signed: bool) -> GoInt {
    if shift >= u64::from(bits) {
        return if signed && canonical_signed(value, bits) < 0 {
            -1
        } else {
            0
        };
    }
    if signed {
        canonical_signed(value, bits) >> (shift as u32)
    } else {
        canonical_unsigned(
            (((value as u64) & mask(bits)) >> (shift as u32)) as GoInt,
            bits,
        )
    }
}

macro_rules! exact_width_integer_operations {
    (
        $div:ident, $rem:ident,
        $shl_signed:ident, $shr_signed:ident,
        $shl_unsigned:ident, $shr_unsigned:ident,
        $bits:expr, $signed:expr
    ) => {
        #[must_use]
        pub fn $div(left: GoInt, right: GoInt) -> GoInt {
            if $signed {
                signed_div(left, right, $bits)
            } else {
                unsigned_div(left, right, $bits)
            }
        }

        #[must_use]
        pub fn $rem(left: GoInt, right: GoInt) -> GoInt {
            if $signed {
                signed_rem(left, right, $bits)
            } else {
                unsigned_rem(left, right, $bits)
            }
        }

        #[must_use]
        pub fn $shl_signed(value: GoInt, shift: GoInt) -> GoInt {
            shift_left(value, signed_shift_count(shift), $bits, $signed)
        }

        #[must_use]
        pub fn $shr_signed(value: GoInt, shift: GoInt) -> GoInt {
            shift_right(value, signed_shift_count(shift), $bits, $signed)
        }

        #[must_use]
        pub fn $shl_unsigned(value: GoInt, shift: GoInt) -> GoInt {
            shift_left(value, shift as u64, $bits, $signed)
        }

        #[must_use]
        pub fn $shr_unsigned(value: GoInt, shift: GoInt) -> GoInt {
            shift_right(value, shift as u64, $bits, $signed)
        }
    };
}

exact_width_integer_operations!(
    int_div_i8,
    int_rem_i8,
    int_shl_signed_i8,
    int_shr_signed_i8,
    int_shl_unsigned_i8,
    int_shr_unsigned_i8,
    8,
    true
);
exact_width_integer_operations!(
    int_div_i16,
    int_rem_i16,
    int_shl_signed_i16,
    int_shr_signed_i16,
    int_shl_unsigned_i16,
    int_shr_unsigned_i16,
    16,
    true
);
exact_width_integer_operations!(
    int_div_i32,
    int_rem_i32,
    int_shl_signed_i32,
    int_shr_signed_i32,
    int_shl_unsigned_i32,
    int_shr_unsigned_i32,
    32,
    true
);
exact_width_integer_operations!(
    int_div,
    int_rem,
    int_shl,
    int_shr,
    int_shl_unsigned_i64,
    int_shr_unsigned_i64,
    64,
    true
);
exact_width_integer_operations!(
    int_div_u8,
    int_rem_u8,
    int_shl_signed_u8,
    int_shr_signed_u8,
    int_shl_unsigned_u8,
    int_shr_unsigned_u8,
    8,
    false
);
exact_width_integer_operations!(
    int_div_u16,
    int_rem_u16,
    int_shl_signed_u16,
    int_shr_signed_u16,
    int_shl_unsigned_u16,
    int_shr_unsigned_u16,
    16,
    false
);
exact_width_integer_operations!(
    int_div_u32,
    int_rem_u32,
    int_shl_signed_u32,
    int_shr_signed_u32,
    int_shl_unsigned_u32,
    int_shr_unsigned_u32,
    32,
    false
);
exact_width_integer_operations!(
    int_div_u64,
    int_rem_u64,
    int_shl_signed_u64,
    int_shr_signed_u64,
    int_shl_unsigned_u64,
    int_shr_unsigned_u64,
    64,
    false
);

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn integer_divide_by_zero() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: integer divide by zero"))
}

#[cold]
#[inline(never)]
#[allow(clippy::panic)] // This is the Go language panic boundary, not an invariant failure.
fn negative_shift_amount() -> ! {
    std::panic::resume_unwind(Box::new("runtime error: negative shift amount"))
}
