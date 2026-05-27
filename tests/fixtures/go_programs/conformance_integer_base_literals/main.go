package main

import "fmt"

func main() {
	decimal := 42
	binary := 0b101010
	octal := 0o52
	legacyOctal := 052
	hex := 0x2a
	separated := 0x_2A
	fmt.Println(decimal, binary, octal, legacyOctal, hex, separated)
	fmt.Println(decimal == binary, binary == octal, octal == legacyOctal, legacyOctal == hex, hex == separated)
}
