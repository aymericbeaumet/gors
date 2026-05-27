package main

import "fmt"

func main() {
	// Line comments and block comments are ignored.
	decimal := 1_000
	binary := 0b101010
	octal := 0o52
	legacyOctal := 052
	separatedLegacyOctal := 0_52
	hex := 0x2a
	decimalFloat := 1.25
	hexFloat := 0x1p-2
	fraction := 0x1.Fp+0
	imaginary := 2i
	runeValue := '\n'
	interpreted := "go\nrs"
	raw := `go\nrs`

	if decimal == 1000 &&
		binary == 42 &&
		octal == 42 &&
		legacyOctal == 42 &&
		separatedLegacyOctal == 42 &&
		hex == 42 &&
		decimalFloat == 1.25 &&
		hexFloat == 0.25 &&
		fraction == 1.9375 &&
		imaginary == 2i &&
		runeValue == 10 &&
		interpreted != raw {
		fmt.Println("lexical ok")
	}
}
