package main



type myBool bool
type myString string
type myFloat float64
type myInt int32

const (
	cmp    = "foo" > "bar" // untyped boolean constant
	concat = "hi" + "!"    // untyped string constant
	kilo   = 1 << 10       // untyped integer constant
	frac   = 22.0 / 7      // untyped floating-point constant
)

const typedF float32 = 0.5 // typed constant: retains float32, not default float64

func main() {
	var b myBool = cmp // untyped constants are assignable to defined types
	var s myString = concat
	var f myFloat = kilo // integer constant to float-kinded defined type
	var i myInt = 'x'    // rune constant to int32-kinded defined type
	var g myFloat = frac
	println(b, s, f, i, g == 22.0/7)
	if _, ok := interface{}(typedF).(float32); !ok {
		panic("typedF must retain its explicit type float32")
	}
	if _, ok := interface{}(kilo).(int); !ok {
		panic("kilo must default to int")
	}
	if !b || s != "hi!" || f != 1024 || i != 120 {
		panic("untyped constant propagation changed")
	}
}
