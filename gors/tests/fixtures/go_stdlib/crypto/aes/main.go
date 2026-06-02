package main

import (
	"crypto/aes"
	"fmt"
)

func caseAesConstantsAndErrors() {
	// gors:stdlib-cover crypto/aes::BlockSize
	fmt.Println(aes.BlockSize)
	// gors:stdlib-cover crypto/aes::KeySizeError
	// gors:stdlib-cover crypto/aes::KeySizeError.Error
	fmt.Println(aes.KeySizeError.Error(aes.KeySizeError(7)))
}

func main() {
	caseAesConstantsAndErrors()
}
