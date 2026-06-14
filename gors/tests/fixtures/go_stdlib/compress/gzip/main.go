package main

import (
	"compress/gzip"
	"fmt"
)

func caseGzipConstants() {
	// gors:stdlib-cover compress/gzip::BestCompression
	// gors:stdlib-cover compress/gzip::BestSpeed
	// gors:stdlib-cover compress/gzip::DefaultCompression
	// gors:stdlib-cover compress/gzip::HuffmanOnly
	// gors:stdlib-cover compress/gzip::NoCompression
	fmt.Println(gzip.NoCompression, gzip.BestSpeed, gzip.BestCompression, gzip.DefaultCompression, gzip.HuffmanOnly)
}

func caseGzipErrors() {
	// gors:stdlib-cover compress/gzip::ErrChecksum
	fmt.Println(gzip.ErrChecksum.Error())
	// gors:stdlib-cover compress/gzip::ErrHeader
	fmt.Println(gzip.ErrHeader.Error())
}

func caseGzipHeader() {
	// gors:stdlib-cover compress/gzip::Header
	h := gzip.Header{Name: "name.txt", Comment: "fixture", OS: 3}
	fmt.Println(h.Name, h.Comment, h.OS)
}

func main() {
	caseGzipConstants()
	caseGzipErrors()
	caseGzipHeader()
}
