package main

const (
	enabled = true
	first   = iota
	second
	repeated = 10
	repeatedAgain
	doubled = iota * 2
	doubledAgain
)

const (
	sized int32 = 1024
	sizedAgain
)

const greeting = "go" + "rs"
const numeric = 1 + 2*3
const complexValue = 1 + 2i
const shifted = 1 << 3
const typedGreeting string = greeting
const typedNumeric int = numeric
const fromFloat int = 14 / 2.0
const shiftedFromFloat = 1.0 << 3

func main() {
	if !enabled || first != 1 || second != 2 || repeatedAgain != 10 {
		panic("iota constants changed")
	}
	if doubled != 10 || doubledAgain != 12 {
		panic("repeated iota expression not textually substituted")
	}
	if sizedAgain != 1024 {
		panic("repeated typed constant changed")
	}
	if greeting != "gors" || numeric != 7 || shifted != 8 {
		panic("untyped constants changed")
	}
	if typedGreeting != "gors" || typedNumeric != 7 {
		panic("typed constants changed")
	}
	if real(complexValue) != 1 || imag(complexValue) != 2 {
		panic("complex constants changed")
	}
	var floated float64 = numeric
	if floated != 7.0 || floated/2 != 3.5 {
		panic("integer constant coerced to float changed")
	}
	if fromFloat != 7 {
		panic("float-kind constant division at int site changed")
	}
	x := 1.0 << 3
	if x%3 != 2 {
		panic("constant shift of untyped float must yield an integer value")
	}
	if shiftedFromFloat != 8 {
		panic("shifted float constant value changed")
	}
}
