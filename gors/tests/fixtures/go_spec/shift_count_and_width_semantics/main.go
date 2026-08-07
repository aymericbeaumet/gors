package main

func negativeShiftPanics(value int64, count int) (panicked bool) {
	defer func() { panicked = recover() != nil }()
	_ = value << count
	return false
}

func main() {
	var s uint = 33
	var j int32 = 1 << s
	k := uint64(1 << s)
	var v uint8 = 255
	wide := v << 9
	var neg int8 = -128
	arith := neg >> 100
	var u8max uint8 = 255
	logical := u8max >> 3
	signedCount := 3
	var w int64 = 1

	if j != 0 {
		panic("untyped 1 did not assume int32 in non-constant shift")
	}
	if !(1.0<<s == j) {
		panic("untyped 1.0 did not assume int32 in shift comparison")
	}
	if k != 8589934592 {
		panic("untyped 1 did not assume uint64 inside conversion operand")
	}
	if wide != 0 {
		panic("uint8 << 9 (count >= width) did not produce 0")
	}
	if arith != -1 {
		panic("arithmetic right shift by 100 did not fill with sign bits")
	}
	if logical != 31 {
		panic("logical right shift changed")
	}
	if w<<signedCount != 8 {
		panic("signed integer shift count changed")
	}
	if !negativeShiftPanics(w, -1) {
		panic("negative run-time shift count did not panic")
	}
}
