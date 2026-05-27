package main

import "fmt"

const (
	enabled = true
	first   = iota
	second
)

const greeting = "go" + "rs"
const numeric = 1 + 2*3
const complexValue = 1 + 2i

func main() {
	fmt.Println(enabled, first, second, greeting, numeric, real(complexValue), imag(complexValue))
}
