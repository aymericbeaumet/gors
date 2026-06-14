package main

import (
	"crypto/des"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/des::BlockSize
	fmt.Println(des.BlockSize)
	// gors:stdlib-cover crypto/des::KeySizeError
	// gors:stdlib-cover crypto/des::KeySizeError.Error
	fmt.Println(des.KeySizeError.Error(des.KeySizeError(3)))
}
