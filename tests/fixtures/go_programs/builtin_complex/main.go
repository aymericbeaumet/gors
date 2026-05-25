package main

import "fmt"

func main() {
	// complex, real, imag
	c := complex(3.0, 4.0)
	fmt.Println(real(c))
	fmt.Println(imag(c))

	// arithmetic
	c2 := complex(1.0, 2.0)
	sum := c + c2
	fmt.Println(real(sum))
	fmt.Println(imag(sum))
}
