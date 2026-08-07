//! Go builtin print formatting at the standard-error runtime boundary.

use std::io::Write as _;

use super::{GoInt, GoString, write_go_string_to, write_stderr_bytes};

/// Print an exact Go boolean representation.
pub fn print_bool(value: bool) {
    write_stderr_bytes(if value { b"true" } else { b"false" });
}

/// Print an exact 64-bit Go `int` representation.
pub fn print_i64(value: GoInt) {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    drop(write!(output, "{value}"));
}

/// Print Go 1.26's shortest-round-trip floating-point representation.
pub fn print_f64(value: f64) {
    let rendered = format_f64(value);
    write_stderr_bytes(rendered.as_bytes());
}

fn format_f64(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "+Inf".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Inf".to_owned()
    } else {
        let scientific = format!("{value:e}");
        let Some((mantissa, exponent)) = scientific.rsplit_once('e') else {
            return scientific;
        };
        let Ok(exponent) = exponent.parse::<i16>() else {
            return scientific;
        };
        if (-4..6).contains(&exponent) {
            return value.to_string();
        }
        let sign = if exponent.is_negative() { '-' } else { '+' };
        let magnitude = exponent.unsigned_abs();
        format!("{mantissa}e{sign}{magnitude:02}")
    }
}

/// Emit the separator used between arguments to Go `println`.
pub fn print_space() {
    write_stderr_bytes(b" ");
}

/// Emit the line terminator used by Go `println`.
pub fn print_newline() {
    write_stderr_bytes(b"\n");
}

/// Print the exact bytes of a Go string.
pub fn print_go_string(value: GoString) {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    drop(write_go_string_to(&mut output, &value));
}

#[cfg(test)]
mod tests {
    use super::format_f64;

    #[test]
    fn float_format_matches_go_1_26_shortest_round_trip_output() {
        for (value, expected) in [
            (1.0, "1"),
            (-0.0, "-0"),
            (3.5, "3.5"),
            (3.75, "3.75"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (999_999.0, "999999"),
            (1_000_000.0, "1e+06"),
            (16_777_216.0, "1.6777216e+07"),
            (1e100, "1e+100"),
            (f64::from_bits(1), "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::INFINITY, "+Inf"),
            (f64::NEG_INFINITY, "-Inf"),
        ] {
            assert_eq!(format_f64(value), expected, "{value:?}");
        }
        assert_eq!(format_f64(f64::NAN), "NaN");
    }
}
