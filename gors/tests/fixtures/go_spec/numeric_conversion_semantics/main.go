package main

func main() {
	v := uint16(0x10F0)
	if uint32(int8(v)) != 0xFFFFFFF0 {
		panic("truncation to int8 then sign extension to uint32 changed")
	}
	if uint64(int8(v)) != 0xFFFFFFFFFFFFFFF0 {
		panic("truncation to int8 then sign extension to uint64 changed")
	}
	i8 := int8(-1)
	if uint64(i8) != ^uint64(0) {
		panic("sign extension of -1 changed")
	}
	u8 := uint8(0xFF)
	if int64(u8) != 255 {
		panic("zero extension of unsigned value changed")
	}
	m := int64(-1)
	if uint16(m) != 0xFFFF || uint8(m) != 0xFF {
		panic("truncation of negative int64 changed")
	}
	wide := int64(0x1_0000_0001)
	if int32(wide) != 1 || uint32(wide) != 1 {
		panic("truncation of wide positive value changed")
	}
	f := 2.9
	if int32(f) != 2 {
		panic("positive float truncation changed")
	}
	g := -2.9
	if int32(g) != -2 {
		panic("negative float must truncate toward zero, not floor")
	}
	h := -0.9
	if int64(h) != 0 {
		panic("small negative float truncation changed")
	}
	big := 1e9 + 0.5
	if uint32(big) != 1000000000 {
		panic("float to uint32 truncation changed")
	}
	exact := -3.0
	if int8(exact) != -3 {
		panic("exact float to int conversion changed")
	}
	c := complex(1.5, -2.5)
	c64 := complex64(c)
	if real(c64) != 1.5 || imag(c64) != -2.5 {
		panic("complex128 to complex64 conversion changed")
	}
	back := complex128(c64)
	if real(back) != 1.5 || imag(back) != -2.5 {
		panic("complex64 to complex128 conversion changed")
	}
}
