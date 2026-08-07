package main

func main() {
	zero := 0.0
	one := 1.0
	negZero := -zero
	inf := one / zero
	negInf := -one / zero
	nan := zero / zero

	if min(nan, one) == min(nan, one) {
		panic("min with NaN did not return NaN")
	}
	if max(one, nan) == max(one, nan) {
		panic("max with NaN did not return NaN")
	}
	if min(one, nan, negInf) == min(one, nan, negInf) {
		panic("variadic min with NaN did not return NaN")
	}

	if one/min(negZero, zero) != negInf {
		panic("min(-0.0, 0.0) did not return -0.0")
	}
	if one/max(zero, negZero) != inf {
		panic("max(0.0, -0.0) did not return 0.0")
	}

	if min(negInf, -123.0) != negInf || max(inf, 123.0) != inf {
		panic("min/max returning infinities changed")
	}
	if min(inf, 3.0) != 3.0 || max(negInf, 3.0) != 3.0 {
		panic("min/max against infinity changed")
	}

	if max("", "foo", "bar") != "foo" || min("go", "Go") != "Go" {
		panic("string min/max changed")
	}

	c := max(1, 2.0, 10)
	var floatSink float64 = c
	if floatSink != 10.0 {
		panic("max(1, 2.0, 10) changed")
	}

	f := min(3, float32(one))
	var f32Sink float32 = f
	if f32Sink != 1.0 {
		panic("min with float32 operand changed")
	}

	println("min-max-float-string: ok")
}
