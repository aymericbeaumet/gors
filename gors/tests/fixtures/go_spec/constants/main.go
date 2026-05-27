package main

import "fmt"

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
	fmt.Println(enabled, first, second, repeatedAgain, greeting, numeric, shifted, typedGreeting, typedNumeric, real(complexValue), imag(complexValue))
}
