package main

const (
	Theta float64 = 3 / 2    // 1.0: 3/2 is integer division
	Pi    float64 = 3 / 2.   // 1.5: 3/2. is float division
	b             = 15 / 4   // untyped integer constant 3
	c             = 15 / 4.0 // untyped float constant 3.75
	neg           = -7 / 2   // constant integer division truncates toward zero: -3
)

const asInt int = 15 / 5.0 // 3.0 is a float constant with integral value, representable by int

func main() {
	var f float64 = b // untyped integer constant coerced to float64 at use site
	println(Theta, Pi, b, c, neg)
	println(f, asInt)
	if Theta != 1.0 || Pi != 1.5 || b != 3 || c != 3.75 || neg != -3 || f != 3.0 || asInt != 3 {
		panic("constant division semantics changed")
	}
}
