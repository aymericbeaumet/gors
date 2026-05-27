package main

import (
	"fmt"
	"hash/adler32"
)

func main() {
	fmt.Println("== adler32/basic ==")
	fmt.Println(adler32.Size)
}
