use super::*;

#[test]
fn exact_width_division_remainder_and_shifts_execute_with_go_semantics() {
    let run = compile_and_run(
        r#"
            package main

            func markLeft(order *int) int8 {
                *order = 1
                return 1
            }

            func markRight(order *int) int16 {
                if *order == 1 { *order = 2 }
                return -1
            }

            func orderedNegativeShift() (ok bool) {
                order := 0
                defer func() { ok = recover() != nil && order == 2 }()
                _ = markLeft(&order) << markRight(&order)
                return false
            }

            func main() {
                var i8 int8 = -128
                var i16 int16 = -32768
                var i32 int32 = -2147483648
                var i64 int64 = -1 << 63
                if i8 / int8(-1) != i8 || i8 % int8(-1) != 0 { panic("int8 MIN/-1") }
                if i16 / int16(-1) != i16 || i16 % int16(-1) != 0 { panic("int16 MIN/-1") }
                if i32 / int32(-1) != i32 || i32 % int32(-1) != 0 { panic("int32 MIN/-1") }
                if i64 / int64(-1) != i64 || i64 % int64(-1) != 0 { panic("int64 MIN/-1") }

                if int8(-7) / int8(3) != -2 || int8(-7) % int8(3) != -1 { panic("int8 signs") }
                if int16(7) / int16(-3) != -2 || int16(7) % int16(-3) != 1 { panic("int16 signs") }
                if uint8(255) / uint8(2) != 127 || uint8(255) % uint8(2) != 1 { panic("uint8") }
                if uint16(65535) / uint16(2) != 32767 { panic("uint16") }
                if uint32(4294967295) / uint32(2) != 2147483647 { panic("uint32") }
                var high uint64 = 1 << 63
                if high / uint64(2) != 1 << 62 || uint64(18446744073709551615) % 2 != 1 {
                    panic("uint64 high bit")
                }

                var unsigned uint8 = 255
                var signedCount int32 = 3
                var unsignedCount uint64 = 100
                var signed int16 = -2
                if unsigned >> signedCount != 31 { panic("logical right") }
                if signed >> unsignedCount != -1 { panic("signed fill") }
                if unsigned << unsignedCount != 0 { panic("wide left") }
                var huge uint64 = 1 << 63
                if unsigned << huge != 0 { panic("high-bit uint64 count") }
                if unsigned << (1 << 63) != 0 { panic("high-bit untyped count") }
                unsigned <<= unsignedCount
                if unsigned != 0 { panic("compound shift") }
                unsigned = 1
                unsigned <<= 1 << 63
                if unsigned != 0 { panic("compound high-bit untyped count") }

                var context uint = 33
                var contextual int32 = 1 << context
                if contextual != 0 || !(1.0 << context == contextual) { panic("contextual lhs") }
                if !orderedNegativeShift() { panic("negative shift order") }
            }
        "#,
    );
    assert!(
        run.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn shift_typing_rejects_invalid_constants_and_noninteger_dynamic_counts() {
    let negative = crate::compiler::lower_to_hir(
        "negative.go",
        "package main\nfunc bad() int { const count = -1; return 1 << count }\n",
    )
    .unwrap_err();
    assert!(
        negative
            .iter()
            .any(|diagnostic| diagnostic.message.contains("non-negative integer")),
        "{negative:?}"
    );

    let noninteger = crate::compiler::lower_to_hir(
        "float.go",
        "package main\nfunc bad(value int, count float64) int { return value << count }\n",
    )
    .unwrap_err();
    assert!(
        noninteger
            .iter()
            .any(|diagnostic| diagnostic.message.contains("shift count must be integer")),
        "{noninteger:?}"
    );

    let wider_than_uint = crate::compiler::lower_to_hir(
        "too-wide.go",
        "package main\nfunc bad(value uint8) uint8 { return value << (1 << 64) }\n",
    )
    .unwrap_err();
    assert!(
        wider_than_uint
            .iter()
            .any(|diagnostic| diagnostic.message.contains("not representable")),
        "{wider_than_uint:?}"
    );
}
