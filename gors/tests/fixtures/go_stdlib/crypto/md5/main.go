package main

import (
	"crypto/md5"
	"fmt"
)

func main() {
	// gors:stdlib-cover crypto/md5::Size crypto/md5::BlockSize
	fmt.Println(md5.Size, md5.BlockSize)
}
