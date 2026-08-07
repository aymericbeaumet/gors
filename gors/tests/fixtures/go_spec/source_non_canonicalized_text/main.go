package main

func main() {
	precomposed := "é"
	combining := "é"
	if precomposed == combining {
		panic("source text must not be canonicalized: precomposed and combining forms are distinct")
	}
	if len(precomposed) != 2 {
		panic("U+00E9 must be two UTF-8 bytes")
	}
	if len(combining) != 3 {
		panic("e followed by combining U+0301 must be three UTF-8 bytes")
	}
	É := 1
	é := 2
	if É+é != 3 {
		panic("uppercase and lowercase letters must be distinct identifiers")
	}
	println(len(precomposed), len(combining), É+é)
}
