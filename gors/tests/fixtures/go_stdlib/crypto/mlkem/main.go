package main

import (
	"crypto/mlkem"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/mlkem::CiphertextSize768 crypto/mlkem::CiphertextSize1024
	// gors:stdlib-cover crypto/mlkem::EncapsulationKeySize768 crypto/mlkem::EncapsulationKeySize1024
	// gors:stdlib-cover crypto/mlkem::SeedSize crypto/mlkem::SharedKeySize
	fmt.Println(
		mlkem.CiphertextSize768,
		mlkem.CiphertextSize1024,
		mlkem.EncapsulationKeySize768,
		mlkem.EncapsulationKeySize1024,
		mlkem.SeedSize,
		mlkem.SharedKeySize,
	)
}
