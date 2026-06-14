package main

import (
	"compress/zlib"
	"fmt"
)

func caseZlibConstants() {
	// gors:stdlib-cover compress/zlib::BestCompression
	// gors:stdlib-cover compress/zlib::BestSpeed
	// gors:stdlib-cover compress/zlib::DefaultCompression
	// gors:stdlib-cover compress/zlib::HuffmanOnly
	// gors:stdlib-cover compress/zlib::NoCompression
	fmt.Println(zlib.NoCompression, zlib.BestSpeed, zlib.BestCompression, zlib.DefaultCompression, zlib.HuffmanOnly)
}

func caseZlibErrors() {
	// gors:stdlib-cover compress/zlib::ErrChecksum
	fmt.Println(zlib.ErrChecksum.Error())
	// gors:stdlib-cover compress/zlib::ErrDictionary
	fmt.Println(zlib.ErrDictionary.Error())
	// gors:stdlib-cover compress/zlib::ErrHeader
	fmt.Println(zlib.ErrHeader.Error())
}

func main() {
	caseZlibConstants()
	caseZlibErrors()
}
