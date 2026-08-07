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
