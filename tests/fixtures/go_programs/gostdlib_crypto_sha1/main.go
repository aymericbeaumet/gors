package main

import (
	"crypto/sha1"
	"fmt"
)

func main() {
	fmt.Println(sha1.Size, sha1.BlockSize)
}
