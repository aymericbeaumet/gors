package main

import (
	"crypto/aes"
	"fmt"
)

func main() {
	fmt.Println(aes.BlockSize)
	fmt.Println(aes.KeySizeError.Error(aes.KeySizeError(7)))
}
