package main

var packageCount int = 3

func main() {
	var zero int
	var text = "value"
	short := packageCount + zero
	if text != "value" {
		panic("type-inferred var text not initialized correctly")
	}
	var inferred = 42
	var truth = true
	var (
		grouped int
		u, v, s = 2.0, 3, "bar"
	)
	if inferred/5 != 8 { // int division: only holds if inferred has default type int
		panic("untyped constant 42 not given default type int")
	}
	if truth != true {
		panic("untyped bool not converted to bool")
	}
	if grouped != 0 || u != 2.0 || v != 3 || s != "bar" {
		panic("grouped var spec initialized incorrectly")
	}
	first, second := 1, 2
	firstAlias := &first
	captureFirst := func() int { return first }
	first, third := second, first+second
	if *firstAlias != 2 || captureFirst() != 2 {
		panic("redeclaration must assign to the original variable, not create a new one")
	}
	first, second = second, first
	pointer := &short
	*pointer = *pointer + 1
	if zero != 0 || short != 4 {
		panic("zero or pointer-updated value changed")
	}
	if first != 2 || second != 2 || third != 3 {
		panic("short declaration assignments changed")
	}

	// Short declarations must apply the untyped-constant default-type rule.
	quotient := 7 / 2 // untyped int constant expression -> int
	if quotient != 3 {
		panic("short-declared int must use truncating integer division")
	}
	f := 2.5 // untyped float constant -> float64
	if f*2 != 5 {
		panic("short-declared float must not truncate to int")
	}
	precise := 16777217.0 // 2^24+1: exact in float64, rounds in float32
	if precise-16777216.0 != 1 {
		panic("short-declared float default type must be float64, not float32")
	}
	r := 'A' // untyped rune constant -> rune (int32)
	if r != 65 {
		panic("short-declared rune must have value 65")
	}
	word := "go" + "rs" // untyped string constant -> string
	if word != "gors" || len(word) != 4 {
		panic("short-declared string incorrect")
	}
	ok := 1 < 2 // untyped boolean -> bool
	if !ok {
		panic("short-declared bool incorrect")
	}
	c := 3 + 4i // untyped complex constant -> complex128
	if real(c) != 3 || imag(c) != 4 || real(c*c) != -7 {
		panic("short-declared complex128 incorrect")
	}

	// Short declarations in if/for/switch initializer contexts.
	if half := f / 2; half != 1.25 {
		panic("if-initializer short declaration incorrect")
	}
	total := 0
	for i := 1; i <= 3; i++ {
		total += i
	}
	if total != 6 {
		panic("for-initializer short declaration incorrect")
	}
	switch grade := quotient * quotient; grade {
	case 9:
		// expected
	default:
		panic("switch-initializer short declaration incorrect")
	}
}
