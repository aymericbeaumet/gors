package main

import "fmt"

func main() {
	var small complex64 = complex64(complex(1, 2))
	var wide complex128 = complex(3, 4)

	fmt.Println(real(complex128(small)), imag(complex128(small)))
	fmt.Println(real(wide), imag(wide))
}
