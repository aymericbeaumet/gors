package main

func main() {
	notHexFloat := 0x15e-2
	leadingZeroFloat := 072.40
	trailingDot := 1_5.
	underscoreExponent := 0.15e+0_2
	prefixUnderscoreHex := 0x_67_7a_2f_cc_40_c6
	elidedFraction := 1.e+0
	elidedInteger := .12345E+5
	zeroDot := 0.

	if notHexFloat != 348 {
		panic("0x15e-2 must lex as integer subtraction 0x15e - 2 == 348")
	}
	if leadingZeroFloat != 72.40 {
		panic("072.40 must be the decimal float 72.40, not an octal literal")
	}
	if trailingDot != 15.0 {
		panic("1_5. must equal 15.0")
	}
	if underscoreExponent != 15.0 {
		panic("0.15e+0_2 must equal 15.0")
	}
	if prefixUnderscoreHex != 0x677a2fcc40c6 {
		panic("underscores after the base prefix and between digits must not change the value")
	}
	if elidedFraction != 1.0 || elidedInteger != 12345.0 || zeroDot != 0.0 {
		panic("elided-part decimal float literals changed")
	}
	println(notHexFloat, leadingZeroFloat, trailingDot, elidedInteger)
	println(prefixUnderscoreHex)
}
