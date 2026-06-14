package main

import (
	"crypto/sha1"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/sha1::Size crypto/sha1::BlockSize
	fmt.Println(sha1.Size, sha1.BlockSize)
}
