package main

import (
	"crypto/md5"
	"fmt"
)

func main() {
	fmt.Println(md5.Size, md5.BlockSize)
}
