package main

import "fmt"

const wide = 0123i
const small complex64 = 2.5i
const combo = 1 + 2i
const tiny complex64 = 3 + 4i

func main() {
	z := 0x1p-2i
	sum := 1 + 2i
	mixed := sum + 3
	fmt.Println(real(wide), imag(wide))
	fmt.Println(real(complex128(small)), imag(complex128(small)))
	fmt.Println(real(combo), imag(combo))
	fmt.Println(real(complex128(tiny)), imag(complex128(tiny)))
	fmt.Println(real(z), imag(z))
	fmt.Println(real(sum), imag(sum))
	fmt.Println(real(mixed), imag(mixed))
}
