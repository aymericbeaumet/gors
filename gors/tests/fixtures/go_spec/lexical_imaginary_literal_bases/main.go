package main

func main() {
	backCompat := 0123i
	separated := 0_123i
	octal := 0o123i
	hex := 0xabci
	binary := 0b101i
	hexFloat := 0x1p-2i
	elidedInt := .25i
	elidedFrac := 1.e+0i
	exponent := 1E6i
	zero := 0i

	if backCompat != 123i {
		panic("0123i must be decimal 123i for backward compatibility, not octal")
	}
	if separated != 123i {
		panic("0_123i must be decimal 123i")
	}
	if octal != 83i {
		panic("0o123i must be 0o123 * 1i == 83i")
	}
	if hex != 2748i {
		panic("0xabci must be 0xabc * 1i == 2748i")
	}
	if binary != 5i {
		panic("0b101i must be 0b101 * 1i == 5i")
	}
	if hexFloat != 0.25i {
		panic("0x1p-2i must be 0x1p-2 * 1i == 0.25i")
	}
	if elidedInt != 0.25i || elidedFrac != 1i || exponent != 1000000i {
		panic("elided-part imaginary literals changed")
	}
	if zero != 0 {
		panic("0i must equal 0")
	}
	sum := backCompat + octal
	println(real(sum), imag(sum))
	println(imag(hex), imag(hexFloat))
}
