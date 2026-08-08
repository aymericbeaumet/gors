package main

func main() {
	huge := 1e308
	inf := huge + huge
	if !(inf > huge) {
		panic("float64 overflow did not produce +Inf")
	}
	negInf := -inf
	nan := inf + negInf
	if nan == nan {
		panic("NaN compared equal to itself")
	}
	if !(nan != nan) {
		panic("NaN != NaN was not true")
	}
	if nan < nan || nan <= nan || nan > nan || nan >= nan {
		panic("ordered comparison involving NaN was not false")
	}
	if nan == inf || nan < inf || nan > inf || nan <= negInf || nan >= inf {
		panic("comparison of NaN with infinity was not false")
	}
	zero := 0.0
	negZero := -zero
	if negZero != zero || !(negZero == 0.0) || negZero < 0 || negZero > 0 {
		panic("negative zero must compare equal to positive zero")
	}
	if !(negInf < -huge) || !(huge < inf) || !(inf == inf) || !(negInf == -inf) {
		panic("infinity ordering changed")
	}
	var i1 any = nan
	var i2 any = nan
	if i1 == i2 {
		panic("interfaces holding NaN compared equal")
	}
	var iz1 any = zero
	var iz2 any = negZero
	if iz1 != iz2 {
		panic("interfaces holding +0 and -0 float64 did not compare equal")
	}
	nan32 := float32(nan)
	if nan32 == nan32 {
		panic("float32 NaN compared equal to itself")
	}
	inf32 := float32(inf)
	if !(inf32 > 0) || float64(inf32) != inf {
		panic("float32 conversion of +Inf changed")
	}
}
