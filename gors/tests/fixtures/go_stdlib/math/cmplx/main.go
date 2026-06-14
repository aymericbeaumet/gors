package main

import (
	"fmt"
	"math/cmplx"
)

func main() {
	fmt.Println("== math/cmplx/conj ==")
	caseCmplxConj()
}

func printParts(z complex128) {
	fmt.Println(real(z), imag(z))
}

func caseCmplxConj() {
	z := complex(3.0, 4.0)
	printParts(cmplx.Conj(z))
}
