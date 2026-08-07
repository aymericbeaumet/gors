package main

const (
	strLen = len("héllo")  // 6 (bytes): len of a constant string is a constant
	mn     = min(3, 2.0)   // constant result of builtin min: 2.0 (floating-point kind)
	mx     = max('a', 100) // constant result of builtin max: 100 (rune kind)
	cplx   = complex(1, 2) // untyped complex constant
	re     = real(cplx)    // untyped float constant 1
	im     = imag(cplx)    // untyped float constant 2
)

var arr [strLen]int // array length requires a constant

func main() {
	if strLen != 6 || len(arr) != 6 || mn != 2 || mx != 100 || re != 1 || im != 2 {
		panic("constant builtins changed")
	}
	mnDefault := mn
	mxDefault := mx
	reDefault := re
	imDefault := im
	_, mnIsFloat64 := any(mnDefault).(float64)
	_, mxIsInt32 := any(mxDefault).(int32)
	_, reIsFloat64 := any(reDefault).(float64)
	_, imIsFloat64 := any(imDefault).(float64)
	if !mnIsFloat64 || !mxIsInt32 || !reIsFloat64 || !imIsFloat64 {
		panic("constant builtin result kinds changed")
	}
	println(strLen, len(arr), int(mn), int(mx), int(re), int(im))
	println(mnIsFloat64, mxIsInt32, reIsFloat64, imIsFloat64)
}
