package main

import (
	"crypto/dsa"
	"fmt"
)

func main() {
	fmt.Println("== dsa/basic ==")
	// gors:stdlib-cover crypto/dsa::ParameterSizes
	// gors:stdlib-cover crypto/dsa::L1024N160
	// gors:stdlib-cover crypto/dsa::L2048N224
	// gors:stdlib-cover crypto/dsa::L2048N256
	// gors:stdlib-cover crypto/dsa::L3072N256
	fmt.Println(int(dsa.L1024N160))
	fmt.Println(int(dsa.L2048N224))
	fmt.Println(int(dsa.L2048N256))
	fmt.Println(int(dsa.L3072N256))

	// gors:stdlib-cover crypto/dsa::ErrInvalidPublicKey
	fmt.Println(dsa.ErrInvalidPublicKey.Error())
}
