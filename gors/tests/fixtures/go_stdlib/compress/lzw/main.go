package main

import (
	"compress/lzw"
	"fmt"
)

func main() {
	fmt.Println(lzw.LSB == lzw.LSB, lzw.LSB == lzw.MSB)
}
