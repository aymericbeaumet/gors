package main

import (
	"crypto/sha256"
	"fmt"
)

func main() {
	fmt.Println(sha256.Size, sha256.Size224, sha256.BlockSize)
}
