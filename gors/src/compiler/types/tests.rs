use super::{ConstValue, FloatTy, parse_go_float};

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

fn bits(spelling: &str, ty: FloatTy) -> Option<u64> {
    ConstValue::Float(spelling.to_owned()).ieee_bits_for(ty)
}

#[test]
fn exact_float_quantization_rounds_integer_ties_to_even() {
    assert_eq!(bits("16777217", FloatTy::Float32), Some(0x4b80_0000));
    assert_eq!(bits("16777219", FloatTy::Float32), Some(0x4b80_0002));
    assert_eq!(
        bits("9007199254740993", FloatTy::Float64),
        Some(0x4340_0000_0000_0000)
    );
}

#[test]
fn exact_float_quantization_avoids_float64_to_float32_double_rounding() {
    let halfway = "1.000000059604644775390625";
    let just_above = "1.0000000596046447753906250000000000000001";
    assert_eq!(bits(halfway, FloatTy::Float32), Some(0x3f80_0000));
    assert_eq!(bits(just_above, FloatTy::Float32), Some(0x3f80_0001));

    let halfway = "1.00000000000000011102230246251565404236316680908203125";
    let just_above = "1.00000000000000011102230246251565404236316680908203126";
    assert_eq!(bits(halfway, FloatTy::Float64), Some(0x3ff0_0000_0000_0000));
    assert_eq!(
        bits(just_above, FloatTy::Float64),
        Some(0x3ff0_0000_0000_0001)
    );
}

#[test]
fn exact_float_quantization_handles_subnormals_and_positive_zero_underflow() {
    assert_eq!(bits("0x1p-149", FloatTy::Float32), Some(1));
    assert_eq!(bits("0x1p-150", FloatTy::Float32), Some(0));
    assert_eq!(bits("-0x1p-150", FloatTy::Float32), Some(0));
    assert_eq!(bits("0x1.000002p-150", FloatTy::Float32), Some(1));
    assert_eq!(
        bits("-0x1.000002p-150", FloatTy::Float32),
        Some(0x8000_0001)
    );
    assert_eq!(bits("0x1.fffffep-127", FloatTy::Float32), Some(0x0080_0000));

    assert_eq!(bits("0x1p-1074", FloatTy::Float64), Some(1));
    assert_eq!(bits("0x1p-1075", FloatTy::Float64), Some(0));
    assert_eq!(bits("-0x1p-1075", FloatTy::Float64), Some(0));
    assert_eq!(bits("0x1.0000000000001p-1075", FloatTy::Float64), Some(1));
    assert_eq!(
        bits("0x1.fffffffffffffp-1023", FloatTy::Float64),
        Some(0x0010_0000_0000_0000)
    );
}

#[test]
fn exact_float_quantization_rejects_rounding_to_infinity() {
    assert_eq!(bits("0x1.fffffep127", FloatTy::Float32), Some(0x7f7f_ffff));
    assert_eq!(
        bits("0x1.fffffeffffffffp127", FloatTy::Float32),
        Some(0x7f7f_ffff)
    );
    assert_eq!(bits("0x1.ffffffp127", FloatTy::Float32), None);

    assert_eq!(
        bits("0x1.fffffffffffffp1023", FloatTy::Float64),
        Some(0x7fef_ffff_ffff_ffff)
    );
    assert_eq!(
        bits("0x1.fffffffffffff7ffffffffp1023", FloatTy::Float64),
        Some(0x7fef_ffff_ffff_ffff)
    );
    assert_eq!(bits("0x1.fffffffffffff8p1023", FloatTy::Float64), None);
}
