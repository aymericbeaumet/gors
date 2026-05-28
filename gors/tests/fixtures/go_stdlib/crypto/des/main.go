package main

import (
	"crypto/des"
	"fmt"
)

func main() {
	fmt.Println(des.BlockSize)
	fmt.Println(des.KeySizeError.Error(des.KeySizeError(3)))
}
