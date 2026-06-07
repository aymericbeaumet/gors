package main

func main() {
	// Line comments and block comments are ignored.
	decimal := 1_000
	binary := 0b101010
	upperBinary := 0B101010
	octal := 0o52
	upperOctal := 0O52
	legacyOctal := 052
	separatedLegacyOctal := 0_52
	hex := 0x2a
	upperHex := 0X2A
	decimalFloat := 1.25
	decimalExponent := 1.5e1
	hexFloat := 0x1p-2
	fraction := 0x1.Fp+0
	upperHexFloat := 0X.8P+0
	imaginary := 2i
	runeValue := '\n'
	escapedRune := '\u0041'
	hexByteRune := '\x41'
	interpreted := "go\nrs"
	raw := `go\nrs`

	if !(decimal == 1000 &&
		binary == 42 &&
		upperBinary == 42 &&
		octal == 42 &&
		upperOctal == 42 &&
		legacyOctal == 42 &&
		separatedLegacyOctal == 42 &&
		hex == 42 &&
		upperHex == 42 &&
		decimalFloat == 1.25 &&
		decimalExponent == 15.0 &&
		hexFloat == 0.25 &&
		fraction == 1.9375 &&
		upperHexFloat == 0.5 &&
		imaginary == 2i &&
		runeValue == 10 &&
		escapedRune == 65 &&
		hexByteRune == 65 &&
		interpreted != raw) {
		panic("lexical literal values changed")
	}
}
