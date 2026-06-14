package main

import (
	"crypto/sha512"
	"fmt"
)

func main() {
	fmt.Println(sha512.Size, sha512.Size224, sha512.Size256, sha512.Size384, sha512.BlockSize)
}
