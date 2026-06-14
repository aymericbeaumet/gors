package main

import (
	"compress/lzw"
	"fmt"
)

func caseLzwOrder() {
	// gors:stdlib-cover compress/lzw::LSB
	// gors:stdlib-cover compress/lzw::MSB
	// gors:stdlib-cover compress/lzw::Order
	fmt.Println(lzw.LSB == lzw.LSB, lzw.LSB == lzw.MSB)
}

func main() {
	caseLzwOrder()
}
