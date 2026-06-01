package main

import (
	"fmt"
	"hash/crc64"
)

func main() {
	fmt.Println("== hash/crc64/constants ==")
	fmt.Println(crc64.Size, uint64(crc64.ISO), uint64(crc64.ECMA))
}
