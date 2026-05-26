package main

import "fmt"

const wide = 0123i
const small complex64 = 2.5i

func main() {
	z := 0x1p-2i
	fmt.Println(real(wide), imag(wide))
	fmt.Println(real(complex128(small)), imag(complex128(small)))
	fmt.Println(real(z), imag(z))
}
