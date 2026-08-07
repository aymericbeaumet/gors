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

/// Print Go's floating-point debug representation.
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
        let raw = format!("{value:+.6e}");
        let Some((mantissa, exponent)) = raw.rsplit_once('e') else {
            return raw;
        };
        let (sign, digits) = exponent
            .strip_prefix('-')
            .map_or(("+", exponent), |digits| ("-", digits));
        format!("{mantissa}e{sign}{digits:0>3}")
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
    fn float_format_matches_the_go_builtin_debug_surface() {
        assert_eq!(format_f64(1.0), "+1.000000e+000");
        assert_eq!(format_f64(-0.0), "-0.000000e+000");
        assert_eq!(format_f64(1e100), "+1.000000e+100");
        assert_eq!(format_f64(f64::INFINITY), "+Inf");
        assert_eq!(format_f64(f64::NEG_INFINITY), "-Inf");
        assert_eq!(format_f64(f64::NAN), "NaN");
    }
}
