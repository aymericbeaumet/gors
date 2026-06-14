package main

import (
	"fmt"
	"hash/crc32"
)

func main() {
	fmt.Println("== hash/crc32/constants ==")
	fmt.Println(crc32.Size, crc32.IEEE, crc32.Castagnoli, crc32.Koopman)
}
