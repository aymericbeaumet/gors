use super::*;

type BinaryIntegerFn = fn(GoInt, GoInt) -> GoInt;

struct IntegerFunctions {
    bits: u32,
    signed: bool,
    div: BinaryIntegerFn,
    rem: BinaryIntegerFn,
    shl_signed: BinaryIntegerFn,
    shr_signed: BinaryIntegerFn,
    shl_unsigned: BinaryIntegerFn,
    shr_unsigned: BinaryIntegerFn,
}

const INTEGER_FUNCTIONS: &[IntegerFunctions] = &[
    IntegerFunctions {
        bits: 8,
        signed: true,
        div: int_div_i8,
        rem: int_rem_i8,
        shl_signed: int_shl_signed_i8,
        shr_signed: int_shr_signed_i8,
        shl_unsigned: int_shl_unsigned_i8,
        shr_unsigned: int_shr_unsigned_i8,
    },
    IntegerFunctions {
        bits: 16,
        signed: true,
        div: int_div_i16,
        rem: int_rem_i16,
        shl_signed: int_shl_signed_i16,
        shr_signed: int_shr_signed_i16,
        shl_unsigned: int_shl_unsigned_i16,
        shr_unsigned: int_shr_unsigned_i16,
    },
    IntegerFunctions {
        bits: 32,
        signed: true,
        div: int_div_i32,
        rem: int_rem_i32,
        shl_signed: int_shl_signed_i32,
        shr_signed: int_shr_signed_i32,
        shl_unsigned: int_shl_unsigned_i32,
        shr_unsigned: int_shr_unsigned_i32,
    },
    IntegerFunctions {
        bits: 64,
        signed: true,
        div: int_div,
        rem: int_rem,
        shl_signed: int_shl,
        shr_signed: int_shr,
        shl_unsigned: int_shl_unsigned_i64,
        shr_unsigned: int_shr_unsigned_i64,
    },
    IntegerFunctions {
        bits: 8,
        signed: false,
        div: int_div_u8,
        rem: int_rem_u8,
        shl_signed: int_shl_signed_u8,
        shr_signed: int_shr_signed_u8,
        shl_unsigned: int_shl_unsigned_u8,
        shr_unsigned: int_shr_unsigned_u8,
    },
    IntegerFunctions {
        bits: 16,
        signed: false,
        div: int_div_u16,
        rem: int_rem_u16,
        shl_signed: int_shl_signed_u16,
        shr_signed: int_shr_signed_u16,
        shl_unsigned: int_shl_unsigned_u16,
        shr_unsigned: int_shr_unsigned_u16,
    },
    IntegerFunctions {
        bits: 32,
        signed: false,
        div: int_div_u32,
        rem: int_rem_u32,
        shl_signed: int_shl_signed_u32,
        shr_signed: int_shr_signed_u32,
        shl_unsigned: int_shl_unsigned_u32,
        shr_unsigned: int_shr_unsigned_u32,
    },
    IntegerFunctions {
        bits: 64,
        signed: false,
        div: int_div_u64,
        rem: int_rem_u64,
        shl_signed: int_shl_signed_u64,
        shr_signed: int_shr_signed_u64,
        shl_unsigned: int_shl_unsigned_u64,
        shr_unsigned: int_shr_unsigned_u64,
    },
];

fn signed_minimum(bits: u32) -> GoInt {
    if bits == 64 {
        GoInt::MIN
    } else {
        -(1_i64 << (bits - 1))
    }
}

fn unsigned_maximum(bits: u32) -> GoInt {
    if bits == 64 {
        -1
    } else {
        ((1_u64 << bits) - 1) as GoInt
    }
}

#[test]
fn division_and_remainder_match_every_exact_integer_kind() {
    for functions in INTEGER_FUNCTIONS {
        assert_eq!((functions.div)(9, 4), 2);
        assert_eq!((functions.rem)(9, 4), 1);
        if functions.signed {
            assert_eq!((functions.div)(-5, 3), -1);
            assert_eq!((functions.rem)(-5, 3), -2);
            assert_eq!((functions.div)(5, -3), -1);
            assert_eq!((functions.rem)(5, -3), 2);
            let minimum = signed_minimum(functions.bits);
            assert_eq!((functions.div)(minimum, -1), minimum);
            assert_eq!((functions.rem)(minimum, -1), 0);
        }
    }

    assert_eq!(int_div_u64(GoInt::MIN, 2), 1_i64 << 62);
    assert_eq!(int_rem_u64(-1, 2), 1);
}

#[test]
fn divide_by_zero_panics_for_every_exact_integer_kind() {
    for functions in INTEGER_FUNCTIONS {
        assert!(std::panic::catch_unwind(|| (functions.div)(1, 0)).is_err());
        assert!(std::panic::catch_unwind(|| (functions.rem)(1, 0)).is_err());
    }
}

#[test]
fn shifts_use_lhs_width_and_signedness_for_every_kind() {
    for functions in INTEGER_FUNCTIONS {
        let width = GoInt::from(functions.bits);
        assert_eq!((functions.shl_signed)(1, width), 0);
        assert_eq!((functions.shl_unsigned)(1, width), 0);
        assert_eq!((functions.shl_unsigned)(1, -1), 0);

        if functions.signed {
            let minimum = signed_minimum(functions.bits);
            assert_eq!((functions.shr_signed)(minimum, width), -1);
            assert_eq!((functions.shr_unsigned)(minimum, -1), -1);
        } else {
            let maximum = unsigned_maximum(functions.bits);
            assert_eq!((functions.shr_signed)(maximum, width), 0);
            assert_eq!((functions.shr_unsigned)(maximum, -1), 0);
            assert_eq!(
                (functions.shr_signed)(maximum, 3),
                ((maximum as u64) >> 3) as GoInt
            );
        }
    }

    assert_eq!(int_shr_unsigned_u64(GoInt::MIN, 1), 1_i64 << 62);
    assert_eq!(int_shl_unsigned_u64(1, GoInt::MIN), 0);
}

#[test]
fn negative_signed_shift_counts_panic_after_operand_admission() {
    for functions in INTEGER_FUNCTIONS {
        assert!(std::panic::catch_unwind(|| (functions.shl_signed)(1, -1)).is_err());
        assert!(std::panic::catch_unwind(|| (functions.shr_signed)(1, -1)).is_err());
    }
}
