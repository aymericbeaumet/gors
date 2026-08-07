package main

const Huge = 1 << 100
const Four int8 = Huge >> 98

func main() {
	var d float64 = 1e30 + 1 - 1e30
	var x float64 = 1e300 * 1e300 / 1e300
	if _, ok := interface{}(Four).(int8); !ok {
		panic("Four must have type int8")
	}
	if d != 1 || x != 1e300 || Four != 4 {
		panic("constant precision changed")
	}
	println("constant-exact-evaluation: ok")
}
