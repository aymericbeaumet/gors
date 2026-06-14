package main

const (
	enabled = true
	first   = iota
	second
	repeated = 10
	repeatedAgain
)

const greeting = "go" + "rs"
const numeric = 1 + 2*3
const complexValue = 1 + 2i
const shifted = 1 << 3
const typedGreeting string = greeting
const typedNumeric int = numeric

func main() {
	if !enabled || first != 1 || second != 2 || repeatedAgain != 10 {
		panic("iota constants changed")
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
}
