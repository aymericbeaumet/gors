package main

func main() {
	const minI64 int64 = -1 << 63
	const maxI64 int64 = 1<<63 - 1

	x := maxI64
	m := minI64
	minusOne := int64(-1)

	if x+1 != minI64 {
		panic("int64 max + 1 did not wrap to min")
	}
	if m-1 != maxI64 {
		panic("int64 min - 1 did not wrap to max")
	}
	if x < x+1 {
		panic("compiler assumed x < x+1 is always true")
	}
	q := m / minusOne
	r := m % minusOne
	if q != m || r != 0 {
		panic("MinInt64 / -1 wraparound semantics changed")
	}
	var b uint8 = 255
	b++
	if b != 0 {
		panic("uint8 wraparound changed")
	}
	var u uint32
	u--
	if u != 4294967295 {
		panic("uint32 underflow wraparound changed")
	}
	two := int32(2)
	big := int32(2147483647)
	if big*two != -2 {
		panic("int32 multiplication overflow changed")
	}
	a, d := -5, 3
	if a/d != -1 || a%d != -2 {
		panic("negative dividend truncated division changed")
	}
	c, e := 5, -3
	if c/e != -1 || c%e != 2 {
		panic("negative divisor truncated division changed")
	}
	n := -11
	if n/4 != -2 || n%4 != -3 || n>>2 != -3 || n&3 != 1 {
		panic("division truncates toward zero but shift toward negative infinity")
	}
	sh := minI64
	if sh<<1 != 0 {
		panic("int64 min << 1 did not wrap to zero")
	}
}
