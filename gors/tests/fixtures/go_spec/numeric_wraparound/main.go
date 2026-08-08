package main

func main() {
	i8 := int8(127)
	i8++
	u8 := uint8(0)
	u8--
	i16 := int16(-32768)
	i16--
	u16 := uint16(65535)
	u16++
	i32 := int32(2147483647)
	i32++
	u32 := uint32(4294967295)
	u32++
	i64 := int64(-9223372036854775808)
	i64--
	u64 := uint64(18446744073709551615)
	u64++
	if i8 != -128 || u8 != 255 || i16 != 32767 || u16 != 0 {
		panic("8/16-bit wraparound changed")
	}
	if i32 != -2147483648 || u32 != 0 || i64 != 9223372036854775807 || u64 != 0 {
		panic("32/64-bit wraparound changed")
	}
	minI8 := int8(-128)
	negated := -minI8
	if negated != -128 {
		panic("negation of minimum int8 changed")
	}
	reinterpreted := int8(u8)
	widened := int32(reinterpreted)
	unsignedWidened := uint16(u8)
	if reinterpreted != -1 || widened != -1 || unsignedWidened != 255 {
		panic("two's complement conversion changed")
	}
	doubled := i8 * 2
	if doubled != 0 {
		panic("multiplication wraparound changed")
	}
	println(i8, u8, i16, u16, i32, u32, i64, u64, negated, doubled)
}
